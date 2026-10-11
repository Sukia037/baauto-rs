use baauto_config::{editor::*, legacy::*, *};
use serde_json::json;
fn load(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}
#[test]
fn import_alternating_becomes_card_items() {
    let imported = import(
        &load(include_str!(
            "../../../tests/fixtures/synthetic/points.json.json"
        )),
        "legacy",
        Mode::Ten,
        None,
    )
    .unwrap();
    let p = imported.points.unwrap();
    assert_eq!(
        p,
        from_json::<PointSet>(include_str!(
            "../../../tests/fixtures/synthetic/points.expected.json"
        ))
        .unwrap()
    );
    for code in [
        "alternating_to_card",
        "boolean_number",
        "side_modifier_normalized",
        "duplicate_id",
        "dangling_point",
        "orphan_point",
        "unsupported_hotkey",
    ] {
        assert!(
            imported.report.issues.iter().any(|i| i.code == code),
            "missing report {code}"
        );
    }
    assert_eq!(imported.report.id_map.len(), 5);
}
#[test]
fn import_functions_json_to_keys() {
    let i = import(
        &load(include_str!(
            "../../../tests/fixtures/synthetic/functions.json"
        )),
        "functions",
        Mode::Six,
        None,
    )
    .unwrap();
    let k = i.keys.unwrap();
    assert!(i.points.is_none());
    assert_eq!(k.presets.len(), 23);
    assert!(!k.presets["skill_3"].enabled);
    assert_eq!(k.timing.source, ProfileSource::ImportedLegacy);
    assert!(!k.timing.verified);
    assert_eq!(
        k.timing.escape_gap_ms,
        template_keys().timing.escape_gap_ms,
        "timing_ms never actually used by AHK"
    );
    assert_eq!(
        i.report.source_timing,
        Some(json!({"frame_fast":3,"frame_slow":7}))
    );
    assert!(
        !analyze_conflicts(&template_points(), &k)
            .preset_errors
            .is_empty()
    );
}
#[test]
fn import_points_json_to_pointset_with_mode() {
    let v = load(include_str!(
        "../../../tests/fixtures/synthetic/keys-absolute.json"
    ));
    let i = import(&v, "keys", Mode::Ten, None).unwrap();
    let p = i.points.unwrap();
    assert_eq!(p.mode, Mode::Ten);
    assert_eq!((p.items[0].x, p.items[0].y), (Some(0.25), Some(0.2)));
    assert_eq!((p.items[1].x, p.items[1].y), (Some(1.0), Some(0.0)));
}
#[test]
fn absolute_coordinates_require_resolution() {
    let v = load(include_str!(
        "../../../tests/fixtures/synthetic/keys-no-resolution.json"
    ));
    let i = import(&v, "keys", Mode::Six, None).unwrap();
    assert!(i.points.unwrap().items.is_empty());
    assert_eq!(i.report.pending_resolution.len(), 1);
    let i = import(&v, "keys", Mode::Six, Some([1000., 500.])).unwrap();
    assert_eq!(i.points.unwrap().items[0].x, Some(0.25));
}
#[test]
fn mumu_candidates_follow_reference_rules() {
    let i = import(
        &load(include_str!("../../../tests/fixtures/synthetic/mumu.json")),
        "mumu",
        Mode::Six,
        None,
    )
    .unwrap();
    assert!(i.points.is_none() && i.keys.is_none());
    assert_eq!(
        i.candidates
            .iter()
            .map(|c| (&*c.name, c.position))
            .collect::<Vec<_>>(),
        vec![
            ("技能", NormPoint { x: 0.2, y: 0.3 }),
            ("曲線", NormPoint { x: 0.5, y: 0.5 })
        ]
    );
}
#[test]
fn ahk_default_keys_and_modifiers() {
    let k = template_keys();
    for (role, p) in &k.presets {
        let chord = p.hotkey.unwrap();
        let old = chord.to_string().replace("Ctrl+", "^");
        assert_eq!(parse_ahk(&old).unwrap().0, chord, "{role}");
    }
    assert_eq!(parse_ahk("^+a").unwrap().0, "Ctrl+Shift+a".parse().unwrap());
    assert_eq!(parse_ahk("<^F9").unwrap().0, "Ctrl+F9".parse().unwrap());
    assert_eq!(
        parse_ahk("*XButton1").unwrap().0,
        "XButton1".parse().unwrap()
    );
    assert!(parse_ahk("~a").is_err());
    assert!(parse_ahk("garbage").is_err());
}
#[test]
fn editor_uses_ids_and_never_overwrites_candidates() {
    let mut e = PointSetEditor::new(PointSet {
        items: vec![
            PointItem {
                id: 9,
                name: "same".into(),
                hotkey: Some("z".parse().unwrap()),
                function: PointFunction::Point,
                x: Some(0.3),
                y: Some(0.4),
            },
            PointItem {
                id: 10,
                name: "same".into(),
                hotkey: Some("z".parse().unwrap()),
                function: PointFunction::Point,
                x: Some(0.3),
                y: Some(0.4),
            },
        ],
        ..template_points()
    });
    e.delete_item(9).unwrap();
    assert_eq!(e.document.items[0].id, 10);
    let id = e
        .add_candidate(Candidate {
            id: 1,
            name: "same".into(),
            position: NormPoint { x: 0.5, y: 0.5 },
        })
        .unwrap();
    assert_ne!(id, 10);
    assert_eq!(e.document.items.len(), 2);
    let candidate = e.move_to_candidates(id).unwrap();
    assert_eq!(candidate.position.x, 0.5);
}
#[test]
fn set_mode_six_to_ten_and_back() {
    let mut e = PointSetEditor::new(template_points());
    let h = HandSettings::default();
    let k = template_keys();
    for mode in [Mode::Ten, Mode::Six] {
        e.set_mode(mode);
        let c = compile(&e.document, &k, &h, TickRate(1_000_000)).unwrap();
        assert_eq!(c.slots, mode.slots());
        assert_eq!(c.hand, h.layout(mode));
        assert_eq!(c.slot_keys, h.slot_keys);
    }
}
#[test]
fn editor_timing_changes_clear_verification_transactionally() {
    let mut e = KeysEditor::new(template_keys());
    e.document.timing.verified = true;
    let old = e.document.clone();
    let mut t = old.timing.clone();
    t.key_hold_ms = -1.;
    assert!(e.set_timing(t).is_err());
    assert_eq!(e.document, old);
    let mut t = old.timing.clone();
    t.key_hold_ms = 30.;
    t.repeat_period_ms = 100.;
    e.set_timing(t).unwrap();
    assert!(!e.document.timing.verified);
    let mut hand = HandEditor::new(HandSettings::default());
    assert!(hand.set_slot_keys([Key(49); 5]).is_err());
}
#[test]
fn real_legacy_files_never_panic() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/legacy");
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        if entry
            .path()
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let v = load(&text);
            let result = std::panic::catch_unwind(|| import(&v, "real", Mode::Six, None));
            assert!(result.is_ok());
            assert!(result.unwrap().is_ok(), "{}", entry.path().display());
        }
    }
}

