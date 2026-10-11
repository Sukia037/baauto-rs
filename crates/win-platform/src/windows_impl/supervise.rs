//! Supervisor executes the pure FSM, samples shared memory, and never performs blocking pipe/file I/O while armed.
use super::*;
use baauto_config::{Chord, CompiledConfig};
use baauto_core::{SessionId, StopReason};
use baauto_protocol::{
    ipc::*,
    shm::LedgerSnapshot,
    supervisor::{Effect, Effects, Event as FsmEvent, State, SupervisorFsm, Timeouts},
};
use std::{
    ffi::OsString,
    path::PathBuf,
    sync::{Arc, atomic::Ordering::SeqCst, mpsc},
    thread,
};
use windows::{Win32::System::Threading::INFINITE, core::Result};
#[derive(Clone)]
pub struct SupervisorOptions {
    pub cfg: Arc<CompiledConfig>,
    pub engine: PathBuf,
    pub target: OsString,
    pub stop_hotkey: Chord,
    pub record_hotkey: Chord,
    pub health_timeout_ms: u64,
    pub recovery_timeout_ms: u64,
    pub logs: PathBuf,
    pub injection: Option<String>,
}
pub enum SupervisorCommand {
    Enable,
    Disable,
    Apply {
        cfg: Arc<CompiledConfig>,
        stop_hotkey: Chord,
        record_hotkey: Chord,
    },
    Rehook,
    Quit,
}
#[derive(Clone, Debug)]
pub struct SupervisorStatus {
    pub state: State,
    pub session: SessionId,
    pub detail: String,
    pub records: u64,
}
struct Child {
    process: Arc<Process>,
    mapping: Arc<Mapping>,
    io: Option<PipePump<EngineToApp, AppToEngine>>,
    stop: Option<Arc<Event>>,
    session: SessionId,
    candidates: LedgerSnapshot,
    marker: usize,
    recovered: bool,
    release_success: bool,
    recovery_ms: u32,
    clock: Clock,
    stop_request: Option<u64>,
    exit_confirm: Option<u64>,
    receipt: Option<RecoveryReceipt>,
}
impl Child {
    fn launch(options: &SupervisorOptions, session: SessionId, clock: Clock) -> Result<Self> {
        let name = random_name("BAauto-rs")?;
        let pipe_name = format!("\\\\.\\pipe\\{}", name.trim_start_matches("Local\\"));
        let mapping = Arc::new(Mapping::create(&name, session, clock)?);
        let pipe = Arc::new(Pipe::server(&pipe_name)?);
        let mut args = vec![
            OsString::from("--managed"),
            OsString::from("--shm"),
            OsString::from(&name),
            OsString::from("--pipe"),
            OsString::from(&pipe_name),
            OsString::from("--parent"),
            OsString::from(std::process::id().to_string()),
            OsString::from("--target-exe"),
            options.target.clone(),
        ];
        if let Some(rule) = &options.injection {
            args.extend([OsString::from("--inject"), OsString::from(rule)]);
        }
        let marker = random_marker()?;
        let process = Arc::new(Process::spawn(options.engine.as_os_str(), &args)?);
        let mut child = Self {
            process,
            mapping,
            io: None,
            stop: None,
            session,
            candidates: LedgerSnapshot::candidates(&options.cfg.candidates),
            marker,
            recovered: false,
            release_success: false,
            clock,
            stop_request: None,
            exit_confirm: None,
            receipt: None,
            recovery_ms: options.recovery_timeout_ms.min(u64::from(u32::MAX)) as u32,
        };
        let abort = console_stop()?;
        pipe.connect(
            &abort,
            &child.process,
            (options.health_timeout_ms.saturating_mul(2)).min(u64::from(u32::MAX)) as u32,
        )?;
        child.io = Some(PipePump::start(pipe, child.process.clone())?);
        Ok(child)
    }
    fn send(&self, command: AppToEngine) -> Result<()> {
        self.io
            .as_ref()
            .ok_or_else(|| failure("pipe not connected"))?
            .send(command)
    }
    fn snapshot(&self) -> Option<LedgerSnapshot> {
        self.mapping
            .data()
            .header
            .valid(self.session)
            .then(|| self.mapping.data().ledger.snapshot())
            .flatten()
    }
    fn recover(&mut self, candidates: bool) -> Result<bool> {
        let proof = self
            .process
            .exit_proof()
            .ok_or_else(|| failure("release attempted before confirmed process exit"))?;
        let held = if candidates {
            self.candidates
        } else {
            self.snapshot().unwrap_or(self.candidates)
        };
        self.exit_confirm = Some(self.clock.now().0);
        let receipt = release_after_exit_recorded(&proof, held, self.marker, self.clock);
        let success = receipt.success();
        self.receipt = Some(receipt);
        self.recovered = true;
        self.release_success = success;
        Ok(success)
    }
}
impl Drop for Child {
    fn drop(&mut self) {
        if !self.process.exited() {
            if let Some(stop) = &self.stop {
                let _ = stop.set();
            }
            let waited = wait(&[self.process.raw()], self.recovery_ms);
            if !matches!(waited, Ok(0)) {
                let _ = self.process.terminate();
            }
            if self.process.wait_exit().is_err() {
                return;
            }
        }
        if !self.recovered {
            let _ = self.recover(self.snapshot().is_none());
        }
        self.io.take();
    }
}
fn next_seq(seq: &mut u64) -> Result<u64> {
    *seq = seq
        .checked_add(1)
        .ok_or_else(|| failure("command sequence exhausted"))?;
    Ok(*seq)
}
pub fn run_supervisor(
    mut options: SupervisorOptions,
    commands: mpsc::Receiver<SupervisorCommand>,
    statuses: mpsc::Sender<SupervisorStatus>,
) -> Result<()> {
    let _ = set_dpi_awareness();
    let clock = Clock::new()?;
    let console = console_stop()?;
    let ticks = |ms: u64| {
        (u128::from(ms) * u128::from(clock.rate.0))
            .div_ceil(1000)
            .min(u128::from(u64::MAX)) as u64
    };
    if options.health_timeout_ms == 0 || options.recovery_timeout_ms == 0 {
        return Err(failure("timeouts must be positive"));
    }
    let mut window = HealthWindow::new(
        options.cfg.related_keys(),
        options.stop_hotkey,
        options.record_hotkey,
    )?;
    let timer = Timer::new()?;
    let mut fsm = SupervisorFsm::new(Timeouts {
        health: ticks(options.health_timeout_ms),
        recovery: ticks(options.recovery_timeout_ms),
    });
    fsm.on(FsmEvent::HotkeysRegistered(true));
    let mut session = SessionId(0);
    let mut seq = 0;
    let mut child = Some(Child::launch(&options, session, clock)?);
    let mut wanted = false;
    let mut quitting = false;
    let mut installed = false;
    let mut released = false;
    let mut pending_cfg = None;
    let mut handshake_deadline = clock
        .now()
        .after(ticks(options.health_timeout_ms.saturating_mul(2)));
    let mut last_status = None;
    let mut dump_workers = Vec::new();
    let mut restart = false;
    let mut arm_deadline = None;
    let mut confirm_raw = None;
    child.as_ref().unwrap().send(AppToEngine::Hello {
        cmd_seq: next_seq(&mut seq)?,
        protocol: PROTOCOL_VERSION,
        shm_layout: baauto_protocol::shm::LAYOUT_VERSION,
    })?;
    let result = (|| -> Result<()> {
        loop {
            window.pump();
            let now = clock.now();
            if console.signaled() {
                quitting = true;
                wanted = false;
            }
            if window.take_stop() {
                wanted = false;
                let effects = fsm.on(FsmEvent::RequestStop {
                    reason: StopReason::Emergency,
                    cmd_seq: next_seq(&mut seq)?,
                    now,
                });
                execute_effects(
                    effects,
                    &mut child,
                    &options,
                    &mut seq,
                    &mut wanted,
                    &mut restart,
                    &mut dump_workers,
                )?;
            }
            for _ in 0..32 {
                let Ok(command) = commands.try_recv() else {
                    break;
                };
                match command {
                    SupervisorCommand::Enable => wanted = true,
                    SupervisorCommand::Disable => {
                        wanted = false;
                        let effects = fsm.on(FsmEvent::RequestStop {
                            reason: StopReason::User,
                            cmd_seq: next_seq(&mut seq)?,
                            now,
                        });
                        execute_effects(
                            effects,
                            &mut child,
                            &options,
                            &mut seq,
                            &mut wanted,
                            &mut restart,
                            &mut dump_workers,
                        )?;
                    }
                    SupervisorCommand::Apply {
                        cfg,
                        stop_hotkey,
                        record_hotkey,
                    } => {
                        if cfg.version.0 <= options.cfg.version.0 {
                            return Err(failure("configuration version must increase"));
                        }
                        pending_cfg = Some((cfg, stop_hotkey, record_hotkey));
                        released = false;
                        let effects = fsm.on(FsmEvent::RequestStop {
                            reason: StopReason::ConfigChange,
                            cmd_seq: next_seq(&mut seq)?,
                            now,
                        });
                        execute_effects(
                            effects,
                            &mut child,
                            &options,
                            &mut seq,
                            &mut wanted,
                            &mut restart,
                            &mut dump_workers,
                        )?;
                    }
                    SupervisorCommand::Rehook => {
                        wanted = false;
                        let effects = fsm.on(FsmEvent::RequestRehook);
                        execute_effects(
                            effects,
                            &mut child,
                            &options,
                            &mut seq,
                            &mut wanted,
                            &mut restart,
                            &mut dump_workers,
                        )?;
                    }
                    SupervisorCommand::Quit => {
                        quitting = true;
                        wanted = false;
                    }
                }
            }
            if quitting && matches!(fsm.state(), State::Armed(_) | State::AwaitArm { .. }) {
                let effects = fsm.on(FsmEvent::RequestStop {
                    reason: StopReason::User,
                    cmd_seq: next_seq(&mut seq)?,
                    now,
                });
                execute_effects(
                    effects,
                    &mut child,
                    &options,
                    &mut seq,
                    &mut wanted,
                    &mut restart,
                    &mut dump_workers,
                )?;
            }
            let mut received = Vec::new();
            if let Some(c) = &child
                && let Some(io) = &c.io
            {
                for _ in 0..32 {
                    let Ok(message) = io.incoming.try_recv() else {
                        break;
                    };
                    received.push(message);
                }
            }
            for message in received {
                match message {
                    EngineToApp::HelloAck {
                        protocol,
                        shm_layout,
                        ..
                    } => {
                        handshake(protocol, shm_layout)
                            .map_err(|_| failure("engine protocol mismatch"))?;
                        fsm.on(FsmEvent::HelloAccepted);
                        child.as_ref().unwrap().send(AppToEngine::ApplyConfig {
                            cmd_seq: next_seq(&mut seq)?,
                            version: options.cfg.version,
                            compiled: Box::new(CompiledWire::from(&*options.cfg)),
                        })?;
                    }
                    EngineToApp::ConfigInstalled { version, .. }
                        if version == options.cfg.version =>
                    {
                        installed = true;
                        fsm.on(FsmEvent::Installed { version });
                    }
                    EngineToApp::KeysReleased {
                        session: s,
                        version,
                        ..
                    } if s == session && version == options.cfg.version => {
                        released = true;
                        fsm.on(FsmEvent::KeysReleased {
                            session: s,
                            version,
                        });
                    }
                    EngineToApp::Armed {
                        session: s,
                        cmd_seq,
                    } => {
                        fsm.on(FsmEvent::Armed {
                            session: s,
                            cmd_seq,
                            now,
                        });
                    }
                    EngineToApp::Stopped {
                        session: s,
                        cmd_seq,
                        reason,
                    } if s == session => {
                        if matches!(fsm.state(), State::Armed(_) | State::AwaitArm { .. }) {
                            if !matches!(
                                reason,
                                StopReason::FocusLost | StopReason::GeometryChanged
                            ) {
                                wanted = false;
                            }
                            let effects = fsm.on(FsmEvent::RequestStop {
                                reason,
                                cmd_seq: next_seq(&mut seq)?,
                                now,
                            });
                            execute_effects(
                                effects,
                                &mut child,
                                &options,
                                &mut seq,
                                &mut wanted,
                                &mut restart,
                                &mut dump_workers,
                            )?;
                        } else {
                            fsm.on(FsmEvent::Stopped {
                                session: s,
                                cmd_seq,
                            });
                        }
                        released = false;
                    }
                    EngineToApp::Faulted { session: s, .. } if s == session => {
                        wanted = false;
                        let effects = fsm.on(FsmEvent::RequestStop {
                            reason: StopReason::SupervisorLost,
                            cmd_seq: next_seq(&mut seq)?,
                            now,
                        });
                        execute_effects(
                            effects,
                            &mut child,
                            &options,
                            &mut seq,
                            &mut wanted,
                            &mut restart,
                            &mut dump_workers,
                        )?;
                    }
                    EngineToApp::Rejected {
                        session: s,
                        cmd_seq,
                        why,
                    } if s == session => {
                        let _ = statuses.send(SupervisorStatus {
                            state: fsm.state(),
                            session,
                            detail: format!("command {cmd_seq} rejected: {why}"),
                            records: window.record_count(),
                        });
                        if matches!(fsm.state(), State::AwaitArm { .. }) {
                            wanted = false;
                            let effects = fsm.on(FsmEvent::RequestStop {
                                reason: StopReason::User,
                                cmd_seq: next_seq(&mut seq)?,
                                now,
                            });
                            execute_effects(
                                effects,
                                &mut child,
                                &options,
                                &mut seq,
                                &mut wanted,
                                &mut restart,
                                &mut dump_workers,
                            )?;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(c) = &child {
                if c.process.exited() {
                    wanted = false;
                    let effects = fsm.on(FsmEvent::ProcessExited {
                        header_valid: c.mapping.data().header.valid(session),
                        ledger_consistent: c.mapping.data().ledger.snapshot().is_some(),
                    });
                    execute_effects(
                        effects,
                        &mut child,
                        &options,
                        &mut seq,
                        &mut wanted,
                        &mut restart,
                        &mut dump_workers,
                    )?;
                    if fsm.state() == State::Recovering {
                        let effects = fsm.on(FsmEvent::ReleaseCompleted {
                            success: child.as_ref().unwrap().release_success,
                        });
                        execute_effects(
                            effects,
                            &mut child,
                            &options,
                            &mut seq,
                            &mut wanted,
                            &mut restart,
                            &mut dump_workers,
                        )?;
                    }
                } else if c.io.as_ref().is_some_and(|io| io.closed.signaled()) {
                    wanted = false;
                    let effects = fsm.on(FsmEvent::RequestStop {
                        reason: StopReason::SupervisorLost,
                        cmd_seq: next_seq(&mut seq)?,
                        now,
                    });
                    execute_effects(
                        effects,
                        &mut child,
                        &options,
                        &mut seq,
                        &mut wanted,
                        &mut restart,
                        &mut dump_workers,
                    )?;
                }
            }
            let target = target_for_foreground(&options.target)?;
            window.set_target(target.map_or(0, |t| t.hwnd));
            if let Some(c) = &child {
                let p = &c.mapping.data().progress;
                let effects = fsm.on(FsmEvent::Health {
                    now,
                    scheduler: p.scheduler.load(SeqCst),
                    hook_loop: p.hook_loop.load(SeqCst),
                    hook_physical: p.hook_bound_events.load(SeqCst),
                    raw_bound: window.raw_count(),
                    foreground: target.is_some(),
                });
                execute_effects(
                    effects,
                    &mut child,
                    &options,
                    &mut seq,
                    &mut wanted,
                    &mut restart,
                    &mut dump_workers,
                )?;
            }
            if fsm.requires_confirmation() && confirm_raw.is_none() {
                confirm_raw = Some((
                    window.raw_count(),
                    child.as_ref().map_or(0, |c| {
                        c.mapping.data().progress.hook_physical_events.load(SeqCst)
                    }),
                ));
            }
            if let Some(base) = confirm_raw
                && window.raw_count() > base.0
                && child.as_ref().is_some_and(|c| {
                    c.mapping.data().progress.hook_physical_events.load(SeqCst) > base.1
                })
            {
                fsm.on(FsmEvent::RehookConfirmed);
                confirm_raw = None;
            }
            if matches!(fsm.state(), State::AwaitArm { .. })
                && arm_deadline.is_some_and(|due| now >= due)
            {
                wanted = false;
                let effects = fsm.on(FsmEvent::RequestStop {
                    reason: StopReason::SupervisorLost,
                    cmd_seq: next_seq(&mut seq)?,
                    now,
                });
                execute_effects(
                    effects,
                    &mut child,
                    &options,
                    &mut seq,
                    &mut wanted,
                    &mut restart,
                    &mut dump_workers,
                )?;
            }
            let effects = fsm.on(FsmEvent::Tick { now });
            execute_effects(
                effects,
                &mut child,
                &options,
                &mut seq,
                &mut wanted,
                &mut restart,
                &mut dump_workers,
            )?;
            if restart {
                child.take();
                installed = false;
                released = false;
                restart = false;
                if quitting {
                    break;
                }
                child = Some(Child::launch(&options, session, clock)?);
                if fsm.requires_confirmation() {
                    confirm_raw = Some((window.raw_count(), 0));
                }
                handshake_deadline = now.after(ticks(options.health_timeout_ms.saturating_mul(2)));
                child.as_ref().unwrap().send(AppToEngine::Hello {
                    cmd_seq: next_seq(&mut seq)?,
                    protocol: PROTOCOL_VERSION,
                    shm_layout: baauto_protocol::shm::LAYOUT_VERSION,
                })?;
            }
            if !installed && now >= handshake_deadline {
                return Err(failure("engine configuration handshake timed out"));
            }
            if matches!(fsm.state(), State::Disarmed | State::Faulted)
                && let Some((cfg, stop_hotkey, record_hotkey)) = pending_cfg.take()
            {
                window.rebind(stop_hotkey, record_hotkey)?;
                options.stop_hotkey = stop_hotkey;
                options.record_hotkey = record_hotkey;
                options.cfg = cfg;
                installed = false;
                released = false;
                window.set_related(options.cfg.related_keys());
                handshake_deadline = now.after(ticks(options.health_timeout_ms.saturating_mul(2)));
                if let Some(c) = &mut child {
                    c.candidates = LedgerSnapshot::candidates(&options.cfg.candidates);
                    c.send(AppToEngine::ApplyConfig {
                        cmd_seq: next_seq(&mut seq)?,
                        version: options.cfg.version,
                        compiled: Box::new(CompiledWire::from(&*options.cfg)),
                    })?;
                }
            }
            if wanted
                && installed
                && released
                && target.is_some()
                && matches!(fsm.state(), State::Disarmed | State::Faulted)
            {
                let next = SessionId(
                    session
                        .0
                        .checked_add(1)
                        .ok_or_else(|| failure("session exhausted"))?,
                );
                let q = next_seq(&mut seq)?;
                let effects = fsm.on(FsmEvent::RequestArm {
                    session: next,
                    cmd_seq: q,
                });
                if !effects.is_empty() {
                    session = next;
                    arm_deadline = Some(now.after(ticks(options.health_timeout_ms)));
                    execute_effects(
                        effects,
                        &mut child,
                        &options,
                        &mut seq,
                        &mut wanted,
                        &mut restart,
                        &mut dump_workers,
                    )?;
                    released = false;
                }
            }
            let state = fsm.state();
            if last_status != Some(state) {
                last_status = Some(state);
                let detail = if target.is_none() {
                    "等待遊戲前景與按鍵放開"
                } else if target.is_some_and(|t| t.warning()) {
                    "遊戲不是全螢幕 16:9，點位和牌鍵會不準"
                } else {
                    ""
                };
                let _ = statuses.send(SupervisorStatus {
                    state,
                    session,
                    detail: detail.into(),
                    records: window.record_count(),
                });
            }
            if quitting && matches!(state, State::Disarmed | State::Faulted | State::Booting) {
                if let Some(c) = &child {
                    let _ = c.send(AppToEngine::Shutdown {
                        cmd_seq: next_seq(&mut seq)?,
                    });
                }
                break;
            }
            timer.set_deadline(
                clock.now().after(
                    u64::from(options.cfg.params.health_period_us) * clock.rate.0 / 1_000_000,
                ),
                clock,
            )?;
            window.wait(&[timer.raw()], INFINITE)?;
        }
        Ok(())
    })();
    drop(child);
    for worker in dump_workers {
        let _ = worker.join();
    }
    result
}
fn execute_effects(
    effects: Effects,
    child: &mut Option<Child>,
    options: &SupervisorOptions,
    seq: &mut u64,
    wanted: &mut bool,
    restart: &mut bool,
    dumps: &mut Vec<thread::JoinHandle<()>>,
) -> Result<()> {
    for effect in effects {
        let Some(c) = child.as_mut() else {
            continue;
        };
        match effect {
            Effect::Arm { session, cmd_seq } => {
                let stop = Arc::new(Event::new(true)?);
                let remote = c.process.duplicate_event(&stop)?;
                c.mapping.data().header.engine_state.store(0, SeqCst);
                c.mapping
                    .data()
                    .header
                    .config_version
                    .store(options.cfg.version.0, SeqCst);
                c.mapping.data().header.session.store(session.0, SeqCst);
                c.session = session;
                c.stop_request = None;
                c.exit_confirm = None;
                c.receipt = None;
                c.release_success = false;
                c.recovered = false;
                c.stop = Some(stop);
                c.send(AppToEngine::Arm {
                    cmd_seq,
                    session,
                    stop_event: remote,
                })?;
            }
            Effect::SetStop(_) => {
                c.stop_request.get_or_insert(c.clock.now().0);
                if let Some(stop) = &c.stop {
                    stop.set()?;
                }
            }
            Effect::SendStop {
                session,
                cmd_seq,
                reason,
            } => {
                *seq = (*seq).max(cmd_seq);
                let _ = c.send(AppToEngine::Stop {
                    cmd_seq,
                    session,
                    reason,
                });
            }
            Effect::Terminate => {
                c.process.terminate()?;
            }
            Effect::WaitExit => {} // Next health sample observes the process handle; no synchronous supervisor wait.
            Effect::ReleaseFromLedger | Effect::ReleaseCandidates => {
                *wanted = false;
                if !c.recover(effect == Effect::ReleaseCandidates)? {
                    eprintln!("recovery release failed; keep Faulted");
                }
            }
            Effect::DumpRing => {
                let map = c.mapping.clone();
                let folder = options.logs.clone();
                let id = c.session.0;
                let metadata = serde_json::json!({"session":id,"qpc_frequency":c.clock.rate.0,"stop_request":c.stop_request,"exit_confirm":c.exit_confirm,"release":c.receipt,"release_success":c.release_success,"ledger_keys":c.snapshot().unwrap_or(c.candidates).keys,"ledger_mouse":c.snapshot().unwrap_or(c.candidates).mouse});
                dumps.push(thread::spawn(move || {
                    if std::fs::create_dir_all(&folder).is_ok() {
                        let stamp = Clock::new().map_or(0, |clock| clock.now().0);
                        let _ = dump_ring(&map, &folder.join(format!("fault-{id}-{stamp}.csv")));
                        if let Ok(bytes) = serde_json::to_vec_pretty(&metadata) {
                            let _ = std::fs::write(
                                folder.join(format!("fault-{id}-{stamp}.json")),
                                bytes,
                            );
                        }
                    }
                }));
            }
            Effect::RestartDisarmed => *restart = true,
            Effect::Rehook => {
                c.send(AppToEngine::Rehook {
                    cmd_seq: next_seq(seq)?,
                })?;
            }
            Effect::ReportReleaseFailure => {
                eprintln!("recovery release failed; manual inspection required")
            }
            Effect::ReportReady => {}
        }
    }
    Ok(())
}
