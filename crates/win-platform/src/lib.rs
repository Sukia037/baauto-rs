#![deny(unsafe_op_in_unsafe_fn)]
pub mod geometry;
pub mod hook;
#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::*;
