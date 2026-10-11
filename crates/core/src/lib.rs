#![forbid(unsafe_code)]
pub mod sim;
use arrayvec::ArrayVec;
use baauto_config::{
    ActionSpec, AutoRepeatPolicy, CompiledConfig, NO_TRIGGER, TickTiming, TrailingEscPolicy,
    slot_at,
};
pub use baauto_config::{ConfigVersion, Key, NormPoint, TickRate, Ticks};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PressId(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeometryVersion(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionId {
    pub slot: u16,
    pub generation: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FaultKind {
    ShortSend,
    InvalidCommit,
    TimeOverflow,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopReason {
    User,
    Emergency,
    FocusLost,
    GeometryChanged,
    Overflow,
    SeqGap,
    ConfigChange,
    PhysicalConflict,
    SupervisorLost,
    Fault(FaultKind),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineState {
    Disarmed,
    Armed(SessionId),
    Running(SessionId),
    Stopping(SessionId, StopReason),
    Stopped(SessionId),
    Faulted(SessionId, FaultKind),
}
#[derive(Clone, Copy, Debug)]
pub enum InputKind {
    TriggerDown {
        press: PressId,
        trigger: u16,
        cursor: NormPoint,
    },
    TriggerRepeat {
        press: PressId,
        cursor: NormPoint,
    },
    TriggerUp {
        press: PressId,
    },
    PressCancelled {
        press: PressId,
    },
    Focus {
        focused: bool,
    },
    GeometryChanged,
    PhysicalConflict {
        key: OutKey,
    },
}
#[derive(Clone, Copy, Debug)]
pub struct InputEvent {
    pub seq: u64,
    pub at: Ticks,
    pub session: SessionId,
    pub config: ConfigVersion,
    pub geometry: GeometryVersion,
    pub kind: InputKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKey {
    Key(Key),
    LeftButton,
}
impl OutKey {
    fn index(self) -> usize {
        match self {
            Self::Key(k) => k.0 as usize,
            Self::LeftButton => 256,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutEvent {
    KeyDown(Key),
    KeyUp(Key),
    MoveTo(NormPoint),
    LeftDown,
    LeftUp,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Batch {
    pub id: u64,
    pub session: SessionId,
    pub action: Option<ActionId>,
    pub stage: u32,
    pub deadline: Ticks,
    pub events: ArrayVec<OutEvent, 257>,
}
#[derive(Clone, Copy, Debug)]
pub enum SubmitOutcome {
    Ok { begin: Ticks, end: Ticks },
    Short { inserted: u16, end: Ticks },
    NotSubmitted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmError {
    NotDisarmed,
    StaleSession,
    HeldOutputs,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Report {
    ResourceBusy,
    OutsideHand,
    ConfirmPause,
    Capacity,
    StaleVersion,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub scheduler_progress: u64,
    pub action_progress: u64,
    pub skipped_cycles: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldSet([bool; 257]);
impl HeldSet {
    pub fn contains(&self, k: OutKey) -> bool {
        self.0[k.index()]
    }
    pub fn is_empty(&self) -> bool {
        !self.0.iter().any(|b| *b)
    }
    pub fn keys(&self) -> impl Iterator<Item = OutKey> + '_ {
        self.0.iter().enumerate().filter_map(|(i, b)| {
            if *b {
                Some(if i == 256 {
                    OutKey::LeftButton
                } else {
                    OutKey::Key(Key(i as u8))
                })
            } else {
                None
            }
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Start,
    TapUp,
    EscUp,
    Gap,
    SecondUp,
    WaitCycle,
    RepeatKeyUp,
    ClickDown,
    ClickUp,
    Maintain,
    Clean,
    TrailingDown,
    TrailingUp,
    /// Card key: move the cursor back to the user after the target click.
    Return,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunMode {
    Normal,
    Calibration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrialError {
    NormalSession,
    Disarmed,
    Busy,
    InvalidSpec,
}
#[derive(Clone, Copy, Debug)]
struct Action {
    id: ActionId,
    press: PressId,
    spec: ActionSpec,
    timing: TickTiming,
    trial: u64,
    stop_at: Option<Ticks>,
    phase: Phase,
    due: Ticks,
    key: Option<Key>,
    mouse: bool,
    key_reserved: bool,
    cycle_start: Ticks,
    card_started: bool,
    trailing: bool,
    /// Card key moved the cursor to its target and has not returned it yet.
    displaced: bool,
    return_due: Option<Ticks>,
    off_hand: bool,
}
impl Action {
    fn next_due(&self) -> Ticks {
        self.return_due.map_or(self.due, |r| r.min(self.due))
    }
}
#[derive(Clone, Copy)]
struct PressBinding {
    press: PressId,
    trigger: u16,
}
struct Pending {
    batch: Batch,
}
pub struct Engine {
    calibration: bool,
    cfg: Arc<CompiledConfig>,
    rate: TickRate,
    state: EngineState,
    session: SessionId,
    last_session: u32,
    seq: u64,
    slots: Vec<Option<Action>>,
    generations: Vec<u32>,
    bindings: Vec<Option<PressBinding>>,
    held: HeldSet,
    mouse: Option<ActionId>,
    esc: Option<ActionId>,
    pending: Option<Pending>,
    next_id: u64,
    health: Ticks,
    progress: Progress,
    reports: ArrayVec<Report, 128>,
    fair_slot: usize,
    user_cursor: Option<NormPoint>,
}
impl Engine {
    pub fn new(cfg: Arc<CompiledConfig>, rate: TickRate) -> Self {
        let n = usize::from(cfg.params.max_actions);
        Self {
            calibration: false,
            cfg,
            rate,
            state: EngineState::Disarmed,
            session: SessionId(0),
            last_session: 0,
            seq: 0,
            slots: vec![None; n],
            generations: vec![0; n],
            bindings: vec![None; 256],
            held: HeldSet([false; 257]),
            mouse: None,
            esc: None,
            pending: None,
            next_id: 0,
            health: Ticks(0),
            progress: Progress::default(),
            reports: ArrayVec::new(),
            fair_slot: 0,
            user_cursor: None,
        }
    }
    pub fn new_calibration(cfg: Arc<CompiledConfig>, rate: TickRate) -> Self {
        let mut engine = Self::new(cfg, rate);
        engine.calibration = true;
        engine
    }
    pub fn trial_for_action(&self, id: ActionId) -> u64 {
        self.slots
            .get(id.slot as usize)
            .and_then(|a| a.filter(|a| a.id == id))
            .map_or(0, |a| a.trial)
    }
    pub fn run_trial(
        &mut self,
        id: u64,
        spec: ActionSpec,
        timing: TickTiming,
        start: Ticks,
        duration: u64,
        cursor: NormPoint,
    ) -> Result<ActionId, TrialError> {
        if !self.calibration {
            return Err(TrialError::NormalSession);
        }
        if !self.running() {
            return Err(TrialError::Disarmed);
        }
        if self.active_actions() != 0 || self.pending.is_some() {
            return Err(TrialError::Busy);
        }
        if id == 0
            || duration == 0
            || start.0.checked_add(duration).is_none()
            || !cursor.valid()
            || timing.repeat_period == 0
            || timing.maintain == 0
            || matches!(spec, ActionSpec::Repeat { .. } | ActionSpec::Card(_))
                && timing.repeat_period
                    < timing
                        .key_hold
                        .saturating_add(timing.key_to_click)
                        .saturating_add(timing.mouse_hold)
            || matches!(spec,ActionSpec::HoldPoint(p) | ActionSpec::CardKey(p) if !p.valid())
            || matches!(spec,ActionSpec::Tap(k)|ActionSpec::Card(k)|ActionSpec::Repeat{key:Some(k)} if k.0==0||k.is_mouse())
        {
            return Err(TrialError::InvalidSpec);
        }
        // A card key trial treats the trial cursor as the user's hand position.
        self.user_cursor = Some(cursor);
        let action = self
            .start_spec(PressId(0), spec, start)
            .ok_or(TrialError::Busy)?;
        let a = self.slots[action.slot as usize].as_mut().unwrap();
        a.timing = timing;
        a.trial = id;
        a.stop_at = Some(start.after(duration));
        Ok(action)
    }
    pub fn config(&self) -> &Arc<CompiledConfig> {
        &self.cfg
    }
    pub fn arm(&mut self, session: SessionId, now: Ticks) -> Result<(), ArmError> {
        if self.state != EngineState::Disarmed {
            return Err(ArmError::NotDisarmed);
        }
        if session.0 <= self.last_session {
            return Err(ArmError::StaleSession);
        }
        if !self.held.is_empty() {
            return Err(ArmError::HeldOutputs);
        }
        self.last_session = session.0;
        self.session = session;
        self.seq = 0;
        self.state = EngineState::Armed(session);
        self.health = now.after(self.health_period());
        self.bindings.fill(None);
        Ok(())
    }
    pub fn disarm(&mut self) -> Result<(), ArmError> {
        if !matches!(self.state, EngineState::Stopped(_) | EngineState::Disarmed)
            || !self.held.is_empty()
            || self.pending.is_some()
        {
            return Err(ArmError::NotDisarmed);
        }
        self.state = EngineState::Disarmed;
        self.slots.fill(None);
        self.bindings.fill(None);
        self.mouse = None;
        self.esc = None;
        Ok(())
    }
    pub fn state(&self) -> EngineState {
        self.state
    }
    pub fn held(&self) -> HeldSet {
        self.held
    }
    pub fn progress(&self) -> Progress {
        self.progress
    }
    pub fn actions_for_press(&self, press: PressId) -> impl Iterator<Item = ActionId> + '_ {
        self.slots
            .iter()
            .flatten()
            .filter(move |a| a.press == press)
            .map(|a| a.id)
    }
    pub fn active_actions(&self) -> usize {
        self.slots.iter().filter(|a| a.is_some()).count()
    }
    pub fn reports(&self) -> &[Report] {
        &self.reports
    }
    pub fn drain_reports(&mut self, out: &mut impl FnMut(Report)) {
        for r in self.reports.drain(..) {
            out(r);
        }
    }
    /// Where the user's own mouse is, excluding cursor moves made by card keys.
    pub fn set_user_cursor(&mut self, cursor: Option<NormPoint>) {
        self.user_cursor = cursor;
    }
    /// True while a card key holds the cursor on its target; the user position must then be tracked.
    pub fn pointer_displaced(&self) -> bool {
        self.slots.iter().flatten().any(|a| a.displaced)
    }
    fn report(&mut self, r: Report) {
        if self.reports.is_full() {
            self.reports.remove(0);
        }
        self.reports.push(r);
    }
    fn health_period(&self) -> u64 {
        (u128::from(self.cfg.params.health_period_us) * u128::from(self.rate.0))
            .div_ceil(1_000_000)
            .max(1)
            .min(u128::from(u64::MAX)) as u64
    }
    fn running(&self) -> bool {
        matches!(self.state, EngineState::Armed(_) | EngineState::Running(_))
    }
    pub fn on_input(&mut self, ev: InputEvent, now: Ticks) {
        if ev.session != self.session || !self.running() {
            return;
        }
        if ev.config != self.cfg.version {
            self.report(Report::StaleVersion);
            self.request_stop(StopReason::ConfigChange, now);
            return;
        }
        if self.seq.checked_add(1) != Some(ev.seq) {
            self.request_stop(StopReason::SeqGap, now);
            return;
        }
        self.seq = ev.seq;
        match ev.kind {
            InputKind::TriggerDown { press, trigger, .. } => {
                if self.bindings.iter().flatten().any(|b| b.press == press) {
                    return;
                }
                let Some(i) = self.bindings.iter().position(Option::is_none) else {
                    self.report(Report::Capacity);
                    self.request_stop(StopReason::Overflow, now);
                    return;
                };
                self.bindings[i] = Some(PressBinding { press, trigger });
                self.start(press, trigger, now);
            }
            InputKind::TriggerRepeat { press, .. } => {
                if let Some(binding) = self
                    .bindings
                    .iter()
                    .flatten()
                    .find(|b| b.press == press)
                    .copied()
                    && let Some(spec) = self.cfg.actions.get(binding.trigger as usize)
                    && self.cfg.timing.auto_repeat == AutoRepeatPolicy::FollowTypematic
                    && matches!(spec, ActionSpec::Tap(_))
                {
                    self.start(press, binding.trigger, now);
                }
            }
            InputKind::TriggerUp { press } => self.cancel(press, now, false),
            InputKind::PressCancelled { press } => self.cancel(press, now, true),
            InputKind::Focus { focused: false } => self.request_stop(StopReason::FocusLost, now),
            InputKind::GeometryChanged => {
                for a in self.slots.iter_mut().flatten() {
                    if matches!(a.spec, ActionSpec::HoldPoint(_)) {
                        a.phase = Phase::Clean;
                        a.due = now;
                    }
                }
            }
            InputKind::PhysicalConflict { key } => {
                if self.held.contains(key) {
                    self.request_stop(StopReason::PhysicalConflict, now);
                }
            }
            InputKind::Focus { focused: true } => {}
        }
    }
    fn key_busy(&self, key: Key, except: Option<ActionId>) -> bool {
        self.slots
            .iter()
            .flatten()
            .any(|a| Some(a.id) != except && a.key_reserved && a.key == Some(key))
    }
    fn start(&mut self, press: PressId, trigger: u16, now: Ticks) {
        if trigger == NO_TRIGGER {
            return;
        }
        let Some(spec) = self.cfg.actions.get(trigger as usize).copied() else {
            return;
        };
        self.start_spec(press, spec, now);
    }
    fn start_spec(&mut self, press: PressId, spec: ActionSpec, now: Ticks) -> Option<ActionId> {
        let key = match spec {
            ActionSpec::Tap(k) | ActionSpec::Card(k) => Some(k),
            ActionSpec::Repeat { key } => key,
            ActionSpec::DoubleEsc { .. } => Some(Key::ESC),
            // The slot key is chosen at the start of every cycle from the user's cursor.
            ActionSpec::CardKey(_) | ActionSpec::HoldPoint(_) => None,
        };
        let needs_esc = matches!(spec, ActionSpec::Card(_) | ActionSpec::DoubleEsc { .. })
            || key == Some(Key::ESC);
        let reserve_key = matches!(spec, ActionSpec::Tap(_) | ActionSpec::DoubleEsc { .. });
        if (needs_esc && self.esc.is_some())
            || (reserve_key && key.is_some_and(|k| self.key_busy(k, None)))
            || (matches!(spec, ActionSpec::HoldPoint(_) | ActionSpec::Card(_))
                && self.mouse.is_some())
        {
            self.report(Report::ResourceBusy);
            return None;
        }
        let Some(i) = self.slots.iter().position(Option::is_none) else {
            self.report(Report::Capacity);
            return None;
        };
        let Some(generation) = self.generations[i].checked_add(1) else {
            self.request_stop(StopReason::Overflow, now);
            return None;
        };
        self.generations[i] = generation;
        let id = ActionId {
            slot: i as u16,
            generation,
        };
        if needs_esc {
            self.esc = Some(id);
        }
        let mouse = matches!(spec, ActionSpec::HoldPoint(_));
        if mouse {
            self.mouse = Some(id);
        }
        self.slots[i] = Some(Action {
            id,
            press,
            spec,
            timing: self.cfg.timing,
            trial: 0,
            stop_at: None,
            phase: if matches!(spec, ActionSpec::Repeat { .. } | ActionSpec::CardKey(_)) {
                Phase::WaitCycle
            } else {
                Phase::Start
            },
            due: now,
            key,
            mouse,
            key_reserved: reserve_key,
            cycle_start: now,
            card_started: false,
            trailing: false,
            displaced: false,
            return_due: None,
            off_hand: false,
        });
        self.state = EngineState::Running(self.session);
        Some(id)
    }
    fn cancel(&mut self, press: PressId, now: Ticks, forced: bool) {
        for b in &mut self.bindings {
            if b.is_some_and(|b| b.press == press) {
                *b = None;
            }
        }
        for a in self.slots.iter_mut().flatten() {
            if a.press == press
                && (forced || !matches!(a.spec, ActionSpec::Tap(_) | ActionSpec::DoubleEsc { .. }))
            {
                a.trailing = !forced
                    && matches!(a.spec, ActionSpec::Card(_))
                    && a.timing.trailing == TrailingEscPolicy::IfStarted;
                a.phase = Phase::Clean;
                a.due = now;
            }
        }
    }
    pub fn request_stop(&mut self, reason: StopReason, now: Ticks) {
        if !self.running() {
            return;
        }
        if self
            .slots
            .iter()
            .flatten()
            .any(|a| matches!(a.spec, ActionSpec::Card(_)))
        {
            self.report(Report::ConfirmPause);
        }
        self.state = EngineState::Stopping(self.session, reason);
        for a in self.slots.iter_mut().flatten() {
            a.phase = Phase::Clean;
            a.due = now;
            a.trailing = false;
        }
    }
    fn finish(&mut self, i: usize) {
        if let Some(a) = self.slots[i].take() {
            if self.mouse == Some(a.id) {
                self.mouse = None;
            }
            if self.esc == Some(a.id) {
                self.esc = None;
            }
        }
        if self.running() && self.active_actions() == 0 {
            self.state = EngineState::Armed(self.session);
        }
    }
    fn batch(
        &mut self,
        action: Option<ActionId>,
        stage: u32,
        deadline: Ticks,
        events: ArrayVec<OutEvent, 257>,
    ) -> Option<Batch> {
        let id = self.next_id.checked_add(1)?;
        self.next_id = id;
        let b = Batch {
            id,
            session: self.session,
            action,
            stage,
            deadline,
            events,
        };
        self.pending = Some(Pending { batch: b.clone() });
        Some(b)
    }
    pub fn next_batch(&mut self, now: Ticks) -> Option<Batch> {
        self.progress.scheduler_progress = self.progress.scheduler_progress.saturating_add(1);
        if now >= self.health {
            self.health = now.after(self.health_period());
        }
        if self.pending.is_some() {
            return None;
        }
        if matches!(self.state, EngineState::Stopping(..)) {
            let mut events = ArrayVec::new();
            for k in self.held.keys() {
                events.push(match k {
                    OutKey::Key(k) => OutEvent::KeyUp(k),
                    OutKey::LeftButton => OutEvent::LeftUp,
                });
            }
            if events.is_empty() {
                self.slots.fill(None);
                self.mouse = None;
                self.esc = None;
                self.state = EngineState::Stopped(self.session);
                return None;
            }
            return self.batch(None, Phase::Clean as u32, now, events);
        }
        if !self.running() {
            return None;
        }
        for i in 0..self.slots.len() {
            if let Some(a) = self.slots[i]
                && a.stop_at.is_some_and(|due| now >= due)
            {
                self.slots[i].as_mut().unwrap().stop_at = None;
                self.cancel(a.press, now, false);
            }
        }
        // Scan a bounded number of preallocated slots. Resource waiters do not monopolize the loop.
        let mut inspected = [false; 256];
        for _ in 0..self.slots.len() {
            let i = self
                .slots
                .iter()
                .enumerate()
                .filter(|(i, a)| !inspected[*i] && a.is_some_and(|a| a.next_due() <= now))
                .min_by_key(|(i, a)| {
                    let a = a.unwrap();
                    (
                        a.next_due(),
                        if a.phase == Phase::WaitCycle {
                            (*i + self.slots.len() - self.fair_slot) % self.slots.len()
                        } else {
                            *i
                        },
                    )
                })
                .map(|(i, _)| i)?;
            inspected[i] = true;
            let mut a = self.slots[i].unwrap();
            let mut events = ArrayVec::new();
            if let Some(due) = a.return_due.filter(|r| *r <= now && *r <= a.due) {
                a.return_due = None;
                a.displaced = false;
                self.slots[i] = Some(a);
                // Without a known user position the cursor simply stays on the target.
                let Some(user) = self.user_cursor.filter(|c| c.valid()) else {
                    continue;
                };
                events.push(OutEvent::MoveTo(user));
                return self.batch(Some(a.id), Phase::Return as u32, due, events);
            }
            if a.phase == Phase::WaitCycle {
                if matches!(a.spec, ActionSpec::CardKey(_)) {
                    let slot = self
                        .user_cursor
                        .and_then(|c| slot_at(&self.cfg.hand, self.cfg.slots, self.cfg.order, c));
                    let Some(slot) = slot else {
                        if !a.off_hand {
                            self.report(Report::OutsideHand);
                        }
                        a.off_hand = true;
                        a.due = now.after(a.timing.repeat_period);
                        self.slots[i] = Some(a);
                        continue;
                    };
                    a.off_hand = false;
                    a.key = Some(self.cfg.slot_keys[slot as usize]);
                }
                if self.mouse.is_some_and(|m| m != a.id)
                    || a.key.is_some_and(|k| self.key_busy(k, Some(a.id)))
                {
                    self.slots[i] = Some(a);
                    continue;
                }
                self.mouse = Some(a.id);
                a.mouse = true;
                a.key_reserved = a.key.is_some();
                self.fair_slot = (i + 1) % self.slots.len();
                if now.0 > a.due.0 {
                    let skipped = (now.0 - a.due.0) / a.timing.repeat_period;
                    self.progress.skipped_cycles =
                        self.progress.skipped_cycles.saturating_add(skipped);
                }
                a.cycle_start = now;
                events.push(a.key.map_or(OutEvent::LeftDown, OutEvent::KeyDown));
            } else {
                match a.phase {
                    Phase::Start => match a.spec {
                        ActionSpec::Tap(_) => events.push(OutEvent::KeyDown(a.key.unwrap())),
                        ActionSpec::Card(_) | ActionSpec::DoubleEsc { .. } => {
                            events.push(OutEvent::KeyDown(Key::ESC))
                        }
                        ActionSpec::HoldPoint(p) => {
                            events.push(OutEvent::MoveTo(p));
                            events.push(OutEvent::LeftDown);
                        }
                        ActionSpec::Repeat { .. } | ActionSpec::CardKey(_) => unreachable!(),
                    },
                    Phase::TapUp | Phase::RepeatKeyUp => {
                        events.push(OutEvent::KeyUp(a.key.unwrap()))
                    }
                    Phase::EscUp | Phase::SecondUp | Phase::TrailingUp => {
                        events.push(OutEvent::KeyUp(Key::ESC))
                    }
                    Phase::Gap => events.push(OutEvent::KeyDown(Key::ESC)),
                    Phase::ClickDown => {
                        if let ActionSpec::CardKey(target) = a.spec {
                            // Moving to the target supersedes any return still pending.
                            a.displaced = true;
                            a.return_due = None;
                            events.push(OutEvent::MoveTo(target));
                        }
                        events.push(OutEvent::LeftDown);
                    }
                    Phase::ClickUp => events.push(OutEvent::LeftUp),
                    Phase::Maintain => {
                        if let ActionSpec::HoldPoint(p) = a.spec {
                            events.push(OutEvent::MoveTo(p));
                        }
                    }
                    Phase::Clean => {
                        if a.key_reserved
                            && a.key.is_some_and(|k| self.held.contains(OutKey::Key(k)))
                        {
                            events.push(OutEvent::KeyUp(a.key.unwrap()));
                        }
                        if self.esc == Some(a.id) && self.held.contains(OutKey::Key(Key::ESC)) {
                            events.push(OutEvent::KeyUp(Key::ESC));
                        }
                        if a.mouse && self.held.contains(OutKey::LeftButton) {
                            events.push(OutEvent::LeftUp);
                        }
                        if events.is_empty() && a.displaced {
                            // Released between click and return: wait out the return delay first.
                            let due = *a.return_due.get_or_insert(now);
                            a.due = due;
                            self.slots[i] = Some(a);
                            continue;
                        }
                        if events.is_empty() {
                            if a.trailing && a.card_started {
                                a.phase = Phase::TrailingDown;
                                a.due = now;
                                self.slots[i] = Some(a);
                            } else {
                                self.finish(i);
                            }
                            continue;
                        }
                    }
                    Phase::TrailingDown => events.push(OutEvent::KeyDown(Key::ESC)),
                    Phase::WaitCycle | Phase::Return => unreachable!(),
                }
            }
            self.slots[i] = Some(a);
            return self.batch(Some(a.id), a.phase as u32, a.due, events);
        }
        None
    }
    pub fn commit(&mut self, id: u64, outcome: SubmitOutcome, _now: Ticks) {
        if self
            .pending
            .as_ref()
            .is_none_or(|p| p.batch.id != id || p.batch.session != self.session)
        {
            return;
        }
        let p = self.pending.take().unwrap();
        let b = p.batch;
        if matches!(outcome, SubmitOutcome::NotSubmitted) {
            if let Some(id) = b.action
                && let Some(a) = self.slots[id.slot as usize].as_mut()
                && a.phase == Phase::WaitCycle
            {
                if self.mouse == Some(a.id) {
                    self.mouse = None;
                }
                a.mouse = false;
                a.key_reserved = false;
            }
            return;
        }
        let (end, ok, invalid) = match outcome {
            SubmitOutcome::Ok { begin, end } => {
                let invalid = begin > end || begin < b.deadline;
                (end, !invalid, invalid)
            }
            SubmitOutcome::Short { end, .. } => (end, false, false),
            SubmitOutcome::NotSubmitted => unreachable!(),
        };
        for ev in &b.events {
            match *ev {
                OutEvent::KeyDown(k) => self.held.0[k.0 as usize] = true,
                OutEvent::LeftDown => self.held.0[256] = true,
                OutEvent::KeyUp(k) if ok => self.held.0[k.0 as usize] = false,
                OutEvent::LeftUp if ok => self.held.0[256] = false,
                _ => {}
            }
        }
        if invalid {
            self.state = EngineState::Faulted(self.session, FaultKind::InvalidCommit);
            return;
        }
        if !ok {
            self.state = EngineState::Faulted(self.session, FaultKind::ShortSend);
            return;
        }
        self.progress.action_progress = self.progress.action_progress.saturating_add(1);
        if b.action.is_none() {
            self.slots.fill(None);
            self.mouse = None;
            self.esc = None;
            self.state = EngineState::Stopped(self.session);
            return;
        }
        let id = b.action.unwrap();
        let i = id.slot as usize;
        let Some(mut a) = self.slots[i].filter(|a| a.id == id) else {
            return;
        };
        if b.stage == Phase::Start as u32 && matches!(a.spec, ActionSpec::Card(_)) {
            a.card_started = true;
        }
        // A stop/cancel may arrive while the batch is in flight. Account for it, then only clean up.
        if matches!(self.state, EngineState::Stopping(..)) {
            return;
        }
        // Return batches only move the cursor; dispatch already cleared the displacement.
        if b.stage == Phase::Return as u32 {
            self.slots[i] = Some(a);
            return;
        }
        if a.phase == Phase::Clean && b.stage != Phase::Clean as u32 {
            self.slots[i] = Some(a);
            return;
        }
        let t = a.timing;
        match a.phase {
            Phase::Start => {
                a.phase = match a.spec {
                    ActionSpec::Tap(_) => Phase::TapUp,
                    ActionSpec::Card(_) | ActionSpec::DoubleEsc { .. } => Phase::EscUp,
                    ActionSpec::HoldPoint(_) => Phase::Maintain,
                    _ => unreachable!(),
                };
                a.due = end.after(if a.phase == Phase::Maintain {
                    t.maintain
                } else {
                    t.key_hold
                });
            }
            Phase::TapUp | Phase::SecondUp | Phase::TrailingUp => {
                self.finish(i);
                return;
            }
            Phase::EscUp => {
                if let ActionSpec::DoubleEsc { slow } = a.spec {
                    a.phase = Phase::Gap;
                    a.due = end.after(if slow { t.esc_gap_slow } else { t.esc_gap_fast });
                } else {
                    a.card_started = true;
                    a.phase = Phase::WaitCycle;
                    a.due = end.after(t.card_pre_esc);
                }
            }
            Phase::Gap => {
                a.phase = Phase::SecondUp;
                a.due = end.after(t.key_hold);
            }
            Phase::WaitCycle => {
                a.phase = if a.key.is_some() {
                    Phase::RepeatKeyUp
                } else {
                    Phase::ClickUp
                };
                a.due = end.after(if a.key.is_some() {
                    t.key_hold
                } else {
                    t.mouse_hold
                });
            }
            Phase::RepeatKeyUp => {
                a.phase = Phase::ClickDown;
                a.due = end.after(t.key_to_click);
            }
            Phase::ClickDown => {
                a.phase = Phase::ClickUp;
                a.due = end.after(t.mouse_hold);
            }
            Phase::ClickUp => {
                if matches!(a.spec, ActionSpec::CardKey(_)) {
                    // Keep the mouse across cycles so nothing else moves the cursor before it returns.
                    a.return_due = Some(end.after(t.card_return));
                } else {
                    if self.mouse == Some(a.id) {
                        self.mouse = None;
                    }
                    a.mouse = false;
                }
                a.key_reserved = false;
                a.phase = Phase::WaitCycle;
                a.due = Ticks(a.cycle_start.after(t.repeat_period).0.max(end.0));
            }
            Phase::Maintain => a.due = end.after(t.maintain),
            Phase::Clean => {
                if a.displaced {
                    a.due = *a.return_due.get_or_insert(end.after(t.card_return));
                } else if a.trailing && a.card_started {
                    a.phase = Phase::TrailingDown;
                    a.due = end.after(t.card_pre_esc);
                } else {
                    self.finish(i);
                    return;
                }
            }
            Phase::TrailingDown => {
                a.phase = Phase::TrailingUp;
                a.due = end.after(t.key_hold);
            }
            Phase::Return => unreachable!(),
        }
        self.slots[i] = Some(a);
    }
    pub fn next_wake(&self) -> Option<Ticks> {
        if !self.running() && !matches!(self.state, EngineState::Stopping(..)) {
            return None;
        }
        let mut due = self.health;
        for a in self.slots.iter().flatten() {
            if let Some(stop_at) = a.stop_at {
                due = due.min(stop_at);
            }
            if let Some(r) = a.return_due {
                due = due.min(r);
            }
            if a.phase == Phase::WaitCycle
                && (self.mouse.is_some_and(|m| m != a.id)
                    || a.key.is_some_and(|k| self.key_busy(k, Some(a.id))))
            {
                continue;
            }
            due = due.min(a.due);
        }
        Some(due)
    }
}
