use crate::{Chord, Key};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Ticks(pub u64);
impl Ticks {
    pub fn after(self, delta: u64) -> Self {
        Self(self.0.saturating_add(delta))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickRate(pub u64);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigVersion(pub u32);
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormPoint {
    pub x: f64,
    pub y: f64,
}
impl NormPoint {
    pub fn valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && (0.0..=1.0).contains(&self.x)
            && (0.0..=1.0).contains(&self.y)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Six,
    Ten,
}
impl Mode {
    pub fn slots(self) -> u8 {
        match self {
            Self::Six => 3,
            Self::Ten => 5,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardUiSide {
    Right,
    Left,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotOrder {
    LeftToRight,
    RightToLeft,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoRepeatPolicy {
    Ignore,
    FollowTypematic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrailingEscPolicy {
    Never,
    IfStarted,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandRect {
    pub x: [f64; 2],
    pub y: [f64; 2],
    pub calibrated: bool,
}
impl HandRect {
    pub fn valid(self) -> bool {
        NormPoint {
            x: self.x[0],
            y: self.y[0],
        }
        .valid()
            && NormPoint {
                x: self.x[1],
                y: self.y[1],
            }
            .valid()
            && self.x[0] < self.x[1]
            && self.y[0] < self.y[1]
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideRects {
    pub right: HandRect,
    pub left: HandRect,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandRects {
    pub six: SideRects,
    pub ten: SideRects,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandSettings {
    pub card_ui_side: CardUiSide,
    pub slot_order: SlotOrder,
    pub slot_keys: [Key; 5],
    pub card_auto_repeat: AutoRepeatPolicy,
    pub rects: HandRects,
}
impl HandSettings {
    pub fn layout(&self, mode: Mode) -> HandRect {
        let r = match mode {
            Mode::Six => &self.rects.six,
            Mode::Ten => &self.rects.ten,
        };
        match self.card_ui_side {
            CardUiSide::Right => r.right,
            CardUiSide::Left => r.left,
        }
    }
}
impl Default for HandSettings {
    fn default() -> Self {
        fn sides(x: [f64; 2]) -> SideRects {
            SideRects {
                right: HandRect {
                    x,
                    y: [0.781, 0.924],
                    calibrated: false,
                },
                left: HandRect {
                    x: [1.0 - x[1], 1.0 - x[0]],
                    y: [0.781, 0.924],
                    calibrated: false,
                },
            }
        }
        Self {
            card_ui_side: CardUiSide::Right,
            slot_order: SlotOrder::LeftToRight,
            slot_keys: [Key(49), Key(50), Key(51), Key(52), Key(53)],
            card_auto_repeat: AutoRepeatPolicy::Ignore,
            rects: HandRects {
                six: sides([0.664, 0.910]),
                ten: sides([0.500, 0.910]),
            },
        }
    }
}
pub fn slot_at(r: &HandRect, n: u8, order: SlotOrder, p: NormPoint) -> Option<u8> {
    if n == 0
        || !r.valid()
        || !p.valid()
        || p.x < r.x[0]
        || p.x > r.x[1]
        || p.y < r.y[0]
        || p.y > r.y[1]
    {
        return None;
    }
    let pos = (p.x - r.x[0]) / (r.x[1] - r.x[0]) * f64::from(n);
    let rounded = pos.round();
    let pos = if (pos - rounded).abs() < 1e-12 {
        rounded
    } else {
        pos
    };
    let i = (pos.floor() as u8).min(n - 1);
    Some(match order {
        SlotOrder::LeftToRight => i,
        SlotOrder::RightToLeft => n - 1 - i,
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointFunction {
    Point,
    Card,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointItem {
    pub id: u32,
    pub name: String,
    pub hotkey: Option<Chord>,
    pub function: PointFunction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointSet {
    pub schema: String,
    pub name: String,
    pub mode: Mode,
    pub items: Vec<PointItem>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub hotkey: Option<Chord>,
    pub enabled: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileIdentity {
    pub game_version: String,
    pub window_mode: String,
    pub fps: String,
    pub sync: String,
    pub machine: String,
    pub conditions: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    ImportedLegacy,
    Manual,
    Calibrated,
}
/// Older keys files carry the removed `allowed_lateness_ms` table; it loads but is never written back.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct RemovedField;
impl<'de> Deserialize<'de> for RemovedField {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(d).map(|_| Self)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimingProfile {
    pub identity: ProfileIdentity,
    pub source: ProfileSource,
    pub verified: bool,
    pub key_hold_ms: f64,
    pub mouse_hold_ms: f64,
    pub key_to_click_ms: f64,
    pub repeat_period_ms: f64,
    /// Fixed waits between the two Escs: [frame_fast, frame_slow]. The second Esc is always sent.
    pub escape_gap_ms: [f64; 2],
    pub card_pre_esc_ms: f64,
    pub hold_maintain_period_ms: f64,
    #[serde(default, skip_serializing, rename = "allowed_lateness_ms")]
    removed_lateness: RemovedField,
    pub card_trailing_esc: TrailingEscPolicy,
    pub auto_repeat: AutoRepeatPolicy,
}
impl TimingProfile {
    pub fn set_identity(&mut self, identity: ProfileIdentity) {
        if self.identity != identity {
            self.verified = false;
            self.identity = identity;
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeysProfile {
    pub schema: String,
    pub presets: BTreeMap<String, Preset>,
    pub timing: TimingProfile,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineParams {
    pub max_actions: u16,
    pub queue_capacity: u32,
    pub max_events_per_round: u16,
    pub max_batches_per_round: u16,
    pub health_period_us: u32,
    #[serde(default = "default_hook_priority")]
    pub hook_time_critical: bool,
    #[serde(default)]
    pub engine_highest: bool,
    #[serde(default)]
    pub tail_spin_us: u32,
}
fn default_hook_priority() -> bool {
    true
}
impl Default for EngineParams {
    fn default() -> Self {
        Self {
            max_actions: 64,
            queue_capacity: 1024,
            max_events_per_round: 64,
            max_batches_per_round: 16,
            health_period_us: 50_000,
            hook_time_critical: true,
            engine_highest: false,
            tail_spin_us: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {reason}")]
pub struct ConfigError {
    pub path: String,
    pub reason: String,
}
impl ConfigError {
    pub fn new(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            reason: reason.into(),
        }
    }
}
