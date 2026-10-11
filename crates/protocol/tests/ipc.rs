use baauto_core::SessionId;
use baauto_protocol::ipc::*;
#[test]
fn ipc_roundtrip_and_versions() {
    let m = AppToEngine::Hello {
        cmd_seq: 1,
        protocol: PROTOCOL_VERSION,
        shm_layout: baauto_protocol::shm::LAYOUT_VERSION,
    };
    let bytes = encode_frame(&m).unwrap();
    let decoded: AppToEngine = decode_frame(&bytes).unwrap();
    assert_eq!(decoded, m);
    assert!(handshake(PROTOCOL_VERSION, baauto_protocol::shm::LAYOUT_VERSION).is_ok());
    assert!(handshake(PROTOCOL_VERSION + 1, baauto_protocol::shm::LAYOUT_VERSION).is_err());
    assert!(handshake(PROTOCOL_VERSION, 99).is_err());
}
#[test]
fn ipc_truncated_unknown_and_oversized_frames_rejected() {
    let m = AppToEngine::Probe { cmd_seq: 2 };
    let b = encode_frame(&m).unwrap();
    for n in 0..b.len() {
        assert!(decode_frame::<AppToEngine>(&b[..n]).is_err());
    }
    assert!(decode_frame::<AppToEngine>(&[255, 255, 255, 255]).is_err());
    let unknown = encode_frame(&serde_json::json!({"type":"Mystery","data":{}})).unwrap();
    assert!(decode_frame::<AppToEngine>(&unknown).is_err());
    let wrong = encode_frame(&serde_json::json!({"type":"Probe","data":{"cmd_seq":2,"unknown":1}}))
        .unwrap();
    assert!(decode_frame::<AppToEngine>(&wrong).is_err());
    let reply = EngineToApp::Stopped {
        session: SessionId(5),
        cmd_seq: 9,
        reason: baauto_core::StopReason::User,
    };
    assert_eq!(
        decode_frame::<EngineToApp>(&encode_frame(&reply).unwrap()).unwrap(),
        reply
    );
}

#[test]
fn compiled_wire_roundtrip_and_invalid_indices_rejected() {
    use baauto_config::*;
    let p: PointSet = from_json(include_str!("../../../points/範例.json")).unwrap();
    let k: KeysProfile = from_json(include_str!("../../../keys/預設按鍵.json")).unwrap();
    let c = compile(&p, &k, &HandSettings::default(), TickRate(10_000_000)).unwrap();
    let wire = CompiledWire::from(&c);
    let decoded: CompiledWire = decode_frame(&encode_frame(&wire).unwrap()).unwrap();
    let rebuilt = CompiledConfig::try_from(decoded).unwrap();
    assert_eq!(rebuilt.triggers, c.triggers);
    assert_eq!(rebuilt.timing, c.timing);
    let mut invalid = wire.clone();
    invalid.triggers[0][0] = 500;
    assert!(CompiledConfig::try_from(invalid).is_err());
    let mut invalid = wire;
    invalid.candidates[27] = false;
    assert!(CompiledConfig::try_from(invalid).is_err());
}

#[test]
fn trial_mode_and_result_preserve_session_and_command() {
    use baauto_config::*;
    let cfg = compile(
        &template_points(),
        &template_keys(),
        &HandSettings::default(),
        TickRate(1_000_000),
    )
    .unwrap();
    let trial = TrialSpec {
        id: 9,
        action: ActionSpec::Tap(Key(49)),
        timing: cfg.timing,
        start: Ticks(100),
        duration: 100_000,
        cursor: NormPoint { x: 0.5, y: 0.5 },
    };
    assert!(trial.allowed_outputs(&cfg));
    let message = AppToEngine::RunTrial {
        cmd_seq: 8,
        session: SessionId(3),
        trial: Box::new(trial),
    };
    assert_eq!(
        decode_frame::<AppToEngine>(&encode_frame(&message).unwrap()).unwrap(),
        message
    );
    let result = TrialResult {
        id: 9,
        cmd_seq: 8,
        started: Ticks(100),
        finished: Ticks(200),
        outcome: TrialOutcome::Stopped,
    };
    let reply = EngineToApp::TrialResult {
        session: SessionId(3),
        cmd_seq: 8,
        result,
    };
    assert_eq!(
        decode_frame::<EngineToApp>(&encode_frame(&reply).unwrap()).unwrap(),
        reply
    );
    let bad = TrialSpec {
        action: ActionSpec::Tap(Key(255)),
        ..trial
    };
    assert!(!bad.allowed_outputs(&cfg));
}
