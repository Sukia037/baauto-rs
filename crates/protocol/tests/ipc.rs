use baauto_protocol::ipc::*;
use baauto_core::SessionId;
#[test]
fn ipc_roundtrip_and_versions() {
    let m = AppToEngine::Hello { cmd_seq:1, protocol:PROTOCOL_VERSION, shm_layout:baauto_protocol::shm::LAYOUT_VERSION };
    let bytes = encode_frame(&m).unwrap();
    let decoded: AppToEngine = decode_frame(&bytes).unwrap();
    assert_eq!(decoded,m);
    assert!(handshake(PROTOCOL_VERSION,baauto_protocol::shm::LAYOUT_VERSION).is_ok());
    assert!(handshake(PROTOCOL_VERSION+1,baauto_protocol::shm::LAYOUT_VERSION).is_err());
    assert!(handshake(PROTOCOL_VERSION,99).is_err());
}
#[test]
fn ipc_truncated_unknown_and_oversized_frames_rejected() {
    let m = AppToEngine::Probe { cmd_seq:2 };
    let b = encode_frame(&m).unwrap();
    for n in 0..b.len() { assert!(decode_frame::<AppToEngine>(&b[..n]).is_err()); }
    assert!(decode_frame::<AppToEngine>(&[255,255,255,255]).is_err());
    let unknown = encode_frame(&serde_json::json!({"type":"Mystery","data":{}})).unwrap();
    assert!(decode_frame::<AppToEngine>(&unknown).is_err());
    let wrong = encode_frame(&serde_json::json!({"type":"Probe","data":{"cmd_seq":2,"unknown":1}})).unwrap();
    assert!(decode_frame::<AppToEngine>(&wrong).is_err());
    let reply = EngineToApp::Stopped {session:SessionId(5), cmd_seq:9, reason:baauto_core::StopReason::User};
    assert_eq!(decode_frame::<EngineToApp>(&encode_frame(&reply).unwrap()).unwrap(),reply);
}
