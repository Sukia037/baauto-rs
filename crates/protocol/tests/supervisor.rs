use baauto_config::ConfigVersion;
use baauto_core::{SessionId, StopReason, Ticks};
use baauto_protocol::supervisor::*;
fn ready() -> SupervisorFsm {
    let mut f = SupervisorFsm::new(Timeouts {
        health: 100,
        recovery: 50,
    });
    f.on(Event::HelloAccepted);
    f.on(Event::Installed {
        version: ConfigVersion(1),
    });
    f.on(Event::HotkeysRegistered(true));
    f.on(Event::KeysReleased {
        session: SessionId(0),
        version: ConfigVersion(1),
    });
    assert!(f.ready());
    f
}
fn armed() -> SupervisorFsm {
    let mut f = ready();
    assert!(
        f.on(Event::RequestArm {
            session: SessionId(1),
            cmd_seq: 1
        })
        .contains(&Effect::Arm {
            session: SessionId(1),
            cmd_seq: 1
        })
    );
    f.on(Event::Armed {
        session: SessionId(1),
        cmd_seq: 1,
        now: Ticks(0),
    });
    assert_eq!(f.state(), State::Armed(SessionId(1)));
    f
}
#[test]
fn fsm_cooperative_stop() {
    let mut f = armed();
    let effects = f.on(Event::RequestStop {
        reason: StopReason::User,
        cmd_seq: 2,
        now: Ticks(10),
    });
    assert_eq!(
        effects.as_slice(),
        &[
            Effect::SetStop(SessionId(1)),
            Effect::SendStop {
                session: SessionId(1),
                cmd_seq: 2,
                reason: StopReason::User
            }
        ]
    );
    let effects = f.on(Event::Stopped {
        session: SessionId(1),
        cmd_seq: 2,
    });
    assert!(!effects.contains(&Effect::Terminate));
    assert_eq!(f.state(), State::Disarmed);
}
#[test]
fn fsm_timeout_terminates_then_waits_exit_then_releases() {
    let mut f = armed();
    f.on(Event::RequestStop {
        reason: StopReason::Emergency,
        cmd_seq: 2,
        now: Ticks(10),
    });
    assert!(f.on(Event::Tick { now: Ticks(59) }).is_empty());
    assert_eq!(
        f.on(Event::Tick { now: Ticks(60) }).as_slice(),
        &[Effect::Terminate, Effect::WaitExit]
    );
    assert!(f.on(Event::Tick { now: Ticks(500) }).is_empty());
    assert_eq!(
        f.on(Event::ProcessExited {
            header_valid: true,
            ledger_consistent: true
        })
        .as_slice(),
        &[Effect::ReleaseFromLedger]
    );
    assert_eq!(
        f.on(Event::ReleaseCompleted { success: true }).as_slice(),
        &[Effect::DumpRing, Effect::RestartDisarmed]
    );
    assert_eq!(f.state(), State::Faulted);
}
#[test]
fn fsm_header_mismatch_uses_candidate_set() {
    let mut f = armed();
    assert_eq!(
        f.on(Event::ProcessExited {
            header_valid: false,
            ledger_consistent: true
        })
        .as_slice(),
        &[Effect::ReleaseCandidates]
    );
}
#[test]
fn fsm_stale_stopped_from_old_session_ignored() {
    let mut f = armed();
    f.on(Event::RequestStop {
        reason: StopReason::User,
        cmd_seq: 2,
        now: Ticks(10),
    });
    assert!(
        f.on(Event::Stopped {
            session: SessionId(0),
            cmd_seq: 2
        })
        .is_empty()
    );
    assert!(matches!(f.state(), State::Stopping { .. }));
    assert!(
        f.on(Event::Stopped {
            session: SessionId(1),
            cmd_seq: 1
        })
        .is_empty()
    );
    assert!(matches!(f.state(), State::Stopping { .. }));
}
/// preview.4 field report: holding the mouse button that clicked the game to the front made the
/// engine reject Arm. Nothing ran, so the session returns to Disarmed without a stop, and arms
/// again once the keys are reported released for that session.
#[test]
fn fsm_rejected_arm_returns_to_disarmed_without_stop() {
    let mut f = ready();
    f.on(Event::RequestArm {
        session: SessionId(1),
        cmd_seq: 1,
    });
    assert!(
        f.on(Event::ArmRejected {
            session: SessionId(1),
            cmd_seq: 1,
        })
        .is_empty()
    );
    assert_eq!(f.state(), State::Disarmed);
    assert!(!f.ready());
    for n in 0..10 {
        assert!(
            f.on(Event::Tick {
                now: Ticks(n * 100)
            })
            .is_empty()
        );
    }
    f.on(Event::KeysReleased {
        session: SessionId(1),
        version: ConfigVersion(1),
    });
    assert!(f.ready());
    assert!(
        f.on(Event::RequestArm {
            session: SessionId(2),
            cmd_seq: 2,
        })
        .contains(&Effect::Arm {
            session: SessionId(2),
            cmd_seq: 2,
        })
    );
}
#[test]
fn fsm_rejection_of_another_command_keeps_waiting_for_arm() {
    let mut f = ready();
    f.on(Event::RequestArm {
        session: SessionId(1),
        cmd_seq: 3,
    });
    for (session, cmd_seq) in [(SessionId(1), 2), (SessionId(0), 3)] {
        assert!(f.on(Event::ArmRejected { session, cmd_seq }).is_empty());
        assert_eq!(
            f.state(),
            State::AwaitArm {
                session: SessionId(1),
                cmd_seq: 3
            }
        );
    }
}
/// Spec §9A: a user holding the same key or the left button stops the session and must release
/// it before arming again. That is a stop, not a fault: the engine is not killed.
#[test]
fn fsm_physical_conflict_is_a_cooperative_stop() {
    let mut f = armed();
    f.on(Event::RequestStop {
        reason: StopReason::PhysicalConflict,
        cmd_seq: 2,
        now: Ticks(10),
    });
    assert!(matches!(f.state(), State::Stopping { fault: false, .. }));
    f.on(Event::Stopped {
        session: SessionId(1),
        cmd_seq: 2,
    });
    assert_eq!(f.state(), State::Disarmed);
    assert!(f.on(Event::Tick { now: Ticks(1000) }).is_empty());
}
#[test]
fn fsm_no_auto_resume_after_fault() {
    let mut f = armed();
    f.on(Event::ProcessExited {
        header_valid: true,
        ledger_consistent: true,
    });
    f.on(Event::ReleaseCompleted { success: true });
    f.on(Event::KeysReleased {
        session: SessionId(1),
        version: ConfigVersion(1),
    });
    for n in 0..100 {
        assert!(f.on(Event::Tick { now: Ticks(n) }).is_empty());
        assert!(
            f.on(Event::Armed {
                session: SessionId(1),
                cmd_seq: 1,
                now: Ticks(n)
            })
            .is_empty()
        );
        assert_eq!(f.state(), State::Faulted);
    }
}
#[test]
fn fsm_rehook_only_when_disarmed() {
    let mut f = armed();
    assert!(f.on(Event::RequestRehook).is_empty());
    let mut f = ready();
    assert_eq!(f.on(Event::RequestRehook).as_slice(), &[Effect::Rehook]);
    assert!(!f.ready());
}
#[test]
fn fsm_hotkey_registration_failure_blocks_ready() {
    let mut f = ready();
    f.on(Event::HotkeysRegistered(false));
    assert!(!f.ready());
    assert!(
        f.on(Event::RequestArm {
            session: SessionId(1),
            cmd_seq: 1
        })
        .is_empty()
    );
}
#[test]
fn fsm_idle_engine_not_treated_as_stuck() {
    let mut f = armed();
    for n in 1..1000 {
        assert!(
            f.on(Event::Health {
                now: Ticks(n * 50),
                scheduler: n,
                hook_loop: n,
                hook_physical: 0,
                raw_bound: 0,
                foreground: true
            })
            .is_empty()
        );
        assert_eq!(f.state(), State::Armed(SessionId(1)));
    }
}
#[test]
fn fsm_raw_without_foreground_and_input_never_accuses_hook() {
    let mut f = armed();
    for n in 1..10 {
        f.on(Event::Health {
            now: Ticks(n * 50),
            scheduler: n,
            hook_loop: n,
            hook_physical: 0,
            raw_bound: n,
            foreground: false,
        });
    }
    assert_eq!(f.state(), State::Armed(SessionId(1)));
}
#[test]
fn fsm_late_arm_cannot_undo_stop() {
    let mut f = ready();
    f.on(Event::RequestArm {
        session: SessionId(1),
        cmd_seq: 1,
    });
    f.on(Event::RequestStop {
        reason: StopReason::User,
        cmd_seq: 2,
        now: Ticks(0),
    });
    f.on(Event::Armed {
        session: SessionId(1),
        cmd_seq: 1,
        now: Ticks(1),
    });
    assert!(matches!(f.state(), State::Stopping { .. }));
}

