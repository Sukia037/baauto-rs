use baauto_app::backend::{Backend, Job, UiEvent};
use baauto_config::{
    store::{Document, FileKind, read_document},
    *,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering::SeqCst},
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(1);
fn root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "baauto-gui-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, SeqCst)
    ))
}
fn loaded(b: &Backend) -> Box<baauto_app::backend::Snapshot> {
    loop {
        match b.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            UiEvent::Loaded(s, _) => return s,
            UiEvent::Message(e) => panic!("load failed: {e}"),
            _ => {}
        }
    }
}
fn finished(b: &Backend) {
    loop {
        match b.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            UiEvent::Finished(ok) => {
                assert!(ok);
                return;
            }
            UiEvent::Message(e) => panic!("job failed: {e}"),
            _ => {}
        }
    }
}
fn shutdown(b: Backend, path: PathBuf) {
    b.jobs.send(Job::Quit).unwrap();
    loop {
        if matches!(
            b.events.recv_timeout(Duration::from_secs(5)).unwrap(),
            UiEvent::Quit
        ) {
            break;
        }
    }
    drop(b);
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn worker_routes_by_content_and_keeps_other_document_when_opening_external_keys() {
    let path = root();
    let b = Backend::start(path.clone(), true, "unused.exe".into());
    let initial = loaded(&b);
    let external = path.join("另一個資料夾");
    std::fs::create_dir_all(&external).unwrap();
    let file = external.join("看似點位.json");
    std::fs::write(&file, to_json(&template_keys()).unwrap()).unwrap();
    b.jobs.send(Job::Inspect(file.clone())).unwrap();
    // The GUI ignores a new job while busy, so the follow-up must come after Finished.
    finished(&b);
    loop {
        if let UiEvent::OpenReady(p, kind) = b.events.recv_timeout(Duration::from_secs(5)).unwrap()
        {
            assert_eq!(p, file);
            assert_eq!(kind, FileKind::Keys);
            break;
        }
    }
    b.jobs
        .send(Job::Guarded(Box::new(Job::Open(file.clone(), false)), 0))
        .unwrap();
    let next = loaded(&b);
    assert_eq!(
        next.settings.current_points,
        initial.settings.current_points
    );
    assert_eq!(next.points, initial.points);
    assert_eq!(next.settings.current_keys, file.canonicalize().unwrap());
    finished(&b);
    shutdown(b, path);
}
/// preview.6 field report: 「從舊檔／MuMu 匯入…」 did nothing. ImportReady arrived before the
/// planning job's Finished, while the GUI was still busy, so the import was dropped.
#[test]
fn legacy_import_ready_follows_finished_and_imports_points() {
    let path = root();
    let b = Backend::start(path.clone(), true, "unused.exe".into());
    loaded(&b);
    let file = path.join("舊點位.json");
    std::fs::write(
        &file,
        include_str!("../../../tests/fixtures/synthetic/points.json.json"),
    )
    .unwrap();
    b.jobs
        .send(Job::PlanImport {
            path: file.clone(),
            mode: Mode::Ten,
            resolution: None,
        })
        .unwrap();
    finished(&b);
    let (mode, resolution, kind) = loop {
        match b.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            UiEvent::ImportReady(p, mode, resolution, kind) => {
                assert_eq!(p, file);
                break (mode, resolution, kind);
            }
            UiEvent::Finished(_) => panic!("ImportReady must follow the plan's Finished"),
            _ => {}
        }
    };
    assert_eq!(kind, FileKind::Points);
    b.jobs
        .send(Job::Guarded(
            Box::new(Job::Import {
                expected_kind: kind,
                path: file,
                mode,
                resolution,
                resume: false,
            }),
            0,
        ))
        .unwrap();
    let next = loop {
        match b.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            UiEvent::Loaded(s, _) => break s,
            UiEvent::Message(e) if e.starts_with("問題") => panic!("import failed: {e}"),
            _ => {}
        }
    };
    assert_eq!(next.points.mode, Mode::Ten);
    assert!(next.points.items.iter().any(|i| i.name == "拖曳一"));
    finished(&b);
    shutdown(b, path);
}
#[test]
fn worker_saves_each_json_without_combining_machine_settings_or_losing_retry_fingerprint() {
    let path = root();
    let b = Backend::start(path.clone(), true, "unused.exe".into());
    let initial = loaded(&b);
    let mut p = initial.points.clone();
    p.mode = Mode::Ten;
    let mut k = initial.keys.clone();
    k.timing.spam.click_to_next_ms += 1.;
    b.jobs
        .send(Job::Save {
            points: Some(p.clone()),
            keys: Some(Box::new(k.clone())),
            hand: None,
            expected_generation: None,
            resume: false,
        })
        .unwrap();
    loaded(&b);
    finished(&b);
    assert_eq!(
        read_document(&initial.settings.current_points).unwrap(),
        Document::Points(p.clone())
    );
    assert_eq!(
        read_document(&initial.settings.current_keys).unwrap(),
        Document::Keys(Box::new(k))
    );
    p.items[0].name = "再儲存一次".into();
    b.jobs
        .send(Job::Save {
            points: Some(p.clone()),
            keys: None,
            hand: None,
            expected_generation: None,
            resume: false,
        })
        .unwrap();
    loaded(&b);
    finished(&b);
    assert_eq!(
        read_document(&initial.settings.current_points).unwrap(),
        Document::Points(p)
    );
    shutdown(b, path);
}
