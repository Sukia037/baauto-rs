#![cfg(windows)]
use baauto_config::{store::*, *};
use baauto_win_platform::WindowsFs;
#[test]
fn native_unicode_long_case_slash_backup_and_readonly() {
    let root = std::env::temp_dir().join(format!("baauto-native-store-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let result = std::panic::catch_unwind(|| {
        let (mut store, _) = FileStore::initialize(&root, WindowsFs).unwrap();
        let folder = root
            .join("中文 日本語 spaces")
            .join("a".repeat(120))
            .join("b".repeat(120));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("Points.JSON");
        let doc = Document::Points(template_points());
        atomic_save(&WindowsFs, &path, &doc).unwrap();
        let before = fingerprint(&path).unwrap();
        store.open(&path).unwrap();
        let alias = std::path::PathBuf::from(
            path.to_string_lossy()
                .replace('\\', "/")
                .replace("Points.JSON", "points.json"),
        );
        store.open(&alias).unwrap();
        assert_eq!(store.paths(FileKind::Points).len(), 2);
        atomic_save(&WindowsFs, &path, &doc).unwrap();
        assert_eq!(read_document(&path).unwrap(), doc);
        assert_eq!(
            read_document(&path.with_extension("json.bak")).unwrap(),
            doc
        );
        let original_permissions = std::fs::metadata(&path).unwrap().permissions();
        let mut permissions = original_permissions.clone();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        assert!(atomic_save(&WindowsFs, &path, &doc).is_err());
        assert_eq!(fingerprint(&path).unwrap().hash, before.hash);
        std::fs::set_permissions(&path, original_permissions).unwrap();
    });
    let _ = std::fs::remove_dir_all(root);
    assert!(result.is_ok());
}
/// Set BAAUTO_TEST_UNC_ROOT to a writable test directory on a real UNC share.
#[test]
#[ignore = "requires a writable UNC share supplied by BAAUTO_TEST_UNC_ROOT"]
fn paths_unc_native() {
    let root = std::path::PathBuf::from(
        std::env::var_os("BAAUTO_TEST_UNC_ROOT").expect("UNC test directory"),
    );
    let path = root.join(format!("baauto-中文-{}.json", std::process::id()));
    let doc = Document::Points(template_points());
    atomic_save(&WindowsFs, &path, &doc).unwrap();
    assert_eq!(read_document(&path).unwrap(), doc);
    atomic_save(&WindowsFs, &path, &doc).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(path.with_extension("json.bak")).unwrap();
}
