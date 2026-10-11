use baauto_win_platform::fault::*;
#[test]
fn rules_are_explicit_and_bounded() {
    for s in [
        "panic@3",
        "abort@3",
        "hang:300ms@3",
        "short@3",
        "pipe-drop@3",
        "unhook@3",
        "hook-stall",
    ] {
        assert!(s.parse::<FaultRule>().is_ok());
    }
    assert_eq!(
        "hang:300ms@3".parse::<FaultRule>().unwrap(),
        FaultRule {
            batch: 3,
            action: FaultAction::Hang(300)
        }
    );
    for s in [
        "panic@0",
        "hang:0ms@1",
        "hang:300@1",
        "short@-1",
        "other@1",
        "abort",
        "hang:9999999999999ms@1",
    ] {
        assert!(s.parse::<FaultRule>().is_err());
    }
}
