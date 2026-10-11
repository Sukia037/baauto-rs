use baauto_app::state::*;
use baauto_config::{store::FileKind, *};
#[test]
fn switching_one_json_requires_stop_and_preserves_other_dirty_document() {
    let mut s = FileFlow::default();
    s.points_dirty = true;
    s.keys_dirty = true;
    s.armed = true;
    assert_eq!(
        s.request(FileKind::Points, "other.json".into()),
        Step::AskUnsaved
    );
    assert_eq!(s.choose(UnsavedChoice::Save, true), Step::Stop);
    assert_eq!(s.stopped(), Step::Save(FileKind::Points));
    assert_eq!(
        s.saved(true),
        Step::Load(FileKind::Points, "other.json".into())
    );
    assert_eq!(s.loaded(true), Step::Apply);
    assert_eq!(s.installed(), Step::WaitReleased);
    assert_eq!(s.keys_released(), Step::Arm);
    assert!(s.keys_dirty);
    assert!(!s.points_dirty);
}
#[test]
fn invalid_save_cancel_and_failed_worker_never_arm() {
    let mut s = FileFlow::default();
    s.keys_dirty = true;
    s.armed = true;
    assert_eq!(
        s.request(FileKind::Keys, "new.json".into()),
        Step::AskUnsaved
    );
    assert_eq!(s.choose(UnsavedChoice::Save, false), Step::AskUnsaved);
    assert_eq!(s.choose(UnsavedChoice::Cancel, false), Step::Idle);
    s.request(FileKind::Keys, "new.json".into());
    assert_eq!(s.choose(UnsavedChoice::Discard, true), Step::Stop);
    assert!(matches!(s.stopped(), Step::Load(FileKind::Keys, _)));
    assert_eq!(s.loaded(false), Step::Failed);
    assert_eq!(s.keys_released(), Step::Idle);
}
#[test]
fn conflict_sort_selection_and_button_conditions_use_item_ids() {
    let mut p = template_points();
    let mut k = template_keys();
    p.items = vec![
        PointItem {
            id: 7,
            name: "same".into(),
            hotkey: Some("F12".parse().unwrap()),
            function: PointFunction::Card,
            x: Some(0.5),
            y: Some(0.45),
        },
        PointItem {
            id: 9,
            name: "same".into(),
            hotkey: Some("W".parse().unwrap()),
            function: PointFunction::Card,
            x: Some(0.5),
            y: Some(0.45),
        },
    ];
    assert_eq!(sorted_item_ids(&p, &k)[0], 9);
    assert!(can_save_keys(&p, &k));
    let chord = k.presets["global_stop"].hotkey;
    k.presets.get_mut("record").unwrap().hotkey = chord;
    assert!(!can_save_keys(&p, &k));
    assert_eq!(selected_item(&p, 7).unwrap().id, 7);
    assert!(selected_item(&p, 8).is_none());
}
#[test]
fn calibration_drag_mirror_and_center_pair_validate_boundaries() {
    let r = HandRect {
        x: [0.6, 0.9],
        y: [0.7, 0.9],
        calibrated: false,
    };
    let mirrored = mirror_rect(r);
    assert!((mirrored.x[0] - 0.1).abs() < 1e-12);
    assert_eq!(mirrored.y, r.y);
    assert!(drag_corner(r, Corner::TopLeft, NormPoint { x: 0.95, y: 0.8 }).is_err());
    let moved = drag_corner(r, Corner::TopLeft, NormPoint { x: 0.5, y: 0.6 }).unwrap();
    assert!(moved.calibrated && moved.valid());
    let centers = rect_from_centers(
        NormPoint { x: 0.65, y: 0.8 },
        NormPoint { x: 0.85, y: 0.8 },
        3,
        0.2,
    )
    .unwrap();
    assert!((centers.x[0] - 0.6).abs() < 1e-12);
    assert!((centers.x[1] - 0.9).abs() < 1e-12);
    assert!(
        rect_from_centers(
            NormPoint { x: 0.9, y: 0.8 },
            NormPoint { x: 0.1, y: 0.8 },
            5,
            0.2
        )
        .is_err()
    );
}

#[test]
fn all_calibration_edges_and_corners_adjust_only_the_selected_boundaries() {
    let r = HandRect {
        x: [0.2, 0.8],
        y: [0.3, 0.7],
        calibrated: false,
    };
    for (corner, x, y) in [
        (Corner::TopRight, [0.2, 0.6], [0.4, 0.7]),
        (Corner::BottomLeft, [0.6, 0.8], [0.3, 0.4]),
        (Corner::Left, [0.6, 0.8], [0.3, 0.7]),
        (Corner::Right, [0.2, 0.6], [0.3, 0.7]),
        (Corner::Top, [0.2, 0.8], [0.4, 0.7]),
        (Corner::Bottom, [0.2, 0.8], [0.3, 0.4]),
    ] {
        let moved = drag_corner(r, corner, NormPoint { x: 0.6, y: 0.4 }).unwrap();
        assert_eq!(moved.x, x);
        assert_eq!(moved.y, y);
    }
}

#[test]
fn global_control_bindings_require_keyboard_but_actions_accept_side_buttons() {
    use baauto_config::editor::KeysEditor;
    let mut keys = KeysEditor::new(template_keys());
    assert!(
        keys.set_preset(
            "global_stop",
            Some("XButton1".parse().unwrap()),
            true,
            false
        )
        .is_err()
    );
    assert!(
        keys.set_preset("record", Some("RButton".parse().unwrap()), true, false)
            .is_err()
    );
    keys.set_preset("skill_1", Some("XButton1".parse().unwrap()), true, false)
        .unwrap();
}
