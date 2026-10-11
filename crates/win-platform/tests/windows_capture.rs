#![cfg(all(windows, feature = "test-hooks"))]
//! Requires an interactive Windows desktop. Build release workspace with test-hooks first.
//! Run: cargo test -p baauto-win-platform --features test-hooks --test windows_capture -- --ignored --nocapture --test-threads=1
//! Focus the receiver when prompted; foreign keyboard injections are test-only.
//! If the default global hotkeys are occupied, set BAAUTO_TEST_STOP_HOTKEY and
//! BAAUTO_TEST_RECORD_HOTKEY to unused chords. This only changes the test profile.
use baauto_config::*;
use baauto_protocol::{capture::*, ipc::EngineToApp, supervisor::State};
use baauto_win_platform::*;
use std::{
    ffi::OsStr,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
struct Guard {
    receiver: Process,
    quit: Option<mpsc::Sender<SupervisorCommand>>,
    join: Option<thread::JoinHandle<windows::core::Result<()>>>,
    extra_keys: Vec<Key>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        for key in [32, 87, 50, 90, 17, 16, 18, 91]
            .into_iter()
            .map(Key)
            .chain(self.extra_keys.iter().copied())
        {
            let _ = inject_test_key(key, false);
        }
        if let Some(tx) = &self.quit {
            let _ = tx.send(SupervisorCommand::Quit);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        if !self.receiver.exited() {
            let _ = self.receiver.terminate();
            let _ = self.receiver.wait_exit();
        }
    }
}
fn final_reply(rx: &mpsc::Receiver<Result<EngineToApp, String>>) -> EngineToApp {
    loop {
        let value = rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        if !matches!(value, EngineToApp::CaptureReady { .. }) {
            return value;
        }
    }
}
fn request(
    tx: &mpsc::Sender<SupervisorCommand>,
    generation: u64,
    id: u64,
    timeout_ms: u32,
    kind: CaptureKind,
) -> mpsc::Receiver<Result<EngineToApp, String>> {
    let (reply, rx) = mpsc::channel();
    tx.send(SupervisorCommand::Capture {
        kind,
        id,
        timeout_ms,
        generation,
        reply,
    })
    .unwrap();
    rx
}
#[test]
#[ignore = "requires Windows receiver, foreground focus and test-hooks engine"]
fn ipc_capture_timeout_cancel_chord_and_armed_rejection() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let bin = root.join("target/release");
    let folder = std::env::temp_dir().join(format!("baauto-capture-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let receiver = Process::spawn(
        bin.join("baauto-receiver.exe").as_os_str(),
        &[
            "--output".into(),
            folder.join("receiver.csv").into_os_string(),
        ],
    )
    .unwrap();
    let mut guard = Guard {
        receiver,
        quit: None,
        join: None,
        extra_keys: Vec::new(),
    };
    let clock = Clock::new().unwrap();
    let due = clock.now().after(clock.rate.0 * 15);
    let target = loop {
        if let Some(target) = target_for_foreground(OsStr::new("baauto-receiver.exe")).unwrap() {
            break target;
        }
        assert!(clock.now() < due, "focus receiver");
        wait_control_period(clock, clock.rate.0 / 20).unwrap();
    };
    assert!(
        folder.join("receiver.csv").exists(),
        "receiver must stream evidence before it closes"
    );
    let mut keys = template_keys();
    for (preset, variable) in [
        ("global_stop", "BAAUTO_TEST_STOP_HOTKEY"),
        ("record", "BAAUTO_TEST_RECORD_HOTKEY"),
    ] {
        if let Some(chord) = std::env::var_os(variable) {
            let chord = chord.to_str().expect("test hotkey must be valid Unicode");
            keys.presets.get_mut(preset).unwrap().hotkey =
                Some(chord.parse().expect("test hotkey must be a valid chord"));
        }
    }
    let cfg = Arc::new(
        compile(
            &template_points(),
            &keys,
            &HandSettings::default(),
            clock.rate,
        )
        .unwrap(),
    );
    let options = SupervisorOptions {
        cfg,
        engine: bin.join("baauto-engine.exe"),
        target: "baauto-receiver.exe".into(),
        stop_hotkey: keys.presets["global_stop"].hotkey.unwrap(),
        record_hotkey: keys.presets["record"].hotkey,
        health_timeout_ms: 1000,
        recovery_timeout_ms: 1000,
        logs: folder.clone(),
        injection: None,
        mode: baauto_core::RunMode::Normal,
    };
    let (tx, rx) = mpsc::channel();
    let (status, statuses) = mpsc::channel();
    let join = thread::spawn(move || run_supervisor(options, rx, status));
    guard.quit = Some(tx.clone());
    guard.join = Some(join);
    tx.send(SupervisorCommand::Enable).unwrap();
    let generation = loop {
        let s = statuses.recv_timeout(Duration::from_secs(10)).unwrap();
        if matches!(s.state, State::Armed(_)) {
            break s.resume_generation;
        }
    };
    assert!(
        request(&tx, generation, 1, 1000, CaptureKind::Hotkey)
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_err()
    );
    // Exercise the actual Hook -> Engine -> SendInput -> receiver path, not just IPC.
    assert!(inject_test_key(Key(87), true)); // w -> skill 1
    assert!(inject_test_key(Key(87), false));
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    assert!(inject_test_key(Key(50), true)); // hold 2 -> repeat 1 + LeftButton
    wait_control_period(clock, clock.rate.0 / 3).unwrap();
    assert!(inject_test_key(Key(50), false));
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    assert!(inject_test_key(Key(90), true)); // z -> HoldPoint
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    assert!(key_is_down(Key::LEFT), "HoldPoint must hold LeftButton");
    assert!(inject_test_key(Key(90), false));
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    assert!(
        !key_is_down(Key::LEFT),
        "HoldPoint release must reach Windows"
    );
    let (reply, stopped) = mpsc::channel();
    tx.send(SupervisorCommand::Quiesce { reply }).unwrap();
    let generation = stopped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    assert!(inject_test_key(Key(32), true));
    assert!(inject_test_key(Key(32), false));
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    let timeout = request(&tx, generation, 2, 300, CaptureKind::Point);
    assert!(matches!(
        timeout
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap(),
        EngineToApp::CaptureReady { request_id: 2, .. }
    ));
    assert!(matches!(
        final_reply(&timeout),
        EngineToApp::CapturedInput {
            result: CaptureResult {
                id: 2,
                outcome: CaptureOutcome::TimedOut
            },
            ..
        }
    ));
    let cancelled = request(&tx, generation, 3, 5000, CaptureKind::Hotkey);
    assert!(matches!(
        cancelled
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap(),
        EngineToApp::CaptureReady { request_id: 3, .. }
    ));
    tx.send(SupervisorCommand::CancelCapture { through_id: 3 })
        .unwrap();
    assert!(matches!(
        final_reply(&cancelled),
        EngineToApp::CapturedInput {
            result: CaptureResult {
                id: 3,
                outcome: CaptureOutcome::Cancelled
            },
            ..
        }
    ));
    let chord = request(&tx, generation, 4, 5000, CaptureKind::SlotKey);
    assert!(matches!(
        chord.recv_timeout(Duration::from_secs(2)).unwrap().unwrap(),
        EngineToApp::CaptureReady { request_id: 4, .. }
    ));
    assert!(inject_test_key(Key(32), true));
    let EngineToApp::CapturedInput { result, .. } = final_reply(&chord) else {
        panic!("expected captured Space");
    };
    assert_eq!(
        result.outcome,
        CaptureOutcome::Chord("Space".parse().unwrap())
    );
    assert!(inject_test_key(Key(32), false));
    // Re-arm explicitly, then stop a running repeat with the registered global chord.
    tx.send(SupervisorCommand::Enable).unwrap();
    loop {
        let status = statuses.recv_timeout(Duration::from_secs(5)).unwrap();
        if matches!(status.state, State::Armed(_)) {
            break;
        }
    }
    assert!(inject_test_key(Key(50), true));
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    let stop = keys.presets["global_stop"].hotkey.unwrap();
    let mask = match stop.mods {
        ModsMatch::AtLeast(mask) | ModsMatch::Exact(mask) => mask,
    };
    guard.extra_keys.push(stop.key);
    for (bit, key) in [(1, 17), (2, 16), (4, 18), (8, 91)] {
        if mask & bit != 0 {
            assert!(inject_test_key(Key(key), true));
        }
    }
    assert!(inject_test_key(stop.key, true));
    assert!(inject_test_key(stop.key, false));
    for (bit, key) in [(1, 17), (2, 16), (4, 18), (8, 91)] {
        if mask & bit != 0 {
            assert!(inject_test_key(Key(key), false));
        }
    }
    loop {
        let status = statuses.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_ne!(
            status.state,
            State::Faulted,
            "global stop must release cooperatively"
        );
        if status.state == State::Disarmed && !status.wanted {
            break;
        }
    }
    assert!(inject_test_key(Key(50), false));
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    assert!(
        !key_is_down(Key::LEFT),
        "global stop must release LeftButton"
    );
    assert!(!key_is_down(Key(49)), "global stop must release output 1");
    // A stop invalidates the original continuation even if a later stop barrier finishes.
    tx.send(SupervisorCommand::Disable).unwrap();
    tx.send(SupervisorCommand::EnableAfter { generation })
        .unwrap();
    let due = clock.now().after(clock.rate.0 / 2);
    while clock.now() < due {
        if let Ok(s) = statuses.recv_timeout(Duration::from_millis(50)) {
            assert!(
                !matches!(s.state, State::Armed(_)),
                "stale generation must not resume"
            );
        }
    }
    tx.send(SupervisorCommand::Quit).unwrap();
    guard.join.take().unwrap().join().unwrap().unwrap();
    close_target_window(target).unwrap();
    guard.receiver.wait_exit().unwrap();
    let csv = std::fs::read_to_string(folder.join("receiver.csv")).unwrap();
    assert!(csv.starts_with("qpc,message,wparam,lparam,extra_info"));
    let space_rows = csv
        .lines()
        .skip(1)
        .filter(|l| {
            let f: Vec<_> = l.split(',').collect();
            f.get(2) == Some(&"32") && matches!(f.get(1), Some(&"256") | Some(&"257"))
        })
        .count();
    assert_eq!(
        space_rows, 2,
        "only baseline Space Down/Up may reach receiver; captured pair must not"
    );
    let mut key_down = false;
    let mut left_down = false;
    let mut key_pairs = 0;
    let mut left_pairs = 0;
    let mut output_marker = None;
    for line in csv.lines().skip(1) {
        let fields: Vec<_> = line.split(',').collect();
        if matches!((fields[1], fields[2]), ("256" | "257", "49"))
            || matches!(fields[1], "513" | "514")
        {
            let extra_info = fields[4].parse::<usize>().unwrap();
            assert_ne!(extra_info, 0, "each synthetic output needs a marker");
            let expected = *output_marker.get_or_insert(extra_info);
            assert_eq!(
                extra_info, expected,
                "keyboard and mouse markers must round trip identically"
            );
        }
        match (fields[1], fields[2]) {
            ("256", "49") => {
                assert!(!key_down, "output 1 cannot have duplicate Down");
                key_down = true;
            }
            ("257", "49") => {
                assert!(key_down, "output 1 Up requires Down");
                key_down = false;
                key_pairs += 1;
            }
            ("513", _) => {
                assert!(!left_down, "LeftButton cannot have duplicate Down");
                left_down = true;
            }
            ("514", _) => {
                assert!(left_down, "LeftButton Up requires Down");
                left_down = false;
                left_pairs += 1;
            }
            _ => {}
        }
    }
    assert!(
        key_pairs >= 3,
        "skill and repeat must produce multiple 1 pairs"
    );
    assert!(
        left_pairs >= 3,
        "repeat and HoldPoint must produce mouse pairs"
    );
    assert!(
        !key_down && !left_down,
        "receiver must finish with no held output"
    );
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("receiver.json")).unwrap()).unwrap();
    assert_eq!(meta["lost_records"], 0);
    assert!(meta["written_records"].as_u64().unwrap() >= 2);
}
