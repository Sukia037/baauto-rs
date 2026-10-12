use baauto_config::*;
use baauto_core::{InputKind, SessionId, StopReason, Ticks};
use baauto_win_platform::{geometry::*, hook::*};
fn cfg() -> CompiledConfig {
    let p: PointSet = from_json(include_str!("../../../points/範例.json")).unwrap();
    let k: KeysProfile = from_json(include_str!("../../../keys/預設按鍵.json")).unwrap();
    compile(&p, &k, &HandSettings::default(), TickRate(1000)).unwrap()
}
fn state() -> HookState {
    let c = cfg();
    let mut s = HookState::new(&c, 123);
    s.focused = true;
    assert!(s.arm(SessionId(1), &c));
    s
}
fn raw(key: u8, down: bool) -> RawEvent {
    RawEvent {
        kind: RawKind::Key {
            key: Key(key),
            down,
        },
        injected: false,
        extra: 0,
        cursor: NormPoint { x: 0.8, y: 0.85 },
        inside_window: true,
        at: Ticks(1),
        ledger_keys: [0; 4],
        ledger_mouse: false,
    }
}
#[test]
fn decide_own_injection_passes() {
    let mut s = state();
    let mut e = raw(87, true);
    e.extra = 123;
    let d = s.decide(e);
    assert!(!d.swallow);
    assert!(!s.physical[87]);
    assert!(d.events.is_empty());
}
#[test]
#[cfg(not(feature = "test-hooks"))]
fn decide_foreign_injection_does_not_trigger() {
    let mut s = state();
    let mut e = raw(87, true);
    e.injected = true;
    let d = s.decide(e);
    assert!(!d.swallow);
    assert!(!s.physical[87]);
}
#[test]
fn decide_up_pairs_after_focus_loss() {
    let mut s = state();
    let down = s.decide(raw(87, true));
    let press = match down.events[0].kind {
        InputKind::TriggerDown { press, .. } => press,
        _ => panic!(),
    };
    assert_eq!(s.focus(false, Ticks(2)).stop, Some(StopReason::FocusLost));
    let up = s.decide(raw(87, false));
    assert!(up.swallow);
    assert!(matches!(up.events[0].kind,InputKind::TriggerUp{press:p} if p==press));
}
#[test]
fn decide_modifier_release_cancels() {
    let mut c = cfg();
    c.triggers[90].fill(NO_TRIGGER);
    for m in [1, 3, 5, 7, 9, 11, 13, 15] {
        c.triggers[90][m] = c.triggers[88][0];
    }
    let mut s = HookState::new(&c, 123);
    s.focused = true;
    s.arm(SessionId(1), &c);
    s.decide(raw(17, true));
    assert!(s.decide(raw(90, true)).swallow);
    let d = s.decide(raw(17, false));
    assert!(
        d.events
            .iter()
            .any(|e| matches!(e.kind, InputKind::PressCancelled { .. }))
    );
    assert!(s.decide(raw(90, false)).swallow);
}
#[test]
fn decide_auto_repeat() {
    let mut s = state();
    s.decide(raw(87, true));
    let d = s.decide(raw(87, true));
    assert!(d.swallow);
    assert!(matches!(d.events[0].kind, InputKind::TriggerRepeat { .. }));
}
#[test]
fn decide_physical_conflict_stops() {
    let mut s = state();
    let mut e = raw(49, true);
    e.ledger_keys[0] = 1 << 49;
    let d = s.decide(e);
    assert_eq!(d.stop, Some(StopReason::PhysicalConflict));
    assert!(!s.armed);
}
#[test]
fn decide_overflow_only_swallows_paired_up() {
    let mut s = state();
    s.decide(raw(87, true));
    s.invalidate();
    assert!(!s.decide(raw(69, true)).swallow);
    assert!(s.decide(raw(87, false)).swallow);
}
#[test]
fn decide_mouse_outside_window_passes() {
    let mut s = state();
    let mut e = raw(2, true);
    e.inside_window = false;
    assert!(!s.decide(e).swallow);
}
#[test]
fn coordinates_negative_origin_multiple_monitors() {
    for desktop in [
        Rect {
            left: -2560,
            top: -1440,
            width: 6400,
            height: 3600,
        },
        Rect {
            left: 0,
            top: 0,
            width: 1920,
            height: 1080,
        },
        Rect {
            left: -1920,
            top: 0,
            width: 3840,
            height: 2160,
        },
    ] {
        for (x, y) in [
            (desktop.left, desktop.top),
            (
                desktop.left + desktop.width - 1,
                desktop.top + desktop.height - 1,
            ),
            (
                desktop.left + desktop.width / 2,
                desktop.top + desktop.height / 2,
            ),
        ] {
            let (a, b) = absolute_for_pixel(desktop, x, y).unwrap();
            assert_eq!(pixel_for_absolute(desktop, a, b), (x, y));
        }
    }
}
#[test]
fn geometry_norm_roundtrip_and_warning() {
    let r = Rect {
        left: -2560,
        top: 0,
        width: 2560,
        height: 1440,
    };
    for p in [(-2560, 0), (-1, 1439), (-1234, 567)] {
        assert_eq!(r.pixel(r.norm(p.0, p.1)), Some(p));
    }
    assert!(r.fullscreen_16x9(r));
    assert!(!r.fullscreen_16x9(Rect { width: 900, ..r }));
}

