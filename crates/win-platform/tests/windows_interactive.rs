#![cfg(windows)]
//! Requires an interactive desktop and a separately launched baauto-receiver.exe.
//! cargo test -p baauto-win-platform --test windows_interactive -- --ignored --nocapture --test-threads=1
//! Focus receiver, press W and hold/release z; then Alt+Tab while holding z and release it.
//! The test waits for focus before Arm, runs for 30 seconds, and validates cooperative release.
use baauto_config::*;
use baauto_win_platform::*;
use std::{ffi::OsStr, sync::Arc};
#[test]
#[ignore = "needs receiver and physical foreground/focus interactions"]
fn foreground_hook_focus_loss_and_paired_up() {
    let p: PointSet = from_json(include_str!("../../../tests/fixtures/m2-basic.json")).unwrap();
    let k: KeysProfile = from_json(include_str!("../../../tests/fixtures/m2-keys.json")).unwrap();
    let clock = Clock::new().unwrap();
    let c = compile(&p, &k, &HandSettings::default(), clock.rate).unwrap();
    let dump = std::env::temp_dir().join(format!("baauto-m2-{}.csv", std::process::id()));
    run_standalone(
        Arc::new(c),
        OsStr::new("baauto-receiver.exe"),
        Some(30),
        Some(&dump),
    )
    .unwrap();
    let csv = std::fs::read_to_string(&dump).unwrap();
    assert!(
        csv.lines()
            .skip(1)
            .any(|line| line.split(',').nth(1) == Some("2")),
        "perform at least one action"
    );
    assert!(
        csv.lines()
            .skip(1)
            .any(|line| line.split(',').nth(1) == Some("4")),
        "Stopped must be recorded"
    );
    std::fs::remove_file(dump).unwrap();
}
