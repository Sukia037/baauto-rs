#![cfg(all(windows, feature = "test-hooks"))]
//! Requires an interactive Windows desktop. Build release workspace with test-hooks first.
//! Run: cargo test -p baauto-win-platform --features test-hooks --test windows_actions -- --ignored --nocapture --test-threads=1
//! Keep the receiver foreground; foreign keyboard/mouse injections are test-only.
//! The release test kit sets BAAUTO_TEST_BIN to its own folder of test-hooks binaries.
//! If the default global hotkeys are occupied, set BAAUTO_TEST_STOP_HOTKEY and
//! BAAUTO_TEST_RECORD_HOTKEY to unused chords. This only changes the test profile.
use baauto_config::*;
use baauto_protocol::{capture::*, ipc::EngineToApp, supervisor::State};
use baauto_win_platform::*;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
struct Guard {
    receiver: Process,
    preview: Option<Process>,
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
            let _ = inject_test_button_or_key(key, false);
        }
        if let Some(tx) = &self.quit {
            let _ = tx.send(SupervisorCommand::Quit);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        if let Some(preview) = &self.preview
            && !preview.exited()
        {
            let _ = preview.terminate();
            let _ = preview.wait_exit();
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
#[derive(serde::Serialize)]
struct Case {
    name: String,
    begin: u64,
    end: u64,
    expected: Vec<(u32, u32)>,
    repeating: bool,
}
fn action(clock: Clock, key: Key, name: &str, expected: Vec<(u32, u32)>, repeating: bool) -> Case {
    let begin = clock.now().0;
    assert!(inject_test_button_or_key(key, true));
    wait_control_period(
        clock,
        if repeating {
            clock.rate.0 / 3
        } else {
            clock.rate.0 / 5
        },
    )
    .unwrap();
    assert!(inject_test_button_or_key(key, false));
    wait_control_period(clock, clock.rate.0 / 5).unwrap();
    Case {
        name: name.into(),
        begin,
        end: clock.now().0,
        expected,
        repeating,
    }
}
fn key_pair(key: u32) -> Vec<(u32, u32)> {
    vec![(256, key), (257, key)]
}
fn chord(chord: Chord) {
    let mask = match chord.mods {
        ModsMatch::AtLeast(mask) | ModsMatch::Exact(mask) => mask,
    };
    for (bit, key) in [(1, 17), (2, 16), (4, 18), (8, 91)] {
        if mask & bit != 0 {
            assert!(inject_test_key(Key(key), true));
        }
    }
    assert!(inject_test_key(chord.key, true));
    assert!(inject_test_key(chord.key, false));
    for (bit, key) in [(1, 17), (2, 16), (4, 18), (8, 91)] {
        if mask & bit != 0 {
            assert!(inject_test_key(Key(key), false));
        }
    }
}
fn run_profile(mode: Mode, side: CardUiSide, order: SlotOrder, all_presets: bool) -> Vec<String> {
    let bin = test_bin();
    let folder = std::env::temp_dir().join(format!(
        "baauto-actions-{}-{mode:?}-{side:?}-{order:?}",
        std::process::id()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    let receiver = Process::spawn(
        bin.join("baauto-receiver.exe").as_os_str(),
        &[
            "--hand".into(),
            if mode == Mode::Six { "six" } else { "ten" }.into(),
            "--side".into(),
            if side == CardUiSide::Right {
                "right"
            } else {
                "left"
            }
            .into(),
            "--output".into(),
            folder.join("receiver.csv").into_os_string(),
        ],
    )
    .unwrap();
    let mut guard = Guard {
        receiver,
        preview: None,
        quit: None,
        join: None,
        extra_keys: [
            1, 2, 5, 6, 9, 27, 38, 40, 51, 52, 53, 54, 65, 68, 69, 70, 71, 81, 82, 83, 84, 89, 134,
            135,
        ]
        .into_iter()
        .map(Key)
        .collect(),
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
    std::fs::write(folder.join("keys.json"), to_json(&keys).unwrap()).unwrap();
    std::fs::write(
        folder.join("points.json"),
        to_json(&points_for(mode)).unwrap(),
    )
    .unwrap();
    std::fs::write(
        folder.join("hand.json"),
        to_json(&HandSettings {
            card_ui_side: side,
            slot_order: order,
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    let mut points = template_points();
    points.mode = mode;
    let hand = HandSettings {
        card_ui_side: side,
        slot_order: order,
        ..Default::default()
    };
    let cfg = Arc::new(compile(&points, &keys, &hand, clock.rate).unwrap());
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
    let _generation = loop {
        let s = statuses.recv_timeout(Duration::from_secs(10)).unwrap();
        if matches!(s.state, State::Armed(_)) {
            break s.resume_generation;
        }
    };
    let mut cases = Vec::new();
    if all_presets {
        for i in 1..=5 {
            let role = format!("skill_{i}");
            cases.push(action(
                clock,
                keys.presets[&role].hotkey.unwrap().key,
                &role,
                key_pair(48 + i),
                false,
            ));
        }
        for i in 1..=5 {
            let role = format!("spam_{i}");
            let mut expected = key_pair(48 + i);
            expected.extend([(513, 0), (514, 0)]);
            cases.push(action(
                clock,
                keys.presets[&role].hotkey.unwrap().key,
                &role,
                expected,
                true,
            ));
        }
        for (role, key) in [("pause", 27), ("auto", 65), ("speed", 83)] {
            cases.push(action(
                clock,
                keys.presets[role].hotkey.unwrap().key,
                role,
                key_pair(key),
                false,
            ));
        }
        for role in ["frame_fast", "frame_slow"] {
            let mut expected = key_pair(27);
            expected.extend(key_pair(27));
            cases.push(action(
                clock,
                keys.presets[role].hotkey.unwrap().key,
                role,
                expected,
                false,
            ));
        }
        cases.push(action(
            clock,
            keys.presets["click_spam"].hotkey.unwrap().key,
            "click_spam",
            vec![(513, 0), (514, 0)],
            true,
        ));
        for i in 1..=5 {
            let role = format!("card_{i}");
            // One shot even while held: Esc, number key, click at the cursor, Esc.
            let mut once = key_pair(27);
            once.extend(key_pair(48 + i));
            once.extend([(513, 0), (514, 0)]);
            once.extend(key_pair(27));
            cases.push(action(
                clock,
                keys.presets[&role].hotkey.unwrap().key,
                &role,
                once,
                false,
            ));
        }
        cases.push(action(
            clock,
            Key(90),
            "hold_point",
            vec![(513, 0), (514, 0)],
            false,
        ));
    }
    let rect = hand.layout(mode);
    let card_target = points
        .items
        .iter()
        .find(|i| i.function == PointFunction::Card)
        .and_then(|i| i.x.zip(i.y))
        .map(|(x, y)| target.monitor.pixel(NormPoint { x, y }).unwrap())
        .expect("template card key has a target point");
    // Receiver client coordinates of every card-key click and of each slot the cursor returns to.
    let client = |(x, y): (i32, i32)| -> u64 {
        ((u64::from((y - target.monitor.top) as u16)) << 16)
            | u64::from((x - target.monitor.left) as u16)
    };
    let mut card_slots = Vec::new();
    for slot in 0..mode.slots() {
        let point = NormPoint {
            x: rect.x[0]
                + (rect.x[1] - rect.x[0]) * (f64::from(slot) + 0.5) / f64::from(mode.slots()),
            y: (rect.y[0] + rect.y[1]) / 2.0,
        };
        let (x, y) = target.monitor.pixel(point).unwrap();
        assert!(move_test_cursor(x, y));
        wait_control_period(clock, clock.rate.0 / 10).unwrap();
        let index = if order == SlotOrder::LeftToRight {
            slot
        } else {
            mode.slots() - 1 - slot
        };
        let mut cycle = key_pair(u32::from(hand.slot_keys[index as usize].0));
        cycle.extend([(513, 0), (514, 0)]);
        let name = format!("card_key_slot_{slot}");
        card_slots.push((name.clone(), client((x, y))));
        cases.push(action(clock, Key(81), &name, cycle, true));
    }
    let (x, y) = target
        .monitor
        .pixel(NormPoint { x: 0.45, y: 0.45 })
        .unwrap();
    assert!(move_test_cursor(x, y));
    wait_control_period(clock, clock.rate.0 / 10).unwrap();
    cases.push(action(
        clock,
        Key(81),
        "card_key_outside",
        Vec::new(),
        false,
    ));
    let (reply, stopped) = mpsc::channel();
    tx.send(SupervisorCommand::Quiesce { reply }).unwrap();
    let generation = stopped
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let mut captures = Vec::new();
    let mut capture_begin = 0;
    let mut capture_end = 0;
    if all_presets {
        chord(keys.presets["record"].hotkey.unwrap());
        loop {
            let status = statuses.recv_timeout(Duration::from_secs(5)).unwrap();
            if status.records > 0 {
                break;
            }
        }
        captures.push(("global_record".to_string(), true));
        // Record WM_HOTKEY may be delivered before its modifier Up messages.
        // Finish that baseline input before measuring the separate capture phase.
        wait_control_period(clock, clock.rate.0 / 5).unwrap();
        // Left click must be captured with correct coordinates and never reach receiver.
        capture_begin = clock.now().0;
        let point = request(&tx, generation, 10, 5000, CaptureKind::Point);
        assert!(matches!(
            point.recv_timeout(Duration::from_secs(2)).unwrap().unwrap(),
            EngineToApp::CaptureReady { .. }
        ));
        assert!(inject_test_button_or_key(Key::LEFT, true));
        let result = final_reply(&point);
        assert!(inject_test_button_or_key(Key::LEFT, false));
        let pass = matches!(result, EngineToApp::PointRecorded { request_id: 10, screen_pos, monitor, .. }
            if screen_pos == [x,y] && monitor == [target.monitor.left,target.monitor.top,target.monitor.width,target.monitor.height]);
        captures.push(("record_point".to_string(), pass));
        for (id, key, label) in [
            (11, 2, "RButton"),
            (12, 5, "XButton1"),
            (13, 6, "XButton2"),
            (14, 32, "Space"),
            (15, 9, "Tab"),
        ] {
            let input = request(&tx, generation, id, 5000, CaptureKind::Hotkey);
            assert!(matches!(
                input.recv_timeout(Duration::from_secs(2)).unwrap().unwrap(),
                EngineToApp::CaptureReady { .. }
            ));
            assert!(inject_test_button_or_key(Key(key), true));
            let EngineToApp::CapturedInput { result, .. } = final_reply(&input) else {
                panic!("capture reply");
            };
            assert!(inject_test_button_or_key(Key(key), false));
            captures.push((
                format!("capture_{label}"),
                result.outcome == CaptureOutcome::Chord(label.parse().unwrap()),
            ));
            wait_control_period(clock, clock.rate.0 / 10).unwrap();
        }
        capture_end = clock.now().0;
        tx.send(SupervisorCommand::Enable).unwrap();
        loop {
            if matches!(
                statuses.recv_timeout(Duration::from_secs(5)).unwrap().state,
                State::Armed(_)
            ) {
                break;
            }
        }
        assert!(inject_test_key(Key(50), true));
        wait_control_period(clock, clock.rate.0 / 5).unwrap();
        chord(keys.presets["global_stop"].hotkey.unwrap());
        loop {
            let status = statuses.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_ne!(status.state, State::Faulted);
            if status.state == State::Disarmed && !status.wanted {
                break;
            }
        }
        assert!(inject_test_key(Key(50), false));
        captures.push((
            "global_stop".into(),
            !key_is_down(Key::LEFT) && !key_is_down(Key(49)),
        ));
    }
    if all_presets {
        guard.preview = Some(
            Process::spawn(
                bin.join("baauto-app.exe").as_os_str(),
                &[
                    "--preview".into(),
                    "--data-dir".into(),
                    folder.join("preview").into_os_string(),
                ],
            )
            .unwrap(),
        );
        let due = clock.now().after(clock.rate.0 * 15);
        let preview_target = loop {
            if let Some(t) = target_for_foreground(OsStr::new("baauto-app.exe")).unwrap() {
                break t;
            }
            assert!(clock.now() < due, "preview must receive foreground focus");
            wait_control_period(clock, clock.rate.0 / 20).unwrap();
        };
        let (reply, rx) = mpsc::channel();
        tx.send(SupervisorCommand::ViewTarget { reply }).unwrap();
        let result = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        println!("calibration foreground result: {result:?}");
        captures.push((
            "calibration_foreground_handoff".into(),
            result
                == Ok([
                    target.monitor.left,
                    target.monitor.top,
                    target.monitor.width,
                    target.monitor.height,
                ]),
        ));
        close_target_window(preview_target).unwrap();
        guard.preview.take().unwrap().wait_exit().unwrap();
    }
    tx.send(SupervisorCommand::Quit).unwrap();
    guard.join.take().unwrap().join().unwrap().unwrap();
    close_target_window(target).unwrap();
    guard.receiver.wait_exit().unwrap();
    let csv = std::fs::read_to_string(folder.join("receiver.csv")).unwrap();
    let rows: Vec<Vec<u64>> = csv
        .lines()
        .skip(1)
        .map(|line| line.split(',').map(|f| f.parse().unwrap()).collect())
        .collect();
    let mut failures = Vec::new();
    for case in &cases {
        let actual: Vec<_> = rows
            .iter()
            .filter(|r| r[0] >= case.begin && r[0] < case.end && r[4] != 0)
            .filter_map(|r| match r[1] {
                256 | 257 => Some((r[1] as u32, r[2] as u32)),
                513 | 514 => Some((r[1] as u32, 0)),
                _ => None,
            })
            .collect();
        let sequence = actual.as_slice();
        let pass = if case.repeating {
            let complete = sequence.len() / case.expected.len();
            let tail = &sequence[complete * case.expected.len()..];
            // Release cancels an unsubmitted click after a completed key pair.
            // Require repeated complete cycles and only that paired key prefix as a tail.
            complete >= 2
                && sequence[..complete * case.expected.len()]
                    .chunks(case.expected.len())
                    .all(|chunk| chunk == case.expected)
                && (tail.is_empty() || (case.expected.len() == 4 && tail == &case.expected[..2]))
        } else {
            sequence == case.expected
        };
        println!(
            "{:?}/{:?}/{:?} {}: {} {:?}",
            mode,
            side,
            order,
            case.name,
            if pass { "PASS" } else { "FAIL" },
            actual
        );
        if !pass {
            failures.push(format!(
                "{:?}/{:?}/{:?} {} expected {:?}, got {:?}",
                mode, side, order, case.name, case.expected, actual
            ));
        }
        if let Some((_, slot)) = card_slots.iter().find(|(n, _)| *n == case.name) {
            let own = |r: &&Vec<u64>| r[0] >= case.begin && r[0] < case.end && r[4] != 0;
            let clicks_on_target = rows
                .iter()
                .filter(own)
                .filter(|r| r[1] == 513)
                .all(|r| r[3] == client(card_target));
            let returned = rows
                .iter()
                .filter(own)
                .any(|r| r[1] == 512 && r[3] == *slot);
            captures.push((format!("{}_clicks_target", case.name), clicks_on_target));
            captures.push((format!("{}_returns_cursor", case.name), returned));
        }
    }
    if all_presets {
        let leaked = rows
            .iter()
            .filter(|r| {
                r[0] >= capture_begin
                    && r[0] < capture_end
                    && matches!(r[1], 256 | 257 | 513 | 514 | 516 | 517 | 523 | 524)
            })
            .count();
        captures.push(("captures_swallow_down_and_up".into(), leaked == 0));
    }
    for (label, pass) in &captures {
        println!("{label}: {}", if *pass { "PASS" } else { "FAIL" });
        if !pass {
            failures.push(label.clone());
        }
    }
    std::fs::write(
        folder.join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    std::fs::write(
        folder.join("captures.json"),
        serde_json::to_vec_pretty(&captures).unwrap(),
    )
    .unwrap();
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("receiver.json")).unwrap()).unwrap();
    assert_eq!(meta["lost_records"], 0);
    println!("evidence: {}", folder.display());
    failures
}
fn points_for(mode: Mode) -> PointSet {
    let mut points = template_points();
    points.mode = mode;
    points
}
#[test]
#[ignore = "requires interactive Windows desktop and test-hooks release engine"]
fn all_presets_card_slots_and_capture_reach_windows_receiver() {
    let mut failures = Vec::new();
    for (mode, side, order, all) in [
        (Mode::Six, CardUiSide::Right, SlotOrder::LeftToRight, true),
        (Mode::Six, CardUiSide::Left, SlotOrder::LeftToRight, false),
        (Mode::Ten, CardUiSide::Left, SlotOrder::LeftToRight, false),
        (Mode::Ten, CardUiSide::Right, SlotOrder::RightToLeft, false),
    ] {
        failures.extend(run_profile(mode, side, order, all));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
struct Supervised {
    /// Releases the keys, quits the supervisor and closes the receiver on drop.
    _guard: Guard,
    clock: Clock,
    tx: mpsc::Sender<SupervisorCommand>,
    statuses: mpsc::Receiver<SupervisorStatus>,
    folder: PathBuf,
}
/// Receiver in front, supervisor running the template and Disarmed with its Hook installed.
fn supervised_receiver(name: &str, extra_keys: Vec<Key>) -> Supervised {
    let bin = test_bin();
    let folder = std::env::temp_dir().join(format!("baauto-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let receiver = Process::spawn(
        bin.join("baauto-receiver.exe").as_os_str(),
        &[
            "--output".into(),
            folder.join("receiver.csv").into_os_string(),
        ],
    )
    .unwrap();
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
    let mut guard = Guard {
        receiver,
        preview: None,
        quit: None,
        join: None,
        extra_keys,
    };
    let clock = Clock::new().unwrap();
    let due = clock.now().after(clock.rate.0 * 15);
    while target_for_foreground(OsStr::new("baauto-receiver.exe"))
        .unwrap()
        .is_none()
    {
        assert!(clock.now() < due, "focus receiver");
        wait_control_period(clock, clock.rate.0 / 20).unwrap();
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
    loop {
        let s = statuses.recv_timeout(Duration::from_secs(10)).unwrap();
        if s.state == State::Disarmed {
            break;
        }
    }
    // Let the engine install its Hook and report KeysReleased, so the supervisor's view of the
    // keys is stale when a key goes down later: the order the field reports hit.
    wait_control_period(clock, clock.rate.0 * 2).unwrap();
    Supervised {
        _guard: guard,
        clock,
        tx,
        statuses,
        folder,
    }
}
/// Waits for `want`; a kill, recovery or fault on the way fails the test.
fn until_state(s: &Supervised, what: &str, want: impl Fn(&SupervisorStatus) -> bool) {
    loop {
        let status = s
            .statuses
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("{what}"));
        assert!(
            !matches!(
                status.state,
                State::WaitingExit | State::Recovering | State::Faulted
            ),
            "{what}: ended in {:?}: {}",
            status.state,
            status.detail
        );
        if want(&status) {
            return;
        }
    }
}
fn assert_no_fault_dumps(folder: &Path) {
    let faults: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("fault-"))
        .collect();
    assert!(faults.is_empty(), "fault dumps written: {faults:?}");
    println!("evidence: {}", folder.display());
}
/// preview.4 field report: the mouse button that clicked the game to the front was still held,
/// the engine rejected Arm, the Stop that followed was never confirmed, and the supervisor killed
/// the engine after its recovery timeout. A rejected Arm must end Disarmed, keep the enable
/// request, and arm as soon as the key is released. A keyboard key stands in for the button so the
/// test does not click the receiver; both are the same related-key check.
#[test]
#[ignore = "requires interactive Windows desktop and test-hooks release engine"]
fn arm_rejected_for_held_key_waits_and_arms_after_release() {
    let held = template_keys().presets["skill_1"].hotkey.unwrap().key;
    let s = supervised_receiver("arm-rejected", vec![held]);
    assert!(inject_test_button_or_key(held, true));
    wait_control_period(s.clock, s.clock.rate.0 / 5).unwrap();
    s.tx.send(SupervisorCommand::Enable).unwrap();
    until_state(&s, "no Disarmed status after the rejected Arm", |st| {
        assert!(
            !matches!(st.state, State::Armed(_)),
            "Arm accepted while {held:?} was held"
        );
        if st.state == State::Disarmed && st.detail.contains("放開後自動啟用") {
            assert!(st.wanted, "the enable request was dropped");
            return true;
        }
        false
    });
    // No second Enable: releasing the key alone must arm.
    assert!(inject_test_button_or_key(held, false));
    until_state(&s, "releasing the key never armed", |st| {
        matches!(st.state, State::Armed(_))
    });
    assert_no_fault_dumps(&s.folder);
}
/// preview.4 field faults mid-game (stop reason 8): the user pressed the left button while a hold
/// point held it. Spec §9A makes that a stop until release, not an engine kill.
#[test]
#[ignore = "requires interactive Windows desktop and test-hooks release engine"]
fn physical_left_during_hold_point_stops_and_rearms_without_fault() {
    let s = supervised_receiver("physical-conflict", vec![Key::LEFT]);
    let point = template_points()
        .items
        .iter()
        .find(|i| i.function == PointFunction::Point)
        .and_then(|i| i.hotkey)
        .unwrap()
        .key;
    s.tx.send(SupervisorCommand::Enable).unwrap();
    until_state(&s, "Enable never armed", |st| {
        matches!(st.state, State::Armed(_))
    });
    assert!(inject_test_button_or_key(point, true));
    wait_control_period(s.clock, s.clock.rate.0 / 5).unwrap();
    assert!(key_is_down(Key::LEFT), "hold point must hold LeftButton");
    assert!(inject_test_button_or_key(Key::LEFT, true));
    until_state(&s, "the conflict did not stop the session", |st| {
        st.detail.contains("滑鼠左鍵 被實際按住")
    });
    assert!(inject_test_button_or_key(Key::LEFT, false));
    assert!(inject_test_button_or_key(point, false));
    until_state(&s, "releasing the button never armed again", |st| {
        matches!(st.state, State::Armed(_)) && st.wanted
    });
    assert!(!key_is_down(Key::LEFT), "LeftButton left held");
    assert_no_fault_dumps(&s.folder);
}
/// Built binaries: `BAAUTO_TEST_BIN` when run from the release test kit, otherwise this
/// checkout's `target/release`.
fn test_bin() -> PathBuf {
    std::env::var_os("BAAUTO_TEST_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/release")
                .canonicalize()
                .unwrap()
        })
}
