use baauto_config::*;
use baauto_core::*;
use baauto_protocol::capture::*;
use baauto_win_platform::hook::*;
fn hook()->HookState {
    HookState::new(&compile(&template_points(),&template_keys(),&HandSettings::default(),TickRate(1000)).unwrap(),123)
}
fn key(key:Key,down:bool,at:u64)->RawEvent {
    RawEvent{kind:RawKind::Key{key,down},injected:false,extra:0,cursor:NormPoint{x:0.5,y:0.5},inside_window:true,at:Ticks(at),ledger_keys:[0;4],ledger_mouse:false}
}
#[test]
fn capture_timeout_cancel_and_armed_rejection() {
    let mut h=hook(); h.armed=true;
    assert_eq!(h.begin_capture(CaptureRequest{id:1,kind:CaptureKind::Hotkey,deadline:Ticks(100)}),Err(CaptureError::Armed));
    h.armed=false;
    h.begin_capture(CaptureRequest{id:1,kind:CaptureKind::Hotkey,deadline:Ticks(100)}).unwrap();
    assert_eq!(h.expire_capture(Ticks(99)),None);
    assert_eq!(h.expire_capture(Ticks(100)).unwrap().outcome,CaptureOutcome::TimedOut);
    h.begin_capture(CaptureRequest{id:2,kind:CaptureKind::Hotkey,deadline:Ticks(200)}).unwrap();
    assert_eq!(h.cancel_capture().unwrap().outcome,CaptureOutcome::Cancelled);
}
#[test]
fn chord_and_record_swallow_complete_physical_pairs_after_cancel_or_focus_loss() {
    let mut h=hook();
    h.begin_capture(CaptureRequest{id:1,kind:CaptureKind::Hotkey,deadline:Ticks(100)}).unwrap();
    assert!(h.decide(key(Key(162),true,1)).swallow);
    let d=h.decide(key(Key(9),true,2));
    assert!(d.swallow);
    assert_eq!(d.capture.unwrap().outcome,CaptureOutcome::Chord("Ctrl+Tab".parse().unwrap()));
    h.focus(false,Ticks(3));
    assert!(h.decide(key(Key(9),false,4)).swallow);
    assert!(h.decide(key(Key(162),false,5)).swallow);
    h.focused=true;
    h.begin_capture(CaptureRequest{id:2,kind:CaptureKind::Point,deadline:Ticks(100)}).unwrap();
    let d=h.decide(key(Key::LEFT,true,6));
    assert!(d.swallow);
    assert_eq!(d.capture.unwrap().outcome,CaptureOutcome::Point(NormPoint{x:0.5,y:0.5}));
    h.cancel_capture(); h.focus(false,Ticks(7));
    assert!(h.decide(key(Key::LEFT,false,8)).swallow);
    assert!(d.events.is_empty());
}
#[test]
fn space_side_button_escape_backspace_left_and_slot_rules() {
    for (key,outcome) in [(Key(32),CaptureOutcome::Chord("Space".parse().unwrap())),(Key(5),CaptureOutcome::Chord("XButton1".parse().unwrap())),(Key(27),CaptureOutcome::Cancelled),(Key(8),CaptureOutcome::Cleared),(Key::LEFT,CaptureOutcome::RejectedLeft)] {
        let mut h=hook();
        h.begin_capture(CaptureRequest{id:1,kind:CaptureKind::Hotkey,deadline:Ticks(100)}).unwrap();
        let d=h.decide(super_key(key));
        assert_eq!(d.capture.unwrap().outcome,outcome);
        assert!(d.swallow);
        assert!(h.decide(self::key(key,false,2)).swallow);
    }
    let mut h=hook();
    h.begin_capture(CaptureRequest{id:1,kind:CaptureKind::SlotKey,deadline:Ticks(100)}).unwrap();
    assert_eq!(h.decide(key(Key(2),true,1)).capture.unwrap().outcome,CaptureOutcome::InvalidSlot);
}
fn super_key(k:Key)->RawEvent{ key(k,true,1) }
#[test]
fn capture_callbacks_allocate_nothing_and_ignore_injected_input() {
    let mut h=hook();
    h.begin_capture(CaptureRequest{id:1,kind:CaptureKind::Hotkey,deadline:Ticks(100)}).unwrap();
    let mut injected=key(Key(32),true,1); injected.extra=123;
    assert!(h.decide(injected).capture.is_none());
    let count=allocation_counter::measure(|| { let d=h.decide(key(Key(32),true,2)); assert!(d.capture.is_some()); assert!(h.decide(key(Key(32),false,3)).swallow); });
    assert_eq!(count.count_total,0);
}
