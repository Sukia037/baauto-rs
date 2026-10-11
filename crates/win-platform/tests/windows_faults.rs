#![cfg(all(windows, feature = "fault-injection", feature = "test-hooks"))]
//! Interactive desktop required; temporarily sends test trigger keys to the receiver.
//! First cargo build --release --features fault-injection,test-hooks.
//! cargo test -p baauto-win-platform --features fault-injection,test-hooks --test windows_faults -- --ignored --nocapture --test-threads=1
use baauto_config::*;
use baauto_protocol::supervisor::State;
use baauto_win_platform::*;
use std::{
    ffi::OsStr,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};
fn case(rule: &str) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let bin = root.join("target/release");
    let logs = std::env::temp_dir().join(format!(
        "baauto-fault-{}-{}",
        std::process::id(),
        rule.replace([':', '@'], "_")
    ));
    std::fs::create_dir_all(&logs).unwrap();
    let receiver = Process::spawn(
        bin.join("baauto-receiver.exe").as_os_str(),
        &[
            "--output".into(),
            logs.join("receiver.csv").into_os_string(),
        ],
    )
    .unwrap();
    let clock = Clock::new().unwrap();
    let deadline = clock.now().after(clock.rate.0 * 10);
    let target = loop {
        if let Some(t) = target_for_foreground(OsStr::new("baauto-receiver.exe")).unwrap() {
            break t;
        }
        assert!(clock.now() < deadline, "focus receiver");
        wait_control_period(clock, clock.rate.0 / 20).unwrap();
    };
    let p: PointSet = from_json(include_str!("../../../tests/fixtures/m2-basic.json")).unwrap();
    let k: KeysProfile = from_json(include_str!("../../../tests/fixtures/m2-keys.json")).unwrap();
    let cfg = Arc::new(compile(&p, &k, &HandSettings::default(), clock.rate).unwrap());
    let options = SupervisorOptions {
        cfg,
        engine: bin.join("baauto-engine.exe"),
        target: "baauto-receiver.exe".into(),
        stop_hotkey: k.presets["global_stop"].hotkey.unwrap(),
        record_hotkey: k.presets["record"].hotkey.unwrap(),
        health_timeout_ms: 150,
        recovery_timeout_ms: 100,
        logs: logs.clone(),
        injection: Some(rule.into()),
    };
    let (tx, rx) = mpsc::channel();
    let (status_tx, status_rx) = mpsc::channel();
    let supervisor = thread::spawn(move || run_supervisor(options, rx, status_tx));
    tx.send(SupervisorCommand::Enable).unwrap();
    let until = clock.now().after(clock.rate.0 * 15);
    let mut injected = false;
    let mut faulted = false;
    while clock.now() < until {
        if let Ok(status) = status_rx.recv_timeout(Duration::from_millis(50)) {
            if matches!(status.state, State::Armed(_)) && !injected {
                assert!(inject_test_key(Key(90), true)); // HoldPoint owns LeftButton; Repeat waits for the mouse transaction.
                assert!(inject_test_key(Key(50), true));
                injected = true;
            }
            if status.state == State::Faulted {
                faulted = true;
                break;
            }
        }
    }
    assert!(faulted, "{rule} must fault within the deadline");
    assert!(inject_test_key(Key(90), false));
    assert!(inject_test_key(Key(50), false));
    wait_control_period(clock, clock.rate.0 / 2).unwrap();
    assert!(!key_is_down(Key::LEFT), "LeftButton must be released");
    assert!(!key_is_down(Key(49)), "output 1 must be released");
    for status in status_rx.try_iter() {
        assert!(
            !matches!(status.state, State::Armed(_)),
            "fault must not auto resume"
        );
    }
    tx.send(SupervisorCommand::Quit).unwrap();
    supervisor.join().unwrap().unwrap();
    close_target_window(target).unwrap();
    receiver.wait_exit().unwrap();
    assert!(
        std::fs::read_dir(&logs).unwrap().flatten().any(|p| p
            .file_name()
            .to_string_lossy()
            .starts_with("fault-")
            && p.path().extension() == Some(OsStr::new("csv")))
    );
    let csv = std::fs::read_to_string(logs.join("receiver.csv")).unwrap();
    let mut held = false;
    let mut downs = 0;
    for line in csv.lines().skip(1) {
        let msg = line.split(',').nth(1).unwrap().parse::<u32>().unwrap();
        match msg {
            0x201 => {
                assert!(!held, "duplicate mouse Down");
                held = true;
                downs += 1;
            }
            0x202 => held = false,
            _ => {}
        }
    }
    assert!(!held, "receiver must finish without LeftButton held");
    assert!(downs > 0, "test must exercise a held mouse transaction");
    eprintln!("{rule}: evidence {}", logs.display());
}
macro_rules! fault_test {
    ($name:ident,$rule:expr) => {
        #[test]
        #[ignore = "requires interactive Windows desktop and release test binaries"]
        fn $name() {
            case($rule);
        }
    };
}
fault_test!(panic_recovers, "panic@2");
fault_test!(abort_recovers, "abort@2");
fault_test!(
    hang_then_resume_cannot_output_after_recovery,
    "hang:300ms@2"
);
fault_test!(short_send_recovers, "short@2");
fault_test!(pipe_drop_recovers, "pipe-drop@2");
fault_test!(hook_stall_recovers, "hook-stall");
