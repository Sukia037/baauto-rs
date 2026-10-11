#![cfg(windows)]
//! An occupied global hotkey must keep the supervisor alive, block readiness, and recover on rebind.
use baauto_config::Chord;
use baauto_win_platform::HealthWindow;

#[test]
fn occupied_stop_hotkey_warns_and_recovers_after_rebind() {
    let taken: Chord = "Ctrl+Alt+Shift+F22".parse().unwrap();
    let free: Chord = "Ctrl+Alt+Shift+F21".parse().unwrap();
    let record: Chord = "Ctrl+Alt+Shift+F20".parse().unwrap();
    let owner = HealthWindow::new([false; 256], taken, None).unwrap();
    assert!(owner.stop_registered());
    assert!(owner.hotkey_warning().is_none());

    let mut second = HealthWindow::new([false; 256], taken, Some(record)).unwrap();
    assert!(!second.stop_registered());
    let warning = second.hotkey_warning().unwrap();
    assert!(
        warning.contains("緊急停止") && warning.contains("F22"),
        "{warning}"
    );

    second.rebind(free, Some(record));
    assert!(second.stop_registered());
    assert!(second.hotkey_warning().is_none());

    second.rebind(free, Some(taken));
    assert!(
        second.stop_registered(),
        "record conflict must not block Arm"
    );
    let warning = second.hotkey_warning().unwrap();
    assert!(
        warning.contains("錄點") && !warning.contains("緊急停止"),
        "{warning}"
    );
    drop(owner);
}
