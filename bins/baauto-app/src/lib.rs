#![forbid(unsafe_code)]
pub mod backend;
pub mod fonts;
pub mod gui;
pub mod state;

#[cfg(windows)]
mod telemetry;