#[test]
fn fsm_failed_recovery_release_blocks_rearming() {
    let mut f = armed();
    f.on(Event::ProcessExited {
        header_valid: true,
        ledger_consistent: true,
    });
    let effects = f.on(Event::ReleaseCompleted { success: false });
    assert!(effects.contains(&Effect::ReportReleaseFailure));
    f.on(Event::HelloAccepted);
    f.on(Event::Installed {
        version: ConfigVersion(1),
    });
    f.on(Event::KeysReleased {
        session: SessionId(1),
        version: ConfigVersion(1),
    });
    assert!(
        f.on(Event::RequestArm {
            session: SessionId(2),
            cmd_seq: 100
        })
        .is_empty()
    );
    assert_eq!(f.state(), State::Faulted);
}

#[test]
fn fsm_late_raw_input_matches_earlier_hook_observation() {
    let mut f = armed();
    for (now, raw, hook) in [(10, 1, 1), (20, 1, 2), (30, 2, 2), (150, 2, 2), (250, 2, 2)] {
        assert!(
            f.on(Event::Health {
                now: Ticks(now),
                scheduler: now,
                hook_loop: now,
                hook_physical: hook,
                raw_bound: raw,
                foreground: true
            })
            .is_empty()
        );
    }
    assert_eq!(f.state(), State::Armed(SessionId(1)));
}
#[test]
fn fsm_raw_input_without_hook_observation_stops_after_timeout() {
    let mut f = armed();
    f.on(Event::Health {
        now: Ticks(1),
        scheduler: 1,
        hook_loop: 1,
        hook_physical: 0,
        raw_bound: 1,
        foreground: true,
    });
    let effects = f.on(Event::Health {
        now: Ticks(101),
        scheduler: 2,
        hook_loop: 2,
        hook_physical: 0,
        raw_bound: 1,
        foreground: true,
    });
    assert!(effects.contains(&Effect::SetStop(SessionId(1))));
    assert!(f.requires_confirmation());
}

