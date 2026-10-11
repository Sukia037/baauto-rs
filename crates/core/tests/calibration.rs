use baauto_config::*;
use baauto_core::*;
use std::sync::Arc;
fn cfg() -> Arc<CompiledConfig> {
    Arc::new(
        compile(
            &template_points(),
            &template_keys(),
            &HandSettings::default(),
            TickRate(1_000_000),
        )
        .unwrap(),
    )
}
#[test]
fn run_trial_only_in_calibration_session_and_uses_its_timing() {
    let c = cfg();
    let mut normal = Engine::new(c.clone(), TickRate(1_000_000));
    normal.arm(SessionId(1), Ticks(0)).unwrap();
    let mut timing = c.timing;
    timing.key_hold = 30_000;
    assert_eq!(
        normal.run_trial(
            1,
            ActionSpec::Tap(Key(49)),
            timing,
            Ticks(100),
            100_000,
            NormPoint { x: 0.5, y: 0.5 }
        ),
        Err(TrialError::NormalSession)
    );
    let mut engine = Engine::new_calibration(c, TickRate(1_000_000));
    engine.arm(SessionId(1), Ticks(0)).unwrap();
    let id = engine
        .run_trial(
            1,
            ActionSpec::Tap(Key(49)),
            timing,
            Ticks(100),
            100_000,
            NormPoint { x: 0.5, y: 0.5 },
        )
        .unwrap();
    assert!(engine.next_batch(Ticks(99)).is_none());
    let b = engine.next_batch(Ticks(100)).unwrap();
    assert_eq!(b.action, Some(id));
    assert_eq!(engine.trial_for_action(id), 1);
    engine.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(100),
            end: Ticks(100),
        },
        Ticks(100),
    );
    assert!(engine.next_batch(Ticks(30_099)).is_none());
    assert_eq!(
        engine.next_batch(Ticks(30_100)).unwrap().events.as_slice(),
        &[OutEvent::KeyUp(Key(49))]
    );
}
#[test]
fn trial_repeat_is_bounded_and_stop_cannot_resurrect() {
    let c = cfg();
    let mut engine = Engine::new_calibration(c.clone(), TickRate(1_000_000));
    engine.arm(SessionId(1), Ticks(0)).unwrap();
    engine
        .run_trial(
            1,
            ActionSpec::HoldPoint(NormPoint { x: 0.5, y: 0.5 }),
            c.timing,
            Ticks(0),
            50_000,
            NormPoint { x: 0.5, y: 0.5 },
        )
        .unwrap();
    let b = engine.next_batch(Ticks(0)).unwrap();
    engine.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(0),
            end: Ticks(0),
        },
        Ticks(0),
    );
    engine.request_stop(StopReason::Emergency, Ticks(1));
    let b = engine.next_batch(Ticks(1)).unwrap();
    assert!(
        b.events
            .iter()
            .all(|e| matches!(e, OutEvent::LeftUp | OutEvent::KeyUp(_)))
    );
    engine.commit(
        b.id,
        SubmitOutcome::Ok {
            begin: Ticks(1),
            end: Ticks(1),
        },
        Ticks(1),
    );
    assert!(engine.next_batch(Ticks(100_000)).is_none());
    assert!(engine.held().is_empty());
}

#[test]
fn expired_trial_releases_held_mouse_and_never_repeats() {
    let c = cfg();
    let mut engine = Engine::new_calibration(c.clone(), TickRate(1_000_000));
    engine.arm(SessionId(1), Ticks(0)).unwrap();
    engine
        .run_trial(
            3,
            ActionSpec::HoldPoint(NormPoint { x: 0.5, y: 0.5 }),
            c.timing,
            Ticks(0),
            50_000,
            NormPoint { x: 0.5, y: 0.5 },
        )
        .unwrap();
    let batch = engine.next_batch(Ticks(0)).unwrap();
    engine.commit(
        batch.id,
        SubmitOutcome::Ok {
            begin: Ticks(0),
            end: Ticks(0),
        },
        Ticks(0),
    );
    assert_eq!(
        engine.next_wake(),
        Some(Ticks(c.timing.maintain.min(50_000)))
    );
    let batch = engine.next_batch(Ticks(50_000)).unwrap();
    assert_eq!(batch.events.as_slice(), &[OutEvent::LeftUp]);
    engine.commit(
        batch.id,
        SubmitOutcome::Ok {
            begin: Ticks(50_000),
            end: Ticks(50_000),
        },
        Ticks(50_000),
    );
    assert_eq!(engine.active_actions(), 0);
    assert!(engine.next_batch(Ticks(100_000)).is_none());
}

#[test]
fn calibration_trials_allocate_nothing_after_engine_construction() {
    let c = cfg();
    let mut e = Engine::new_calibration(c.clone(), TickRate(1_000_000));
    e.arm(SessionId(1), Ticks(0)).unwrap();
    let measured = allocation_counter::measure(|| {
        for trial in 1..=1000 {
            let now = Ticks(trial * 1_000_000);
            e.run_trial(
                trial,
                ActionSpec::Tap(Key(49)),
                c.timing,
                now,
                500_000,
                NormPoint { x: 0.5, y: 0.5 },
            )
            .unwrap();
            let down = e.next_batch(now).unwrap();
            e.commit(
                down.id,
                SubmitOutcome::Ok {
                    begin: now,
                    end: now,
                },
                now,
            );
            let at = now.after(c.timing.key_hold);
            let up = e.next_batch(at).unwrap();
            e.commit(up.id, SubmitOutcome::Ok { begin: at, end: at }, at);
            assert_eq!(e.active_actions(), 0);
        }
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}
