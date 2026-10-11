#![forbid(unsafe_code)]
mod compile;
mod key;
mod model;
pub use compile::*;
pub use key::*;
pub use model::*;

pub fn from_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, ConfigError> {
    let mut de = serde_json::Deserializer::from_str(text.trim_start_matches('\u{feff}'));
    let value = serde_path_to_error::deserialize(&mut de)
        .map_err(|e| ConfigError::new(format!("$.{}", e.path()), e.inner().to_string()))?;
    de.end().map_err(|e| ConfigError::new("$", e.to_string()))?;
    Ok(value)
}
pub fn to_json<T: serde::Serialize>(value: &T) -> Result<String, ConfigError> {
    serde_json::to_string_pretty(value)
        .map(|s| s + "\n")
        .map_err(|e| ConfigError::new("$", e.to_string()))
}

pub mod editor;
pub mod legacy;
pub mod store;
pub fn template_points() -> PointSet {
    from_json(include_str!("../../../points/範例.json")).expect("validated embedded point template")
}
pub fn template_keys() -> KeysProfile {
    from_json(include_str!("../../../keys/預設按鍵.json")).expect("validated embedded key template")
}
