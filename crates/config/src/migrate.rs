//! Reading older preset-key files. `baauto-rs-keys-v1` timing is converted to the v2 gap
//! sets (docs/decisions.md, 2026-10-12): values with the same meaning carry over, every other
//! set takes the v2 template value, and the profile is marked unverified.
use crate::*;
use serde_json::Value;

/// Parses a preset-key file of either schema. The notes list what a v1 conversion replaced;
/// they are empty for v2 files.
pub fn parse_keys(text: &str) -> Result<(KeysProfile, Vec<String>), ConfigError> {
    let value: Value = from_json(text)?;
    match value["schema"].as_str() {
        Some(KEYS_SCHEMA) => Ok((from_json(text)?, Vec::new())),
        Some(KEYS_SCHEMA_V1) => migrate_v1(&value),
        _ => Err(ConfigError::new(
            "$.schema",
            format!("expected {KEYS_SCHEMA} or {KEYS_SCHEMA_V1}"),
        )),
    }
}

fn migrate_v1(value: &Value) -> Result<(KeysProfile, Vec<String>), ConfigError> {
    let presets = serde_json::from_value(value["presets"].clone())
        .map_err(|e| ConfigError::new("$.presets", e.to_string()))?;
    let old = &value["timing"];
    let mut timing = template_keys().timing;
    if let Ok(identity) = serde_json::from_value(old["identity"].clone()) {
        timing.identity = identity;
    }
    if let Ok(source) = serde_json::from_value(old["source"].clone()) {
        timing.source = source;
    }
    timing.verified = false;
    let mut notes = Vec::new();
    let mut carry = |from: &str, value: Option<f64>, to: &mut f64| match value {
        Some(v) => *to = v,
        None => notes.push(format!("$.timing.{from} 缺少或格式不對，改用範本值")),
    };
    carry(
        "escape_gap_ms[0]",
        old["escape_gap_ms"][0].as_f64(),
        &mut timing.frame_fast.esc_gap_ms,
    );
    carry(
        "escape_gap_ms[1]",
        old["escape_gap_ms"][1].as_f64(),
        &mut timing.frame_slow.esc_gap_ms,
    );
    carry(
        "card_pre_esc_ms",
        old["card_pre_esc_ms"].as_f64(),
        &mut timing.card.esc_to_key_ms,
    );
    carry(
        "hold_maintain_period_ms",
        old["hold_maintain_period_ms"].as_f64(),
        &mut timing.point.maintain_period_ms,
    );
    notes.push(
        "舊版時間格式已轉換：spam、click_spam、card.key_to_click_ms、card.click_to_esc_ms、point_card 改用範本值，這組時間標成未驗證".into(),
    );
    Ok((
        KeysProfile {
            schema: KEYS_SCHEMA.into(),
            presets,
            timing,
        },
        notes,
    ))
}
