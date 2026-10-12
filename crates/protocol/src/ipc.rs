//! Bounded diagnostic IPC. JSON allocation happens only on background/control threads.
#![forbid(unsafe_code)]
use crate::capture::*;
use baauto_config::*;
pub use baauto_core::{RunMode, TrialError};
use baauto_core::{SessionId, StopReason};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
pub const PROTOCOL_VERSION: u32 = 7;
pub const MAX_FRAME: usize = 1024 * 1024;
/// `Rejected.why` when Arm finds a related key or mouse button still held. The supervisor keeps
/// the enable request and arms again once the engine reports the keys released.
pub const ARM_KEYS_HELD: &str = "release relevant keys before Arm";
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("truncated or trailing IPC frame")]
    Truncated,
    #[error("IPC frame exceeds maximum size")]
    TooLarge,
    #[error("protocol or shared-memory layout mismatch")]
    Version,
    #[error("invalid compiled configuration: {0}")]
    Config(&'static str),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub fn handshake(protocol: u32, layout: u32) -> Result<(), IpcError> {
    if protocol == PROTOCOL_VERSION && layout == crate::shm::LAYOUT_VERSION {
        Ok(())
    } else {
        Err(IpcError::Version)
    }
}
pub fn encode_frame<T: Serialize>(message: &T) -> Result<Vec<u8>, IpcError> {
    let json = serde_json::to_vec(message)?;
    if json.len() > MAX_FRAME {
        return Err(IpcError::TooLarge);
    }
    let mut frame = Vec::with_capacity(json.len() + 4);
    frame.extend_from_slice(&(json.len() as u32).to_le_bytes());
    frame.extend_from_slice(&json);
    Ok(frame)
}
pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Result<T, IpcError> {
    let prefix: [u8; 4] = frame
        .get(..4)
        .ok_or(IpcError::Truncated)?
        .try_into()
        .unwrap();
    let len = u32::from_le_bytes(prefix) as usize;
    if len > MAX_FRAME {
        return Err(IpcError::TooLarge);
    }
    if frame.len() != len + 4 {
        return Err(IpcError::Truncated);
    }
    Ok(serde_json::from_slice(&frame[4..])?)
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
pub enum AppToEngine {
    Hello {
        cmd_seq: u64,
        protocol: u32,
        shm_layout: u32,
    },
    ApplyConfig {
        cmd_seq: u64,
        version: ConfigVersion,
        compiled: Box<CompiledWire>,
    },
    Arm {
        cmd_seq: u64,
        session: SessionId,
        stop_event: u64,
        mode: RunMode,
    },
    BeginRecordPoint {
        cmd_seq: u64,
        session: SessionId,
        request_id: u64,
        timeout_ms: u32,
        abort_chord: Chord,
    },
    CancelRecordPoint {
        cmd_seq: u64,
        session: SessionId,
        through_id: u64,
    },
    CaptureNextInput {
        cmd_seq: u64,
        session: SessionId,
        request_id: u64,
        slot_key: bool,
        timeout_ms: u32,
        abort_chord: Chord,
    },
    RunTrial {
        cmd_seq: u64,
        session: SessionId,
        trial: Box<TrialSpec>,
    },
    Stop {
        cmd_seq: u64,
        session: SessionId,
        reason: StopReason,
    },
    Probe {
        cmd_seq: u64,
    },
    Rehook {
        cmd_seq: u64,
    },
    Shutdown {
        cmd_seq: u64,
    },
}
impl AppToEngine {
    pub fn cmd_seq(&self) -> u64 {
        match self {
            Self::BeginRecordPoint { cmd_seq, .. }
            | Self::CancelRecordPoint { cmd_seq, .. }
            | Self::CaptureNextInput { cmd_seq, .. }
            | Self::Hello { cmd_seq, .. }
            | Self::ApplyConfig { cmd_seq, .. }
            | Self::RunTrial { cmd_seq, .. }
            | Self::Arm { cmd_seq, .. }
            | Self::Stop { cmd_seq, .. }
            | Self::Probe { cmd_seq }
            | Self::Rehook { cmd_seq }
            | Self::Shutdown { cmd_seq } => *cmd_seq,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", deny_unknown_fields)]
pub enum EngineToApp {
    CaptureReady {
        session: SessionId,
        cmd_seq: u64,
        request_id: u64,
    },
    PointRecorded {
        session: SessionId,
        cmd_seq: u64,
        request_id: u64,
        screen_pos: [i32; 2],
        monitor: [i32; 4],
        geometry: u32,
    },
    CapturedInput {
        session: SessionId,
        cmd_seq: u64,
        result: CaptureResult,
    },
    TrialResult {
        session: SessionId,
        cmd_seq: u64,
        result: TrialResult,
    },
    HelloAck {
        session: SessionId,
        cmd_seq: u64,
        protocol: u32,
        shm_layout: u32,
    },
    ConfigInstalled {
        session: SessionId,
        cmd_seq: u64,
        version: ConfigVersion,
    },
    KeysReleased {
        session: SessionId,
        cmd_seq: u64,
        version: ConfigVersion,
    },
    Armed {
        session: SessionId,
        cmd_seq: u64,
    },
    Stopped {
        session: SessionId,
        cmd_seq: u64,
        reason: StopReason,
    },
    Faulted {
        session: SessionId,
        cmd_seq: u64,
        why: String,
    },
    Status {
        session: SessionId,
        cmd_seq: u64,
        state: u32,
        scheduler: u64,
        hook_loop: u64,
        focused: bool,
    },
    Rejected {
        session: SessionId,
        cmd_seq: u64,
        why: String,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledWire {
    pub version: ConfigVersion,
    pub triggers: Vec<[u16; 16]>,
    pub actions: Vec<ActionSpec>,
    pub timing: TickTiming,
    pub params: EngineParams,
    pub hand: HandRect,
    pub slots: u8,
    pub slot_keys: [Key; 5],
    pub order: SlotOrder,
    pub candidates: Vec<bool>,
}
impl From<&CompiledConfig> for CompiledWire {
    fn from(c: &CompiledConfig) -> Self {
        Self {
            version: c.version,
            triggers: c.triggers.to_vec(),
            actions: c.actions.clone(),
            timing: c.timing,
            params: c.params,
            hand: c.hand,
            slots: c.slots,
            slot_keys: c.slot_keys,
            order: c.order,
            candidates: c.candidates.to_vec(),
        }
    }
}
impl TryFrom<CompiledWire> for CompiledConfig {
    type Error = IpcError;
    fn try_from(c: CompiledWire) -> Result<Self, IpcError> {
        if c.triggers.len() != 256
            || c.candidates.len() != 256
            || c.actions.len() > 256
            || c.params.max_actions == 0
            || c.params.max_actions > 256
            || c.params.queue_capacity == 0
            || c.params.max_events_per_round == 0
            || c.params.max_batches_per_round == 0
            || c.params.health_period_us == 0
        {
            return Err(IpcError::Config("invalid capacity"));
        }
        if !matches!(c.slots, 3 | 5)
            || !c.hand.valid()
            || c.actions.iter().any(
                |a| matches!(a,ActionSpec::HoldPoint(p) | ActionSpec::CardKey(p) if !p.valid()),
            )
            || c.triggers
                .iter()
                .flatten()
                .any(|id| *id != NO_TRIGGER && usize::from(*id) >= c.actions.len())
        {
            return Err(IpcError::Config("invalid trigger or geometry"));
        }
        if !c.timing.loops_valid() {
            return Err(IpcError::Config("invalid timing"));
        }
        let mut required = [false; 256];
        for spec in &c.actions {
            match spec {
                ActionSpec::Tap(key) => required[key.0 as usize] = true,
                ActionSpec::Repeat { key } => {
                    required[1] = true;
                    if let Some(key) = key {
                        required[key.0 as usize] = true;
                    }
                }
                ActionSpec::Card(key) => {
                    required[1] = true;
                    required[27] = true;
                    required[key.0 as usize] = true;
                }
                ActionSpec::DoubleEsc { .. } => required[27] = true,
                ActionSpec::HoldPoint(_) => required[1] = true,
                ActionSpec::CardKey(_) => {
                    required[1] = true;
                    for key in c.slot_keys.iter().take(c.slots as usize) {
                        required[key.0 as usize] = true;
                    }
                }
            }
        }
        if required
            .iter()
            .enumerate()
            .any(|(i, needed)| *needed && !c.candidates[i])
        {
            return Err(IpcError::Config("candidate set misses possible output"));
        }
        if c.slot_keys.iter().any(|key| key.is_mouse())
            || (0..c.slots as usize).any(|i| c.slot_keys[..i].contains(&c.slot_keys[i]))
        {
            return Err(IpcError::Config("invalid hand keys"));
        }
        let triggers: Box<[[u16; 16]; 256]> = c
            .triggers
            .into_boxed_slice()
            .try_into()
            .map_err(|_| IpcError::Config("table size"))?;
        let candidates: [bool; 256] = c
            .candidates
            .try_into()
            .map_err(|_| IpcError::Config("candidate size"))?;
        Ok(Self {
            version: c.version,
            triggers,
            actions: c.actions,
            timing: c.timing,
            params: c.params,
            hand: c.hand,
            slots: c.slots,
            slot_keys: c.slot_keys,
            order: c.order,
            reports: Vec::new(),
            candidates,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialSpec {
    pub id: u64,
    pub action: ActionSpec,
    pub timing: TickTiming,
    pub start: Ticks,
    pub duration: u64,
    pub cursor: NormPoint,
}
impl TrialSpec {
    pub fn allowed_outputs(&self, cfg: &CompiledConfig) -> bool {
        let needed = match self.action {
            ActionSpec::Tap(k) => vec![k],
            ActionSpec::Card(k) => vec![k, Key::ESC, Key::LEFT],
            ActionSpec::Repeat { key } => key.into_iter().chain([Key::LEFT]).collect(),
            ActionSpec::DoubleEsc { .. } => vec![Key::ESC],
            ActionSpec::HoldPoint(_) => vec![Key::LEFT],
            ActionSpec::CardKey(_) => cfg.slot_keys[..cfg.slots as usize]
                .iter()
                .copied()
                .chain([Key::LEFT])
                .collect(),
        };
        needed.iter().all(|k| cfg.candidates[k.0 as usize])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrialOutcome {
    Complete,
    Stopped,
    Rejected(TrialError),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialResult {
    pub id: u64,
    pub cmd_seq: u64,
    pub started: Ticks,
    pub finished: Ticks,
    pub outcome: TrialOutcome,
}
