use baauto_config::{store::*, *};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "baauto-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(std::fs::canonicalize(p).unwrap())
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[test]
fn store_first_start_save_rename_trash_and_tabs() {
    let t = Temp::new();
    let (mut store, warnings) = FileStore::initialize(&t.0, PortableFs).unwrap();
    assert!(warnings.is_empty());
    for doc in [
        Document::Points(template_points()),
        Document::Keys(Box::new(template_keys())),
    ] {
        let path = store
            .save_as(&t.0.join(format!("copy-{:?}.json", doc.kind())), &doc)
            .unwrap();
        assert_eq!(read_document(&path).unwrap(), doc);
        store.open(&path).unwrap();
        store.open(&path).unwrap();
        assert_eq!(store.paths(doc.kind()).len(), 2);
        let new = store.rename(&path, "改名 日本語").unwrap();
        assert!(new.exists() && !path.exists());
        let existing = t.0.join("occupied.json");
        std::fs::write(&existing, b"keep").unwrap();
        assert!(store.rename(&new, "occupied").is_err());
        std::fs::remove_file(existing).unwrap();
        let deleted = store.trash(&new).unwrap();
        assert!(deleted.exists() && !new.exists());
        assert_eq!(store.paths(doc.kind()).len(), 1);
        assert!(store.close(store.paths(doc.kind())[0].clone()).is_err());
    }
    assert!(t.0.join("settings.json").exists());
}
#[test]
fn open_detects_kind_by_content() {
    let t = Temp::new();
    let (mut s, _) = FileStore::initialize(&t.0, PortableFs).unwrap();
    let p = t.0.join("keys/points.json");
    s.save_as(&p, &Document::Keys(Box::new(template_keys())))
        .unwrap();
    assert_eq!(s.open(&p).unwrap().kind(), FileKind::Keys);
    let p = t.0.join("keys/actual-points.json");
    s.save_as(&p, &Document::Points(template_points())).unwrap();
    assert_eq!(s.open(&p).unwrap().kind(), FileKind::Points);
}
#[test]
fn same_file_not_opened_twice() {
    let t = Temp::new();
    let (mut s, _) = FileStore::initialize(&t.0, PortableFs).unwrap();
    let p = s.paths(FileKind::Points)[0].clone();
    s.open(&p).unwrap();
    s.open(&p.parent().unwrap().join("./範例.json")).unwrap();
    assert_eq!(s.paths(FileKind::Points).len(), 1);
}
#[test]
fn encoding_utf8_bom_utf16() {
    let text = to_json(&template_points()).unwrap();
    let mut encodings = vec![
        text.as_bytes().to_vec(),
        [&[239, 187, 191][..], text.as_bytes()].concat(),
    ];
    for (bom, le) in [([255, 254], true), ([254, 255], false)] {
        let mut b = bom.to_vec();
        for unit in text.encode_utf16() {
            b.extend(if le {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        encodings.push(b);
    }
    for b in encodings {
        assert_eq!(decode_json(&b).unwrap(), text);
        assert_eq!(
            parse_document(&decode_json(&b).unwrap()).unwrap(),
            Document::Points(template_points())
        );
    }
    assert!(decode_json(&[255, 254, 1]).is_err());
    assert!(parse_document("{\"schema\":\"baauto-rs-points-v99\"}").is_err());
}
#[test]
fn external_change_detected_before_save() {
    let t = Temp::new();
    let (mut s, _) = FileStore::initialize(&t.0, PortableFs).unwrap();
    let p = s.paths(FileKind::Points)[0].clone();
    let doc = s.open(&p).unwrap();
    let before = fingerprint(&p).unwrap();
    std::fs::write(&p, b"external").unwrap();
    assert!(s.save(&p, &doc, Some(before), false).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"external");
    s.save(&p, &doc, Some(before), true).unwrap();
    assert_eq!(read_document(&p).unwrap(), doc);
}
#[test]
fn missing_file_tab_marked_and_bad_settings_preserved() {
    let t = Temp::new();
    let (mut s, _) = FileStore::initialize(&t.0, PortableFs).unwrap();
    let p = s.paths(FileKind::Points)[0].clone();
    let other = t.0.join("outside.json");
    s.save_as(&other, &Document::Points(template_points()))
        .unwrap();
    s.open(&other).unwrap();
    s.persist_settings().unwrap();
    std::fs::remove_file(&other).unwrap();
    let (s, w) = FileStore::initialize(&t.0, PortableFs).unwrap();
    assert!(!w.is_empty());
    assert!(s.missing(&other));
    assert!(s.paths(FileKind::Points).contains(&other));
    assert!(p.exists());
    std::fs::write(t.0.join("settings.json"), b"broken").unwrap();
    let (_, w) = FileStore::initialize(&t.0, PortableFs).unwrap();
    assert!(!w.is_empty());
    assert!(
        std::fs::read_dir(&t.0)
            .unwrap()
            .flatten()
            .any(|f| f.file_name().to_string_lossy().contains("corrupt"))
    );
}
#[test]
fn legacy_converted_next_to_original() {
    let t = Temp::new();
    let (mut s, _) = FileStore::initialize(&t.0, PortableFs).unwrap();
    let p = t.0.join("舊檔.json.json");
    let text = include_bytes!("../../../tests/fixtures/synthetic/keys-absolute.json");
    std::fs::write(&p, text).unwrap();
    let (converted, report) = s.import_legacy(&p, Mode::Ten, None).unwrap();
    assert_eq!(converted.parent(), p.parent());
    assert!(
        converted
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("（新版）")
    );
    assert_eq!(std::fs::read(&p).unwrap(), text);
    assert!(!report.issues.is_empty());
    assert_eq!(s.open(&converted).unwrap().kind(), FileKind::Points);
}
struct FailReplace;
impl FsBackend for FailReplace {
    fn identity(&self, p: &Path) -> std::io::Result<PathBuf> {
        PortableFs.identity(p)
    }
    fn replace(&self, _: &Path, _: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("injected failure"))
    }
}
#[test]
fn interrupted_save_keeps_old_file_and_roundtrip_order() {
    let t = Temp::new();
    let p = t.0.join("file.json");
    std::fs::write(&p, b"original").unwrap();
    assert!(atomic_save(&FailReplace, &p, &Document::Points(template_points())).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"original");
    atomic_save(&PortableFs, &p, &Document::Points(template_points())).unwrap();
    assert_eq!(
        read_document(&p).unwrap(),
        Document::Points(template_points())
    );
    assert_eq!(
        std::fs::read(p.with_extension("json.bak")).unwrap(),
        b"original"
    );
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.find("\"schema\"").unwrap() < text.find("\"name\"").unwrap());
    assert!(!text.starts_with('\u{feff}'));
}
#[test]
fn paths_unicode_spaces_unc_long() {
    let t = Temp::new();
    let folder =
        t.0.join("中文 日本語 space")
            .join("a".repeat(120))
            .join("b".repeat(120));
    std::fs::create_dir_all(&folder).unwrap();
    let p = folder.join("點位.JSON");
    atomic_save(&PortableFs, &p, &Document::Points(template_points())).unwrap();
    assert_eq!(ensure_json_extension(&p), p);
    assert_eq!(
        read_document(&p).unwrap(),
        Document::Points(template_points())
    );
    assert!(valid_filename("CON").is_err());
    assert!(valid_filename("a:b").is_err());
    assert!(valid_filename("foo.").is_err());
}

struct FailHalfWrite;
impl FsBackend for FailHalfWrite {
    fn identity(&self, p: &Path) -> std::io::Result<PathBuf> {
        PortableFs.identity(p)
    }
    fn replace(&self, t: &Path, d: &Path, b: &Path) -> std::io::Result<()> {
        PortableFs.replace(t, d, b)
    }
    fn write_temp(&self, file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        file.write_all(&bytes[..bytes.len() / 2])?;
        Err(std::io::Error::other("half-written temporary"))
    }
}
#[test]
fn half_write_leaves_old_file_and_removes_only_owned_temporary() {
    let t = Temp::new();
    let p = t.0.join("original.json");
    std::fs::write(&p, b"old").unwrap();
    assert!(atomic_save(&FailHalfWrite, &p, &Document::Points(template_points())).is_err());
    assert_eq!(std::fs::read(p).unwrap(), b"old");
    assert_eq!(std::fs::read_dir(&t.0).unwrap().count(), 1);
}

struct FailSettings(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl FsBackend for FailSettings {
    fn identity(&self, p: &Path) -> std::io::Result<PathBuf> {
        PortableFs.identity(p)
    }
    fn replace(&self, t: &Path, d: &Path, b: &Path) -> std::io::Result<()> {
        if d.file_name() == Some(std::ffi::OsStr::new("settings.json"))
            && self.0.load(Ordering::SeqCst)
        {
            return Err(std::io::Error::other("settings write failure"));
        }
        PortableFs.replace(t, d, b)
    }
}
#[test]
fn failed_settings_update_rolls_back_tab_and_file_operations() {
    let t = Temp::new();
    let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (mut s, _) = FileStore::initialize(&t.0, FailSettings(fail.clone())).unwrap();
    let p = s
        .save_as(
            &t.0.join("other.json"),
            &Document::Points(template_points()),
        )
        .unwrap();
    s.open(&p).unwrap();
    let before = s.settings.clone();
    fail.store(true, Ordering::SeqCst);
    assert!(s.rename(&p, "changed").is_err());
    assert!(p.exists());
    assert_eq!(s.settings, before);
    assert!(s.trash(&p).is_err());
    assert!(p.exists());
    assert_eq!(s.settings, before);
    assert!(s.close(p.clone()).is_err());
    assert_eq!(s.settings, before);
    let fresh = s
        .save_as(
            &t.0.join("fresh.json"),
            &Document::Points(template_points()),
        )
        .unwrap();
    assert!(s.open(&fresh).is_err());
    assert_eq!(s.settings, before);
}
