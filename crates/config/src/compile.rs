use crate::*;
use serde::{Deserialize, Serialize};

pub const NO_TRIGGER: u16 = u16::MAX;
pub const PRESET_ROLES: [&str; 23] = [
    "global_stop",
    "record",
    "skill_1",
    "skill_2",
    "skill_3",
    "skill_4",
    "skill_5",
    "spam_1",
    "spam_2",
    "spam_3",
    "spam_4",
    "spam_5",
    "pause",
    "frame_fast",
    "frame_slow",
    "auto",
    "speed",
    "click_spam",
    "card_1",
    "card_2",
    "card_3",
    "card_4",
    "card_5",
];
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActionSpec {
    Tap(Key),
    Repeat { key: Option<Key> },
    Card(Key),
    DoubleEsc { slow: bool },
    HoldPoint(NormPoint),
    CardKey,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TickTiming {
    pub key_hold: u64,
    pub mouse_hold: u64,
    pub key_to_click: u64,
    pub repeat_period: u64,
    pub esc_gap_fast: u64,
    pub esc_gap_slow: u64,
    pub card_pre_esc: u64,
    pub maintain: u64,
    pub trailing: TrailingEscPolicy,
    pub auto_repeat: AutoRepeatPolicy,
}
#[derive(Clone, Debug)]
pub struct CompiledConfig {
    pub version: ConfigVersion,
    pub triggers: Box<[[u16; 16]; 256]>,
    pub actions: Vec<ActionSpec>,
    pub timing: TickTiming,
    pub params: EngineParams,
    pub hand: HandRect,
    pub slots: u8,
    pub slot_keys: [Key; 5],
    pub order: SlotOrder,
    pub card_auto_repeat: AutoRepeatPolicy,
    pub reports: Vec<ConfigError>,
    pub candidates: [bool; 256],
}
impl CompiledConfig {
    /// Relevant physical keys must be released before Arm, including modifier families.
    pub fn related_keys(&self) -> [bool; 256] {
        let mut keys = self.candidates;
        for (i, table) in self.triggers.iter().enumerate() {
            if table.iter().any(|id| *id != NO_TRIGGER) {
                keys[i] = true;
                for (bit, family) in [
                    (1, &[17, 162, 163][..]),
                    (2, &[16, 160, 161][..]),
                    (4, &[18, 164, 165][..]),
                    (8, &[91, 92][..]),
                ] {
                    if table
                        .iter()
                        .enumerate()
                        .any(|(m, id)| *id != NO_TRIGGER && m & bit != 0 && table[m & !bit] != *id)
                    {
                        for k in family {
                            keys[*k] = true;
                        }
                    }
                }
            }
        }
        for family in [[16, 160, 161], [17, 162, 163], [18, 164, 165]] {
            if family.iter().any(|i| keys[*i]) {
                for i in family {
                    keys[i] = true;
                }
            }
        }
        keys
    }
}
#[derive(Clone, Debug, Default)]
pub struct ConflictAnalysis {
    pub preset_errors: Vec<ConfigError>,
    pub disabled_points: Vec<u32>,
    pub reports: Vec<ConfigError>,
}
pub fn analyze_conflicts(points: &PointSet, keys: &KeysProfile) -> ConflictAnalysis {
    let presets: Vec<_> = keys
        .presets
        .iter()
        .filter_map(|(name, p)| {
            if p.enabled {
                p.hotkey.map(|c| (name, c))
            } else {
                None
            }
        })
        .collect();
    let mut a = ConflictAnalysis::default();
    for i in 0..presets.len() {
        for j in i + 1..presets.len() {
            if presets[i].1.overlaps(presets[j].1) {
                for k in [i, j] {
                    a.preset_errors.push(ConfigError::new(
                        format!("$.presets.{}.hotkey", presets[k].0),
                        format!("conflicts with {}", presets[if k == i { j } else { i }].0),
                    ));
                }
            }
        }
    }
    for item in &points.items {
        if let Some(c) = item.hotkey {
            let other = presets
                .iter()
                .find(|(_, p)| c.overlaps(*p))
                .map(|(n, _)| n.as_str())
                .or_else(|| {
                    points
                        .items
                        .iter()
                        .find(|p| p.id != item.id && p.hotkey.is_some_and(|h| c.overlaps(h)))
                        .map(|p| p.name.as_str())
                });
            if let Some(other) = other {
                a.disabled_points.push(item.id);
                a.reports.push(ConfigError::new(
                    format!("$.items[id={}].hotkey", item.id),
                    format!("conflicts with {other}"),
                ));
            }
        }
    }
    a
}
pub fn ticks_ms(ms: f64, rate: TickRate) -> Result<u64, ConfigError> {
    if !ms.is_finite() || ms < 0.0 || rate.0 == 0 {
        return Err(ConfigError::new("$.timing", "invalid time or tick rate"));
    }
    let us = (ms * 1000.0).ceil();
    if us > u32::MAX as f64 {
        return Err(ConfigError::new(
            "$.timing",
            "time exceeds microsecond range",
        ));
    }
    let n = (us as u128 * u128::from(rate.0)).div_ceil(1_000_000);
    u64::try_from(n).map_err(|_| ConfigError::new("$.timing", "tick conversion overflow"))
}
pub fn compile(
    p: &PointSet,
    k: &KeysProfile,
    h: &HandSettings,
    rate: TickRate,
) -> Result<CompiledConfig, Vec<ConfigError>> {
    compile_with_params(p, k, h, rate, EngineParams::default(), ConfigVersion(1))
}
pub fn compile_with_params(
    p: &PointSet,
    k: &KeysProfile,
    h: &HandSettings,
    rate: TickRate,
    params: EngineParams,
    version: ConfigVersion,
) -> Result<CompiledConfig, Vec<ConfigError>> {
    let mut errors = Vec::new();
    if p.schema != "baauto-rs-points-v1" {
        errors.push(ConfigError::new("$.schema", "expected baauto-rs-points-v1"));
    }
    if k.schema != "baauto-rs-keys-v1" {
        errors.push(ConfigError::new("$.schema", "expected baauto-rs-keys-v1"));
    }
    for role in PRESET_ROLES {
        if !k.presets.contains_key(role) {
            errors.push(ConfigError::new(
                format!("$.presets.{role}"),
                "missing preset",
            ));
        }
    }
    for role in k.presets.keys() {
        if !PRESET_ROLES.contains(&role.as_str()) {
            errors.push(ConfigError::new(
                format!("$.presets.{role}"),
                "unknown preset",
            ));
        }
    }
    for role in ["global_stop", "record"] {
        if k.presets
            .get(role)
            .is_some_and(|p| p.hotkey.is_some_and(|h| h.key.is_mouse()))
        {
            errors.push(ConfigError::new(
                format!("$.presets.{role}.hotkey"),
                "全域停止／錄點需鍵盤熱鍵；滑鼠鍵可綁其他動作",
            ));
        }
    }
    for (i, item) in p.items.iter().enumerate() {
        if p.items[..i].iter().any(|p| p.id == item.id) {
            errors.push(ConfigError::new(format!("$.items[{i}].id"), "duplicate ID"));
        }
        let valid = match item.function {
            PointFunction::Point => item
                .x
                .zip(item.y)
                .is_some_and(|(x, y)| NormPoint { x, y }.valid()),
            PointFunction::Card => item.x.is_none() && item.y.is_none(),
        };
        if !valid {
            errors.push(ConfigError::new(
                format!("$.items[{i}]"),
                "point needs valid x/y; card must omit coordinates",
            ));
        }
    }
    for c in p
        .items
        .iter()
        .filter_map(|p| p.hotkey)
        .chain(k.presets.values().filter_map(|p| p.hotkey))
    {
        if c.key == Key::LEFT {
            errors.push(ConfigError::new("$.hotkey", "left button cannot trigger"));
        }
    }
    for (i, key) in h.slot_keys.iter().enumerate() {
        if key.is_mouse() || h.slot_keys[..i].contains(key) {
            errors.push(ConfigError::new(
                format!("$.hand.slot_keys[{i}]"),
                "needs five distinct keyboard keys",
            ));
        }
    }
    for (name, r) in [
        ("six.right", h.rects.six.right),
        ("six.left", h.rects.six.left),
        ("ten.right", h.rects.ten.right),
        ("ten.left", h.rects.ten.left),
    ] {
        if !r.valid() {
            errors.push(ConfigError::new(
                format!("$.hand.rects.{name}"),
                "invalid rectangle",
            ));
        }
    }
    if params.tail_spin_us > 5000
        || params.max_actions == 0
        || params.max_actions > 256
        || params.queue_capacity == 0
        || params.max_events_per_round == 0
        || params.max_batches_per_round == 0
        || params.health_period_us == 0
    {
        errors.push(ConfigError::new(
            "$.engine_params",
            "capacities and health period must be positive; max_actions <= 256",
        ));
    }
    let t = &k.timing;
    if t.key_hold_ms == 0.0 && !t.verified {
        errors.push(ConfigError::new(
            "$.timing.key_hold_ms",
            "zero hold requires verified profile",
        ));
    }
    if t.repeat_period_ms < t.key_hold_ms + t.key_to_click_ms + t.mouse_hold_ms {
        errors.push(ConfigError::new(
            "$.timing.repeat_period_ms",
            "shorter than stages",
        ));
    }
    if t.repeat_period_ms <= 0.0 || t.mouse_hold_ms <= 0.0 || t.hold_maintain_period_ms <= 0.0 {
        errors.push(ConfigError::new(
            "$.timing",
            "periods/holds must be positive",
        ));
    }
    let vals = [
        t.key_hold_ms,
        t.mouse_hold_ms,
        t.key_to_click_ms,
        t.repeat_period_ms,
        t.escape_gap_ms[0],
        t.escape_gap_ms[1],
        t.card_pre_esc_ms,
        t.hold_maintain_period_ms,
    ];
    let mut ticks = [0; 8];
    for (i, v) in vals.into_iter().enumerate() {
        match ticks_ms(v, rate) {
            Ok(n) => ticks[i] = n,
            Err(e) => errors.push(e),
        }
    }
    let a = analyze_conflicts(p, k);
    errors.extend(a.preset_errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    let timing = TickTiming {
        key_hold: ticks[0],
        mouse_hold: ticks[1],
        key_to_click: ticks[2],
        repeat_period: ticks[3],
        esc_gap_fast: ticks[4],
        esc_gap_slow: ticks[5],
        card_pre_esc: ticks[6],
        maintain: ticks[7],
        trailing: t.card_trailing_esc,
        auto_repeat: t.auto_repeat,
    };
    let mut cfg = CompiledConfig {
        version,
        triggers: Box::new([[NO_TRIGGER; 16]; 256]),
        actions: Vec::new(),
        timing,
        params,
        hand: h.layout(p.mode),
        slots: p.mode.slots(),
        slot_keys: h.slot_keys,
        order: h.slot_order,
        card_auto_repeat: h.card_auto_repeat,
        reports: a.reports,
        candidates: [false; 256],
    };
    fn add(cfg: &mut CompiledConfig, chord: Chord, spec: ActionSpec) {
        let id = cfg.actions.len() as u16;
        cfg.actions.push(spec);
        for m in 0..16 {
            if chord.matches(m) {
                cfg.triggers[chord.key.0 as usize][m as usize] = id;
            }
        }
    }
    for (role, preset) in &k.presets {
        if !preset.enabled {
            continue;
        }
        let Some(chord) = preset.hotkey else {
            continue;
        };
        let spec = if let Some(i) = role.strip_prefix("skill_") {
            ActionSpec::Tap(Key(48 + i.parse::<u8>().unwrap()))
        } else if let Some(i) = role.strip_prefix("spam_") {
            ActionSpec::Repeat {
                key: Some(Key(48 + i.parse::<u8>().unwrap())),
            }
        } else if let Some(i) = role.strip_prefix("card_") {
            ActionSpec::Card(Key(48 + i.parse::<u8>().unwrap()))
        } else {
            match role.as_str() {
                "pause" => ActionSpec::Tap(Key::ESC),
                "frame_fast" => ActionSpec::DoubleEsc { slow: false },
                "frame_slow" => ActionSpec::DoubleEsc { slow: true },
                "auto" => ActionSpec::Tap(Key(65)),
                "speed" => ActionSpec::Tap(Key(83)),
                "click_spam" => ActionSpec::Repeat { key: None },
                _ => continue,
            }
        };
        add(&mut cfg, chord, spec);
    }
    for item in &p.items {
        if a.disabled_points.contains(&item.id) {
            continue;
        }
        if let Some(chord) = item.hotkey {
            add(
                &mut cfg,
                chord,
                match item.function {
                    PointFunction::Card => ActionSpec::CardKey,
                    PointFunction::Point => ActionSpec::HoldPoint(NormPoint {
                        x: item.x.unwrap(),
                        y: item.y.unwrap(),
                    }),
                },
            );
        }
    }
    if cfg.actions.len() > usize::from(params.max_actions) {
        return Err(vec![ConfigError::new(
            "$.items",
            "actions exceed max_actions",
        )]);
    }
    for spec in &cfg.actions {
        match spec {
            ActionSpec::Tap(k) | ActionSpec::Card(k) => cfg.candidates[k.0 as usize] = true,
            ActionSpec::Repeat { key: Some(k) } => cfg.candidates[k.0 as usize] = true,
            ActionSpec::DoubleEsc { .. } => cfg.candidates[27] = true,
            _ => {}
        }
    }
    if cfg.actions.iter().any(|a| matches!(a, ActionSpec::CardKey)) {
        for key in h.slot_keys {
            cfg.candidates[key.0 as usize] = true;
        }
    }
    if cfg.actions.iter().any(|a| matches!(a, ActionSpec::Card(_))) {
        cfg.candidates[27] = true;
    }
    if cfg.actions.iter().any(|a| {
        matches!(
            a,
            ActionSpec::HoldPoint(_) | ActionSpec::Card(_) | ActionSpec::Repeat { .. }
        )
    }) {
        cfg.candidates[1] = true;
    }
    Ok(cfg)
}
