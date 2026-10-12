#![deny(unsafe_op_in_unsafe_fn)]
pub mod geometry;
pub mod hook;
#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
pub use windows_impl::*;

pub mod fault;

pub mod hotkeys;

pub mod startup;

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

/// Exit code the supervisor passes to `TerminateProcess`, so a forced stop is distinguishable
/// from a crash or an outside kill.
pub const TERMINATED_BY_SUPERVISOR: u32 = 0x0BAA_0001;

/// One-line reason shown in the log tab when the engine process ends on its own.
pub fn describe_engine_exit(code: Option<u32>) -> String {
    let Some(code) = code else {
        return "引擎意外結束，讀不到結束代碼".into();
    };
    let reason = match code {
        0 => "引擎自己正常結束",
        1 => "引擎回報錯誤後結束，原因見 logs\\engine-error.txt",
        101 => "引擎內部錯誤（panic），原因見 logs\\engine-error.txt",
        TERMINATED_BY_SUPERVISOR => "停止逾時，被監督程式強制結束",
        0xC000_0005 => "程式當掉（記憶體存取違規）",
        0xC000_00FD => "程式當掉（堆疊溢位）",
        0xC000_0409 => "程式偵測到內部錯誤後中止",
        0xC000_0135 | 0xC000_0139 => "找不到需要的系統元件（DLL）",
        0xC000_0022 => "存取被拒，可能被防毒軟體或權限擋下",
        0xC000_013A => "被 Ctrl+C 或關閉主控台中止",
        _ => "被其他程式強制結束或原因不明；常見是防毒軟體擋下 baauto-engine.exe",
    };
    format!("引擎意外結束：{reason}（結束代碼 {code}，0x{code:08X}）")
}

/// Appends one line to the engine error log; failures are ignored because this only runs on
/// the way out of an already failing engine.
pub fn append_error_log(path: &std::path::Path, message: &str) {
    use std::io::Write;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "[unix {secs}] {message}");
    }
}
