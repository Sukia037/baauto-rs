//! File work belongs on a background worker. No dependency on Windows or Engine threads.
use crate::{
    legacy::{self, ImportReport},
    *,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::hash_map::DefaultHasher,
    fs::{self, OpenOptions},
    hash::{Hash, Hasher},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Points,
    Keys,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Document {
    Points(PointSet),
    Keys(Box<KeysProfile>),
}
impl Document {
    pub fn kind(&self) -> FileKind {
        match self {
            Self::Points(_) => FileKind::Points,
            Self::Keys(_) => FileKind::Keys,
        }
    }
    pub fn json(&self) -> Result<String, ConfigError> {
        match self {
            Self::Points(p) => to_json(p),
            Self::Keys(k) => to_json(k),
        }
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        let p;
        let k;
        let (points, keys) = match self {
            Self::Points(value) => {
                k = template_keys();
                (value, &k)
            }
            Self::Keys(value) => {
                p = PointSet {
                    items: Vec::new(),
                    ..template_points()
                };
                (&p, value.as_ref())
            }
        };
        compile(points, keys, &HandSettings::default(), TickRate(1_000_000))
            .map(|_| ())
            .map_err(|e| e.into_iter().next().unwrap())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiPreferences {
    pub dark: bool,
    pub scale: f32,
    pub confirm_delete: bool,
}
impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            dark: true,
            scale: 1.,
            confirm_delete: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub schema: String,
    pub hand: HandSettings,
    pub engine: EngineParams,
    pub current_points: PathBuf,
    pub current_keys: PathBuf,
    pub point_tabs: Vec<PathBuf>,
    pub key_tabs: Vec<PathBuf>,
    pub ui: UiPreferences,
}
impl Settings {
    pub fn default_at(root: &Path) -> Self {
        let p = root.join("points/範例.json");
        let k = root.join("keys/預設按鍵.json");
        Self {
            schema: "baauto-rs-settings-v1".into(),
            hand: HandSettings::default(),
            engine: EngineParams::default(),
            current_points: p.clone(),
            current_keys: k.clone(),
            point_tabs: vec![p],
            key_tabs: vec![k],
            ui: UiPreferences::default(),
        }
    }
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema != "baauto-rs-settings-v1" {
            return Err(ConfigError::new("$.schema", "unknown settings version"));
        }
        if self.point_tabs.is_empty()
            || self.key_tabs.is_empty()
            || !self.ui.scale.is_finite()
            || !(0.5..=3.).contains(&self.ui.scale)
        {
            return Err(ConfigError::new("$.settings", "invalid tabs or UI scale"));
        }
        for mode in [Mode::Six, Mode::Ten] {
            for side in [CardUiSide::Right, CardUiSide::Left] {
                let mut h = self.hand.clone();
                h.card_ui_side = side;
                compile_with_params(
                    &PointSet {
                        mode,
                        ..template_points()
                    },
                    &template_keys(),
                    &h,
                    TickRate(1_000_000),
                    self.engine,
                    ConfigVersion(1),
                )
                .map_err(|e| e.into_iter().next().unwrap())?;
            }
        }
        Ok(())
    }
}
pub fn decode_json(bytes: &[u8]) -> Result<String, ConfigError> {
    let err = |e: String| ConfigError::new("$", e);
    if bytes.starts_with(&[255, 254]) || bytes.starts_with(&[254, 255]) {
        let little = bytes[0] == 255;
        let tail = &bytes[2..];
        if !tail.len().is_multiple_of(2) {
            return Err(err("odd UTF-16 byte count".into()));
        }
        let units = tail
            .as_chunks::<2>()
            .0
            .iter()
            .map(|a| {
                if little {
                    u16::from_le_bytes([a[0], a[1]])
                } else {
                    u16::from_be_bytes([a[0], a[1]])
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&units).map_err(|e| err(e.to_string()))
    } else {
        let b = bytes.strip_prefix(&[239, 187, 191]).unwrap_or(bytes);
        String::from_utf8(b.to_vec()).map_err(|e| err(e.to_string()))
    }
}
pub fn parse_document(text: &str) -> Result<Document, ConfigError> {
    let value: serde_json::Value = from_json(text)?;
    match value["schema"].as_str() {
        Some("baauto-rs-points-v1") => from_json(text).map(Document::Points),
        Some("baauto-rs-keys-v1") => from_json(text).map(|k| Document::Keys(Box::new(k))),
        _ => Err(ConfigError::new(
            "$.schema",
            "unsupported schema; legacy files require explicit import",
        )),
    }
}
pub fn read_document(path: &Path) -> Result<Document, ConfigError> {
    parse_document(&decode_json(
        &fs::read(path).map_err(|e| file_error(path, e))?,
    )?)
}
fn file_error(path: &Path, e: impl std::fmt::Display) -> ConfigError {
    ConfigError::new(path.display().to_string(), e.to_string())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub modified: Option<SystemTime>,
    pub size: u64,
    pub hash: u64,
}
pub fn fingerprint(path: &Path) -> Result<Fingerprint, ConfigError> {
    let b = fs::read(path).map_err(|e| file_error(path, e))?;
    let m = fs::metadata(path).map_err(|e| file_error(path, e))?;
    let mut h = DefaultHasher::new();
    b.hash(&mut h);
    Ok(Fingerprint {
        modified: m.modified().ok(),
        size: m.len(),
        hash: h.finish(),
    })
}
pub trait FsBackend {
    fn identity(&self, path: &Path) -> io::Result<PathBuf>;
    fn replace(&self, temp: &Path, destination: &Path, backup: &Path) -> io::Result<()>;
}
#[derive(Clone, Copy)]
pub struct PortableFs;
impl FsBackend for PortableFs {
    fn identity(&self, p: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(p)
    }
    fn replace(&self, temp: &Path, destination: &Path, backup: &Path) -> io::Result<()> {
        if destination.exists() {
            let backup_temp = unique_sibling(backup, "backup");
            let result = (|| {
                fs::copy(destination, &backup_temp)?;
                OpenOptions::new()
                    .write(true)
                    .open(&backup_temp)?
                    .sync_all()?;
                fs::rename(&backup_temp, backup)
            })();
            if result.is_err() {
                let _ = fs::remove_file(&backup_temp);
            }
            result?;
        }
        fs::rename(temp, destination)
    }
}
static NEXT: AtomicU64 = AtomicU64::new(0);
fn unique_sibling(path: &Path, label: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(
        ".{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    path.with_file_name(name)
}
pub fn ensure_json_extension(path: &Path) -> PathBuf {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        path.into()
    } else {
        let mut p = path.as_os_str().to_os_string();
        p.push(".json");
        p.into()
    }
}
pub fn valid_filename(name: &str) -> Result<(), ConfigError> {
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c))
        || ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        || ["COM", "LPT"].iter().any(|p| {
            base.strip_prefix(p)
                .is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        })
    {
        return Err(ConfigError::new("$.filename", "invalid Windows filename"));
    }
    Ok(())
}
/// Same-directory temporary + validated UTF-8 + sync_all before the only replacement operation.
fn atomic_text<F: FsBackend>(
    backend: &F,
    path: &Path,
    text: &str,
    validate: impl FnOnce(&str) -> Result<(), ConfigError>,
) -> Result<(), ConfigError> {
    let temp = unique_sibling(path, "tmp");
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| file_error(path, e))?;
        created = true;
        file.write_all(text.as_bytes())
            .map_err(|e| file_error(path, e))?;
        file.flush().map_err(|e| file_error(path, e))?;
        let reread = decode_json(&fs::read(&temp).map_err(|e| file_error(path, e))?)?;
        validate(&reread)?;
        file.sync_all().map_err(|e| file_error(path, e))?;
        drop(file);
        backend
            .replace(&temp, path, &path.with_extension("json.bak"))
            .map_err(|e| file_error(path, format!("cannot replace: {e}; choose Save As")))
    })();
    if result.is_err() && created {
        let _ = fs::remove_file(&temp);
    }
    result
}
pub fn atomic_save<F: FsBackend>(
    backend: &F,
    path: &Path,
    doc: &Document,
) -> Result<(), ConfigError> {
    doc.validate()?;
    atomic_text(backend, path, &doc.json()?, |text| {
        let read = parse_document(text)?;
        if read != *doc {
            return Err(ConfigError::new(
                "$",
                "serialization roundtrip changed content",
            ));
        }
        read.validate()
    })
}
pub struct FileStore<F: FsBackend> {
    pub root: PathBuf,
    pub settings: Settings,
    backend: F,
}
impl<F: FsBackend> FileStore<F> {
    pub fn initialize(root: &Path, backend: F) -> Result<(Self, Vec<String>), ConfigError> {
        fs::create_dir_all(root).map_err(|e| file_error(root, e))?;
        let root = fs::canonicalize(root).map_err(|e| file_error(root, e))?;
        let defaults = Settings::default_at(&root);
        let mut warnings = Vec::new();
        for folder in ["points", "keys"] {
            fs::create_dir_all(root.join(folder)).map_err(|e| file_error(&root, e))?;
        }
        for (path, doc) in [
            (
                &defaults.current_points,
                Document::Points(template_points()),
            ),
            (
                &defaults.current_keys,
                Document::Keys(Box::new(template_keys())),
            ),
        ] {
            if !path.exists() {
                atomic_save(&backend, path, &doc)?;
            }
        }
        let settings_path = root.join("settings.json");
        let settings = if settings_path.exists() {
            let result = (|| {
                let s: Settings = from_json(&decode_json(
                    &fs::read(&settings_path).map_err(|e| file_error(&settings_path, e))?,
                )?)?;
                s.validate()?;
                Ok::<_, ConfigError>(s)
            })();
            match result {
                Ok(s) => s,
                Err(e) => {
                    let retained = unique_sibling(&settings_path, "corrupt");
                    fs::rename(&settings_path, &retained)
                        .map_err(|e| file_error(&settings_path, e))?;
                    warnings.push(format!("{e}; original retained at {}", retained.display()));
                    defaults.clone()
                }
            }
        } else {
            defaults.clone()
        };
        let mut store = Self {
            root,
            settings,
            backend,
        };
        for kind in [FileKind::Points, FileKind::Keys] {
            let missing = store
                .paths(kind)
                .iter()
                .filter(|p| !p.exists())
                .cloned()
                .collect::<Vec<_>>();
            for path in missing {
                warnings.push(format!("找不到檔案：{}", path.display()));
            }
            let active = match kind {
                FileKind::Points => &store.settings.current_points,
                FileKind::Keys => &store.settings.current_keys,
            };
            if !active.exists() {
                let fallback = match kind {
                    FileKind::Points => defaults.current_points.clone(),
                    FileKind::Keys => defaults.current_keys.clone(),
                };
                if !store.paths(kind).contains(&fallback) {
                    store.paths_mut(kind).push(fallback.clone());
                }
                store.set_current(kind, fallback);
            }
        }
        store.persist_settings()?;
        Ok((store, warnings))
    }
    pub fn paths(&self, kind: FileKind) -> &Vec<PathBuf> {
        match kind {
            FileKind::Points => &self.settings.point_tabs,
            FileKind::Keys => &self.settings.key_tabs,
        }
    }
    fn paths_mut(&mut self, kind: FileKind) -> &mut Vec<PathBuf> {
        match kind {
            FileKind::Points => &mut self.settings.point_tabs,
            FileKind::Keys => &mut self.settings.key_tabs,
        }
    }
    fn set_current(&mut self, kind: FileKind, path: PathBuf) {
        match kind {
            FileKind::Points => self.settings.current_points = path,
            FileKind::Keys => self.settings.current_keys = path,
        }
    }
    pub fn missing(&self, path: &Path) -> bool {
        !path.exists()
    }
    pub fn list(&self, kind: FileKind) -> Result<Vec<PathBuf>, ConfigError> {
        let folder = self.root.join(match kind {
            FileKind::Points => "points",
            FileKind::Keys => "keys",
        });
        let mut result = Vec::new();
        for e in fs::read_dir(&folder).map_err(|e| file_error(&folder, e))? {
            let p = e.map_err(|e| file_error(&folder, e))?.path();
            if p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                result.push(p);
            }
        }
        result.sort();
        Ok(result)
    }
    pub fn open(&mut self, path: &Path) -> Result<Document, ConfigError> {
        let real = self
            .backend
            .identity(path)
            .map_err(|e| file_error(path, e))?;
        let doc = read_document(&real)?;
        let kind = doc.kind();
        let duplicate = self
            .paths(kind)
            .iter()
            .find(|p| self.backend.identity(p).ok().as_ref() == Some(&real))
            .cloned();
        let real = duplicate.unwrap_or(real);
        if !self.paths(kind).contains(&real) {
            self.paths_mut(kind).push(real.clone());
        }
        self.set_current(kind, real);
        self.persist_settings()?;
        Ok(doc)
    }
    pub fn close(&mut self, path: PathBuf) -> Result<(), ConfigError> {
        for kind in [FileKind::Points, FileKind::Keys] {
            if let Some(i) = self.paths(kind).iter().position(|p| *p == path) {
                if self.paths(kind).len() == 1 {
                    return Err(ConfigError::new("$.tabs", "at least one tab must remain"));
                }
                self.paths_mut(kind).remove(i);
                let active = match kind {
                    FileKind::Points => &self.settings.current_points,
                    FileKind::Keys => &self.settings.current_keys,
                };
                if *active == path {
                    let next = self.paths(kind)[0].clone();
                    self.set_current(kind, next);
                }
                return self.persist_settings();
            }
        }
        Err(file_error(&path, "tab not found"))
    }
    pub fn save(
        &self,
        path: &Path,
        doc: &Document,
        expected: Option<Fingerprint>,
        overwrite: bool,
    ) -> Result<(), ConfigError> {
        if !overwrite
            && let Some(before) = expected
            && fingerprint(path)? != before
        {
            return Err(file_error(
                path,
                "external_change: reload or explicitly overwrite",
            ));
        }
        atomic_save(&self.backend, path, doc)
    }
    pub fn save_as(&self, path: &Path, doc: &Document) -> Result<PathBuf, ConfigError> {
        let p = ensure_json_extension(path);
        valid_filename(
            p.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| file_error(&p, "filename must be UTF-8"))?,
        )?;
        if p.exists() {
            return Err(file_error(&p, "name already exists"));
        }
        atomic_save(&self.backend, &p, doc)?;
        Ok(p)
    }
    pub fn rename(&mut self, path: &Path, name: &str) -> Result<PathBuf, ConfigError> {
        valid_filename(name)?;
        let new = ensure_json_extension(&path.with_file_name(name));
        if new.exists() {
            return Err(file_error(&new, "name already exists"));
        }
        fs::rename(path, &new).map_err(|e| file_error(path, e))?;
        for kind in [FileKind::Points, FileKind::Keys] {
            for p in self.paths_mut(kind) {
                if *p == path {
                    *p = new.clone();
                }
            }
        }
        if self.settings.current_points == path {
            self.settings.current_points = new.clone();
        }
        if self.settings.current_keys == path {
            self.settings.current_keys = new.clone();
        }
        self.persist_settings()?;
        Ok(new)
    }
    pub fn trash(&mut self, path: &Path) -> Result<PathBuf, ConfigError> {
        let doc = read_document(path)?;
        if self.paths(doc.kind()).len() == 1 && self.paths(doc.kind()).iter().any(|p| p == path) {
            return Err(file_error(path, "at least one tab must remain"));
        }
        let folder = path
            .parent()
            .ok_or_else(|| file_error(path, "missing parent"))?
            .join(".trash");
        fs::create_dir_all(&folder).map_err(|e| file_error(&folder, e))?;
        let destination = unique_sibling(
            &folder.join(path.file_name().unwrap_or_default()),
            "deleted",
        );
        fs::rename(path, &destination).map_err(|e| file_error(path, e))?;
        if self.paths(doc.kind()).iter().any(|p| p == path) {
            self.close(path.into())?;
        }
        Ok(destination)
    }
    pub fn persist_settings(&self) -> Result<(), ConfigError> {
        self.settings.validate()?;
        let p = self.root.join("settings.json");
        atomic_text(&self.backend, &p, &to_json(&self.settings)?, |t| {
            let s: Settings = from_json(t)?;
            s.validate()
        })
    }
    pub fn import_legacy(
        &self,
        path: &Path,
        mode: Mode,
        resolution: Option<[f64; 2]>,
    ) -> Result<(PathBuf, ImportReport), ConfigError> {
        let text = decode_json(&fs::read(path).map_err(|e| file_error(path, e))?)?;
        let v = from_json(&text)?;
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("匯入");
        let imported = legacy::import(&v, name, mode, resolution)?;
        let doc = if let Some(p) = imported.points {
            Document::Points(p)
        } else if let Some(k) = imported.keys {
            Document::Keys(Box::new(k))
        } else {
            return Err(file_error(
                path,
                "MuMu provides candidates, not a saved point file",
            ));
        };
        let mut stem = path.file_stem().unwrap_or_default().to_os_string();
        stem.push("（新版）.json");
        let converted = self.save_as(&path.with_file_name(stem), &doc)?;
        Ok((converted, imported.report))
    }
}
