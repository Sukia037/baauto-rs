use baauto_win_platform::hotkeys::global_hotkey_masks;
#[test]
fn global_registration_preserves_atleast_and_exact_modifier_semantics() {
    assert_eq!(
        global_hotkey_masks("Ctrl+F9".parse().unwrap()).as_slice(),
        &[1, 3, 5, 7, 9, 11, 13, 15]
    );
    assert_eq!(
        global_hotkey_masks("=Ctrl+F9".parse().unwrap()).as_slice(),
        &[1]
    );
    let empty = global_hotkey_masks("F10".parse().unwrap());
    assert_eq!(empty.len(), 16);
    for m in 0..16 {
        assert_eq!(empty[m], m as u8);
    }
}