#[test]
fn fsm_disarmed_raw_input_is_not_charged_to_next_session() {
    let mut f = ready();
    f.on(Event::Health {
        now: Ticks(1),
        scheduler: 100,
        hook_loop: 100,
        hook_physical: 0,
        raw_bound: 900,
        foreground: true,
    });
    f.on(Event::RequestArm {
        session: SessionId(1),
        cmd_seq: 1,
    });
    f.on(Event::Armed {
        session: SessionId(1),
        cmd_seq: 1,
        now: Ticks(2),
    });
    for now in [50, 150, 250] {
        assert!(
            f.on(Event::Health {
                now: Ticks(now),
                scheduler: now,
                hook_loop: now,
                hook_physical: 0,
                raw_bound: 900,
                foreground: true
            })
            .is_empty()
        );
    }
    assert_eq!(f.state(), State::Armed(SessionId(1)));
}

#[test]
fn fsm_expired_hook_credit_cannot_hide_removal() {
    let mut f = armed();
    for (now, hook, raw) in [(10, 1, 0), (150, 1, 0), (160, 1, 1), (260, 1, 1)] {
        f.on(Event::Health {
            now: Ticks(now),
            scheduler: now,
            hook_loop: now,
            hook_physical: hook,
            raw_bound: raw,
            foreground: true,
        });
    }
    assert!(matches!(f.state(), State::Stopping { fault: true, .. }));
    assert!(f.requires_confirmation());
}

#[test]
fn resume_guard_rejects_continuation_after_emergency_or_manual_stop() {
    let mut gate = baauto_protocol::supervisor::ResumeGuard::default();
    let token = gate.token();
    assert!(gate.allows(token));
    gate.cancel();
    assert!(!gate.allows(token));
    let next = gate.token();
    assert!(gate.allows(next));
    gate.cancel();
    assert!(!gate.allows(next));
}

#[test]
fn chained_file_jobs_keep_the_original_resume_generation_after_stop() {
    let mut guard = ResumeGuard::default();
    let original_operation = guard.token();
    guard.cancel();
    let later_barrier = guard.token();
    assert!(guard.allows(later_barrier));
    assert!(
        !guard.allows(original_operation),
        "later barriers cannot renew the original operation's authorization"
    );
}
