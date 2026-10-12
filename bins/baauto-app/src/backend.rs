//! All files/dialogs/stop barriers run off the GUI thread. Supervisor has its own independent thread.
use baauto_config::{editor::*, store::*, *};
use baauto_protocol::{capture::*, ipc::EngineToApp};
#[cfg(windows)]
use baauto_win_platform::*;
#[cfg(windows)]
use std::time::Duration;
use std::{path::PathBuf, sync::mpsc, thread};
#[cfg(windows)]
type BackendFs = WindowsFs;
#[cfg(not(windows))]
type BackendFs = PortableFs;
#[derive(Clone)]
pub struct Snapshot {
    pub settings: Settings,
    pub points: PointSet,
    pub keys: KeysProfile,
    pub missing: Vec<PathBuf>,
    pub logs_dir: PathBuf,
}
#[derive(Clone, Debug)]
pub struct Observation {
    pub state: String,
    pub foreground: bool,
    pub monitor: Option<[i32; 4]>,
    pub cursor: Option<NormPoint>,
    pub warning: bool,
    pub records: u64,
    pub hotkey_warning: Option<String>,
    /// False while the emergency stop hotkey is unregistered; Enable is refused until it is fixed.
    pub can_arm: bool,
}
pub enum UiEvent {
    Loaded(Box<Snapshot>, u8),
    Message(String),
    Candidates(Vec<legacy::Candidate>, String),
    Capture(u64, std::result::Result<EngineToApp, String>),
    CaptureReady(u64),
    Observe(Observation),
    Wanted(bool, u64),
    NeedImport(PathBuf, String),
    OpenReady(PathBuf, FileKind),
    ImportReady(PathBuf, Mode, Option<[f64; 2]>, FileKind),
    Finished(bool),
    Timing(Box<TimingProfile>),
    Reports(Vec<PathBuf>),
    Overlay([i32; 4], u64),
    Quit,
}
pub enum Job {
    Guarded(Box<Job>, u64),
    Save {
        points: Option<PointSet>,
        keys: Option<Box<KeysProfile>>,
        hand: Option<HandSettings>,
        expected_generation: Option<u64>,
        resume: bool,
    },
    Open(PathBuf, bool),
    OpenAs(PathBuf, FileKind, bool),
    PlanImport {
        path: PathBuf,
        mode: Mode,
        resolution: Option<[f64; 2]>,
    },
    Reassign(PathBuf, PathBuf, bool),
    Reload(FileKind, bool),
    New(FileKind, bool),
    SaveAs(Document, bool),
    Close(PathBuf, bool, bool),
    Import {
        expected_kind: FileKind,
        path: PathBuf,
        mode: Mode,
        resolution: Option<[f64; 2]>,
        resume: bool,
    },
    Screenshot {
        path: PathBuf,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    DialogOpen,
    Inspect(PathBuf),
    DialogImport,
    LoadTiming,
    Reports,
    PrepareOverlay,
    PlaceOverlay(String, [i32; 4]),
    OpenExternal(PathBuf),
    Preferences(UiPreferences),
    Quit,
}
#[derive(Clone)]
pub struct Controller {
    #[cfg(windows)]
    pub tx: Option<mpsc::Sender<SupervisorCommand>>,
    pub events: mpsc::SyncSender<UiEvent>,
    pub capture_epoch: std::sync::Arc<std::sync::atomic::AtomicU64>,
}
impl Controller {
    pub fn enable(&self) {
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let _ = tx.send(SupervisorCommand::Enable);
        }
    }
    pub fn disable(&self) {
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let _ = tx.send(SupervisorCommand::Disable);
        }
    }
    pub fn cancel_capture(&self) {
        let through_id = self
            .capture_epoch
            .swap(0, std::sync::atomic::Ordering::SeqCst);
        let _ = through_id;
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let _ = tx.send(SupervisorCommand::CancelCapture { through_id });
        }
    }
    pub fn quiesce(&self) -> Result<u64, String> {
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let (reply, rx) = mpsc::channel();
            tx.send(SupervisorCommand::Quiesce { reply })
                .map_err(|e| e.to_string())?;
            return rx
                .recv_timeout(Duration::from_secs(10))
                .map_err(|e| e.to_string())?;
        }
        Ok(0)
    }
    pub fn resume(&self, generation: u64) {
        let _ = generation;
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let _ = tx.send(SupervisorCommand::EnableAfter { generation });
        }
    }
    pub fn capture(&self, kind: CaptureKind, id: u64, expected_generation: u64) {
        self.capture_epoch
            .store(id, std::sync::atomic::Ordering::SeqCst);
        let this = self.clone();
        thread::spawn(move || {
            let result = (|| -> Result<EngineToApp, String> {
                let _stopped_generation = this.quiesce()?;
                let generation = expected_generation;
                let _ = generation;
                if this.capture_epoch.load(std::sync::atomic::Ordering::SeqCst) != id {
                    return Err("擷取已取消".into());
                }
                #[cfg(windows)]
                if let Some(tx) = &this.tx {
                    let (reply, rx) = mpsc::channel();
                    tx.send(SupervisorCommand::Capture {
                        kind,
                        id,
                        timeout_ms: 15_000,
                        generation,
                        reply,
                    })
                    .map_err(|e| e.to_string())?;
                    let deadline = Clock::new().map_err(|e| e.to_string())?;
                    let due = deadline.now().after(deadline.rate.0 * 17);
                    loop {
                        if deadline.now() >= due {
                            return Err("擷取逾時".into());
                        }
                        let message = rx
                            .recv_timeout(Duration::from_secs(17))
                            .map_err(|e| e.to_string())??;
                        if matches!(message, EngineToApp::CaptureReady { .. }) {
                            let _ = this.events.send(UiEvent::CaptureReady(id));
                        } else {
                            return Ok(message);
                        }
                    }
                }
                let _ = (kind, id);
                Err("預覽模式沒有遊戲 Hook；請在 Windows 操作".into())
            })();
            let _ = this.events.send(UiEvent::Capture(id, result));
        });
    }
    fn apply(
        &self,
        s: &Snapshot,
        version: u32,
        resume: bool,
        generation: u64,
    ) -> Result<(), String> {
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let clock = Clock::new().map_err(|e| e.to_string())?;
            let cfg = compile_with_params(
                &s.points,
                &s.keys,
                &s.settings.hand,
                clock.rate,
                s.settings.engine,
                ConfigVersion(version),
            )
            .map_err(|e| format!("{e:?}"))?;
            let stop_hotkey = s.keys.presets["global_stop"]
                .hotkey
                .ok_or("緊急停止熱鍵不可空白")?;
            let record_hotkey = s.keys.presets["record"]
                .enabled
                .then_some(s.keys.presets["record"].hotkey)
                .flatten();
            tx.send(SupervisorCommand::Apply {
                cfg: std::sync::Arc::new(cfg),
                stop_hotkey,
                record_hotkey,
            })
            .map_err(|e| e.to_string())?;
            if resume {
                tx.send(SupervisorCommand::EnableAfter { generation })
                    .map_err(|e| e.to_string())?;
            }
        }
        let _ = (s, version, resume, generation);
        Ok(())
    }
    pub fn quit(&self) {
        #[cfg(windows)]
        if let Some(tx) = &self.tx {
            let _ = tx.send(SupervisorCommand::Quit);
        }
    }
}
pub struct Backend {
    pub jobs: mpsc::Sender<Job>,
    pub events: mpsc::Receiver<UiEvent>,
    pub controller: Controller,
}
/// What a v1 preset-key file lost in conversion, for the log tab. Empty for v2 files.
fn keys_migration_notes(path: &std::path::Path) -> Vec<String> {
    std::fs::read(path)
        .ok()
        .and_then(|b| decode_json(&b).ok())
        .and_then(|text| parse_keys(&text).ok())
        .map(|(_, notes)| {
            notes
                .into_iter()
                .map(|n| format!("{}：{n}", path.display()))
                .collect()
        })
        .unwrap_or_default()
}
fn snapshot(store: &FileStore<BackendFs>) -> Result<Snapshot, String> {
    let points = match read_document(&store.settings.current_points).map_err(|e| e.to_string())? {
        Document::Points(p) => p,
        _ => return Err("點位文件種類錯誤".into()),
    };
    let keys = match read_document(&store.settings.current_keys).map_err(|e| e.to_string())? {
        Document::Keys(k) => *k,
        _ => return Err("按鍵文件種類錯誤".into()),
    };
    let missing = store
        .settings
        .point_tabs
        .iter()
        .chain(&store.settings.key_tabs)
        .filter(|p| store.missing(p))
        .cloned()
        .collect();
    Ok(Snapshot {
        settings: store.settings.clone(),
        points,
        keys,
        missing,
        logs_dir: store.root.join("logs"),
    })
}
impl Backend {
    pub fn start(root: PathBuf, preview: bool, target: std::ffi::OsString) -> Self {
        let (jobs, rx) = mpsc::channel();
        let (events, out) = mpsc::sync_channel(256);
        #[cfg(windows)]
        let (native_tx, native_rx) = mpsc::channel();
        let controller = Controller {
            #[cfg(windows)]
            tx: (!preview).then_some(native_tx),
            events: events.clone(),
            capture_epoch: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let control = controller.clone();
        thread::spawn(move || {
            let _ = (preview, &target);
            let initialized = (|| -> Result<_, String> {
                let first = !root.join("settings.json").exists();
                let (mut store, warnings) =
                    FileStore::initialize(&root, backend_fs()).map_err(|e| e.to_string())?;
                for w in warnings {
                    let _ = events.send(UiEvent::Message(w));
                }
                for note in keys_migration_notes(&store.settings.current_keys) {
                    let _ = events.send(UiEvent::Message(note));
                }
                if first {
                    let (p, k) = (
                        store.settings.current_points.clone(),
                        store.settings.current_keys.clone(),
                    );
                    for kind in [FileKind::Points, FileKind::Keys] {
                        for path in store.list(kind).map_err(|e| e.to_string())? {
                            if let Err(e) = store.open(&path) {
                                let _ = events.send(UiEvent::Message(e.to_string()));
                            }
                        }
                    }
                    store.open(&p).map_err(|e| e.to_string())?;
                    store.open(&k).map_err(|e| e.to_string())?;
                }
                let initial = snapshot(&store)?;
                Ok((store, initial))
            })();
            let (mut store, initial) = match initialized {
                Ok(v) => v,
                Err(e) => {
                    let _ = events.send(UiEvent::Message(format!("問題：{e}")));
                    return;
                }
            };
            let mut version = 1u32;
            #[cfg(windows)]
            let supervisor = if !preview {
                let clock = match Clock::new() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = events.send(UiEvent::Message(e.to_string()));
                        return;
                    }
                };
                let cfg = match compile_with_params(
                    &initial.points,
                    &initial.keys,
                    &initial.settings.hand,
                    clock.rate,
                    initial.settings.engine,
                    ConfigVersion(version),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = events.send(UiEvent::Message(format!("{e:?}")));
                        return;
                    }
                };
                let options = SupervisorOptions {
                    cfg: std::sync::Arc::new(cfg),
                    engine: baauto_win_platform::engine_path().unwrap_or_default(),
                    target: target.clone(),
                    stop_hotkey: initial.keys.presets["global_stop"].hotkey.unwrap(),
                    record_hotkey: initial.keys.presets["record"]
                        .enabled
                        .then_some(initial.keys.presets["record"].hotkey)
                        .flatten(),
                    health_timeout_ms: 1000,
                    recovery_timeout_ms: 1000,
                    logs: root.join("logs"),
                    injection: None,
                    mode: baauto_core::RunMode::Normal,
                };
                let (status_tx, status_rx) = mpsc::channel();
                let observe = events.clone();
                let target_name = target.clone();
                thread::spawn(move || {
                    let mut state = "啟動中".to_owned();
                    let mut records = 0;
                    let mut hotkey_warning = None;
                    let mut can_arm = true;
                    let mut cached = None;
                    let mut generation = 0;
                    let mut alive = true;
                    loop {
                        loop {
                            let s: SupervisorStatus = match status_rx.try_recv() {
                                Ok(s) => s,
                                Err(mpsc::TryRecvError::Empty) => break,
                                Err(mpsc::TryRecvError::Disconnected) => {
                                    state = "監督已結束，請重開程式".into();
                                    alive = false;
                                    let _ = observe.send(UiEvent::Wanted(false, generation));
                                    break;
                                }
                            };
                            generation = s.resume_generation;
                            state = match s.state {
                                baauto_protocol::supervisor::State::Disarmed => "已停止",
                                baauto_protocol::supervisor::State::Armed(_) => "運作中",
                                baauto_protocol::supervisor::State::Faulted => "故障，請檢查紀錄",
                                _ => "停止／套用中",
                            }
                            .into();
                            let _ = observe.send(UiEvent::Wanted(s.wanted, s.resume_generation));
                            records = s.records;
                            hotkey_warning = s.hotkey_warning;
                            can_arm = s.stop_hotkey_registered;
                            if !s.detail.is_empty() {
                                let _ = observe.send(UiEvent::Message(s.detail));
                            }
                        }
                        let target = target_for_foreground(&target_name).ok().flatten();
                        if target.is_some() {
                            cached = target;
                        }
                        let cursor = cursor_screen().ok();
                        let obs = Observation {
                            state: state.clone(),
                            foreground: target.is_some(),
                            monitor: target.map(|t| {
                                [
                                    t.monitor.left,
                                    t.monitor.top,
                                    t.monitor.width,
                                    t.monitor.height,
                                ]
                            }),
                            cursor: cached.and_then(|t| cursor.map(|p| t.monitor.norm(p[0], p[1]))),
                            warning: target.is_some_and(|t| t.warning()),
                            records,
                            hotkey_warning: hotkey_warning.clone(),
                            can_arm,
                        };
                        if observe.send(UiEvent::Observe(obs)).is_err() {
                            break;
                        }
                        if !alive {
                            break;
                        }
                        thread::sleep(Duration::from_millis(67));
                    }
                });
                let error_events = events.clone();
                Some(thread::spawn(move || {
                    if let Err(e) = run_supervisor(options, native_rx, status_tx) {
                        let _ = error_events.send(UiEvent::Message(format!("問題：{e}")));
                    }
                }))
            } else {
                None
            };
            let _ = events.send(UiEvent::Loaded(Box::new(initial), 3));
            let mut fingerprints = std::collections::BTreeMap::new();
            for p in store
                .settings
                .point_tabs
                .iter()
                .chain(&store.settings.key_tabs)
            {
                if let Ok(f) = fingerprint(p) {
                    fingerprints.insert(p.clone(), f);
                }
            }
            #[cfg(windows)]
            let _telemetry = control.tx.as_ref().map(|tx| {
                crate::telemetry::Telemetry::start(tx.clone(), events.clone(), root.join("logs"))
            });
            while let Ok(job) = rx.recv() {
                let (job, inherited) = match job {
                    Job::Guarded(job, generation) => (*job, Some(generation)),
                    other => (other, None),
                };
                if matches!(job, Job::Quit) {
                    control.quit();
                    #[cfg(windows)]
                    if let Some(join) = supervisor {
                        let _ = join.join();
                    }
                    #[cfg(windows)]
                    drop(_telemetry);
                    let _ = events.send(UiEvent::Quit);
                    return;
                }
                let notify_finished = !matches!(
                    &job,
                    Job::PlaceOverlay(..)
                        | Job::Preferences(_)
                        | Job::Reports
                        | Job::OpenExternal(_)
                        | Job::Screenshot { .. }
                );
                let mut saved_paths = Vec::new();
                let result = (|| -> Result<(), String> {
                    let mut changed = 0;
                    let resume;
                    let generation;
                    let mut reload = false;
                    match job {
                        Job::DialogOpen | Job::DialogImport => {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .pick_file()
                            {
                                if matches!(job, Job::DialogOpen) {
                                    match read_document(&p) {
                                        Ok(d) => {
                                            for note in keys_migration_notes(&p) {
                                                let _ = events.send(UiEvent::Message(note));
                                            }
                                            let _ = events.send(UiEvent::OpenReady(p, d.kind()));
                                        }
                                        Err(e) => {
                                            let _ =
                                                events.send(UiEvent::NeedImport(p, e.to_string()));
                                        }
                                    }
                                } else {
                                    let _ = events.send(UiEvent::NeedImport(p, "import".into()));
                                }
                            }
                            return Ok(());
                        }
                        Job::PlaceOverlay(title, rect) => {
                            #[cfg(windows)]
                            place_owned_overlay(&title, rect).map_err(|e| e.to_string())?;
                            let _ = (title, rect);
                            return Ok(());
                        }
                        Job::Inspect(p) => {
                            match read_document(&p) {
                                Ok(d) => {
                                    for note in keys_migration_notes(&p) {
                                        let _ = events.send(UiEvent::Message(note));
                                    }
                                    let _ = events.send(UiEvent::OpenReady(p, d.kind()));
                                }
                                Err(e) => {
                                    let _ = events.send(UiEvent::NeedImport(p, e.to_string()));
                                }
                            }
                            return Ok(());
                        }
                        Job::Screenshot {
                            path,
                            width,
                            height,
                            rgba,
                        } => {
                            if let Some(parent) = path.parent() {
                                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                            }
                            image::save_buffer_with_format(
                                path,
                                &rgba,
                                width,
                                height,
                                image::ColorType::Rgba8,
                                image::ImageFormat::Png,
                            )
                            .map_err(|e| e.to_string())?;
                            return Ok(());
                        }
                        Job::Reports => {
                            let mut paths = Vec::new();
                            if let Ok(entries) = std::fs::read_dir(root.join("logs")) {
                                for e in entries.flatten() {
                                    if e.path().is_file() {
                                        paths.push(e.path());
                                    }
                                }
                            }
                            paths.sort();
                            let _ = events.send(UiEvent::Reports(paths));
                            return Ok(());
                        }
                        Job::LoadTiming => {
                            if let Some(p) = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .pick_file()
                            {
                                let text =
                                    decode_json(&std::fs::read(p).map_err(|e| e.to_string())?)
                                        .map_err(|e| e.to_string())?;
                                let timing = parse_keys(&text)
                                    .map(|(k, _)| k.timing)
                                    .or_else(|_| from_json::<TimingProfile>(&text))
                                    .map_err(|e| e.to_string())?;
                                let _ = events.send(UiEvent::Timing(Box::new(timing)));
                            }
                            return Ok(());
                        }
                        Job::PrepareOverlay => {
                            let generation = inherited.unwrap_or(control.quiesce()?);
                            let _ = generation;
                            #[cfg(windows)]
                            if let Some(tx) = &control.tx {
                                let (reply, rx) = mpsc::channel();
                                tx.send(SupervisorCommand::ViewTarget { reply })
                                    .map_err(|e| e.to_string())?;
                                let monitor = rx
                                    .recv_timeout(Duration::from_secs(2))
                                    .map_err(|e| e.to_string())??;
                                let _ = events.send(UiEvent::Overlay(monitor, generation));
                                return Ok(());
                            }
                            return Err("預覽模式沒有遊戲覆蓋層".into());
                        }
                        Job::OpenExternal(p) => {
                            open_external(p)?;
                            return Ok(());
                        }
                        Job::Preferences(ui) => {
                            store.settings.ui = ui;
                            store.persist_settings().map_err(|e| e.to_string())?;
                            return Ok(());
                        }
                        Job::Save {
                            points,
                            keys,
                            hand,
                            expected_generation,
                            resume: r,
                        } => {
                            let token = control.quiesce()?;
                            generation = expected_generation.or(inherited).unwrap_or(token);
                            resume = r;
                            if let Some(p) = &points {
                                Document::Points(p.clone())
                                    .validate()
                                    .map_err(|e| e.to_string())?;
                            }
                            if let Some(k) = &keys {
                                Document::Keys(k.clone())
                                    .validate()
                                    .map_err(|e| e.to_string())?;
                            }
                            if let Some(p) = points {
                                changed |= 1;
                                let path = &store.settings.current_points;
                                store
                                    .save(
                                        path,
                                        &Document::Points(p),
                                        fingerprints.get(path).copied(),
                                        false,
                                    )
                                    .map_err(|e| e.to_string())?;
                                fingerprints.insert(
                                    path.clone(),
                                    fingerprint(path).map_err(|e| e.to_string())?,
                                );
                                saved_paths.push(path.clone());
                            }
                            if let Some(k) = keys {
                                changed |= 2;
                                let path = &store.settings.current_keys;
                                store
                                    .save(
                                        path,
                                        &Document::Keys(k),
                                        fingerprints.get(path).copied(),
                                        false,
                                    )
                                    .map_err(|e| e.to_string())?;
                                fingerprints.insert(
                                    path.clone(),
                                    fingerprint(path).map_err(|e| e.to_string())?,
                                );
                                saved_paths.push(path.clone());
                            }
                            if let Some(h) = hand {
                                store.settings.hand = HandEditor::new(h).document;
                                store.persist_settings().map_err(|e| e.to_string())?;
                            }
                            reload = true;
                        }
                        Job::Reassign(old, new, r) => {
                            generation = inherited.unwrap_or(control.quiesce()?);
                            resume = r;
                            let expected = if store.settings.point_tabs.contains(&old) {
                                FileKind::Points
                            } else {
                                FileKind::Keys
                            };
                            let doc = read_document(&new).map_err(|e| e.to_string())?;
                            if doc.kind() != expected {
                                return Err("重新指定必須選擇同一種類的 JSON".into());
                            }
                            store.open(&new).map_err(|e| e.to_string())?;
                            if old != new {
                                store.close(old).map_err(|e| e.to_string())?;
                            }
                            changed = kind_mask(expected);
                            reload = true;
                        }
                        Job::PlanImport {
                            path,
                            mode,
                            resolution,
                        } => {
                            let text =
                                decode_json(&std::fs::read(&path).map_err(|e| e.to_string())?)
                                    .map_err(|e| e.to_string())?;
                            let v = from_json(&text).map_err(|e| e.to_string())?;
                            let imported = legacy::import(&v, "匯入", mode, resolution)
                                .map_err(|e| e.to_string())?;
                            let kind = if imported.keys.is_some() {
                                FileKind::Keys
                            } else {
                                FileKind::Points
                            };
                            let _ = events.send(UiEvent::ImportReady(path, mode, resolution, kind));
                            return Ok(());
                        }
                        Job::Open(ref p, r) | Job::OpenAs(ref p, _, r) => {
                            if let Job::OpenAs(_, expected, _) = &job
                                && read_document(p).map_err(|e| e.to_string())?.kind() != *expected
                            {
                                return Err("檔案內容種類已改變，請重新開啟".into());
                            }
                            let p = p.clone();
                            generation = inherited.unwrap_or(control.quiesce()?);
                            resume = r;
                            match store.open(&p) {
                                Ok(doc) => {
                                    changed = kind_mask(doc.kind());
                                    reload = true;
                                }
                                Err(e) => {
                                    let _ = events.send(UiEvent::NeedImport(p, e.to_string()));
                                    return Ok(());
                                }
                            }
                        }
                        Job::Reload(kind, r) => {
                            generation = inherited.unwrap_or(control.quiesce()?);
                            resume = r;
                            let path = if kind == FileKind::Points {
                                store.settings.current_points.clone()
                            } else {
                                store.settings.current_keys.clone()
                            };
                            store.open(&path).map_err(|e| e.to_string())?;
                            changed = kind_mask(kind);
                            reload = true;
                        }
                        Job::New(_, r) | Job::SaveAs(_, r) => {
                            generation = inherited.unwrap_or(control.quiesce()?);
                            resume = r;
                            let doc = match &job {
                                Job::SaveAs(d, _) => d.clone(),
                                _ => {
                                    if kind_from_job(&job) == FileKind::Points {
                                        Document::Points(template_points())
                                    } else {
                                        Document::Keys(Box::new(template_keys()))
                                    }
                                }
                            };
                            let path = if doc.kind() == FileKind::Points {
                                &store.settings.current_points
                            } else {
                                &store.settings.current_keys
                            };
                            let selected = rfd::FileDialog::new()
                                .add_filter("JSON", &["json"])
                                .set_directory(path.parent().unwrap_or(&root))
                                .set_file_name(if matches!(job, Job::New(..)) {
                                    "新增.json"
                                } else {
                                    "另存.json"
                                })
                                .save_file();
                            if let Some(path) = selected {
                                let path = store.save_as(&path, &doc).map_err(|e| e.to_string())?;
                                let d = store.open(&path).map_err(|e| e.to_string())?;
                                changed = kind_mask(d.kind());
                                reload = true;
                            } else if resume {
                                control.resume(generation);
                            }
                        }
                        Job::Close(p, trash, r) => {
                            generation = inherited.unwrap_or(control.quiesce()?);
                            resume = r;
                            let kind = if store.settings.point_tabs.contains(&p) {
                                FileKind::Points
                            } else {
                                FileKind::Keys
                            };
                            if trash {
                                store.trash(&p).map_err(|e| e.to_string())?;
                            } else {
                                store.close(p).map_err(|e| e.to_string())?;
                            }
                            changed = kind_mask(kind);
                            reload = true;
                        }
                        Job::Import {
                            expected_kind,
                            path,
                            mode,
                            resolution,
                            resume: r,
                        } => {
                            generation = inherited.unwrap_or(control.quiesce()?);
                            resume = r;
                            let text =
                                decode_json(&std::fs::read(&path).map_err(|e| e.to_string())?)
                                    .map_err(|e| e.to_string())?;
                            let value = from_json(&text).map_err(|e| e.to_string())?;
                            let imported = legacy::import(
                                &value,
                                path.file_stem().and_then(|s| s.to_str()).unwrap_or("匯入"),
                                mode,
                                resolution,
                            )
                            .map_err(|e| e.to_string())?;
                            if (imported.keys.is_some() && expected_kind != FileKind::Keys)
                                || (imported.points.is_some() && expected_kind != FileKind::Points)
                            {
                                return Err("匯入來源種類已改變，請重新匯入".into());
                            }
                            let report = to_json(&imported.report).map_err(|e| e.to_string())?;
                            let report_path = store
                                .save_import_report(&imported.report)
                                .map_err(|e| e.to_string())?;
                            let _ = events.send(UiEvent::Message(format!(
                                "匯入報告：{}",
                                report_path.display()
                            )));
                            if !imported.candidates.is_empty() {
                                let _ =
                                    events.send(UiEvent::Candidates(imported.candidates, report));
                                if resume {
                                    control.resume(generation);
                                }
                                return Ok(());
                            }
                            let (p, _) = store
                                .import_legacy(&path, mode, resolution)
                                .map_err(|e| e.to_string())?;
                            let doc = store.open(&p).map_err(|e| e.to_string())?;
                            changed = kind_mask(doc.kind());
                            reload = true;
                            let _ = events.send(UiEvent::Message(report));
                        }
                        Job::Quit | Job::Guarded(..) => unreachable!(),
                    }
                    if reload {
                        let snapshot = snapshot(&store)?;
                        version = version.checked_add(1).ok_or("設定版本耗盡")?;
                        control.apply(&snapshot, version, resume, generation)?;
                        for p in store
                            .settings
                            .point_tabs
                            .iter()
                            .chain(&store.settings.key_tabs)
                        {
                            if let Ok(f) = fingerprint(p) {
                                fingerprints.insert(p.clone(), f);
                            }
                        }
                        let _ = events.send(UiEvent::Loaded(Box::new(snapshot), changed));
                    }
                    Ok(())
                })();
                let success = result.is_ok();
                if let Err(e) = result {
                    let e = if saved_paths.is_empty() {
                        e
                    } else {
                        format!(
                            "{e}；已儲存：{}；保持停止，修正後再儲存",
                            saved_paths
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect::<Vec<_>>()
                                .join("、")
                        )
                    };
                    let _ = events.send(UiEvent::Message(format!("問題：{e}")));
                }
                if notify_finished {
                    let _ = events.send(UiEvent::Finished(success));
                }
            }
            control.quit();
            #[cfg(windows)]
            if let Some(join) = supervisor {
                let _ = join.join();
            }
        });
        Self {
            jobs,
            events: out,
            controller,
        }
    }
}
fn kind_from_job(job: &Job) -> FileKind {
    match job {
        Job::New(kind, _) => *kind,
        Job::SaveAs(d, _) => d.kind(),
        _ => FileKind::Points,
    }
}
fn open_external(path: PathBuf) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = std::process::Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    command.arg(path).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn backend_fs() -> BackendFs {
    #[cfg(windows)]
    {
        WindowsFs
    }
    #[cfg(not(windows))]
    {
        PortableFs
    }
}
fn kind_mask(kind: FileKind) -> u8 {
    if kind == FileKind::Points { 1 } else { 2 }
}
