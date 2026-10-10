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
