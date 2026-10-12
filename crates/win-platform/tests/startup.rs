use baauto_win_platform::startup::{MISSING_ENGINE, hook_conflict};
#[test]
fn autohotkey_variants_are_reported_as_hook_conflicts() {
    for name in [
        "AutoHotkey.exe",
        "AutoHotkey64.exe",
        "AutoHotkeyU64.exe",
        "AutoHotkey32.exe",
        "AutoHotkeyUX.exe",
        "autohotkey64.EXE",
    ] {
        assert_eq!(hook_conflict(name), Some("AutoHotkey"), "{name}");
    }
}
#[test]
fn unrelated_processes_are_not_hook_conflicts() {
    for name in [
        "BlueArchive.exe",
        "baauto-app.exe",
        "explorer.exe",
        "AutoHotkey.ahk",
        "MyAutoHotkey.exe",
        "",
    ] {
        assert_eq!(hook_conflict(name), None, "{name}");
    }
}
#[test]
fn missing_engine_message_names_the_engine_and_both_common_causes() {
    assert!(MISSING_ENGINE.contains(r"engine\baauto-engine.exe"));
    assert!(MISSING_ENGINE.contains("zip"));
    assert!(MISSING_ENGINE.contains("防毒"));
}
