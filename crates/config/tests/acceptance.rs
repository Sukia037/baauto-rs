use baauto_config::*;
fn points() -> PointSet {
    from_json(include_str!("../../../points/範例.json")).unwrap()
}
fn keys() -> KeysProfile {
    from_json(include_str!("../../../keys/預設按鍵.json")).unwrap()
}
fn check(
    p: &PointSet,
    k: &KeysProfile,
    h: &HandSettings,
) -> Result<CompiledConfig, Vec<ConfigError>> {
    compile(p, k, h, TickRate(1_000_000))
}
#[test]
fn config_rejects_left_button_trigger() {
    let mut p = points();
    p.items[0].hotkey = Some("LButton".parse().unwrap());
    assert!(check(&p, &keys(), &HandSettings::default()).is_err());
}
#[test]
fn config_preset_conflict_blocks_save() {
    let p = points();
    let mut k = keys();
    k.presets.get_mut("skill_2").unwrap().hotkey = k.presets["skill_1"].hotkey;
    assert_eq!(analyze_conflicts(&p, &k).preset_errors.len(), 2);
    assert!(check(&p, &k, &HandSettings::default()).is_err());
}
#[test]
fn config_point_hotkey_conflict_disables_point() {
    let mut p = points();
    p.items[0].hotkey = Some("w".parse().unwrap());
    let c = check(&p, &keys(), &HandSettings::default()).unwrap();
    assert_eq!(c.reports.len(), 1);
    assert!(matches!(
        c.actions[c.triggers[87][0] as usize],
        ActionSpec::Tap(_)
    ));
}
#[test]
fn config_cycle_shorter_than_stages_rejected() {
    let mut k = keys();
    k.timing.repeat_period_ms = 40.;
    assert!(
        check(&points(), &k, &HandSettings::default())
            .unwrap_err()
            .iter()
            .any(|e| e.path.contains("repeat_period"))
    );
}
#[test]
fn hand_slot_keys_need_five_unique() {
    let mut h = HandSettings::default();
    h.slot_keys[1] = h.slot_keys[0];
    assert!(check(&points(), &keys(), &h).is_err());
    let json = to_json(&h).unwrap().replace("\"5\"", "\"4\"");
    assert!(
        check(
            &points(),
            &keys(),
            &from_json::<HandSettings>(&json).unwrap()
        )
        .is_err()
    );
    let short = to_json(&h).unwrap().replace("\"4\",\n    \"5\"", "\"4\"");
    assert!(from_json::<HandSettings>(&short).is_err());
}
#[test]
fn config_hand_rect_must_be_valid() {
    let mut h = HandSettings::default();
    h.rects.ten.left.x = [0.8, 0.2];
    assert!(check(&points(), &keys(), &h).is_err());
    h.rects.ten.left.x = [f64::NAN, 0.9];
    assert!(check(&points(), &keys(), &h).is_err());
}
#[test]
fn mode_picks_hand_rect_and_slot_count() {
    let mut p = points();
    let h = HandSettings {
        card_ui_side: CardUiSide::Left,
        ..Default::default()
    };
    for (mode, n) in [(Mode::Six, 3), (Mode::Ten, 5)] {
        p.mode = mode;
        let c = check(&p, &keys(), &h).unwrap();
        assert_eq!(c.slots, n);
        assert_eq!(c.hand, h.layout(mode));
    }
}
#[test]
fn config_templates_load_and_compile() {
    let mut p = points();
    let k = keys();
    assert_eq!(k.presets.len(), 23);
    assert!(k.presets.values().all(|p| p.enabled));
    for mode in [Mode::Six, Mode::Ten] {
        p.mode = mode;
        assert!(check(&p, &k, &HandSettings::default()).is_ok());
    }
    assert!(!k.timing.verified);
}
#[test]
fn points_card_item_carries_target_coord() {
    let p = points();
    assert_eq!(p.items[2].function, PointFunction::Card);
    assert!(p.items[2].x.is_some() && p.items[2].y.is_some());
    assert!(check(&p, &keys(), &HandSettings::default()).is_ok());
}
#[test]
fn points_item_conflict_disables_item_but_file_saves() {
    let mut p = points();
    p.items[1].hotkey = p.items[0].hotkey;
    let a = analyze_conflicts(&p, &keys());
    assert_eq!(a.disabled_points, vec![1, 2]);
    assert!(check(&p, &keys(), &HandSettings::default()).is_ok());
    assert_eq!(from_json::<PointSet>(&to_json(&p).unwrap()).unwrap(), p);
}
#[test]
fn switching_keys_file_keeps_points_file() {
    let p = points();
    let k = keys();
    let mut k2 = k.clone();
    k2.timing.key_hold_ms = 15.;
    check(&p, &k2, &HandSettings::default()).unwrap();
    assert_eq!(p, points());
    assert_eq!(k, keys());
}
#[test]
fn config_reports_json_path_on_error() {
    let s = include_str!("../../../points/範例.json").replace("\"x\": 0.5", "\"x\": \"oops\"");
    assert!(
        from_json::<PointSet>(&s)
            .unwrap_err()
            .path
            .contains("items[0].x")
    );
    let s = include_str!("../../../points/範例.json").replace("\"x\": 0.5", "\"unknown\": 0.5");
    let e = from_json::<PointSet>(&s).unwrap_err();
    assert!(e.path.contains("items[0]"));
    assert!(e.reason.contains("unknown"));
    assert!(from_json::<PointSet>(&format!("{} trailing", to_json(&points()).unwrap())).is_err());
}
#[test]
fn config_hotkey_string_roundtrip() {
    for s in [
        "w",
        "Down",
        "XButton1",
        "Ctrl+F9",
        "=Ctrl+F9",
        "Shift+RButton",
    ] {
        let c: Chord = s.parse().unwrap();
        assert_eq!(c.to_string(), s);
        assert_eq!(from_json::<Chord>(&to_json(&c).unwrap()).unwrap(), c);
    }
    let a: Chord = "Ctrl+F9".parse().unwrap();
    let b: Chord = "=Ctrl+Shift+F9".parse().unwrap();
    assert!(a.overlaps(b));
}
#[test]
fn tap_zero_hold_requires_verified_profile() {
    let mut k = keys();
    k.timing.key_hold_ms = 0.;
    assert!(check(&points(), &k, &HandSettings::default()).is_err());
    k.timing.verified = true;
    assert!(check(&points(), &k, &HandSettings::default()).is_ok());
    k.timing.set_identity(ProfileIdentity {
        game_version: "changed".into(),
        ..Default::default()
    });
    assert!(!k.timing.verified);
}
#[test]
fn integer_time_conversion_rounds_up() {
    assert_eq!(ticks_ms(0.001, TickRate(3_000_000)).unwrap(), 3);
    assert_eq!(ticks_ms(0.0001, TickRate(3)).unwrap(), 1);
    assert!(ticks_ms(f64::INFINITY, TickRate(1)).is_err());
    assert!(ticks_ms(1., TickRate(0)).is_err());
}
#[test]
fn removed_lateness_table_still_loads_and_is_dropped_on_save() {
    let mut legacy: serde_json::Value =
        serde_json::from_str(include_str!("../../../keys/預設按鍵.json")).unwrap();
    legacy["timing"]["allowed_lateness_ms"] = serde_json::json!({
        "tap_start": null,
        "repeat_click": null,
        "card_start": null,
        "card_click": null,
        "double_esc_second": 0,
        "hold_start": null,
        "card_key_start": null
    });
    let k: KeysProfile = from_json(&legacy.to_string()).unwrap();
    assert_eq!(k, keys());
    assert!(!to_json(&k).unwrap().contains("allowed_lateness_ms"));
    legacy["timing"]["unknown_field"] = serde_json::json!(1);
    assert!(from_json::<KeysProfile>(&legacy.to_string()).is_err());
}
#[test]
fn escape_gaps_are_independent_waits() {
    let mut k = keys();
    k.timing.escape_gap_ms = [50., 16.];
    assert!(check(&points(), &k, &HandSettings::default()).is_ok());
}
#[test]
fn card_key_compiles_with_its_target_point() {
    let p = points();
    let card = p
        .items
        .iter()
        .find(|i| i.function == PointFunction::Card)
        .unwrap();
    let target = NormPoint {
        x: card.x.unwrap(),
        y: card.y.unwrap(),
    };
    let c = check(&p, &keys(), &HandSettings::default()).unwrap();
    assert!(c.actions.contains(&ActionSpec::CardKey(target)));
}
#[test]
fn card_key_without_position_is_disabled_until_recorded() {
    let mut p = points();
    let card = p
        .items
        .iter_mut()
        .find(|i| i.function == PointFunction::Card)
        .unwrap();
    let id = card.id;
    card.x = None;
    card.y = None;
    let a = analyze_conflicts(&p, &keys());
    assert!(a.disabled_points.contains(&id));
    assert!(
        a.reports
            .iter()
            .any(|e| e.path == format!("$.items[id={id}].position"))
    );
    let c = check(&p, &keys(), &HandSettings::default()).unwrap();
    assert!(
        !c.actions
            .iter()
            .any(|a| matches!(a, ActionSpec::CardKey(_)))
    );
}
#[test]
fn removed_card_auto_repeat_still_loads_and_is_dropped_on_save() {
    let mut legacy = serde_json::to_value(HandSettings::default()).unwrap();
    legacy["card_auto_repeat"] = serde_json::json!("follow_typematic");
    let h: HandSettings = from_json(&legacy.to_string()).unwrap();
    assert_eq!(h, HandSettings::default());
    assert!(!to_json(&h).unwrap().contains("card_auto_repeat"));
}
#[test]
fn keys_without_card_return_wait_default_to_sixteen_ms() {
    let mut old: serde_json::Value =
        serde_json::from_str(include_str!("../../../keys/預設按鍵.json")).unwrap();
    old["timing"]
        .as_object_mut()
        .unwrap()
        .remove("card_return_wait_ms");
    let k: KeysProfile = from_json(&old.to_string()).unwrap();
    assert_eq!(k.timing.card_return_wait_ms, 16.);
}
