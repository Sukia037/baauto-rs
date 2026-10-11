//! Native file operations run only on the app's file worker.
use baauto_config::store::FsBackend;
use std::{
    fs::File,
    io,
    os::windows::{ffi::OsStringExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{Foundation::*, Storage::FileSystem::*},
    core::PCWSTR,
};
#[derive(Clone, Copy)]
pub struct WindowsFs;
fn long_path(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let absolute = std::path::absolute(path)?;
    let units: Vec<u16> = absolute
        .as_os_str()
        .encode_wide()
        .map(|c| if c == 47 { 92 } else { c })
        .collect();
    if units.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "NUL in path"));
    }
    let mut result = if units.starts_with(&[92, 92, 63, 92]) {
        units
    } else if units.starts_with(&[92, 92]) {
        let mut prefix: Vec<u16> = "\\\\?\\UNC\\".encode_utf16().collect();
        prefix.extend_from_slice(&units[2..]);
        prefix
    } else {
        let mut prefix: Vec<u16> = "\\\\?\\".encode_utf16().collect();
        prefix.extend(units);
        prefix
    };
    result.push(0);
    Ok(result)
}
fn native_error() -> io::Error {
    io::Error::last_os_error()
}
impl FsBackend for WindowsFs {
    fn identity(&self, path: &Path) -> io::Result<PathBuf> {
        let file = File::open(path)?;
        let handle = HANDLE(file.as_raw_handle());
        let mut buffer = vec![0u16; 512];
        loop {
            // SAFETY: The live File owns the handle and buffer is writable for its complete advertised length. File remains open through retries.
            let count = unsafe {
                GetFinalPathNameByHandleW(handle, &mut buffer, GETFINALPATHNAMEBYHANDLE_FLAGS(0))
            };
            if count == 0 {
                return Err(native_error());
            }
            if count as usize >= buffer.len() {
                buffer.resize(count as usize + 1, 0);
                continue;
            }
            buffer.truncate(count as usize);
            return Ok(std::ffi::OsString::from_wide(&buffer).into());
        }
    }
    fn replace(&self, temp: &Path, destination: &Path, backup: &Path) -> io::Result<()> {
        let replacement = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(temp)?;
        // SAFETY: File remains open and writable until FlushFileBuffers completes; no pointers retained.
        unsafe {
            FlushFileBuffers(HANDLE(replacement.as_raw_handle())).map_err(|_| native_error())?;
        }
        drop(replacement);
        let t = long_path(temp)?;
        let d = long_path(destination)?;
        let b = long_path(backup)?;
        if destination.exists() {
            // Keep a recoverable old copy even if ReplaceFileW partially renames before failing.
            std::fs::copy(destination, backup)?;
            std::fs::OpenOptions::new()
                .write(true)
                .open(backup)?
                .sync_all()?;
            // SAFETY: Three NUL-terminated absolute same-volume paths remain valid through the call; optional reserved parameters are null.
            if unsafe {
                ReplaceFileW(
                    PCWSTR(d.as_ptr()),
                    PCWSTR(t.as_ptr()),
                    PCWSTR(b.as_ptr()),
                    REPLACE_FILE_FLAGS(0),
                    None,
                    None,
                )
            }
            .is_ok()
            {
                return Ok(());
            }
            let error = native_error();
            let code = error.raw_os_error().unwrap_or(0) as u32;
            if !destination.exists() && backup.exists() {
                // SAFETY: On partial failure the backup contains the old complete file; restore its original path before reporting the failure.
                unsafe {
                    MoveFileExW(
                        PCWSTR(b.as_ptr()),
                        PCWSTR(d.as_ptr()),
                        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                    )
                    .map_err(|_| native_error())?;
                }
                return Err(error);
            }
            if code != ERROR_NOT_SUPPORTED.0 && code != ERROR_INVALID_FUNCTION.0 {
                return Err(error);
            }
        }
        // SAFETY: Same-directory validated temporary file; MoveFileExW replaces atomically where supported and waits for the write-through operation.
        unsafe {
            MoveFileExW(
                PCWSTR(t.as_ptr()),
                PCWSTR(d.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
            .map_err(|_| native_error())
        }
    }
}
