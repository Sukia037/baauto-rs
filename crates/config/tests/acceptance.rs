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
fn timing_rejects_zero_loop_total() {
    for set in ["spam", "click_spam", "point", "point_card"] {
        let mut k = keys();
        let t = &mut k.timing;
        match set {
            "spam" => {
                t.spam = SpamGaps {
                    key_to_click_ms: 0.,
                    click_to_next_ms: 0.,
                }
            }
            "click_spam" => t.click_spam.click_to_next_ms = 0.,
            "point" => t.point.maintain_period_ms = 0.,
            _ => {
                t.point_card = PointCardGaps {
                    key_to_down_ms: 0.,
                    down_to_up_ms: 0.,
                    up_to_return_ms: 0.,
                    return_to_next_ms: 0.,
                }
            }
        }
        let errors = check(&points(), &k, &HandSettings::default()).unwrap_err();
        assert!(
            errors.iter().any(|e| e.path == format!("$.timing.{set}")),
            "{set}: {errors:?}"
        );
    }
    for bad in [-1., f64::NAN, f64::INFINITY] {
        let mut k = keys();
        k.timing.card.esc_to_key_ms = bad;
        let errors = check(&points(), &k, &HandSettings::default()).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.path == "$.timing.card.esc_to_key_ms")
        );
    }
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
    k2.timing.spam.click_to_next_ms = 15.;
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
fn zero_gaps_are_allowed_and_identity_change_unverifies() {
    let mut k = keys();
    k.timing.frame_fast.esc_gap_ms = 0.;
    k.timing.card = CardGaps {
        esc_to_key_ms: 0.,
        key_to_click_ms: 0.,
        click_to_esc_ms: 0.,
    };
    k.timing.spam.key_to_click_ms = 0.;
    k.timing.point_card.key_to_down_ms = 0.;
    k.timing.point_card.down_to_up_ms = 0.;
    assert!(check(&points(), &k, &HandSettings::default()).is_ok());
    k.timing.verified = true;
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
fn keys_v1_migrates_to_v2_timing() {
    let text = include_str!("../../../tests/fixtures/keys-v1.json");
    let (k, notes) = parse_keys(text).unwrap();
    let template = keys().timing;
    assert_eq!(k.schema, KEYS_SCHEMA);
    assert_eq!(
        k.presets,
        from_json::<serde_json::Value>(text)
            .map(|v| {
                serde_json::from_value::<std::collections::BTreeMap<String, Preset>>(
                    v["presets"].clone(),
                )
                .unwrap()
            })
            .unwrap()
    );
    assert_eq!(k.timing.frame_fast.esc_gap_ms, 20.);
    assert_eq!(k.timing.frame_slow.esc_gap_ms, 60.);
    assert_eq!(k.timing.card.esc_to_key_ms, 12.);
    assert_eq!(k.timing.point.maintain_period_ms, 24.);
    assert_eq!(k.timing.spam, template.spam);
    assert_eq!(k.timing.click_spam, template.click_spam);
    assert_eq!(k.timing.card.key_to_click_ms, template.card.key_to_click_ms);
    assert_eq!(k.timing.card.click_to_esc_ms, template.card.click_to_esc_ms);
    assert_eq!(k.timing.point_card, template.point_card);
    assert!(!k.timing.verified);
    assert!(notes.iter().any(|n| n.contains("point_card")));
    let saved = to_json(&k).unwrap();
    assert!(saved.contains(KEYS_SCHEMA));
    for old in [
        "key_hold_ms",
        "repeat_period_ms",
        "escape_gap_ms",
        "auto_repeat",
        "allowed_lateness_ms",
    ] {
        assert!(!saved.contains(old), "{old}");
    }
    assert!(matches!(
        baauto_config::store::parse_document(text).unwrap(),
        baauto_config::store::Document::Keys(_)
    ));
    assert!(check(&points(), &k, &HandSettings::default()).is_ok());
}
#[test]
fn keys_v2_rejects_old_fields() {
    let mut v: serde_json::Value =
        serde_json::from_str(include_str!("../../../keys/預設按鍵.json")).unwrap();
    v["timing"]["key_hold_ms"] = serde_json::json!(16);
    assert!(parse_keys(&v.to_string()).is_err());
}
/// preview.6 added `point_card.up_to_return_ms`. Files saved before it still load, wait 0 there
/// as they did before, and get the field written back.
#[test]
fn keys_without_up_to_return_load_as_zero_and_save_it() {
    let mut v: serde_json::Value =
        serde_json::from_str(include_str!("../../../keys/預設按鍵.json")).unwrap();
    v["timing"]["point_card"]
        .as_object_mut()
        .unwrap()
        .remove("up_to_return_ms");
    let (k, _) = parse_keys(&v.to_string()).unwrap();
    assert_eq!(k.timing.point_card.up_to_return_ms, 0.);
    assert!(to_json(&k).unwrap().contains("\"up_to_return_ms\": 0"));
    let c = check(&points(), &k, &HandSettings::default()).unwrap();
    assert_eq!(c.timing.point_card_up_to_return, 0);
}
#[test]
fn escape_gaps_are_independent_waits() {
    let mut k = keys();
    k.timing.frame_fast.esc_gap_ms = 50.;
    k.timing.frame_slow.esc_gap_ms = 16.;
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
fn auto_repeat_option_is_gone() {
    assert!(!include_str!("../../../keys/預設按鍵.json").contains("auto_repeat"));
}
