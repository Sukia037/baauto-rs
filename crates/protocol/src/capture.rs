//! Fixed-size input capture messages; callbacks only fill/push these values.
#![forbid(unsafe_code)]
use baauto_config::{Chord, NormPoint, Ticks};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureKind {
    Point,
    Hotkey,
    SlotKey,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureRequest {
    pub id: u64,
    pub kind: CaptureKind,
    pub deadline: Ticks,
    pub abort_chord: Option<Chord>,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CaptureOutcome {
    Point(NormPoint),
    Chord(Chord),
    Cleared,
    Cancelled,
    TimedOut,
    RejectedLeft,
    InvalidSlot,
    NotForeground,
    EmergencyStop,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaptureResult {
    pub id: u64,
    pub outcome: CaptureOutcome,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureError {
    Armed,
    Busy,
    InvalidRequest,
}
