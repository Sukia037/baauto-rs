use baauto_config::*;
use baauto_core::*;
use proptest::prelude::*;
use std::sync::Arc;
fn config(mode: Mode, side: CardUiSide, follow: bool) -> CompiledConfig {
    let mut p: PointSet = from_json(include_str!("../../../points/範例.json")).unwrap();
    p.mode = mode;
    let mut k: KeysProfile = from_json(include_str!("../../../keys/預設按鍵.json")).unwrap();
    k.timing.key_hold_ms = 2.;
    k.timing.mouse_hold_ms = 2.;
    k.timing.key_to_click_ms = 3.;
    k.timing.repeat_period_ms = 10.;
    k.timing.escape_gap_ms = [3., 6.];
    k.timing.card_pre_esc_ms = 2.;
    k.timing.hold_maintain_period_ms = 4.;
    k.timing.allowed_lateness_ms = LatenessTable::default();
    k.timing.auto_repeat = if follow {
        AutoRepeatPolicy::FollowTypematic
    } else {
        AutoRepeatPolicy::Ignore
    };
    let h = HandSettings {
        card_ui_side: side,
        card_auto_repeat: k.timing.auto_repeat,
        ..Default::default()
    };
    compile(&p, &k, &h, TickRate(1000)).unwrap()
}
struct Rig {
    e: Engine,
    c: CompiledConfig,
    seq: u64,
    now: u64,
}
impl Rig {
    fn new() -> Self {
        Self::with(Mode::Six, CardUiSide::Right, false)
    }
    fn with(mode: Mode, side: CardUiSide, follow: bool) -> Self {
        let c = config(mode, side, follow);
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
fn repeat_late_wake_shifts_click_stage() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    assert_eq!(r.send(0).events.as_slice(), &[OutEvent::KeyDown(Key(49))]);
    let b = r.batch(2);
    r.e.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(2),
            end: Ticks(5),
        },
        Ticks(5),
    );
    assert!(r.e.next_batch(Ticks(7)).is_none());
    assert_eq!(r.send(8).events.as_slice(), &[OutEvent::LeftDown]);
}
#[test]
fn repeat_skips_overdue_cycles_without_burst() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    for n in [0, 2, 5, 7] {
        r.send(n);
    }
    let b = r.send(50);
    assert_eq!(b.events[0], OutEvent::KeyDown(Key(49)));
    for n in [52, 55, 57] {
        r.send(n);
    }
    assert!(r.e.next_batch(Ticks(59)).is_none());
    assert!(r.e.progress().skipped_cycles >= 4);
}
#[test]
fn quick_release_repress_uses_new_press() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.send(0);
    r.up(1);
    r.send(1);
    r.down(90, 2, p());
    r.send(2);
    r.up(1);
    assert!(r.e.next_batch(Ticks(3)).is_none());
    assert!(r.e.held().contains(OutKey::LeftButton));
}
#[test]
fn stop_releases_everything_then_silent() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.send(0);
    r.down(87, 2, p());
    r.send(0);
    r.e.request_stop(StopReason::Emergency, Ticks(1));
    let b = r.send(1);
    assert!(b.events.contains(&OutEvent::LeftUp));
    assert!(b.events.contains(&OutEvent::KeyUp(Key(49))));
    assert!(matches!(r.e.state(), EngineState::Stopped(SessionId(1))));
    assert!(r.e.held().is_empty());
    assert!(r.e.next_batch(Ticks(1000)).is_none());
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
fn double_esc_cancels_when_beyond_max_gap() {
    let mut r = Rig::new();
    r.down(40, 1, p());
    r.send(0);
    r.send(2);
    assert!(r.e.next_batch(Ticks(9)).is_none());
    assert_eq!(r.e.active_actions(), 0);
    assert!(r.e.reports().contains(&Report::LateCancelled));
}
#[test]
fn card_no_trailing_esc_on_focus_loss() {
    let mut r = Rig::new();
    r.down(65, 1, p());
    r.send(0);
    r.send(2);
    r.ev(InputKind::Focus { focused: false });
    assert!(r.e.next_batch(Ticks(3)).is_none());
    assert!(r.e.reports().contains(&Report::ConfirmPause));
    assert!(matches!(r.e.state(), EngineState::Stopped(_)));
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
    r.down(90, 1, p());
    r.send(0);
    r.down(50, 2, p());
    assert!(r.e.next_batch(Ticks(0)).is_none());
    assert_eq!(
        r.e.held()
            .keys()
            .filter(|k| *k == OutKey::Key(Key(49)))
            .count(),
        0
    );
    r.up(1);
    r.send(1);
    assert_eq!(r.send(1).events[0], OutEvent::KeyDown(Key(49)));
}
#[test]
fn modifier_release_cancels_press() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    r.send(0);
    r.ev(InputKind::PressCancelled { press: PressId(1) });
    assert_eq!(r.send(1).events[0], OutEvent::KeyUp(Key(49)));
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn auto_repeat_policy_ignore_vs_follow() {
    for follow in [false, true] {
        let mut r = Rig::with(Mode::Six, CardUiSide::Right, follow);
        r.down(87, 1, p());
        r.send(0);
        r.send(2);
        r.ev(InputKind::TriggerRepeat {
            press: PressId(1),
            cursor: p(),
        });
        assert_eq!(r.e.next_batch(Ticks(3)).is_some(), follow);
    }
}
#[test]
fn geometry_change_cancels_hold_point_and_releases_left() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.send(0);
    r.ev(InputKind::GeometryChanged);
    assert_eq!(r.send(1).events.as_slice(), &[OutEvent::LeftUp]);
}
#[test]
fn card_key_sends_key_of_slot_under_cursor() {
    for mode in [Mode::Six, Mode::Ten] {
        let mut r = Rig::with(mode, CardUiSide::Right, false);
        for i in 0..r.c.slots {
            r.down(81, u32::from(i) + 1, r.center(i));
            assert_eq!(
                r.send(u64::from(i) * 3).events[0],
                OutEvent::KeyDown(r.c.slot_keys[i as usize])
            );
            r.send(u64::from(i) * 3 + 2);
        }
    }
}
#[test]
fn card_key_outside_hand_sends_nothing_and_reports() {
    let mut r = Rig::new();
    for (i, p) in [
        NormPoint { x: 0., y: 0.85 },
        NormPoint { x: 1., y: 0.85 },
        NormPoint { x: 0.8, y: 0. },
        NormPoint { x: 0.8, y: 1. },
    ]
    .into_iter()
    .enumerate()
    {
        r.down(81, i as u32 + 1, p);
        assert!(r.e.next_batch(Ticks(0)).is_none());
    }
    assert!(r.e.reports().contains(&Report::OutsideHand));
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
fn card_key_left_side_uses_left_layout() {
    let mut r = Rig::with(Mode::Six, CardUiSide::Left, false);
    r.down(81, 1, r.center(1));
    assert_eq!(r.send(0).events[0], OutEvent::KeyDown(Key(50)));
}
#[test]
fn card_key_slot_order_right_to_left() {
    let mut c = config(Mode::Ten, CardUiSide::Left, false);
    c.order = SlotOrder::RightToLeft;
    let point = NormPoint {
        x: c.hand.x[0],
        y: c.hand.y[0],
    };
    let mut e = Engine::new(Arc::new(c.clone()), TickRate(1000));
    e.arm(SessionId(1), Ticks(0)).unwrap();
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
fn card_repeat(follow: bool) {
    let mut r = Rig::with(Mode::Six, CardUiSide::Right, follow);
    r.down(81, 1, r.center(0));
    r.send(0);
    r.send(2);
    r.ev(InputKind::TriggerRepeat {
        press: PressId(1),
        cursor: r.center(1),
    });
    let b = r.e.next_batch(Ticks(3));
    assert_eq!(b.is_some(), follow);
    if let Some(b) = b {
        assert_eq!(b.events[0], OutEvent::KeyDown(Key(50)));
    }
}
#[test]
fn card_key_ignores_auto_repeat_by_default() {
    card_repeat(false);
}
#[test]
fn card_key_follow_typematic_when_configured() {
    card_repeat(true);
}
#[test]
fn card_key_rejected_when_slot_key_held() {
    let mut r = Rig::new();
    r.down(87, 1, p());
    r.send(0);
    r.down(81, 2, r.center(0));
    assert_eq!(r.e.active_actions(), 1);
    assert!(r.e.reports().contains(&Report::ResourceBusy));
}
#[test]
fn card_key_completes_pulse_after_release() {
    let mut r = Rig::new();
    r.down(81, 1, r.center(0));
    r.send(0);
    r.up(1);
    assert!(r.e.next_batch(Ticks(1)).is_none());
    assert_eq!(r.send(2).events[0], OutEvent::KeyUp(Key(49)));
    assert!(r.e.held().is_empty());
}
#[test]
fn card_key_runs_alongside_hold_point() {
    let mut r = Rig::new();
    r.down(90, 1, p());
    r.send(0);
    r.down(81, 2, r.center(2));
    assert_eq!(r.send(0).events[0], OutEvent::KeyDown(Key(51)));
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
    assert!(r.e.next_batch(Ticks(0)).is_none());
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
            r.send(i * 4 + 2);
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
fn cancel_waiting_repeat_does_not_release_another_actions_key() {
    let mut r = Rig::new();
    r.down(87, 1, p());
    r.send(0);
    r.down(50, 2, p());
    r.ev(InputKind::PressCancelled { press: PressId(2) });
    assert!(r.e.next_batch(Ticks(1)).is_none());
    assert!(r.e.held().contains(OutKey::Key(Key(49))));
    assert_eq!(r.send(2).events.as_slice(), &[OutEvent::KeyUp(Key(49))]);
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
    assert!(r.e.next_batch(Ticks(100)).is_none());
    assert!(r.e.arm(SessionId(2), Ticks(100)).is_err());
}
#[test]
fn repeat_arbiter_serves_multiple_actions_without_stealing_mouse() {
    let mut r = Rig::new();
    r.down(50, 1, p());
    r.down(51, 2, p());
    for n in [0, 2, 5, 7] {
        r.send(n);
    }
    assert_eq!(r.send(7).events[0], OutEvent::KeyDown(Key(50)));
    for n in [9, 12, 14] {
        r.send(n);
    }
    assert_eq!(r.send(14).events[0], OutEvent::KeyDown(Key(49)));
}
#[test]
fn card_normal_release_finishes_trailing_escape_pulse() {
    let mut r = Rig::new();
    r.down(65, 1, p());
    r.send(0);
    r.send(2);
    r.up(1);
    r.e.next_batch(Ticks(3));
    assert_eq!(r.send(3).events[0], OutEvent::KeyDown(Key::ESC));
    assert_eq!(r.send(5).events[0], OutEvent::KeyUp(Key::ESC));
    assert!(r.e.held().is_empty());
}
#[test]
fn expired_tap_start_cancels_without_down() {
    let mut r = Rig::new();
    r.c.timing.stage_lateness[0] = Some(1);
    r.e = Engine::new(Arc::new(r.c.clone()), TickRate(1000));
    r.e.arm(SessionId(1), Ticks(0)).unwrap();
    r.down(87, 1, p());
    assert!(r.e.next_batch(Ticks(2)).is_none());
    assert!(r.e.reports().contains(&Report::LateCancelled));
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
    r.send(2);
    r.ev(InputKind::PressCancelled { press: PressId(1) });
    assert!(r.e.next_batch(Ticks(5)).is_none());
    assert!(r.e.held().is_empty());
    assert_eq!(r.e.active_actions(), 0);
}
#[test]
fn card_release_during_first_escape_finishes_trailing_pulse() {
    for in_flight in [false, true] {
        let mut r = Rig::new();
        r.down(65, 1, p());
        let first = r.batch(0);
        if !in_flight {
            r.e.commit(
                first.id,
                SubmitOutcome::Ok {
                    begin: Ticks(0),
                    end: Ticks(0),
                },
                Ticks(0),
            );
        }
        r.up(1);
        if in_flight {
            r.e.commit(
                first.id,
                SubmitOutcome::Ok {
                    begin: Ticks(0),
                    end: Ticks(0),
                },
                Ticks(0),
            );
        }
        assert_eq!(r.send(0).events[0], OutEvent::KeyUp(Key::ESC));
        assert_eq!(r.send(2).events[0], OutEvent::KeyDown(Key::ESC));
        assert_eq!(r.send(4).events[0], OutEvent::KeyUp(Key::ESC));
        assert!(r.e.held().is_empty());
    }
}
