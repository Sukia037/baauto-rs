//! Legacy parsers are tolerant only with explicit diagnostics. Runtime formats remain strict.
use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: u32,
    pub name: String,
    pub position: NormPoint,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub path: String,
    pub code: String,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdMapping {
    pub old: String,
    pub new: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    pub issues: Vec<Issue>,
    pub id_map: Vec<IdMapping>,
    pub pending_resolution: Vec<Value>,
    pub source_timing: Option<Value>,
}
impl ImportReport {
    fn note(&mut self, path: &str, code: &str, reason: impl Into<String>) {
        self.issues.push(Issue {
            path: path.into(),
            code: code.into(),
            reason: reason.into(),
        });
    }
}
#[derive(Clone, Debug)]
pub struct Imported {
    pub points: Option<PointSet>,
    pub keys: Option<KeysProfile>,
    pub candidates: Vec<Candidate>,
    pub report: ImportReport,
}
/// Side prefixes are recognized and normalized to the generic modifier, with a warning.
/// Wheel pulses have no paired Down/Up representation in the current engine; they are disabled.
pub fn parse_ahk(text: &str) -> Result<(Chord, Vec<String>), String> {
    let mut s = text.trim();
    let mut mask = 0;
    let mut warnings = Vec::new();
    while let Some(c) = s.chars().next() {
        match c {
            '~' => return Err("pass-through (~) is unsupported".into()),
            '$' | '*' => s = &s[1..],
            '<' | '>' => {
                let next = s.as_bytes().get(1).copied();
                if !matches!(next, Some(b'^' | b'!' | b'+' | b'#')) {
                    return Err("side prefix needs a modifier".into());
                }
                warnings.push("side_modifier_normalized".into());
                s = &s[1..];
            }
            '^' | '!' | '+' | '#' => {
                let bit = match c {
                    '^' => 1,
                    '+' => 2,
                    '!' => 4,
                    _ => 8,
                };
                if mask & bit != 0 {
                    return Err("duplicate modifier".into());
                }
                mask |= bit;
                s = &s[1..];
            }
            _ => break,
        }
    }
    if s.to_ascii_lowercase().starts_with("wheel") {
        return Err("wheel triggers are recognized but unsupported by paired-key input".into());
    }
    let key = s.parse::<Key>()?;
    if key == Key::LEFT {
        return Err("LeftButton cannot trigger macros".into());
    }
    Ok((
        Chord {
            key,
            mods: ModsMatch::AtLeast(mask),
        },
        warnings,
    ))
}
fn number(v: &Value, path: &str, report: &mut ImportReport) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => {
            report.note(
                path,
                "boolean_number",
                "legacy JsonDump boolean repaired in numeric field",
            );
            Some(if *b { 1. } else { 0. })
        }
        Value::Null => None,
        _ => {
            report.note(path, "wrong_type", "expected numeric coordinate");
            None
        }
    }
}
fn coordinate(v: &Value, path: &str, r: &mut ImportReport) -> Option<NormPoint> {
    let x = number(&v["rel_x"], &format!("{path}.rel_x"), r)?;
    let y = number(&v["rel_y"], &format!("{path}.rel_y"), r)?;
    let p = NormPoint { x, y };
    if p.valid() {
        Some(p)
    } else {
        r.note(path, "invalid_coordinate", "coordinate outside 0–1");
        None
    }
}
fn hotkey(v: &Value, path: &str, r: &mut ImportReport) -> Option<Chord> {
    let Some(s) = v.as_str() else {
        if !v.is_null() {
            r.note(path, "wrong_type", "hotkey must be string");
        }
        return None;
    };
    if s.trim().is_empty() {
        return None;
    }
    match parse_ahk(s) {
        Ok((c, w)) => {
            for code in w {
                r.note(
                    path,
                    &code,
                    "left/right modifier normalized; review binding",
                );
            }
            Some(c)
        }
        Err(e) => {
            r.note(path, "unsupported_hotkey", e);
            None
        }
    }
}
fn text(v: &Value, field: &str, fallback: String) -> String {
    v[field].as_str().map(str::to_owned).unwrap_or(fallback)
}
fn id(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "(missing)".into(),
        _ => v.to_string(),
    }
}
pub fn import(
    data: &Value,
    name: &str,
    mode: Mode,
    resolution: Option<[f64; 2]>,
) -> Result<Imported, ConfigError> {
    if !data.is_object() {
        return Err(ConfigError::new("$", "legacy root must be object"));
    }
    let mut out = Imported {
        points: None,
        keys: None,
        candidates: Vec::new(),
        report: ImportReport::default(),
    };
    let r = &mut out.report;
    match data["schema"].as_str() {
        Some("baauto-functions-v1") => {
            let mut k = template_keys();
            let hotkeys = data["hotkeys"]
                .as_object()
                .ok_or_else(|| ConfigError::new("$.hotkeys", "expected object"))?;
            for (role, v) in hotkeys {
                let path = format!("$.hotkeys.{role}");
                if let Some(p) = k.presets.get_mut(role) {
                    p.hotkey = hotkey(v, &path, r);
                    p.enabled = p.hotkey.is_some();
                } else {
                    r.note(&path, "unknown_role", "unknown preset ignored");
                }
            }
            k.timing.source = ProfileSource::ImportedLegacy;
            k.timing.verified = false;
            if !data["timing_ms"].is_null() {
                r.source_timing = Some(data["timing_ms"].clone());
            }
            r.note("$.timing_ms","timing_unverified","AHK ignored timing_ms; source values retained only in this report. Actual waits depend on Windows timer granularity; calibrate before trusting them.");
            out.keys = Some(k);
            return Ok(out);
        }
        Some("baauto-points-v1") | None => {}
        Some(_) => return Err(ConfigError::new("$.schema", "unsupported legacy schema")),
    }
    if let Some(maps) = data["keymaps"].as_array() {
        for (index, m) in maps.iter().enumerate() {
            let path = format!("$.keymaps[{index}]");
            let trigger = m["key"]["text"].as_str().unwrap_or("").trim();
            if m["key"]["device"]
                .as_str()
                .is_some_and(|d| d.eq_ignore_ascii_case("mouse"))
                || trigger.is_empty()
            {
                continue;
            }
            let base = coordinate(&m["rel_work_position"], &path, r)
                .or_else(|| coordinate(&m["icon"]["rel_position"], &path, r));
            let p = if m["type"].as_str() == Some("Macro") {
                let p = action_position(&m["press_actions"])
                    .or_else(|| action_position(&m["release_actions"]));
                if p.zip(base)
                    .is_some_and(|(a, b)| (a.x - b.x).abs() < 0.0001 && (a.y - b.y).abs() < 0.0001)
                {
                    continue;
                }
                p
            } else {
                base
            };
            if let Some(position) = p.filter(|p| p.valid() && !ignored(*p)) {
                out.candidates.push(Candidate {
                    id: out.candidates.len() as u32 + 1,
                    name: trigger.into(),
                    position,
                });
            }
        }
        return Ok(out);
    }
    let mut p = PointSet {
        schema: "baauto-rs-points-v1".into(),
        name: name.into(),
        mode,
        items: Vec::new(),
    };
    if let Some(bindings) = data["bindings"].as_array() {
        let points: Vec<(String, &Value)> = match &data["points"] {
            Value::Array(a) => a.iter().map(|v| (id(&v["id"]), v)).collect(),
            Value::Object(o) => o
                .iter()
                .map(|(key, v)| (v.get("id").map(id).unwrap_or_else(|| key.clone()), v))
                .collect(),
            Value::Null => Vec::new(),
            _ => {
                r.note(
                    "$.points",
                    "wrong_type",
                    "expected array or object; ignored",
                );
                Vec::new()
            }
        };
        let mut used = HashSet::new();
        let mut ids = BTreeSet::new();
        for (index, b) in bindings.iter().enumerate() {
            let path = format!("$.bindings[{index}]");
            if !b.is_object() {
                r.note(&path, "wrong_type", "binding must be object");
                continue;
            }
            let old = id(&b["id"]);
            if old != "(missing)" && !ids.insert(old.clone()) {
                r.note(&path, "duplicate_id", format!("reassigned duplicate {old}"));
            }
            let reference = id(&b["point_id"]);
            let source = points
                .iter()
                .find(|(key, _)| *key == reference)
                .map(|(_, v)| *v);
            if source.is_some() {
                used.insert(reference.clone());
            } else if reference != "(missing)" {
                r.note(
                    &path,
                    "dangling_point",
                    format!("missing point {reference}; binding coordinates retained if present"),
                );
            }
            let name = text(b, "name", format!("點位 {}", index + 1));
            let h = hotkey(
                b.get("hotkey").unwrap_or(&b["key"]),
                &format!("{path}.hotkey"),
                r,
            );
            let position = coordinate(b, &path, r).or_else(|| {
                source.and_then(|v| coordinate(v, &format!("$.points[{reference}]"), r))
            });
            let function = match b["function"].as_str().unwrap_or("normal") {
                "normal" | "normal_point" => PointFunction::Point,
                "alternating" => {
                    r.note(
                        &path,
                        "alternating_to_card",
                        if position.is_some() {
                            "drag endpoint becomes the card key target; cursor-under-card chooses the key"
                        } else {
                            "drag endpoint missing; card key disabled until its position is recorded"
                        },
                    );
                    PointFunction::Card
                }
                _ => {
                    r.note(&path, "unknown_function", "unsupported action disabled");
                    continue;
                }
            };
            if function == PointFunction::Point && position.is_none() {
                r.note(
                    &path,
                    "missing_coordinate",
                    "ordinary point requires a valid coordinate",
                );
                continue;
            }
            let new = p.items.len() as u32 + 1;
            r.id_map.push(IdMapping { old, new });
            p.items.push(PointItem {
                id: new,
                name,
                hotkey: h,
                function,
                x: position.map(|p| p.x),
                y: position.map(|p| p.y),
            });
        }
        for (old, _) in points {
            if !used.contains(&old) {
                r.note(
                    "$.points",
                    "orphan_point",
                    format!("unreferenced {old} removed"),
                );
            }
        }
    } else if let Some(items) = data["keys"].as_array() {
        let width = number(data.get("width").unwrap_or(&data["screenW"]), "$.width", r);
        let height = number(
            data.get("height").unwrap_or(&data["screenH"]),
            "$.height",
            r,
        );
        let dimensions = resolution
            .or_else(|| width.zip(height).map(|(w, h)| [w, h]))
            .filter(|d| d.iter().all(|n| n.is_finite() && *n > 0.));
        for (index, item) in items.iter().enumerate() {
            let path = format!("$.keys[{index}]");
            let mut position = coordinate(item, &path, r);
            if position.is_none() {
                let absolute = number(&item["x"], &path, r).zip(number(&item["y"], &path, r));
                if let Some((x, y)) = absolute {
                    if let Some([w, h]) = dimensions {
                        position = Some(NormPoint { x: x / w, y: y / h }).filter(|p| p.valid());
                    } else {
                        r.pending_resolution.push(item.clone());
                        r.note(
                            &path,
                            "needs_resolution",
                            "absolute coordinates need the original resolution",
                        );
                        continue;
                    }
                }
            }
            if let Some(position) = position {
                let new = p.items.len() as u32 + 1;
                p.items.push(PointItem {
                    id: new,
                    name: text(item, "name", format!("舊版點位 {}", index + 1)),
                    hotkey: hotkey(&item["key"], &path, r),
                    function: PointFunction::Point,
                    x: Some(position.x),
                    y: Some(position.y),
                });
                r.id_map.push(IdMapping {
                    old: id(&item["id"]),
                    new,
                });
            }
        }
    } else {
        return Err(ConfigError::new("$", "expected bindings, keys or keymaps"));
    }
    out.points = Some(p);
    Ok(out)
}
fn action_position(v: &Value) -> Option<NormPoint> {
    for action in v.as_array()? {
        let Some(text) = action.as_str() else {
            continue;
        };
        for prefix in ["click_rel:(", "curve_rel:("] {
            if let Some(rest) = text.split_once(prefix).map(|(_, s)| s)
                && let Some(pair) = rest.split(')').next()
                && let Some((x, y)) = pair.split_once(',')
                && let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse())
            {
                return Some(NormPoint { x, y });
            }
        }
    }
    None
}
fn ignored(p: NormPoint) -> bool {
    [
        (1700., 2330., 1125., 1330.),
        (2324., 2526., 1203., 1395.),
        (2419., 2516., 42., 134.),
    ]
    .iter()
    .any(|&(l, r, t, b)| {
        p.x >= l / 2560. && p.x <= r / 2560. && p.y >= t / 1440. && p.y <= b / 1440.
    })
}
