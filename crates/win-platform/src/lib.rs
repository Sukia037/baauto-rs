#![deny(unsafe_op_in_unsafe_fn)]
pub mod geometry;
pub mod hook;
#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::*;

pub mod fault;

pub mod hotkeys;

/// Released builds keep the engine in an `engine` subfolder so players only see one exe;
/// development builds (`target/release`) keep it beside the caller.
pub fn engine_path() -> std::io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    let nested = exe.with_file_name("engine").join("baauto-engine.exe");
    Ok(if nested.is_file() {
        nested
    } else {
        exe.with_file_name("baauto-engine.exe")
    })
}
