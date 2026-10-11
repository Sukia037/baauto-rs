use arrayvec::ArrayVec;
use baauto_config::{Chord, ModsMatch};
use baauto_config::{CompiledConfig, ConfigVersion, Key, NO_TRIGGER, NormPoint};
use baauto_core::{
    GeometryVersion, InputEvent, InputKind, OutEvent, OutKey, PressId, SessionId, StopReason, Ticks,
};
use baauto_protocol::capture::*;
#[derive(Clone, Copy, Debug)]
pub enum RawKind {
    Key { key: Key, down: bool },
    Move,
}
#[derive(Clone, Copy, Debug)]
pub struct RawEvent {
    pub kind: RawKind,
    pub injected: bool,
    pub extra: usize,
    pub cursor: NormPoint,
    pub inside_window: bool,
    pub at: Ticks,
    pub ledger_keys: [u64; 4],
    pub ledger_mouse: bool,
}
#[derive(Clone, Copy, Debug)]
struct Pair {
    press: PressId,
    session: SessionId,
    config: ConfigVersion,
    allowed_mods: u16,
    cancelled: bool,
}
#[derive(Debug)]
pub struct Decision {
    pub swallow: bool,
    pub stop: Option<StopReason>,
    pub events: ArrayVec<InputEvent, 257>,
    pub physical: bool,
    pub capture: Option<CaptureResult>,
}
impl Default for Decision {
    fn default() -> Self {
        Self {
            swallow: false,
            stop: None,
            events: ArrayVec::new(),
            physical: false,
            capture: None,
        }
    }
}
pub struct HookState {
    pub physical: [bool; 256],
    pairs: [Option<Pair>; 256],
    pub triggers: Box<[[u16; 16]; 256]>,
    pub version: ConfigVersion,
    pub session: SessionId,
    pub geometry: GeometryVersion,
    pub focused: bool,
    pub armed: bool,
    pub invalid: bool,
    pub marker: usize,
    seq: u64,
    press: u32,
    capture: Option<CaptureRequest>,
    capture_pairs: [bool; 256],
}
impl HookState {
    pub fn new(cfg: &CompiledConfig, marker: usize) -> Self {
        Self {
            physical: [false; 256],
            pairs: [None; 256],
            triggers: cfg.triggers.clone(),
            version: cfg.version,
            session: SessionId(0),
            geometry: GeometryVersion(1),
            focused: false,
            armed: false,
            invalid: false,
            marker,
            seq: 0,
            press: 0,
            capture: None,
            capture_pairs: [false; 256],
        }
    }
    pub fn begin_capture(&mut self, request: CaptureRequest) -> Result<(), CaptureError> {
        if self.armed {
            return Err(CaptureError::Armed);
        }
        if self.capture.is_some() || self.capture_pairs.iter().any(|p| *p) {
            return Err(CaptureError::Busy);
        }
        if request.id == 0 || request.deadline.0 == 0 {
            return Err(CaptureError::InvalidRequest);
        }
        self.capture = Some(request);
        Ok(())
    }
    pub fn cancel_capture(&mut self) -> Option<CaptureResult> {
        self.capture.take().map(|c| CaptureResult {
            id: c.id,
            outcome: CaptureOutcome::Cancelled,
        })
    }
    pub fn expire_capture(&mut self, now: Ticks) -> Option<CaptureResult> {
        if self.capture.is_some_and(|c| now >= c.deadline) {
            self.capture.take().map(|c| CaptureResult {
                id: c.id,
                outcome: CaptureOutcome::TimedOut,
            })
        } else {
            None
        }
    }
    pub fn seed_physical(&mut self, physical: [bool; 256]) {
        self.physical = physical;
    }
    pub fn keys_released(&self, cfg: &CompiledConfig) -> bool {
        let related = cfg.related_keys();
        self.physical
            .iter()
            .enumerate()
            .all(|(i, held)| !*held || !related[i])
            && self.pairs.iter().all(Option::is_none)
            && self.capture.is_none()
            && self.capture_pairs.iter().all(|p| !p)
    }
    pub fn arm(&mut self, s: SessionId, cfg: &CompiledConfig) -> bool {
        if self.armed || s.0 <= self.session.0 || !self.keys_released(cfg) {
            return false;
        }
        self.session = s;
        self.seq = 0;
        self.armed = true;
        self.invalid = false;
        true
    }
    pub fn invalidate(&mut self) {
        self.invalid = true;
        self.armed = false;
    }
    fn mods(&self) -> u8 {
        let held = |keys: &[usize]| keys.iter().any(|k| self.physical[*k]);
        u8::from(held(&[17, 162, 163]))
            | (u8::from(held(&[16, 160, 161])) * 2)
            | (u8::from(held(&[18, 164, 165])) * 4)
            | (u8::from(held(&[91, 92])) * 8)
    }
    fn emit(
        &mut self,
        d: &mut Decision,
        kind: InputKind,
        at: Ticks,
        session: SessionId,
        config: ConfigVersion,
    ) {
        if let Some(seq) = self.seq.checked_add(1) {
            self.seq = seq;
            d.events.push(InputEvent {
                seq,
                at,
                session,
                config,
                geometry: self.geometry,
                kind,
            });
        } else {
            self.invalidate();
            d.stop = Some(StopReason::Overflow);
        }
    }
    pub fn focus(&mut self, focused: bool, at: Ticks) -> Decision {
        let lost = self.focused && !focused;
        self.focused = focused;
        let mut d = Decision::default();
        if lost && self.capture.is_some_and(|r| r.kind == CaptureKind::Point) {
            d.capture = self.cancel_capture();
        }
        if !focused && self.armed {
            self.emit(
                &mut d,
                InputKind::Focus { focused: false },
                at,
                self.session,
                self.version,
            );
            self.invalidate();
            d.stop = Some(StopReason::FocusLost);
        }
        d
    }
    pub fn decide(&mut self, raw: RawEvent) -> Decision {
        let mut d = Decision::default();
        if raw.extra == self.marker || raw.injected && !cfg!(feature = "test-hooks") {
            return d;
        }
        let RawKind::Key { key, down } = raw.kind else {
            return d;
        };
        let i = usize::from(key.0);
        let repeat = down && self.physical[i];
        self.physical[i] = down;
        let alias = match key.0 {
            160 | 161 => 16,
            162 | 163 => 17,
            164 | 165 => 18,
            other => other,
        };
        if alias != key.0 {
            let (left, right) = match alias {
                16 => (160, 161),
                17 => (162, 163),
                _ => (164, 165),
            };
            self.physical[alias as usize] = self.physical[left] || self.physical[right];
        }
        d.physical = true;
        let mods = self.mods();
        d.capture = self.expire_capture(raw.at);
        if self.capture_pairs[i] {
            d.swallow = true;
            if !down {
                self.capture_pairs[i] = false;
            }
            return d;
        }
        if down
            && !repeat
            && let Some(capture) = self.capture
        {
            let modifier = matches!(key.0,16..=18|91|92|160..=165);
            if capture.kind == CaptureKind::Point
                && key.0 != 27
                && key != Key::LEFT
                && !capture
                    .abort_chord
                    .is_some_and(|c| c.key == key && c.matches(mods))
            {
                return d;
            }
            self.capture_pairs[i] = true;
            d.swallow = true;
            if modifier {
                return d;
            }
            let outcome = if capture
                .abort_chord
                .is_some_and(|c| c.key == key && c.matches(mods))
            {
                CaptureOutcome::EmergencyStop
            } else {
                match key.0 {
                    27 => CaptureOutcome::Cancelled,
                    8 if capture.kind != CaptureKind::Point => {
                        if capture.kind == CaptureKind::SlotKey {
                            CaptureOutcome::InvalidSlot
                        } else {
                            CaptureOutcome::Cleared
                        }
                    }
                    1 if capture.kind == CaptureKind::Point => {
                        if self.focused && raw.inside_window && raw.cursor.valid() {
                            CaptureOutcome::Point(raw.cursor)
                        } else {
                            CaptureOutcome::NotForeground
                        }
                    }
                    1 => CaptureOutcome::RejectedLeft,
                    _ if capture.kind == CaptureKind::SlotKey && key.is_mouse() => {
                        CaptureOutcome::InvalidSlot
                    }
                    _ => CaptureOutcome::Chord(Chord {
                        key,
                        mods: ModsMatch::AtLeast(mods),
                    }),
                }
            };
            self.capture = None;
            d.capture = Some(CaptureResult {
                id: capture.id,
                outcome,
            });
            return d;
        }
        if !down {
            if let Some(pair) = self.pairs[i].take() {
                d.swallow = true;
                self.emit(
                    &mut d,
                    InputKind::TriggerUp { press: pair.press },
                    raw.at,
                    pair.session,
                    pair.config,
                );
            }
            for k in 0..256 {
                if let Some(mut pair) = self.pairs[k]
                    && !pair.cancelled
                    && pair.allowed_mods & (1u16 << mods) == 0
                {
                    self.emit(
                        &mut d,
                        InputKind::PressCancelled { press: pair.press },
                        raw.at,
                        pair.session,
                        pair.config,
                    );
                    pair.cancelled = true;
                    self.pairs[k] = Some(pair);
                }
            }
            return d;
        }
        for k in 0..256 {
            if let Some(mut pair) = self.pairs[k]
                && !pair.cancelled
                && pair.allowed_mods & (1u16 << mods) == 0
            {
                self.emit(
                    &mut d,
                    InputKind::PressCancelled { press: pair.press },
                    raw.at,
                    pair.session,
                    pair.config,
                );
                pair.cancelled = true;
                self.pairs[k] = Some(pair);
            }
        }
        if let Some(pair) = self.pairs[i] {
            d.swallow = true;
            if !pair.cancelled && self.armed && !self.invalid {
                self.emit(
                    &mut d,
                    InputKind::TriggerRepeat {
                        press: pair.press,
                        cursor: raw.cursor,
                    },
                    raw.at,
                    pair.session,
                    pair.config,
                );
            }
            return d;
        }
        let conflict = if key == Key::LEFT {
            raw.ledger_mouse
        } else {
            raw.ledger_keys[i / 64] & (1u64 << (i % 64)) != 0
                || raw.ledger_keys[alias as usize / 64] & (1u64 << (alias % 64)) != 0
        };
        if conflict && self.armed {
            self.emit(
                &mut d,
                InputKind::PhysicalConflict {
                    key: if key == Key::LEFT {
                        OutKey::LeftButton
                    } else {
                        OutKey::Key(key)
                    },
                },
                raw.at,
                self.session,
                self.version,
            );
            self.invalidate();
            d.stop = Some(StopReason::PhysicalConflict);
            return d;
        }
        if repeat
            || !self.armed
            || self.invalid
            || !self.focused
            || key.is_mouse() && !raw.inside_window
        {
            return d;
        }
        let table_index = if self.triggers[i][usize::from(mods)] == NO_TRIGGER {
            alias as usize
        } else {
            i
        };
        let trigger = self.triggers[table_index][usize::from(mods)];
        if trigger == NO_TRIGGER {
            return d;
        }
        let Some(press) = self.press.checked_add(1) else {
            self.invalidate();
            d.stop = Some(StopReason::Overflow);
            return d;
        };
        self.press = press;
        let allowed_mods = (0..16).fold(0u16, |mask, m| {
            if self.triggers[table_index][m] == trigger {
                mask | (1 << m)
            } else {
                mask
            }
        });
        self.pairs[i] = Some(Pair {
            press: PressId(press),
            session: self.session,
            config: self.version,
            allowed_mods,
            cancelled: false,
        });
        d.swallow = true;
        self.emit(
            &mut d,
            InputKind::TriggerDown {
                press: PressId(press),
                trigger,
                cursor: raw.cursor,
            },
            raw.at,
            self.session,
            self.version,
        );
        d
    }
}

/// Both physical-before-synthetic and synthetic-before-physical must stop the session.
pub fn outputs_conflict(events: &[OutEvent], physical: [u64; 4]) -> bool {
    events.iter().any(|e| {
        let key = match e {
            OutEvent::KeyDown(k) => k.0,
            OutEvent::LeftDown => 1,
            _ => return false,
        };
        physical[key as usize / 64] & (1u64 << (key % 64)) != 0
    })
}
