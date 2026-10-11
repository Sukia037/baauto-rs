//! Managed Control owns configuration installation and all retired allocations.
use super::*;
use baauto_config::CompiledConfig;
use baauto_core::{GeometryVersion, SessionId, StopReason};
use baauto_protocol::ipc::*;
use rtrb::RingBuffer;
use std::{ffi::OsStr, sync::Arc};
use windows::{Win32::System::Threading::INFINITE, core::Result};
fn placeholder() -> Target {
    Target {
        hwnd: 0,
        pid: 0,
        monitor: crate::geometry::Rect {
            left: 0,
            top: 0,
            width: 1,
            height: 1,
        },
        window: crate::geometry::Rect {
            left: 0,
            top: 0,
            width: 1,
            height: 1,
        },
        geometry: GeometryVersion(0),
    }
}
pub fn run_managed(shm: &str, pipe: &str, parent_pid: u32, target_exe: &OsStr) -> Result<()> {
    let _ = set_dpi_awareness();
    let clock = Clock::new()?;
    let parent = Arc::new(Process::parent(parent_pid)?);
    let mapping = Arc::new(Mapping::open(shm)?);
    if mapping.data().header.qpc_frequency != clock.rate.0 {
        return Err(failure("QPC frequency mismatch"));
    }
    let io =
        PipePump::<AppToEngine, EngineToApp>::start(Arc::new(Pipe::client(pipe)?), parent.clone())?;
    let timer = Timer::new()?;
    let wake = Arc::new(Event::new(false)?);
    let marker = random_marker()?;
    let mut cfg: Option<Arc<CompiledConfig>> = None;
    let mut hook: Option<HookThread> = None;
    let mut consumer = None;
    let mut engine: Option<EngineThread> = None;
    let mut stop = Arc::new(Event::new(true)?);
    let mut pending = None;
    let mut session = SessionId(
        mapping
            .data()
            .header
            .session
            .load(std::sync::atomic::Ordering::SeqCst),
    );
    let mut last_cmd = 0;
    let mut hello = false;
    let mut stop_reply = None;
    let mut keys_notified = false;
    let mut geometry = 0u32;
    let mut shutdown = false;
    let result = (|| -> Result<()> {
        loop {
            #[cfg(feature = "fault-injection")]
            if crate::fault::PIPE_DROP.swap(false, std::sync::atomic::Ordering::SeqCst) {
                io.disconnect();
            }
            let lost = parent.exited() || io.closed.signaled();
            if lost {
                let _ = stop.set();
                shutdown = true;
            }
            if engine.as_ref().is_some_and(|e| e.done.signaled()) {
                let finished = engine.take().unwrap();
                let outcome = finished.join();
                if let Some(h) = &hook {
                    h.disarm()?;
                }
                match outcome {
                    Ok(input) => consumer = Some(input),
                    Err(error) => {
                        let _ = io.send(EngineToApp::Faulted {
                            session,
                            cmd_seq: last_cmd,
                            why: error.to_string(),
                        });
                        return Err(error);
                    }
                }
                if !lost {
                    let (cmd_seq, reason) = stop_reply.take().unwrap_or((
                        last_cmd,
                        stop_reason(
                            mapping
                                .data()
                                .progress
                                .stop_reason
                                .load(std::sync::atomic::Ordering::SeqCst),
                        ),
                    ));
                    io.send(EngineToApp::Stopped {
                        session,
                        cmd_seq,
                        reason,
                    })?;
                }
                keys_notified = false;
            }
            if shutdown && engine.is_none() {
                break;
            }
            for _ in 0..32 {
                let Ok(command) = io.incoming.try_recv() else {
                    break;
                };
                let seq = command.cmd_seq();
                if seq <= last_cmd {
                    io.send(EngineToApp::Rejected {
                        session,
                        cmd_seq: seq,
                        why: "stale cmd_seq".into(),
                    })?;
                    continue;
                }
                last_cmd = seq;
                match command {
                    AppToEngine::Hello {
                        protocol,
                        shm_layout,
                        ..
                    } => {
                        handshake(protocol, shm_layout)
                            .map_err(|_| failure("IPC version mismatch"))?;
                        hello = true;
                        io.send(EngineToApp::HelloAck {
                            session,
                            cmd_seq: seq,
                            protocol: PROTOCOL_VERSION,
                            shm_layout: baauto_protocol::shm::LAYOUT_VERSION,
                        })?;
                    }
                    _ if !hello => return Err(failure("Hello must be first")),
                    AppToEngine::ApplyConfig {
                        version, compiled, ..
                    } => {
                        let installed: CompiledConfig = (*compiled)
                            .try_into()
                            .map_err(|_| failure("invalid compiled config"))?;
                        if version != installed.version
                            || cfg.as_ref().is_some_and(|c| version.0 <= c.version.0)
                        {
                            io.send(EngineToApp::Rejected {
                                session,
                                cmd_seq: seq,
                                why: "stale configuration version".into(),
                            })?;
                            continue;
                        }
                        pending = Some((Arc::new(installed), seq));
                        keys_notified = false;
                        if engine.is_some() {
                            stop.set()?;
                            stop_reply = Some((seq, StopReason::ConfigChange));
                        }
                    }
                    AppToEngine::Arm {
                        session: new_session,
                        stop_event,
                        ..
                    } => {
                        let new_stop = Arc::new(Event::take_transferred(stop_event)?);
                        let target = target_for_foreground(target_exe)?;
                        if engine.is_some()
                            || pending.is_some()
                            || new_session.0 <= session.0
                            || !mapping.data().header.valid(new_session)
                            || target.is_none()
                            || cfg.is_none()
                            || hook.is_none()
                        {
                            io.send(EngineToApp::Rejected {
                                session: new_session,
                                cmd_seq: seq,
                                why: "Arm prerequisites not satisfied".into(),
                            })?;
                            continue;
                        }
                        let mut target = target.unwrap();
                        geometry = geometry
                            .checked_add(1)
                            .ok_or_else(|| failure("geometry version exhausted"))?;
                        target.geometry = GeometryVersion(geometry);
                        let current = cfg.as_ref().unwrap().clone();
                        let h = hook.as_ref().unwrap();
                        h.install(current.clone(), target)?;
                        if let Some(input) = consumer.as_mut() {
                            while input.pop().is_ok() {}
                        }
                        mapping
                            .data()
                            .progress
                            .stop_reason
                            .store(0, std::sync::atomic::Ordering::SeqCst);
                        if !h.arm(new_session, new_stop.clone())? {
                            io.send(EngineToApp::Rejected {
                                session: new_session,
                                cmd_seq: seq,
                                why: "release relevant keys before Arm".into(),
                            })?;
                            continue;
                        }
                        session = new_session;
                        stop = new_stop;
                        keys_notified = false;
                        let context = RuntimeContext {
                            cfg: current,
                            mapping: mapping.clone(),
                            stop: stop.clone(),
                            wake: wake.clone(),
                            target,
                            marker,
                            session,
                            clock,
                        };
                        engine = Some(EngineThread::spawn(
                            context,
                            consumer
                                .take()
                                .ok_or_else(|| failure("missing input consumer"))?,
                        )?);
                        io.send(EngineToApp::Armed {
                            session,
                            cmd_seq: seq,
                        })?;
                    }
                    AppToEngine::Stop {
                        session: requested,
                        reason,
                        ..
                    } => {
                        if requested != session {
                            io.send(EngineToApp::Rejected {
                                session: requested,
                                cmd_seq: seq,
                                why: "stale stop session".into(),
                            })?;
                            continue;
                        }
                        mapping
                            .data()
                            .progress
                            .stop_reason
                            .store(stop_code(reason), std::sync::atomic::Ordering::SeqCst);
                        stop.set()?;
                        stop_reply = Some((seq, reason));
                        if engine.is_none() {
                            if let Some(h) = &hook {
                                h.disarm()?;
                            }
                            io.send(EngineToApp::Stopped {
                                session,
                                cmd_seq: seq,
                                reason,
                            })?;
                            stop_reply = None;
                            keys_notified = false;
                        }
                    }
                    AppToEngine::Probe { .. } => {
                        if let Some(h) = &hook {
                            h.probe()?;
                        }
                        io.send(EngineToApp::Status {
                            session,
                            cmd_seq: seq,
                            state: mapping
                                .data()
                                .header
                                .engine_state
                                .load(std::sync::atomic::Ordering::SeqCst),
                            scheduler: mapping
                                .data()
                                .progress
                                .scheduler
                                .load(std::sync::atomic::Ordering::SeqCst),
                            hook_loop: mapping
                                .data()
                                .progress
                                .hook_loop
                                .load(std::sync::atomic::Ordering::SeqCst),
                            focused: target_for_foreground(target_exe)?.is_some(),
                        })?;
                    }
                    AppToEngine::Rehook { .. } => {
                        if engine.is_some()
                            || hook
                                .as_ref()
                                .is_some_and(|h| !h.keys_released().unwrap_or(false))
                        {
                            io.send(EngineToApp::Rejected {
                                session,
                                cmd_seq: seq,
                                why: "Rehook requires Disarmed".into(),
                            })?;
                            continue;
                        }
                        if let Some(h) = hook.take() {
                            h.shutdown()?;
                        }
                        consumer = None;
                        if let Some(c) = &cfg {
                            pending = Some((c.clone(), seq));
                        }
                    }
                    AppToEngine::Shutdown { .. } => {
                        stop.set()?;
                        shutdown = true;
                    }
                }
            }
            if engine.is_none()
                && !pending.as_ref().is_some_and(|(new_cfg, _)| {
                    cfg.as_ref().is_some_and(|old| {
                        old.params.queue_capacity != new_cfg.params.queue_capacity
                    }) && hook
                        .as_ref()
                        .is_some_and(|h| !h.keys_released().unwrap_or(false))
                })
                && let Some((new_cfg, seq)) = pending.take()
            {
                let target = target_for_foreground(target_exe)?.unwrap_or_else(placeholder);
                if cfg
                    .as_ref()
                    .is_some_and(|old| old.params.queue_capacity != new_cfg.params.queue_capacity)
                {
                    if let Some(h) = hook.take() {
                        h.shutdown()?;
                    }
                    consumer = None;
                }
                if let Some(h) = &hook {
                    h.install(new_cfg.clone(), target)?;
                } else {
                    let (producer, input) = RingBuffer::new(new_cfg.params.queue_capacity as usize);
                    let context = RuntimeContext {
                        cfg: new_cfg.clone(),
                        mapping: mapping.clone(),
                        stop: stop.clone(),
                        wake: wake.clone(),
                        target,
                        marker,
                        session,
                        clock,
                    };
                    hook = Some(HookThread::spawn(context, producer)?);
                    consumer = Some(input);
                }
                cfg = Some(new_cfg); // Old config is released here, on Control.
                io.send(EngineToApp::ConfigInstalled {
                    session,
                    cmd_seq: seq,
                    version: cfg.as_ref().unwrap().version,
                })?;
                keys_notified = false;
            }
            if engine.is_none()
                && !keys_notified
                && let (Some(h), Some(c)) = (&hook, &cfg)
                && h.keys_released()?
            {
                io.send(EngineToApp::KeysReleased {
                    session,
                    cmd_seq: last_cmd,
                    version: c.version,
                })?;
                keys_notified = true;
            }
            if !shutdown && let Some(h) = &hook {
                h.probe()?;
            }
            let period = cfg.as_ref().map_or(50_000, |c| c.params.health_period_us);
            timer.set_deadline(
                clock
                    .now()
                    .after(u64::from(period) * clock.rate.0 / 1_000_000),
                clock,
            )?;
            if shutdown && let Some(e) = &engine {
                wait(&[e.done.raw(), timer.raw()], INFINITE)?;
            } else if let Some(e) = &engine {
                wait(
                    &[parent.raw(), io.closed.raw(), e.done.raw(), timer.raw()],
                    INFINITE,
                )?;
            } else {
                wait(&[parent.raw(), io.closed.raw(), timer.raw()], INFINITE)?;
            }
        }
        Ok(())
    })();
    let _ = stop.set();
    if let Some(active) = engine.take() {
        let _ = active.join();
    }
    if let Some(h) = hook.take() {
        let _ = h.disarm();
        let _ = h.shutdown();
    }
    drop(cfg);
    drop(pending);
    drop(io);
    if parent.exited() {
        Err(failure("supervisor process exited"))
    } else {
        result
    }
}
