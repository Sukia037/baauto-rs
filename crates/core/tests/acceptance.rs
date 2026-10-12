use baauto_config::*;
use baauto_core::*;
use proptest::prelude::*;
use std::sync::Arc;
/// Test gaps in ms; the engine runs at 1000 ticks per second, so 1 tick = 1 ms.
fn test_timing(t: &mut TimingProfile) {
    t.frame_fast.esc_gap_ms = 3.;
    t.frame_slow.esc_gap_ms = 6.;
    t.spam = SpamGaps {
        key_to_click_ms: 2.,
        click_to_next_ms: 5.,
    };
    t.click_spam.click_to_next_ms = 4.;
    t.card = CardGaps {
        esc_to_key_ms: 2.,
        key_to_click_ms: 1.,
        click_to_esc_ms: 3.,
    };
    t.point.maintain_period_ms = 4.;
    t.point_card = PointCardGaps {
        key_to_down_ms: 1.,
        down_to_up_ms: 2.,
        up_to_return_ms: 3.,
        return_to_next_ms: 4.,
    };
}
fn config(mode: Mode, side: CardUiSide, tweak: impl Fn(&mut TimingProfile)) -> CompiledConfig {
    let mut p: PointSet = from_json(include_str!("../../../points/範例.json")).unwrap();
    p.mode = mode;
    let mut k: KeysProfile = from_json(include_str!("../../../keys/預設按鍵.json")).unwrap();
    test_timing(&mut k.timing);
    tweak(&mut k.timing);
    let h = HandSettings {
        card_ui_side: side,
        ..Default::default()
    };
    compile(&p, &k, &h, TickRate(1000)).unwrap()
}
const ESC: [OutEvent; 2] = [OutEvent::KeyDown(Key::ESC), OutEvent::KeyUp(Key::ESC)];
const CLICK: [OutEvent; 2] = [OutEvent::LeftDown, OutEvent::LeftUp];
fn tap(k: u8) -> [OutEvent; 2] {
    [OutEvent::KeyDown(Key(k)), OutEvent::KeyUp(Key(k))]
}
struct Rig {
    e: Engine,
    c: CompiledConfig,
    seq: u64,
    now: u64,
}
impl Rig {
    fn new() -> Self {
        Self::with(Mode::Six, CardUiSide::Right)
    }
    fn with(mode: Mode, side: CardUiSide) -> Self {
        Self::build(config(mode, side, |_| {}))
    }
    fn timed(tweak: impl Fn(&mut TimingProfile)) -> Self {
        Self::build(config(Mode::Six, CardUiSide::Right, tweak))
    }
    fn build(c: CompiledConfig) -> Self {
        let mut e = Engine::new(Arc::new(c.clone()), TickRate(1000));
        e.arm(SessionId(1), Ticks(0)).unwrap();
        Self {
            e,
            c,
            seq: 0,
            now: 0,
        }
    }
    fn ev(&mut self, kind: InputKind) {
        self.seq += 1;
        self.e.on_input(
            InputEvent {
                seq: self.seq,
                at: Ticks(self.now),
                session: SessionId(1),
                config: ConfigVersion(1),
                geometry: GeometryVersion(1),
                kind,
            },
            Ticks(self.now),
        );
    }
    fn down(&mut self, key: u8, press: u32, p: NormPoint) {
        self.ev(InputKind::TriggerDown {
            press: PressId(press),
            trigger: self.c.triggers[key as usize][0],
            cursor: p,
        });
    }
    fn up(&mut self, p: u32) {
        self.ev(InputKind::TriggerUp { press: PressId(p) });
    }
    fn repeat(&mut self, p: u32) {
        self.ev(InputKind::TriggerRepeat {
            press: PressId(p),
            cursor: NormPoint { x: 0.5, y: 0.5 },
        });
    }
    fn batch(&mut self, now: u64) -> Batch {
        self.now = now;
        self.e.next_batch(Ticks(now)).expect("batch expected")
    }
    fn send(&mut self, now: u64) -> Batch {
        let b = self.batch(now);
        self.e.commit(
            b.id,
            SubmitOutcome::Ok {
                begin: Ticks(now),
                end: Ticks(now),
            },
            Ticks(now),
        );
        b
    }
    fn none_at(&mut self, now: u64) -> bool {
        self.e.next_batch(Ticks(now)).is_none()
    }
    fn card_point(&self) -> NormPoint {
        self.c
            .actions
            .iter()
            .find_map(|a| match a {
                ActionSpec::CardKey(p) => Some(*p),
                _ => None,
            })
            .expect("template has a card key with a target point")
    }
    /// The hold point bound to `key` in the template (z or x).
    fn point_of(&self, key: u8) -> NormPoint {
        match self.c.actions[self.c.triggers[key as usize][0] as usize] {
            ActionSpec::HoldPoint(p) => p,
            other => panic!("{key} is not a hold point: {other:?}"),
        }
    }
    fn center(&self, i: u8) -> NormPoint {
        NormPoint {
            x: self.c.hand.x[0]
                + (self.c.hand.x[1] - self.c.hand.x[0]) * (f64::from(i) + 0.5)
                    / f64::from(self.c.slots),
            y: (self.c.hand.y[0] + self.c.hand.y[1]) / 2.,
        }
    }
}
fn p() -> NormPoint {
    NormPoint { x: 0.5, y: 0.5 }
}
#[test]
fn tap_sends_down_and_up_in_one_batch() {
    let mut r = Rig::new();
    r.down(87, 1, p());
    assert_eq!(r.send(0).events.as_slice(), &tap(49));
    assert!(r.none_at(100));
    assert_eq!(r.e.active_actions(), 0);
    // A zero gap still sends the next step as its own batch.
    let mut r = Rig::timed(|t| t.spam.key_to_click_ms = 0.);
    r.down(50, 1, p());
    assert_eq!(r.send(0).events.as_slice(), &tap(49));
    assert_eq!(r.send(0).events.as_slice(), &CLICK);
}
#[test]
fn repeat_late_wake_shifts_click_stage() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    let b = r.batch(0);
    assert_eq!(b.events.as_slice(), &tap(49));
    r.e.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(0),
            end: Ticks(3),
        },
        Ticks(3),
    );
    assert!(r.none_at(4));
    assert_eq!(r.send(5).events.as_slice(), &CLICK);
}
#[test]
fn repeat_skips_overdue_cycles_without_burst() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    assert_eq!(r.send(0).events.as_slice(), &tap(49));
    assert_eq!(r.send(2).events.as_slice(), &CLICK);
    // The engine stalls far past the next cycle: exactly one cycle, then normal spacing.
    assert_eq!(r.send(50).events.as_slice(), &tap(49));
    assert_eq!(r.send(52).events.as_slice(), &CLICK);
    assert!(r.none_at(56));
    assert_eq!(r.send(57).events.as_slice(), &tap(49));
}
#[test]
fn click_spam_clicks_until_release() {
    let mut r = Rig::new();
    r.down(2, 1, p());
    assert_eq!(r.send(0).events.as_slice(), &CLICK);
    assert!(r.none_at(3));
    assert_eq!(r.send(4).events.as_slice(), &CLICK);
    r.up(1);
    assert!(r.none_at(100));
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn quick_release_repress_uses_new_press() {
    let mut r = Rig::new();
    let z = r.point_of(90);
    r.down(90, 1, p());
    r.send(0);
    r.up(1);
    assert_eq!(
        r.send(1).events.as_slice(),
        &[OutEvent::MoveTo(z), OutEvent::LeftUp]
    );
    r.down(90, 2, p());
    r.send(2);
    r.up(1);
    assert!(r.none_at(3));
    assert!(r.e.held().contains(OutKey::LeftButton));
}
#[test]
fn stop_releases_everything_then_silent() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.send(0);
    r.down(50, 2, p());
    r.e.request_stop(StopReason::Emergency, Ticks(1));
    assert_eq!(r.send(1).events.as_slice(), &[OutEvent::LeftUp]);
    assert!(matches!(r.e.state(), EngineState::Stopped(SessionId(1))));
    assert!(r.e.held().is_empty());
    assert!(r.none_at(1000));
}
#[test]
fn short_send_goes_faulted_and_keeps_held() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.send(0);
    r.e.request_stop(StopReason::User, Ticks(1));
    let b = r.batch(1);
    r.e.commit(
        b.id,
        SubmitOutcome::Short {
            inserted: 0,
            end: Ticks(1),
        },
        Ticks(1),
    );
    assert!(matches!(r.e.state(), EngineState::Faulted(..)));
    assert!(r.e.held().contains(OutKey::LeftButton));
}
#[test]
fn double_esc_second_esc_always_sent() {
    let mut r = Rig::new();
    r.down(40, 1, p());
    let b = r.batch(0);
    assert_eq!(b.events.as_slice(), &ESC);
    r.e.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(0),
            end: Ticks(1),
        },
        Ticks(1),
    );
    r.up(1);
    assert!(r.none_at(3));
    assert_eq!(r.send(4).events.as_slice(), &ESC);
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn slow_double_esc_sends_second_esc_even_when_late() {
    let mut r = Rig::new();
    r.down(38, 1, p());
    assert_eq!(r.send(0).events.as_slice(), &ESC);
    assert!(r.none_at(5));
    assert_eq!(r.send(50).events.as_slice(), &ESC);
    assert_eq!(r.e.active_actions(), 0);
    assert!(r.e.held().is_empty());
}
#[test]
fn card_is_one_shot_regardless_of_release() {
    for release_early in [true, false] {
        let mut r = Rig::new();
        r.down(65, 1, p());
        assert_eq!(r.send(0).events.as_slice(), &ESC);
        if release_early {
            r.up(1);
        }
        r.repeat(1);
        assert!(r.none_at(1));
        assert_eq!(r.send(2).events.as_slice(), &tap(49));
        assert_eq!(r.send(3).events.as_slice(), &CLICK);
        assert!(r.none_at(5));
        assert_eq!(r.send(6).events.as_slice(), &ESC);
        if !release_early {
            r.up(1);
        }
        assert!(r.none_at(100));
        assert_eq!(r.e.active_actions(), 0);
        assert!(r.e.held().is_empty());
        assert!(!r.e.reports().contains(&Report::ConfirmPause));
    }
}
#[test]
fn card_clicks_at_user_cursor_without_moving() {
    let mut r = Rig::new();
    r.down(65, 1, p());
    let mut events = Vec::new();
    for now in [0, 2, 3, 6] {
        events.extend(r.send(now).events);
    }
    assert!(!events.iter().any(|e| matches!(e, OutEvent::MoveTo(_))));
}
#[test]
fn card_no_trailing_esc_on_focus_loss() {
    let mut r = Rig::new();
    r.down(65, 1, p());
    r.send(0);
    r.ev(InputKind::Focus { focused: false });
    assert!(r.none_at(3));
    assert!(r.e.reports().contains(&Report::ConfirmPause));
    assert!(matches!(r.e.state(), EngineState::Stopped(_)));
}
#[test]
fn card_forced_cancel_skips_remaining_steps_and_asks_to_confirm() {
    let mut r = Rig::new();
    r.down(65, 1, p());
    r.send(0);
    r.ev(InputKind::PressCancelled { press: PressId(1) });
    assert!(r.none_at(100));
    assert!(r.e.reports().contains(&Report::ConfirmPause));
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn hold_point_rejected_while_another_hold_point_active() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.down(88, 2, p());
    assert_eq!(r.e.active_actions(), 1);
    assert!(r.e.reports().contains(&Report::ResourceBusy));
}
#[test]
fn esc_resource_is_exclusive() {
    let mut r = Rig::new();
    r.down(40, 1, p());
    for (k, press) in [(65, 2), (32, 3)] {
        r.down(k, press, p());
    }
    assert_eq!(r.e.active_actions(), 1);
}
#[test]
fn repeat_with_click_takes_mouse_for_whole_cycle() {
    let mut r = Rig::new();
    let z = r.point_of(90);
    r.down(90, 1, p());
    r.send(0);
    r.down(50, 2, p());
    // The skill key is not sent while a hold point owns the mouse.
    assert!(r.none_at(0));
    r.up(1);
    assert_eq!(
        r.send(1).events.as_slice(),
        &[OutEvent::MoveTo(z), OutEvent::LeftUp]
    );
    assert_eq!(r.send(1).events.as_slice(), &tap(49));
}
#[test]
fn modifier_release_cancels_press() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    r.send(0);
    r.ev(InputKind::PressCancelled { press: PressId(1) });
    assert!(r.none_at(100));
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn auto_repeat_is_always_ignored() {
    let mut r = Rig::new();
    r.e.set_user_cursor(Some(r.center(0)));
    r.down(87, 1, p());
    r.send(0);
    r.repeat(1);
    assert!(r.none_at(1));
    for (key, press) in [(50, 2), (65, 3), (81, 4), (51, 5)] {
        r.down(key, press, p());
        r.repeat(press);
        r.repeat(press);
    }
    assert_eq!(r.e.active_actions(), 4);
}
#[test]
fn geometry_change_cancels_hold_point_and_releases_left() {
    let mut r = Rig::new();
    let z = r.point_of(90);
    r.down(90, 1, p());
    r.send(0);
    r.ev(InputKind::GeometryChanged);
    assert_eq!(
        r.send(1).events.as_slice(),
        &[OutEvent::MoveTo(z), OutEvent::LeftUp]
    );
}
#[test]
fn hold_point_release_moves_then_ups() {
    let mut r = Rig::new();
    let z = r.point_of(90);
    r.down(90, 1, p());
    assert_eq!(
        r.send(0).events.as_slice(),
        &[OutEvent::MoveTo(z), OutEvent::LeftDown]
    );
    assert!(r.none_at(3));
    assert_eq!(r.send(4).events.as_slice(), &[OutEvent::MoveTo(z)]);
    r.up(1);
    assert_eq!(
        r.send(5).events.as_slice(),
        &[OutEvent::MoveTo(z), OutEvent::LeftUp]
    );
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn point_card_cycle_order_and_return() {
    let mut r = Rig::new();
    let card = r.card_point();
    r.e.set_user_cursor(Some(r.center(0)));
    r.down(81, 1, p());
    assert_eq!(r.send(0).events.as_slice(), &tap(49));
    assert!(r.none_at(0));
    assert_eq!(
        r.send(1).events.as_slice(),
        &[OutEvent::MoveTo(card), OutEvent::LeftDown]
    );
    assert!(r.e.pointer_displaced());
    // The user keeps moving the physical mouse while the cursor sits on the target.
    r.e.set_user_cursor(Some(r.center(1)));
    assert!(r.none_at(2));
    assert_eq!(
        r.send(3).events.as_slice(),
        &[OutEvent::MoveTo(card), OutEvent::LeftUp]
    );
    // The cursor stays on the target for `up_to_return` after the button is released.
    assert!(r.none_at(5));
    assert!(r.e.pointer_displaced());
    assert_eq!(
        r.send(6).events.as_slice(),
        &[OutEvent::MoveTo(r.center(1))]
    );
    assert!(!r.e.pointer_displaced());
    assert!(r.none_at(9));
    // The next cycle sends the key of the card now under the cursor.
    assert_eq!(r.send(10).events.as_slice(), &tap(50));
    assert_eq!(
        r.send(11).events.as_slice(),
        &[OutEvent::MoveTo(card), OutEvent::LeftDown]
    );
    r.up(1);
    assert_eq!(
        r.send(11).events.as_slice(),
        &[OutEvent::MoveTo(card), OutEvent::LeftUp]
    );
    assert!(r.none_at(13));
    assert_eq!(
        r.send(14).events.as_slice(),
        &[OutEvent::MoveTo(r.center(1))]
    );
    assert!(r.none_at(100));
    assert_eq!(r.e.active_actions(), 0);
    assert!(r.e.held().is_empty());
}
#[test]
fn card_key_sends_key_of_slot_under_user_cursor() {
    for mode in [Mode::Six, Mode::Ten] {
        for side in [CardUiSide::Right, CardUiSide::Left] {
            let mut r = Rig::with(mode, side);
            for i in 0..r.c.slots {
                let start = u64::from(i) * 100;
                r.e.set_user_cursor(Some(r.center(i)));
                r.down(81, u32::from(i) + 1, p());
                assert_eq!(
                    r.send(start).events[0],
                    OutEvent::KeyDown(r.c.slot_keys[i as usize])
                );
                r.up(u32::from(i) + 1);
                assert!(r.none_at(start + 50));
                assert_eq!(r.e.active_actions(), 0);
            }
        }
    }
}
#[test]
fn card_key_off_hand_waits_and_resumes_on_a_card() {
    let mut r = Rig::new();
    r.e.set_user_cursor(Some(p()));
    r.down(81, 1, p());
    assert!(r.none_at(0));
    assert!(r.e.reports().contains(&Report::OutsideHand));
    assert!(r.none_at(3));
    r.e.set_user_cursor(Some(r.center(2)));
    assert_eq!(r.send(4).events.as_slice(), &tap(51));
}
#[test]
fn card_key_completes_pulse_after_release() {
    let mut r = Rig::new();
    let card = r.card_point();
    r.e.set_user_cursor(Some(r.center(0)));
    r.down(81, 1, p());
    r.send(0);
    assert_eq!(
        r.send(1).events.as_slice(),
        &[OutEvent::MoveTo(card), OutEvent::LeftDown]
    );
    r.up(1);
    assert_eq!(
        r.send(1).events.as_slice(),
        &[OutEvent::MoveTo(card), OutEvent::LeftUp]
    );
    assert!(r.none_at(3));
    assert_eq!(
        r.send(4).events.as_slice(),
        &[OutEvent::MoveTo(r.center(0))]
    );
    assert!(r.none_at(20));
    assert_eq!(r.e.active_actions(), 0);
    assert!(r.e.held().is_empty());
}
#[test]
fn card_key_release_during_up_to_return_still_waits() {
    let mut r = Rig::new();
    let card = r.card_point();
    r.e.set_user_cursor(Some(r.center(0)));
    r.down(81, 1, p());
    r.send(0);
    r.send(1);
    assert_eq!(
        r.send(3).events.as_slice(),
        &[OutEvent::MoveTo(card), OutEvent::LeftUp]
    );
    r.now = 4;
    r.up(1);
    assert!(r.none_at(5));
    assert_eq!(
        r.send(6).events.as_slice(),
        &[OutEvent::MoveTo(r.center(0))]
    );
    assert!(r.none_at(100));
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn card_key_stop_releases_without_moving_cursor() {
    let mut r = Rig::new();
    r.e.set_user_cursor(Some(r.center(0)));
    r.down(81, 1, p());
    r.send(0);
    r.send(1);
    r.e.request_stop(StopReason::User, Ticks(2));
    assert_eq!(r.send(2).events.as_slice(), &[OutEvent::LeftUp]);
    assert!(matches!(r.e.state(), EngineState::Stopped(_)));
}
#[test]
fn card_key_seam_and_boundary() {
    let rect = HandRect {
        x: [0.1, 0.7],
        y: [0.2, 0.4],
        calibrated: true,
    };
    for (x, slot) in [(0.1, 0), (0.3, 1), (0.5, 2), (0.7, 2)] {
        assert_eq!(
            slot_at(&rect, 3, SlotOrder::LeftToRight, NormPoint { x, y: 0.2 }),
            Some(slot)
        );
    }
    assert_eq!(
        slot_at(
            &rect,
            3,
            SlotOrder::LeftToRight,
            NormPoint { x: 0.09, y: 0.2 }
        ),
        None
    );
}
#[test]
fn card_key_slot_order_right_to_left() {
    let mut c = config(Mode::Ten, CardUiSide::Left, |_| {});
    c.order = SlotOrder::RightToLeft;
    let point = NormPoint {
        x: c.hand.x[0],
        y: c.hand.y[0],
    };
    let mut e = Engine::new(Arc::new(c.clone()), TickRate(1000));
    e.arm(SessionId(1), Ticks(0)).unwrap();
    e.set_user_cursor(Some(point));
    e.on_input(
        InputEvent {
            seq: 1,
            at: Ticks(0),
            session: SessionId(1),
            config: ConfigVersion(1),
            geometry: GeometryVersion(1),
            kind: InputKind::TriggerDown {
                press: PressId(1),
                trigger: c.triggers[81][0],
                cursor: point,
            },
        },
        Ticks(0),
    );
    assert_eq!(
        e.next_batch(Ticks(0)).unwrap().events[0],
        OutEvent::KeyDown(Key(53))
    );
}
#[test]
fn card_key_waits_for_mouse_held_by_point() {
    let mut r = Rig::new();
    let z = r.point_of(90);
    r.e.set_user_cursor(Some(r.center(2)));
    r.down(90, 1, p());
    r.send(0);
    r.down(81, 2, p());
    assert!(r.none_at(1));
    r.up(1);
    assert_eq!(
        r.send(1).events.as_slice(),
        &[OutEvent::MoveTo(z), OutEvent::LeftUp]
    );
    assert_eq!(r.send(1).events.as_slice(), &tap(51));
}
#[test]
fn idle_engine_still_advances_health() {
    let mut r = Rig::new();
    assert_eq!(r.e.next_wake(), Some(Ticks(50)));
    r.e.next_batch(Ticks(50));
    assert!(r.e.progress().scheduler_progress > 0);
    assert_eq!(r.e.next_wake(), Some(Ticks(100)));
}
#[test]
fn stale_session_events_are_ignored() {
    let mut r = Rig::new();
    r.e.request_stop(StopReason::User, Ticks(0));
    r.e.next_batch(Ticks(0));
    r.e.disarm().unwrap();
    r.e.arm(SessionId(2), Ticks(0)).unwrap();
    r.down(90, 1, p());
    r.e.commit(
        99,
        SubmitOutcome::Short {
            inserted: 0,
            end: Ticks(0),
        },
        Ticks(0),
    );
    assert!(matches!(r.e.state(), EngineState::Armed(SessionId(2))));
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn focus_loss_stops_session() {
    let mut r = Rig::new();
    r.ev(InputKind::Focus { focused: false });
    r.e.next_batch(Ticks(0));
    assert!(matches!(r.e.state(), EngineState::Stopped(_)));
    r.down(87, 1, p());
    assert!(r.none_at(0));
}
#[test]
fn seq_gap_stops_session() {
    let mut r = Rig::new();
    r.seq = 1;
    r.down(90, 1, p());
    r.e.next_batch(Ticks(0));
    assert!(matches!(r.e.state(), EngineState::Stopped(_)));
}
#[test]
fn steady_state_zero_allocations() {
    let mut r = Rig::new();
    let info = allocation_counter::measure(|| {
        for i in 0..100_000 {
            r.now = i * 4;
            r.down(87, i as u32 + 1, p());
            r.send(i * 4);
            r.up(i as u32 + 1);
            r.e.drain_reports(&mut |_| {});
        }
    });
    assert_eq!(info.count_total, 0);
}
proptest! {#[test]fn randomized_engine_invariants(ops in prop::collection::vec((0u8..9,0u8..10),1..500)) {let mut r=Rig::new();let mut physical=[false;256];for (i,(op,delta)) in ops.into_iter().enumerate(){r.now+=u64::from(delta);match op{0=>r.down(90,i as u32+1,p()),1=>r.down(50,i as u32+1,p()),2=>r.down(87,i as u32+1,p()),3=>r.up(i.saturating_sub(1) as u32),4=>r.ev(InputKind::GeometryChanged),5=>r.e.request_stop(StopReason::User,Ticks(r.now)),_=>{}}for _ in 0..8{let Some(b)=r.e.next_batch(Ticks(r.now))else{break;};for ev in &b.events{match *ev{OutEvent::KeyDown(k)=>{prop_assert!(!physical[k.0 as usize]);physical[k.0 as usize]=true;},OutEvent::KeyUp(k)=>{prop_assert!(physical[k.0 as usize]);physical[k.0 as usize]=false;},OutEvent::LeftDown=>{prop_assert!(!physical[1]);physical[1]=true;},OutEvent::LeftUp=>{prop_assert!(physical[1]);physical[1]=false;},_=>{}}}r.e.commit(b.id,SubmitOutcome::Ok{begin:Ticks(r.now),end:Ticks(r.now)},Ticks(r.now));}if matches!(r.e.state(),EngineState::Stopped(_)){prop_assert!(r.e.held().is_empty());prop_assert!(r.e.next_batch(Ticks(r.now)).is_none());}r.e.drain_reports(&mut |_|{});}}}
#[test]
fn sim_exports_csv() {
    let mut r = Rig::new();
    r.down(87, 1, p());
    let mut d = sim::FakeDriver::default();
    let b = r.batch(0);
    let result = d.submit(&b, Ticks(0));
    r.e.commit(b.id, result, Ticks(0));
    let csv = d.csv();
    assert!(csv.starts_with("batch_id,action,stage,deadline,begin,end\n"));
    assert!(csv.lines().count() > 1);
}
#[test]
fn cancel_waiting_repeat_does_not_release_another_actions_mouse() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.send(0);
    r.down(50, 2, p());
    r.ev(InputKind::PressCancelled { press: PressId(2) });
    assert!(r.none_at(1));
    assert!(r.e.held().contains(OutKey::LeftButton));
    assert_eq!(r.e.active_actions(), 1);
}
#[test]
fn stop_during_inflight_down_accounts_then_releases() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    let b = r.batch(0);
    r.e.request_stop(StopReason::Emergency, Ticks(1));
    r.e.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(0),
            end: Ticks(1),
        },
        Ticks(1),
    );
    assert!(r.e.held().contains(OutKey::LeftButton));
    assert_eq!(r.send(1).events.as_slice(), &[OutEvent::LeftUp]);
    assert!(matches!(r.e.state(), EngineState::Stopped(_)));
}
#[test]
fn short_down_is_conservatively_held_and_never_retried() {
    let mut r = Rig::new();
    r.down(87, 1, p());
    let b = r.batch(0);
    r.e.commit(
        b.id,
        SubmitOutcome::Short {
            inserted: 0,
            end: Ticks(0),
        },
        Ticks(0),
    );
    assert!(r.e.held().contains(OutKey::Key(Key(49))));
    assert!(r.none_at(100));
    assert!(r.e.arm(SessionId(2), Ticks(100)).is_err());
}
#[test]
fn repeat_arbiter_serves_multiple_actions_without_stealing_mouse() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    r.down(51, 2, p());
    assert_eq!(r.send(0).events.as_slice(), &tap(49));
    assert_eq!(r.send(2).events.as_slice(), &CLICK);
    assert_eq!(r.send(2).events.as_slice(), &tap(50));
    assert_eq!(r.send(4).events.as_slice(), &CLICK);
    assert_eq!(r.send(7).events.as_slice(), &tap(49));
    // Both are due at 9: the one holding the mouse finishes its cycle first.
    assert_eq!(r.send(9).events.as_slice(), &CLICK);
    assert_eq!(r.send(9).events.as_slice(), &tap(50));
}
#[test]
fn late_tap_start_still_sends_down() {
    let mut r = Rig::new();
    r.down(87, 1, p());
    assert_eq!(r.send(50).events.as_slice(), &tap(49));
    assert!(r.e.held().is_empty());
}
#[test]
fn invalid_commit_retains_possible_downs() {
    let mut r = Rig::new();
    r.down(87, 1, p());
    let b = r.batch(0);
    r.e.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(2),
            end: Ticks(1),
        },
        Ticks(2),
    );
    assert!(matches!(
        r.e.state(),
        EngineState::Faulted(_, FaultKind::InvalidCommit)
    ));
    assert!(r.e.held().contains(OutKey::Key(Key(49))));
}
proptest! {
    #[test]
    fn short_send_at_random_phase_preserves_possible_holds(short_at in 0usize..10, delays in prop::collection::vec(0u64..5,10)) {
        let mut r=Rig::new();r.down(50,1,p());let mut possible=[false;257];
        for (i,delay) in delays.into_iter().enumerate() {
            r.now=r.e.next_wake().unwrap().0.max(r.now)+delay;
            if let Some(b)=r.e.next_batch(Ticks(r.now)) {
                for ev in &b.events {match *ev {OutEvent::KeyDown(k)=>possible[k.0 as usize]=true,OutEvent::LeftDown=>possible[256]=true,_=>{}}}
                if i==short_at {
                    r.e.commit(b.id,SubmitOutcome::Short{inserted:0,end:Ticks(r.now)},Ticks(r.now));
                    for (k,held) in possible.iter().enumerate(){if *held {prop_assert!(r.e.held().contains(if k==256{OutKey::LeftButton}else{OutKey::Key(Key(k as u8))}), "possible hold was cleared");}}
                    prop_assert!(r.e.next_batch(Ticks(r.now+100)).is_none());break;
                } else {
                    for ev in &b.events {match *ev {OutEvent::KeyUp(k)=>possible[k.0 as usize]=false,OutEvent::LeftUp=>possible[256]=false,_=>{}}}
                    r.e.commit(b.id,SubmitOutcome::Ok{begin:Ticks(r.now),end:Ticks(r.now)},Ticks(r.now));
                }
            }
        }
    }
}
#[test]
fn forced_cancel_stops_second_escape() {
    let mut r = Rig::new();
    r.down(40, 1, p());
    r.send(0);
    r.ev(InputKind::PressCancelled { press: PressId(1) });
    assert!(r.none_at(5));
    assert!(r.e.held().is_empty());
    assert_eq!(r.e.active_actions(), 0);
}
