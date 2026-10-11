//! Pure supervisor decisions. Only ProcessExited can authorize recovery output.
#![forbid(unsafe_code)]
use arrayvec::ArrayVec;
use baauto_config::ConfigVersion;
use baauto_core::{SessionId, StopReason, Ticks};
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub health: u64,
    pub recovery: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Booting,
    Disarmed,
    AwaitArm {
        session: SessionId,
        cmd_seq: u64,
    },
    Armed(SessionId),
    Stopping {
        session: SessionId,
        cmd_seq: u64,
        deadline: Ticks,
        fault: bool,
    },
    WaitingExit,
    Recovering,
    Faulted,
}
#[derive(Clone, Copy, Debug)]
pub enum Event {
    HelloAccepted,
    Installed {
        version: ConfigVersion,
    },
    HotkeysRegistered(bool),
    KeysReleased {
        session: SessionId,
        version: ConfigVersion,
    },
    RequestArm {
        session: SessionId,
        cmd_seq: u64,
    },
    Armed {
        session: SessionId,
        cmd_seq: u64,
        now: Ticks,
    },
    RequestStop {
        reason: StopReason,
        cmd_seq: u64,
        now: Ticks,
    },
    Stopped {
        session: SessionId,
        cmd_seq: u64,
    },
    Tick {
        now: Ticks,
    },
    ProcessExited {
        header_valid: bool,
        ledger_consistent: bool,
    },
    ReleaseCompleted {
        success: bool,
    },
    Health {
        now: Ticks,
        scheduler: u64,
        hook_loop: u64,
        hook_physical: u64,
        raw_bound: u64,
        foreground: bool,
    },
    RequestRehook,
    RehookConfirmed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Arm {
        session: SessionId,
        cmd_seq: u64,
    },
    SetStop(SessionId),
    SendStop {
        session: SessionId,
        cmd_seq: u64,
        reason: StopReason,
    },
    Terminate,
    WaitExit,
    ReleaseFromLedger,
    ReleaseCandidates,
    DumpRing,
    RestartDisarmed,
    ReportReady,
    Rehook,
    ReportReleaseFailure,
}
pub type Effects = ArrayVec<Effect, 8>;
pub struct SupervisorFsm {
    state: State,
    timeouts: Timeouts,
    hello: bool,
    installed: Option<ConfigVersion>,
    keys: bool,
    hotkeys: bool,
    hook_confirmed: bool,
    release_failed: bool,
    session: SessionId,
    last_cmd: u64,
    scheduler: u64,
    hook_loop: u64,
    last_scheduler: Ticks,
    last_hook: Ticks,
    raw: u64,
    physical: u64,
    unmatched: Option<Ticks>,
    hook_credit: u64,
    raw_pending: u64,
}
impl SupervisorFsm {
    pub fn new(timeouts: Timeouts) -> Self {
        assert!(timeouts.health > 0 && timeouts.recovery > 0);
        Self {
            state: State::Booting,
            timeouts,
            hello: false,
            installed: None,
            keys: false,
            hotkeys: false,
            hook_confirmed: true,
            release_failed: false,
            session: SessionId(0),
            last_cmd: 0,
            scheduler: 0,
            hook_loop: 0,
            last_scheduler: Ticks(0),
            last_hook: Ticks(0),
            raw: 0,
            physical: 0,
            unmatched: None,
            hook_credit: 0,
            raw_pending: 0,
        }
    }
    pub fn requires_confirmation(&self) -> bool {
        !self.hook_confirmed
    }
    pub fn state(&self) -> State {
        self.state
    }
    fn conditions(&self) -> bool {
        self.hello
            && self.installed.is_some()
            && self.keys
            && self.hotkeys
            && self.hook_confirmed
            && !self.release_failed
    }
    pub fn ready(&self) -> bool {
        self.state == State::Disarmed && self.conditions()
    }
    fn stop(&mut self, reason: StopReason, cmd_seq: u64, now: Ticks, effects: &mut Effects) {
        if !matches!(self.state, State::Armed(_) | State::AwaitArm { .. })
            || cmd_seq <= self.last_cmd
        {
            return;
        }
        self.last_cmd = cmd_seq;
        let fault = !matches!(
            reason,
            StopReason::User
                | StopReason::Emergency
                | StopReason::FocusLost
                | StopReason::GeometryChanged
                | StopReason::ConfigChange
        );
        self.state = State::Stopping {
            session: self.session,
            cmd_seq,
            deadline: now.after(self.timeouts.recovery),
            fault,
        };
        self.keys = false;
        effects.push(Effect::SetStop(self.session));
        effects.push(Effect::SendStop {
            session: self.session,
            cmd_seq,
            reason,
        });
    }
    pub fn on(&mut self, event: Event) -> Effects {
        let was_ready = self.ready();
        let mut effects = Effects::new();
        match event {
            Event::HelloAccepted if matches!(self.state, State::Booting | State::Faulted) => {
                self.hello = true;
                if self.state == State::Booting {
                    self.state = State::Disarmed;
                }
            }
            Event::Installed { version }
                if matches!(self.state, State::Disarmed | State::Faulted) =>
            {
                self.installed = Some(version);
                self.keys = false;
            }
            Event::HotkeysRegistered(ok) => {
                self.hotkeys = ok;
            }
            Event::KeysReleased { session, version }
                if session == self.session && Some(version) == self.installed =>
            {
                self.keys = true
            }
            Event::RequestArm { session, cmd_seq }
                if matches!(self.state, State::Disarmed | State::Faulted)
                    && self.conditions()
                    && session.0 > self.session.0
                    && cmd_seq > self.last_cmd =>
            {
                self.session = session;
                self.last_cmd = cmd_seq;
                self.keys = false;
                self.state = State::AwaitArm { session, cmd_seq };
                effects.push(Effect::Arm { session, cmd_seq });
            }
            Event::Armed {
                session,
                cmd_seq,
                now,
            } if self.state == (State::AwaitArm { session, cmd_seq }) => {
                self.state = State::Armed(session);
                self.last_scheduler = now;
                self.last_hook = now;
                self.unmatched = None;
            }
            Event::RequestStop {
                reason,
                cmd_seq,
                now,
            } => self.stop(reason, cmd_seq, now, &mut effects),
            Event::Stopped { session, cmd_seq } => {
                if let State::Stopping {
                    session: s,
                    cmd_seq: q,
                    fault: false,
                    ..
                } = self.state
                    && session == s
                    && cmd_seq == q
                {
                    self.state = State::Disarmed;
                }
            }
            Event::Tick { now } => {
                if let State::Stopping { deadline, .. } = self.state
                    && now >= deadline
                {
                    self.state = State::WaitingExit;
                    effects.push(Effect::Terminate);
                    effects.push(Effect::WaitExit);
                }
            }
            Event::ProcessExited {
                header_valid,
                ledger_consistent,
            } if !matches!(self.state, State::Recovering | State::Faulted) => {
                self.state = State::Recovering;
                self.keys = false;
                effects.push(if header_valid && ledger_consistent {
                    Effect::ReleaseFromLedger
                } else {
                    Effect::ReleaseCandidates
                });
            }
            Event::ReleaseCompleted { success } if self.state == State::Recovering => {
                self.state = State::Faulted;
                self.hello = false;
                self.installed = None;
                self.keys = false;
                self.release_failed = !success;
                if !success {
                    effects.push(Effect::ReportReleaseFailure);
                }
                effects.push(Effect::DumpRing);
                effects.push(Effect::RestartDisarmed);
            }
            Event::Health {
                now,
                scheduler,
                hook_loop,
                hook_physical,
                raw_bound,
                foreground,
            } if matches!(self.state, State::Armed(_)) => {
                if scheduler != self.scheduler {
                    self.scheduler = scheduler;
                    self.last_scheduler = now;
                }
                if hook_loop != self.hook_loop {
                    self.hook_loop = hook_loop;
                    self.last_hook = now;
                }
                if !foreground || hook_physical < self.physical || raw_bound < self.raw {
                    self.hook_credit = 0;
                    self.raw_pending = 0;
                    self.unmatched = None;
                } else {
                    self.hook_credit = self
                        .hook_credit
                        .saturating_add(hook_physical - self.physical);
                    self.raw_pending = self.raw_pending.saturating_add(raw_bound - self.raw);
                    let matched = self.hook_credit.min(self.raw_pending);
                    self.hook_credit -= matched;
                    self.raw_pending -= matched;
                    if self.raw_pending == 0 {
                        self.unmatched = None;
                    } else if self.unmatched.is_none() {
                        self.unmatched = Some(now);
                    }
                }
                self.physical = hook_physical;
                self.raw = raw_bound;
                let removed = self
                    .unmatched
                    .is_some_and(|at| now.0.saturating_sub(at.0) >= self.timeouts.health);
                if removed
                    || now.0.saturating_sub(self.last_scheduler.0) >= self.timeouts.health
                    || now.0.saturating_sub(self.last_hook.0) >= self.timeouts.health
                {
                    let seq = self.last_cmd.checked_add(1).unwrap_or(self.last_cmd);
                    self.stop(StopReason::SupervisorLost, seq, now, &mut effects);
                    if removed {
                        self.hook_confirmed = false;
                    }
                }
            }
            Event::RequestRehook if self.state == State::Disarmed => {
                self.hook_confirmed = false;
                self.keys = false;
                effects.push(Effect::Rehook);
            }
            Event::RehookConfirmed if matches!(self.state, State::Disarmed | State::Faulted) => {
                self.hook_confirmed = true
            }
            _ => {}
        }
        if !was_ready && self.ready() {
            effects.push(Effect::ReportReady);
        }
        effects
    }
}
