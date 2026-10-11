//! UI-independent edits. Identity is always the item ID, never name or shortcut.
use crate::{legacy::Candidate, *};
pub type EditError = ConfigError;
#[derive(Clone, Debug, Default)]
pub struct ItemPatch {
    pub name: Option<String>,
    pub hotkey: Option<Option<Chord>>,
    pub function: Option<PointFunction>,
    pub position: Option<NormPoint>,
}
pub struct PointSetEditor {
    pub document: PointSet,
    next_candidate: u32,
    next_item: Option<u32>,
}
impl PointSetEditor {
    pub fn new(document: PointSet) -> Self {
        let next_item = document
            .items
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            .checked_add(1);
        Self {
            document,
            next_candidate: 1,
            next_item,
        }
    }
    pub fn set_mode(&mut self, mode: Mode) -> bool {
        let changed = self.document.mode != mode;
        self.document.mode = mode;
        changed
    }
    fn next_id(&mut self) -> Result<u32, EditError> {
        let id = self
            .next_item
            .ok_or_else(|| ConfigError::new("$.items.id", "ID space exhausted"))?;
        self.next_item = id.checked_add(1);
        Ok(id)
    }
    pub fn add_candidate(&mut self, c: Candidate) -> Result<u32, EditError> {
        if !c.position.valid() {
            return Err(ConfigError::new("$.candidate", "invalid coordinate"));
        }
        let id = self.next_id()?;
        self.document.items.push(PointItem {
            id,
            name: c.name,
            hotkey: None,
            function: PointFunction::Point,
            x: Some(c.position.x),
            y: Some(c.position.y),
        });
        Ok(id)
    }
    pub fn add_card_item(&mut self, name: String) -> Result<u32, EditError> {
        let id = self.next_id()?;
        self.document.items.push(PointItem {
            id,
            name,
            hotkey: None,
            function: PointFunction::Card,
            x: None,
            y: None,
        });
        Ok(id)
    }
    pub fn update_item(&mut self, id: u32, patch: ItemPatch) -> Result<bool, EditError> {
        let index = self
            .document
            .items
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| ConfigError::new("$.items.id", "not found"))?;
        let mut item = self.document.items[index].clone();
        if let Some(name) = patch.name {
            item.name = name;
        }
        if let Some(h) = patch.hotkey {
            if h.is_some_and(|h| h.key == Key::LEFT) {
                return Err(ConfigError::new(
                    "$.items.hotkey",
                    "LeftButton cannot trigger",
                ));
            }
            item.hotkey = h;
        }
        if let Some(f) = patch.function {
            item.function = f;
        }
        if let Some(p) = patch.position {
            if !p.valid() {
                return Err(ConfigError::new("$.items.position", "invalid coordinate"));
            }
            item.x = Some(p.x);
            item.y = Some(p.y);
        }
        if item.function == PointFunction::Card {
            item.x = None;
            item.y = None;
        } else if !item
            .x
            .zip(item.y)
            .is_some_and(|(x, y)| NormPoint { x, y }.valid())
        {
            return Err(ConfigError::new(
                "$.items.position",
                "ordinary point needs coordinates",
            ));
        }
        let affects = self.document.items[index].hotkey != item.hotkey;
        self.document.items[index] = item;
        Ok(affects)
    }
    pub fn delete_item(&mut self, id: u32) -> Result<bool, EditError> {
        let index = self
            .document
            .items
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| ConfigError::new("$.items.id", "not found"))?;
        self.document.items.remove(index);
        Ok(true)
    }
    pub fn move_to_candidates(&mut self, id: u32) -> Result<Candidate, EditError> {
        let p = self
            .document
            .items
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| ConfigError::new("$.items.id", "not found"))?;
        let position =
            p.x.zip(p.y)
                .map(|(x, y)| NormPoint { x, y })
                .filter(|p| p.valid())
                .ok_or_else(|| {
                    ConfigError::new("$.items.position", "card item has no candidate coordinate")
                })?;
        let c = Candidate {
            id: self.next_candidate,
            name: p.name.clone(),
            position,
        };
        self.next_candidate = self
            .next_candidate
            .checked_add(1)
            .ok_or_else(|| ConfigError::new("$.candidate.id", "ID space exhausted"))?;
        self.delete_item(id)?;
        Ok(c)
    }
}
pub struct KeysEditor {
    pub document: KeysProfile,
}
impl KeysEditor {
    pub fn new(document: KeysProfile) -> Self {
        Self { document }
    }
    pub fn set_preset(
        &mut self,
        role: &str,
        hotkey: Option<Chord>,
        enabled: bool,
        exact: bool,
    ) -> Result<bool, EditError> {
        if role == "global_stop" && (!enabled || hotkey.is_none()) {
            return Err(ConfigError::new(
                "$.presets.global_stop",
                "緊急停止必須啟用並設定熱鍵",
            ));
        }
        let preset = self
            .document
            .presets
            .get_mut(role)
            .ok_or_else(|| ConfigError::new("$.presets", "unknown role"))?;
        if hotkey.is_some_and(|h| h.key == Key::LEFT) {
            return Err(ConfigError::new(
                "$.presets.hotkey",
                "LeftButton cannot trigger",
            ));
        }
        let h = hotkey.map(|mut c| {
            let mask = match c.mods {
                ModsMatch::AtLeast(m) | ModsMatch::Exact(m) => m,
            };
            c.mods = if exact {
                ModsMatch::Exact(mask)
            } else {
                ModsMatch::AtLeast(mask)
            };
            c
        });
        let changed = preset.hotkey != h || preset.enabled != enabled;
        *preset = Preset { hotkey: h, enabled };
        Ok(changed)
    }
    pub fn reset_preset(&mut self, role: &str) -> Result<bool, EditError> {
        let value = template_keys()
            .presets
            .get(role)
            .cloned()
            .ok_or_else(|| ConfigError::new("$.presets", "unknown role"))?;
        let changed = self.document.presets.get(role) != Some(&value);
        self.document.presets.insert(role.into(), value);
        Ok(changed)
    }
    pub fn set_timing(&mut self, mut timing: TimingProfile) -> Result<bool, EditError> {
        timing.verified = false;
        let mut proposed = self.document.clone();
        proposed.timing = timing;
        let mut validation = proposed.clone();
        for p in validation.presets.values_mut() {
            p.enabled = false;
        }
        compile(
            &PointSet {
                items: Vec::new(),
                ..template_points()
            },
            &validation,
            &HandSettings::default(),
            TickRate(1_000_000),
        )
        .map_err(|e| e.into_iter().next().unwrap())?;
        let changed = proposed != self.document;
        self.document = proposed;
        Ok(changed)
    }
}
pub struct HandEditor {
    pub document: HandSettings,
}
impl HandEditor {
    pub fn new(document: HandSettings) -> Self {
        Self { document }
    }
    pub fn set_slot_keys(&mut self, keys: [Key; 5]) -> Result<bool, EditError> {
        if keys.iter().any(|k| k.is_mouse() || k.0 == 0)
            || keys.iter().enumerate().any(|(i, k)| keys[..i].contains(k))
        {
            return Err(ConfigError::new(
                "$.slot_keys",
                "five unique keyboard keys required",
            ));
        }
        let changed = self.document.slot_keys != keys;
        self.document.slot_keys = keys;
        Ok(changed)
    }
    pub fn set_card_auto_repeat(&mut self, p: AutoRepeatPolicy) -> bool {
        let changed = self.document.card_auto_repeat != p;
        self.document.card_auto_repeat = p;
        changed
    }
    pub fn set_card_ui_side(&mut self, s: CardUiSide) -> bool {
        let changed = self.document.card_ui_side != s;
        self.document.card_ui_side = s;
        changed
    }
    pub fn set_slot_order(&mut self, o: SlotOrder) -> bool {
        let changed = self.document.slot_order != o;
        self.document.slot_order = o;
        changed
    }
    pub fn set_hand_rect(
        &mut self,
        mode: Mode,
        side: CardUiSide,
        rect: HandRect,
    ) -> Result<bool, EditError> {
        if !rect.valid() {
            return Err(ConfigError::new("$.hand.rect", "invalid rectangle"));
        }
        let sides = match mode {
            Mode::Six => &mut self.document.rects.six,
            Mode::Ten => &mut self.document.rects.ten,
        };
        let value = match side {
            CardUiSide::Right => &mut sides.right,
            CardUiSide::Left => &mut sides.left,
        };
        let changed = *value != rect;
        *value = rect;
        Ok(changed)
    }
}
pub fn analyze(points: &PointSet, keys: &KeysProfile) -> ConflictAnalysis {
    analyze_conflicts(points, keys)
}