#[test]
fn every_synthetic_import_matches_golden_document_and_report() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/synthetic");
    let expected: serde_json::Value = load(include_str!(
        "../../../tests/fixtures/synthetic/report-codes.expected.json"
    ));
    for (name, counts) in expected.as_object().unwrap() {
        let v = load(&std::fs::read_to_string(root.join(name)).unwrap());
        let imported = import(
            &v,
            if name.starts_with("keys-") {
                "keys"
            } else {
                "legacy"
            },
            Mode::Ten,
            None,
        )
        .unwrap();
        let mut actual = std::collections::BTreeMap::<String, u32>::new();
        for issue in &imported.report.issues {
            *actual.entry(issue.code.clone()).or_default() += 1;
            assert!(issue.path.starts_with('$'));
            assert!(!issue.reason.is_empty());
        }
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            *counts,
            "{name} report"
        );
        match name.as_str() {
            "functions.json" => assert_eq!(
                imported.keys.unwrap(),
                from_json::<KeysProfile>(include_str!(
                    "../../../tests/fixtures/synthetic/functions.expected.json"
                ))
                .unwrap()
            ),
            "keys-absolute.json" => assert_eq!(
                imported.points.unwrap(),
                from_json::<PointSet>(include_str!(
                    "../../../tests/fixtures/synthetic/keys-absolute.expected.json"
                ))
                .unwrap()
            ),
            "mumu.json" => assert_eq!(
                serde_json::to_value(imported.candidates).unwrap(),
                load(include_str!(
                    "../../../tests/fixtures/synthetic/mumu.expected.json"
                ))
            ),
            "keys-no-resolution.json" => assert_eq!(
                serde_json::json!({"pending_resolution":imported.report.pending_resolution,"items":imported.points.unwrap().items}),
                load(include_str!(
                    "../../../tests/fixtures/synthetic/keys-no-resolution.expected.json"
                ))
            ),
            _ => assert_eq!(
                imported.points.unwrap(),
                from_json::<PointSet>(include_str!(
                    "../../../tests/fixtures/synthetic/points.expected.json"
                ))
                .unwrap()
            ),
        }
    }
}

#[test]
fn deleted_ids_not_reused_and_invalid_edit_keeps_document() {
    let mut e = PointSetEditor::new(template_points());
    let id = e.add_card_item("temporary".into()).unwrap();
    e.delete_item(id).unwrap();
    assert!(e.add_card_item("next".into()).unwrap() > id);
    let old = e.document.clone();
    assert!(
        e.update_item(
            1,
            ItemPatch {
                position: Some(NormPoint {
                    x: f64::NAN,
                    y: 0.5
                }),
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(e.document, old);
    assert!(
        import(
            &json!({"schema":"baauto-functions-v99"}),
            "unknown",
            Mode::Six,
            None
        )
        .is_err()
    );
}
#[test]
fn switching_point_and_card_keeps_target_coordinates() {
    let mut e = PointSetEditor::new(template_points());
    let point = e
        .document
        .items
        .iter()
        .find(|i| i.function == PointFunction::Point)
        .unwrap()
        .clone();
    e.update_item(
        point.id,
        ItemPatch {
            function: Some(PointFunction::Card),
            ..Default::default()
        },
    )
    .unwrap();
    let card = e.document.items.iter().find(|i| i.id == point.id).unwrap();
    assert_eq!(
        (card.function, card.x, card.y),
        (PointFunction::Card, point.x, point.y)
    );
    let id = e.add_card_item("new".into()).unwrap();
    assert!(e.document.items.iter().any(|i| i.id == id && i.x.is_none()));
    assert!(
        e.update_item(
            id,
            ItemPatch {
                function: Some(PointFunction::Point),
                ..Default::default()
            },
        )
        .is_err()
    );
    e.update_item(
        id,
        ItemPatch {
            position: Some(NormPoint { x: 0.4, y: 0.6 }),
            ..Default::default()
        },
    )
    .unwrap();
    let card = e.document.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!((card.x, card.y), (Some(0.4), Some(0.6)));
}
