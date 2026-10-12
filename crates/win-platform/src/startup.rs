//! Portable checks for environment problems players hit before BAauto can run.

/// Names a running tool whose low-level keyboard hooks can swallow BAauto's hotkeys.
/// Hooks installed later are called first, so such a tool hides keys from the Engine Hook
/// while Raw Input still sees them, and the supervisor stops with a fault.
pub fn hook_conflict(exe_name: &str) -> Option<&'static str> {
    let name = exe_name.to_ascii_lowercase();
    (name.starts_with("autohotkey") && name.ends_with(".exe")).then_some("AutoHotkey")
}

/// Shown when `engine_path()` finds no engine beside the app.
pub const MISSING_ENGINE: &str = "找不到引擎 engine\\baauto-engine.exe，BAauto 無法啟動。\n\n\
常見原因：\n\
・直接在 zip 裡雙擊開啟：請先對 zip 按右鍵「解壓縮全部」，再從解壓出來的資料夾開 baauto-app.exe。\n\
・engine 資料夾被移走，或被防毒軟體刪掉：重新解壓一次，並把程式資料夾加入防毒例外。";