impl KeysEditor {
    pub fn reset_all(&mut self) {
        self.document.presets = template_keys().presets;
    }
    pub fn install_calibrated_timing(&mut self, timing: TimingProfile) -> Result<(), EditError> {
        if timing.source != ProfileSource::Calibrated || !timing.verified {
            return Err(ConfigError::new("$.timing", "校準結果尚未驗證"));
        }
        let mut proposed = self.document.clone();
        proposed.timing = timing;
        crate::store::Document::Keys(Box::new(proposed.clone())).validate()?;
        self.document = proposed;
        Ok(())
    }
}
impl HandEditor {
    pub fn mirror(&mut self, mode: Mode, side: CardUiSide) -> Result<bool, EditError> {
        let mut view = self.document.clone();
        view.card_ui_side = side;
        let r = view.layout(mode);
        let other = if side == CardUiSide::Right {
            CardUiSide::Left
        } else {
            CardUiSide::Right
        };
        self.set_hand_rect(
            mode,
            other,
            HandRect {
                x: [1. - r.x[1], 1. - r.x[0]],
                calibrated: false,
                ..r
            },
        )
    }
    pub fn reset_layout(&mut self, mode: Mode, side: CardUiSide) -> Result<bool, EditError> {
        let defaults = HandSettings {
            card_ui_side: side,
            ..Default::default()
        };
        self.set_hand_rect(mode, side, defaults.layout(mode))
    }
}