#[test]
fn coordinates_extremely_wide_desktop() {
    let desktop = Rect {
        left: -20000,
        top: 0,
        width: 50000,
        height: 1440,
    };
    for x in desktop.left..desktop.left + desktop.width {
        let (a, b) = absolute_for_pixel(desktop, x, 1).unwrap();
        assert_eq!(pixel_for_absolute(desktop, a, b), (x, 1));
    }
}
#[test]
fn decide_side_modifier_matches_generic_trigger_and_ledger() {
    let mut c = cfg();
    let id = c.triggers[87][0];
    c.triggers[17].fill(id);
    let mut s = HookState::new(&c, 123);
    s.focused = true;
    assert!(s.arm(SessionId(1), &c));
    assert!(s.decide(raw(162, true)).swallow);
    assert!(s.decide(raw(162, false)).swallow);
    let mut s = state();
    let mut e = raw(162, true);
    e.ledger_keys[0] = 1 << 17;
    assert_eq!(s.decide(e).stop, Some(StopReason::PhysicalConflict));
}

#[test]
fn physical_before_synthetic_is_rejected() {
    let physical = [1u64 << 49, 0, 0, 0];
    assert!(outputs_conflict(
        &[baauto_core::OutEvent::KeyDown(Key(49))],
        physical
    ));
    assert!(!outputs_conflict(
        &[baauto_core::OutEvent::KeyUp(Key(49))],
        physical
    ));
    let c = cfg();
    let mut s = HookState::new(&c, 123);
    let mut keys = [false; 256];
    keys[49] = true;
    s.seed_physical(keys);
    assert!(!s.arm(SessionId(1), &c));
    s.decide(raw(49, false));
    assert!(s.arm(SessionId(1), &c));
}

#[test]
fn receiver_fixtures_compile_in_both_modes() {
    let k: KeysProfile = from_json(include_str!("../../../tests/fixtures/m2-keys.json")).unwrap();
    for (text, mode) in [
        (
            include_str!("../../../tests/fixtures/m2-basic.json"),
            Mode::Six,
        ),
        (
            include_str!("../../../tests/fixtures/m2-ten-left.json"),
            Mode::Ten,
        ),
    ] {
        let p: PointSet = from_json(text).unwrap();
        let h = HandSettings {
            card_ui_side: if mode == Mode::Ten {
                CardUiSide::Left
            } else {
                CardUiSide::Right
            },
            ..Default::default()
        };
        let c = compile(&p, &k, &h, TickRate(10_000_000)).unwrap();
        assert_eq!(c.slots, mode.slots());
        assert!(!k.timing.verified);
    }
}

#[test]
fn hook_decisions_allocate_nothing() {
    let mut s = state();
    let info = allocation_counter::measure(|| {
        for _ in 0..100_000 {
            let down = s.decide(raw(87, true));
            assert!(down.swallow);
            let up = s.decide(raw(87, false));
            assert!(up.swallow);
        }
    });
    assert_eq!(info.count_total, 0);
}
#[test]
fn modifier_family_stays_held_until_both_sides_are_released() {
    let mut s = state();
    s.decide(raw(162, true));
    s.decide(raw(163, true));
    s.decide(raw(162, false));
    assert!(s.physical[17]);
    s.decide(raw(163, false));
    assert!(!s.physical[17]);
}
#[test]
fn exact_modifier_chord_cancels_on_extra_modifier_down() {
    let mut c = cfg();
    let id = c.triggers[88][0];
    c.triggers[90].fill(NO_TRIGGER);
    c.triggers[90][1] = id;
    let mut s = HookState::new(&c, 123);
    s.focused = true;
    assert!(s.arm(SessionId(1), &c));
    s.decide(raw(162, true));
    assert!(s.decide(raw(90, true)).swallow);
    let d = s.decide(raw(160, true));
    assert!(
        d.events
            .iter()
            .any(|e| matches!(e.kind, InputKind::PressCancelled { .. }))
    );
    assert!(s.decide(raw(90, false)).swallow);
}

#[test]
fn latency_kit_files_compile_with_uncommon_stop_hotkey() {
    let k: KeysProfile = from_json(include_str!("../../../tools/test-kit/test-keys.json")).unwrap();
    let p: PointSet = from_json(include_str!("../../../tools/test-kit/test-points.json")).unwrap();
    let c = compile(&p, &k, &HandSettings::default(), TickRate(10_000_000)).unwrap();
    assert_eq!(c.slots, Mode::Six.slots());
    assert_ne!(
        k.presets["global_stop"].hotkey,
        Some("Ctrl+F9".parse().unwrap())
    );
    assert!(!k.presets["record"].enabled);
}

#[test]
fn engine_exit_description_names_the_cause() {
    use baauto_win_platform::{TERMINATED_BY_SUPERVISOR, describe_engine_exit};
    assert!(describe_engine_exit(Some(1)).contains("engine-error.txt"));
    assert!(describe_engine_exit(Some(101)).contains("panic"));
    assert!(describe_engine_exit(Some(TERMINATED_BY_SUPERVISOR)).contains("監督程式強制結束"));
    assert!(describe_engine_exit(Some(0xC000_0005)).contains("0xC0000005"));
    assert!(describe_engine_exit(Some(7)).contains("防毒"));
    assert!(describe_engine_exit(None).contains("讀不到"));
}

#[test]
fn error_log_appends_lines() {
    let dir = std::env::temp_dir().join(format!("baauto-errlog-{}", std::process::id()));
    let path = dir.join("logs").join("engine-error.txt");
    baauto_win_platform::append_error_log(&path, "first");
    baauto_win_platform::append_error_log(&path, "second");
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert!(text.lines().last().unwrap().ends_with("second"));
    let _ = std::fs::remove_dir_all(dir);
}
