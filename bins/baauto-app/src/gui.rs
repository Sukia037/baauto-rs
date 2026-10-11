//! GUI edits memory through config::editor and sends commands; no injection or synchronous file/IPC work.
use crate::{backend::*, state::*};
use baauto_config::{editor::*, store::*, *};
use baauto_protocol::{capture::*, ipc::EngineToApp};
use eframe::egui::{self, Color32, RichText, Vec2};
use std::{path::PathBuf, time::Duration};
#[derive(Clone, Debug, PartialEq)]
enum Selection {
    Candidate(usize),
    Item(u32),
    Preset(String),
    Timing(usize),
    Log(usize),
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum CaptureTarget {
    Point,
    ReRecord(u32),
    ConvertToPoint(u32),
    Item(u32),
    Preset(String),
    Slot(usize),
}
struct Capture {
    id: u64,
    target: CaptureTarget,
    ready: bool,
}
struct Pending {
    kind: FileKind,
    job: Job,
    closing: bool,
}
struct Overlay {
    monitor: [i32; 4],
    rect: HandRect,
    side: CardUiSide,
    deadline: UiDeadline,
    dragging: bool,
    generation: u64,
    centers: Vec<NormPoint>,
    centers_mode: bool,
}
pub struct App {
    backend: Backend,
    settings: Option<Settings>,
    points: PointSetEditor,
    keys: KeysEditor,
    hand: HandEditor,
    saved_points: PointSet,
    saved_keys: KeysProfile,
    timing_draft: TimingProfile,
    timing_error: Option<String>,
    logs_dir: PathBuf,
    tab: u8,
    selection: Selection,
    candidates: Vec<legacy::Candidate>,
    logs: Vec<String>,
    reports: Vec<PathBuf>,
    missing: Vec<PathBuf>,
    busy: bool,
    wanted: bool,
    resume_generation: u64,
    preview: bool,
    message: String,
    notice: Option<UiDeadline>,
    observation: Observation,
    monitor: Option<[i32; 4]>,
    records: u64,
    capture: Option<Capture>,
    next_capture: u64,
    pending: Option<Pending>,
    next_job: Option<Job>,
    close_prompt: Option<(PathBuf, FileKind)>,
    reassign: Option<PathBuf>,
    import: Option<(PathBuf, String)>,
    import_mode: Mode,
    import_resolution: [f64; 2],
    quitting: bool,
    allow_close: bool,
    overlay: Option<Overlay>,
    overlay_resume: bool,
    overlay_generation: Option<u64>,
    qa: Option<Qa>,
    frames: u64,
}
struct Qa {
    dir: PathBuf,
    dark: bool,
    scale: f32,
    tab: u8,
    requested: bool,
}
impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        backend: Backend,
        preview: bool,
        qa: Option<(PathBuf, bool, f32, u8)>,
    ) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        cc.egui_ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = Vec2::new(8., 8.);
            s.spacing.button_padding = Vec2::new(10., 5.);
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.));
            s.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.));
            s.visuals.selection.bg_fill = Color32::from_rgb(43, 106, 218);
            s.visuals.selection.stroke = egui::Stroke::new(1., Color32::WHITE);
        });
        if let Some((_, dark, scale, _)) = &qa {
            cc.egui_ctx.set_theme(if *dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
            cc.egui_ctx.set_zoom_factor(*scale);
        }
        let p = template_points();
        let k = template_keys();
        Self {
            backend,
            settings: None,
            points: PointSetEditor::new(p.clone()),
            keys: KeysEditor::new(k.clone()),
            hand: HandEditor::new(HandSettings::default()),
            saved_points: p,
            timing_draft: k.timing.clone(),
            timing_error: None,
            logs_dir: PathBuf::new(),
            saved_keys: k,
            tab: qa.as_ref().map_or(0, |v| v.3),
            selection: Selection::Item(1),
            candidates: Vec::new(),
            logs: Vec::new(),
            reports: Vec::new(),
            missing: Vec::new(),
            busy: false,
            wanted: false,
            resume_generation: 0,
            preview,
            message: if preview {
                "離線預覽：遊戲輸入僅支援 Windows".into()
            } else {
                "啟動中".into()
            },
            notice: None,
            observation: Observation {
                state: "已停止".into(),
                foreground: false,
                monitor: None,
                cursor: None,
                warning: false,
                records: 0,
            },
            monitor: None,
            records: 0,
            capture: None,
            next_capture: 1,
            pending: None,
            next_job: None,
            close_prompt: None,
            reassign: None,
            import: None,
            import_mode: Mode::Six,
            import_resolution: [0., 0.],
            quitting: false,
            allow_close: false,
            overlay: None,
            overlay_resume: false,
            overlay_generation: None,
            qa: qa.map(|(dir, dark, scale, tab)| Qa {
                dir,
                dark,
                scale,
                tab,
                requested: false,
            }),
            frames: 0,
        }
    }
    fn select_tab(&mut self) {
        self.selection = match self.tab {
            0 => Selection::Item(self.points.document.items.first().map_or(0, |p| p.id)),
            1 => Selection::Preset("skill_1".into()),
            2 => Selection::Timing(0),
            _ => Selection::Log(0),
        };
    }
    fn points_dirty(&self) -> bool {
        self.points.document != self.saved_points
    }
    fn keys_dirty(&self) -> bool {
        self.keys.document != self.saved_keys || self.timing_error.is_some()
    }
    fn log(&mut self, text: String) {
        #[cfg(windows)]
        let text = format!(
            "QPC {}  {}",
            baauto_win_platform::Clock::new().expect("QPC").now().0,
            text
        );
        self.message = text.clone();
        self.notice = Some(UiDeadline::new(5));
        if self.logs.len() >= 1000 {
            self.logs.remove(0);
        }
        self.logs.push(text);
    }
    fn log_error(&mut self, text: String) {
        self.log(format!("問題：{text}"));
    }
    fn job(&mut self, job: Job) {
        if self.busy {
            return;
        }
        let job = match job {
            Job::Guarded(..) => job,
            _ => Job::Guarded(Box::new(job), self.resume_generation),
        };
        self.busy = true;
        self.backend.controller.cancel_capture();
        self.capture = None;
        let _ = self.backend.jobs.send(job);
    }
    fn begin(&mut self, kind: FileKind, job: Job) {
        if self.busy {
            return;
        }
        let job = Job::Guarded(Box::new(job), self.resume_generation);
        if (kind == FileKind::Points && self.points_dirty())
            || (kind == FileKind::Keys && self.keys_dirty())
        {
            self.pending = Some(Pending {
                kind,
                job,
                closing: false,
            });
        } else {
            self.job(job);
        }
    }
    fn save(&mut self, kind: Option<FileKind>) {
        let points = (self.points_dirty() && kind != Some(FileKind::Keys))
            .then(|| self.points.document.clone());
        let keys = (self.keys_dirty() && kind != Some(FileKind::Points))
            .then(|| Box::new(self.keys.document.clone()));
        self.job(Job::Save {
            points,
            keys,
            hand: None,
            expected_generation: None,
            resume: self.wanted && self.next_job.is_none(),
        });
    }
    fn store_hand(&mut self) {
        let expected_generation = self.overlay_generation.take();
        let resume = if expected_generation.is_some() {
            self.overlay_resume
        } else {
            self.wanted
        };
        self.job(Job::Save {
            points: None,
            keys: None,
            hand: Some(self.hand.document.clone()),
            expected_generation,
            resume,
        });
    }
    fn capture(&mut self, target: CaptureTarget) {
        if self.busy {
            return;
        }
        self.backend.controller.cancel_capture();
        let id = self.next_capture;
        self.next_capture = self.next_capture.saturating_add(1);
        let kind = match target {
            CaptureTarget::Point
            | CaptureTarget::ReRecord(_)
            | CaptureTarget::ConvertToPoint(_) => CaptureKind::Point,
            CaptureTarget::Slot(_) => CaptureKind::SlotKey,
            _ => CaptureKind::Hotkey,
        };
        self.message = if kind == CaptureKind::Point {
            "準備錄點…請等候停止與 Hook 就緒"
        } else {
            "按下按鍵… Esc 取消，Backspace 清除（15秒）"
        }
        .into();
        self.wanted = false;
        self.capture = Some(Capture {
            id,
            target,
            ready: false,
        });
        self.backend
            .controller
            .capture(kind, id, self.resume_generation);
    }
    fn poll(&mut self, ctx: &egui::Context) {
        for _ in 0..128 {
            let Ok(event) = self.backend.events.try_recv() else {
                break;
            };
            match event {
                UiEvent::Loaded(s, mask) => {
                    let initial = self.settings.is_none();
                    if mask & 1 != 0 {
                        self.saved_points = s.points.clone();
                        self.points = PointSetEditor::new(s.points);
                        if self.tab == 0 {
                            self.select_tab();
                        }
                    }
                    if mask & 2 != 0 {
                        self.saved_keys = s.keys.clone();
                        self.keys = KeysEditor::new(s.keys);
                        self.timing_draft = self.keys.document.timing.clone();
                        self.timing_error = None;
                    }
                    self.hand = HandEditor::new(s.settings.hand.clone());
                    self.missing = s.missing;
                    if initial && self.qa.is_none() {
                        self.tab = s.settings.ui.selected_tab;
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::from(
                            s.settings.ui.window_size,
                        )));
                        if let Some(dark) = s.settings.ui.theme {
                            ctx.set_theme(if dark {
                                egui::ThemePreference::Dark
                            } else {
                                egui::ThemePreference::Light
                            });
                        }
                    }
                    self.logs_dir = s.logs_dir;
                    let mut settings = s.settings;
                    if !initial && let Some(previous) = &self.settings {
                        settings.ui = previous.ui.clone();
                    }
                    self.settings = Some(settings);
                    if initial {
                        self.select_tab();
                    }
                    self.busy = false;
                    self.log(
                        if self.preview {
                            "離線預覽已載入範本；遊戲輸入未啟用"
                        } else {
                            "已載入並送出套用；重新啟用會等待按鍵放開"
                        }
                        .into(),
                    );
                }
                UiEvent::Message(m) => self.log(m),
                UiEvent::Candidates(c, report) => {
                    self.candidates.extend(c);
                    self.log(report);
                }
                UiEvent::Reports(paths) => self.reports = paths,
                UiEvent::Timing(timing) => match self.keys.install_calibrated_timing(*timing) {
                    Ok(()) => {
                        self.timing_draft = self.keys.document.timing.clone();
                        self.timing_error = None;
                        self.log("校準時間已載入，請儲存".into());
                    }
                    Err(e) => self.log_error(e.to_string()),
                },
                UiEvent::Wanted(wanted, generation) => {
                    self.wanted = wanted;
                    self.resume_generation = generation;
                }
                UiEvent::Observe(o) => {
                    if o.monitor.is_some() {
                        self.monitor = o.monitor;
                    }
                    if o.records > self.records && !self.busy {
                        self.records = o.records;
                        self.capture(CaptureTarget::Point);
                    }
                    self.observation = o;
                }
                UiEvent::OpenReady(path, kind) => {
                    if let Some(old) = self.reassign.take() {
                        self.begin(kind, Job::Reassign(old, path, self.wanted));
                    } else {
                        self.begin(kind, Job::OpenAs(path, kind, self.wanted));
                    }
                }
                UiEvent::ImportReady(path, mode, resolution, kind) => {
                    self.begin(
                        kind,
                        Job::Import {
                            path,
                            mode,
                            resolution,
                            expected_kind: kind,
                            resume: self.wanted,
                        },
                    );
                }
                UiEvent::NeedImport(path, reason) => {
                    self.import = Some((path, reason));
                }
                UiEvent::Finished(ok) => {
                    self.busy = false;
                    if ok {
                        if let Some(job) = self.next_job.take() {
                            self.job(job);
                        }
                    } else {
                        self.next_job = None;
                        self.wanted = false;
                        if let Some(settings) = &self.settings {
                            self.hand = HandEditor::new(settings.hand.clone());
                        }
                    }
                }
                UiEvent::Capture(id, result) => {
                    if self.capture.as_ref().is_some_and(|c| c.id == id) {
                        self.captured(result);
                    }
                }
                UiEvent::CaptureReady(id) => {
                    if let Some(c) = self.capture.as_mut().filter(|c| c.id == id) {
                        c.ready = true;
                        self.message =
                            "Hook 已就緒：切回遊戲點一下，或按下要擷取的熱鍵；Esc 取消".into();
                    }
                }
                UiEvent::Overlay(monitor, generation) => {
                    let r = self.hand.document.layout(self.points.document.mode);
                    self.overlay = Some(Overlay {
                        monitor,
                        generation,
                        rect: r,
                        side: self.hand.document.card_ui_side,
                        deadline: UiDeadline::new(60),
                        dragging: false,
                        centers: Vec::new(),
                        centers_mode: false,
                    });
                    self.message = "調整手牌框，確認儲存；Esc 取消（60秒）".into();
                }
                UiEvent::Quit => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if self.overlay.as_ref().is_some_and(|o| o.deadline.expired()) {
            self.finish_overlay(false);
            self.log("手牌校準逾時".into());
        }
        ctx.request_repaint_after(Duration::from_millis(67));
    }
    fn captured(&mut self, result: Result<EngineToApp, String>) {
        let Some(capture) = self.capture.take() else {
            return;
        };
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                self.log_error(e);
                return;
            }
        };
        let outcome = match result {
            EngineToApp::PointRecorded {
                request_id,
                screen_pos,
                monitor,
                ..
            } if request_id == capture.id => {
                let p = NormPoint {
                    x: (f64::from(screen_pos[0]) - f64::from(monitor[0])) / f64::from(monitor[2]),
                    y: (f64::from(screen_pos[1]) - f64::from(monitor[1])) / f64::from(monitor[3]),
                };
                CaptureOutcome::Point(p)
            }
            EngineToApp::CapturedInput { result, .. } if result.id == capture.id => result.outcome,
            _ => {
                self.capture = Some(capture);
                return;
            }
        };
        let edited = (|| -> Result<(), ConfigError> {
            match outcome {
                CaptureOutcome::Point(position) => {
                    if let CaptureTarget::ReRecord(id) | CaptureTarget::ConvertToPoint(id) =
                        capture.target
                    {
                        self.points.update_item(
                            id,
                            ItemPatch {
                                position: Some(position),
                                function: matches!(
                                    capture.target,
                                    CaptureTarget::ConvertToPoint(_)
                                )
                                .then_some(PointFunction::Point),
                                ..Default::default()
                            },
                        )?;
                    } else {
                        self.candidates.push(legacy::Candidate {
                            id: self.next_capture as u32,
                            name: format!("候選 {}", self.candidates.len() + 1),
                            position,
                        });
                    }
                }
                CaptureOutcome::Chord(chord) => match capture.target {
                    CaptureTarget::Item(id) => {
                        self.points.update_item(
                            id,
                            ItemPatch {
                                hotkey: Some(Some(chord)),
                                ..Default::default()
                            },
                        )?;
                    }
                    CaptureTarget::Preset(role) => {
                        let enabled = self.keys.document.presets[&role].enabled;
                        self.keys.set_preset(&role, Some(chord), enabled, false)?;
                    }
                    CaptureTarget::Slot(slot) => {
                        if match chord.mods {
                            ModsMatch::AtLeast(m) | ModsMatch::Exact(m) => m != 0,
                        } {
                            return Err(ConfigError::new("手牌按鍵", "每格只收單鍵，請勿按修飾鍵"));
                        }
                        let mut keys = self.hand.document.slot_keys;
                        keys[slot] = chord.key;
                        self.hand.set_slot_keys(keys)?;
                        self.store_hand();
                    }
                    _ => {}
                },
                CaptureOutcome::Cleared => match capture.target {
                    CaptureTarget::Item(id) => {
                        self.points.update_item(
                            id,
                            ItemPatch {
                                hotkey: Some(None),
                                ..Default::default()
                            },
                        )?;
                    }
                    CaptureTarget::Preset(role) => {
                        let p = &self.keys.document.presets[&role];
                        self.keys.set_preset(&role, None, p.enabled, false)?;
                    }
                    _ => {}
                },
                CaptureOutcome::RejectedLeft => {
                    return Err(ConfigError::new("熱鍵", "左鍵不能當熱鍵，請按其他鍵"));
                }
                CaptureOutcome::InvalidSlot => {
                    return Err(ConfigError::new("手牌按鍵", "每格按鍵不能清除或使用滑鼠鍵"));
                }
                CaptureOutcome::Cancelled
                | CaptureOutcome::TimedOut
                | CaptureOutcome::EmergencyStop => {}
                CaptureOutcome::NotForeground => {
                    return Err(ConfigError::new(
                        "錄點",
                        "此點擊已攔下；請先切回遊戲，再重新錄點",
                    ));
                }
            }
            Ok(())
        })();
        match edited {
            Ok(()) => self.log(match outcome {
                CaptureOutcome::Point(p) => format!("已錄製 ({:.3}, {:.3})", p.x, p.y),
                CaptureOutcome::Chord(c) => format!("已設定熱鍵 {c}"),
                CaptureOutcome::Cleared => "熱鍵已清除".into(),
                CaptureOutcome::EmergencyStop => "緊急停止：擷取已取消，保持停止".into(),
                CaptureOutcome::TimedOut => "擷取逾時，請重新開始".into(),
                _ => "擷取已取消".into(),
            }),
            Err(e) => self.log_error(e.to_string()),
        }
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("BAauto").size(20.).strong());
            if self.preview {
                ui.label("離線預覽");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        self.wanted
                            || (!self.busy && self.capture.is_none() && self.overlay.is_none()),
                        egui::Button::new(if self.wanted { "停止" } else { "啟用" }),
                    )
                    .clicked()
                {
                    self.wanted = !self.wanted;
                    if self.wanted {
                        self.backend.controller.enable();
                    } else {
                        self.backend.controller.disable();
                        self.backend.controller.cancel_capture();
                        self.capture = None;
                    }
                }
                if ui.small_button("緊急停止").clicked() {
                    self.wanted = false;
                    self.overlay_resume = false;
                    self.backend.controller.disable();
                    self.backend.controller.cancel_capture();
                    self.capture = None;
                    if self.overlay.is_some() {
                        self.finish_overlay(false);
                    }
                }
                ui.colored_label(
                    state_color(&self.observation.state),
                    format!("● {}", self.observation.state),
                );
                if ui.small_button("淺／深").clicked() {
                    let dark = !ui.visuals().dark_mode;
                    ui.ctx().set_theme(if dark {
                        egui::ThemePreference::Dark
                    } else {
                        egui::ThemePreference::Light
                    });
                    if let Some(settings) = &mut self.settings {
                        settings.ui.theme = Some(dark);
                        let _ = self
                            .backend
                            .jobs
                            .send(Job::Preferences(settings.ui.clone()));
                    }
                }
            });
        });
        for kind in [FileKind::Points, FileKind::Keys] {
            ui.add_enabled_ui(!self.busy && self.overlay.is_none(), |ui| {
                self.file_row(ui, kind)
            });
        }
        ui.horizontal(|ui| {
            for (tab, name) in ["點位", "預設按鍵", "時間", "紀錄"].iter().enumerate() {
                if ui.selectable_label(self.tab == tab as u8, *name).clicked() {
                    self.tab = tab as u8;
                    self.selection = match self.tab {
                        0 => {
                            Selection::Item(self.points.document.items.first().map_or(0, |p| p.id))
                        }
                        1 => Selection::Preset("skill_1".into()),
                        2 => Selection::Timing(0),
                        _ => Selection::Log(0),
                    };
                    if self.tab == 3 {
                        let _ = self.backend.jobs.send(Job::Reports);
                    }
                    if let Some(settings) = &mut self.settings {
                        settings.ui.selected_tab = self.tab;
                        let _ = self
                            .backend
                            .jobs
                            .send(Job::Preferences(settings.ui.clone()));
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(s) = &self.settings {
                    let p = if self.tab == 0 {
                        &s.current_points
                    } else {
                        &s.current_keys
                    };
                    ui.label(format!(
                        "存在{}：{}",
                        if self.tab == 0 {
                            "點位 JSON"
                        } else {
                            "預設按鍵 JSON"
                        },
                        p.file_name().unwrap_or_default().to_string_lossy()
                    ));
                }
            });
        });
    }
    fn file_row(&mut self, ui: &mut egui::Ui, kind: FileKind) {
        let Some(s) = self.settings.clone() else {
            ui.spinner();
            return;
        };
        let (paths, current, dirty) = if kind == FileKind::Points {
            (s.point_tabs, s.current_points, self.points_dirty())
        } else {
            (s.key_tabs, s.current_keys, self.keys_dirty())
        };
        ui.horizontal(|ui| {
            ui.label(if kind == FileKind::Points {
                "點位"
            } else {
                "預設按鍵"
            });
            egui::ScrollArea::horizontal()
                .id_salt(format!("files-{kind:?}"))
                .max_width(180.)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for path in &paths {
                            let same_name = paths
                                .iter()
                                .filter(|p| p.file_name() == path.file_name())
                                .count()
                                > 1;
                            let name = format!(
                                "{}{}{}{}",
                                path.file_name().unwrap_or_default().to_string_lossy(),
                                if same_name {
                                    format!(
                                        " ({})",
                                        path.parent()
                                            .and_then(|p| p.file_name())
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                    )
                                } else {
                                    String::new()
                                },
                                if *path == current && dirty { "*" } else { "" },
                                if self.missing.contains(path) {
                                    " 找不到檔案"
                                } else {
                                    ""
                                }
                            );
                            let response = ui
                                .selectable_label(*path == current, RichText::new(name).monospace())
                                .on_hover_text(path.display().to_string());
                            if self.missing.contains(path) && ui.small_button("重新指定").clicked()
                            {
                                self.reassign = Some(path.clone());
                                self.job(Job::DialogOpen);
                            }
                            if response.clicked() && *path != current {
                                self.begin(kind, Job::OpenAs(path.clone(), kind, self.wanted));
                            }
                            if *path == current {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                            if ui.small_button("×").clicked() {
                                self.close_prompt = Some((path.clone(), kind));
                            }
                        }
                    });
                });
            if ui.button("新增").clicked() {
                self.begin(kind, Job::New(kind, self.wanted));
            }
            if ui.button("開啟…").clicked() {
                self.job(Job::DialogOpen);
            }
            if kind == FileKind::Points {
                for (mode, label) in [(Mode::Six, "六人"), (Mode::Ten, "十人")] {
                    if ui
                        .selectable_label(self.points.document.mode == mode, label)
                        .clicked()
                    {
                        self.points.set_mode(mode);
                    }
                }
            }
            egui::ScrollArea::horizontal()
                .id_salt(format!("path-{kind:?}"))
                .max_width((ui.available_width() - 150.).max(80.))
                .show(ui, |ui| {
                    ui.label(RichText::new(current.display().to_string()).monospace())
                        .on_hover_text(current.display().to_string());
                });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("重新載入").clicked() {
                    self.begin(kind, Job::Reload(kind, self.wanted));
                }
                if ui
                    .add_enabled(
                        kind == FileKind::Points || self.timing_error.is_none(),
                        egui::Button::new("另存…"),
                    )
                    .clicked()
                {
                    let d = if kind == FileKind::Points {
                        Document::Points(self.points.document.clone())
                    } else {
                        Document::Keys(Box::new(self.keys.document.clone()))
                    };
                    self.job(Job::SaveAs(d, self.wanted));
                }
            });
        });
    }
    fn footer(&mut self, ui: &mut egui::Ui) {
        if self.points_dirty() || self.keys_dirty() {
            ui.horizontal(|ui| {
                let can_save = self.timing_error.is_none()
                    && can_save_keys(&self.points.document, &self.keys.document);
                let text = format!(
                    "{} {} 有未儲存變更",
                    if self.points_dirty() {
                        self.settings
                            .as_ref()
                            .and_then(|s| s.current_points.file_name())
                            .and_then(|s| s.to_str())
                            .unwrap_or("點位 JSON")
                    } else {
                        ""
                    },
                    if self.keys_dirty() {
                        self.settings
                            .as_ref()
                            .and_then(|s| s.current_keys.file_name())
                            .and_then(|s| s.to_str())
                            .unwrap_or("預設按鍵 JSON")
                    } else {
                        ""
                    }
                );
                ui.colored_label(
                    if can_save {
                        ui.visuals().text_color()
                    } else {
                        Color32::from_rgb(200, 65, 65)
                    },
                    text,
                );
                if !can_save {
                    ui.label("預設按鍵重複或時間不合法，請修正");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(can_save && !self.busy, egui::Button::new("儲存"))
                        .clicked()
                    {
                        self.save(None);
                    }
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("還原"))
                        .clicked()
                    {
                        self.points = PointSetEditor::new(self.saved_points.clone());
                        self.keys = KeysEditor::new(self.saved_keys.clone());
                        self.timing_draft = self.saved_keys.timing.clone();
                        self.timing_error = None;
                    }
                });
            });
            ui.separator();
        }
        ui.horizontal(|ui| {
            if self.busy {
                ui.spinner();
                ui.label("停止／處理檔案中");
            } else if self.observation.warning {
                ui.colored_label(
                    Color32::from_rgb(180, 110, 35),
                    "遊戲不是全螢幕 16:9，點位和牌鍵會不準",
                );
            } else if self.capture.is_some()
                || self.overlay.is_some()
                || self.notice.as_ref().is_some_and(|n| !n.expired())
            {
                if self.message.contains("問題：") {
                    ui.colored_label(Color32::from_rgb(200, 65, 65), &self.message);
                } else {
                    ui.label(&self.message);
                }
            } else {
                if ui.small_button("緊急停止").clicked() {
                    self.wanted = false;
                    self.overlay_resume = false;
                    self.backend.controller.disable();
                    self.backend.controller.cancel_capture();
                    self.capture = None;
                    if self.overlay.is_some() {
                        self.finish_overlay(false);
                    }
                }
                ui.colored_label(
                    state_color(&self.observation.state),
                    format!("● {}", self.observation.state),
                );
                ui.label(if self.observation.foreground {
                    "遊戲在前景"
                } else {
                    "等待遊戲前景"
                });
                if let Some(m) = self.monitor {
                    ui.label(format!("{}×{}", m[2], m[3]));
                }
                let hand = self
                    .settings
                    .as_ref()
                    .map_or(&self.hand.document, |s| &s.hand);
                let mode = self.saved_points.mode;
                let slot = self
                    .observation
                    .cursor
                    .filter(|_| self.observation.foreground)
                    .and_then(|p| slot_at(&hand.layout(mode), mode.slots(), hand.slot_order, p));
                ui.label(format!(
                    "游標：{}",
                    slot.map_or("不在手牌".into(), |n| format!("第 {} 格", n + 1))
                ));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(format!(
                    "{} 緊急停止",
                    self.keys.document.presets["global_stop"]
                        .hotkey
                        .map_or("未設定".into(), |c| c.to_string())
                ));
            });
        });
    }
    fn list(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            0 => {
                ui.label(RichText::new("候選點位").strong());
                if ui.button("＋ 錄製").clicked() {
                    self.capture(CaptureTarget::Point);
                }
                if ui.button("匯入舊檔／MuMu…").clicked() {
                    self.job(Job::DialogImport);
                }
                ui.separator();
                let candidates = self.candidates.clone();
                for (i, c) in candidates.iter().enumerate() {
                    if row(
                        ui,
                        self.selection == Selection::Candidate(i),
                        format!("{}\n({:.3}, {:.3})", c.name, c.position.x, c.position.y),
                    )
                    .clicked()
                    {
                        self.selection = Selection::Candidate(i);
                    }
                }
            }
            1 => {
                ui.label("預設功能");
                for (group, name) in [
                    ("skill_", "技能"),
                    ("spam_", "技能連點"),
                    ("card_", "卡幀開牌"),
                    ("", "其他"),
                ] {
                    if row(ui, false, name.into()).clicked() {
                        let role = PRESET_ROLES
                            .iter()
                            .find(|r| {
                                if group.is_empty() {
                                    !r.starts_with("skill_")
                                        && !r.starts_with("spam_")
                                        && !r.starts_with("card_")
                                } else {
                                    r.starts_with(group)
                                }
                            })
                            .unwrap();
                        self.selection = Selection::Preset((*role).into());
                    }
                }
                if ui.button("恢復全部預設").clicked() {
                    self.keys.reset_all();
                }
            }
            2 => {
                ui.label("時間設定");
                ui.label(if self.keys.document.timing.verified {
                    "已驗證"
                } else {
                    "未驗證"
                });
                ui.label(match self.keys.document.timing.source {
                    ProfileSource::ImportedLegacy => "來源：範本／舊版匯入",
                    ProfileSource::Calibrated => "來源：校準",
                    ProfileSource::Manual => "來源：手動",
                });
                let mut repeat = self.timing_draft.auto_repeat == AutoRepeatPolicy::FollowTypematic;
                let mut trailing =
                    self.timing_draft.card_trailing_esc == TrailingEscPolicy::IfStarted;
                let changed = ui.checkbox(&mut repeat, "按住技能鍵跟隨鍵盤重複").changed()
                    | ui.checkbox(&mut trailing, "卡幀動作開始後送結尾 Esc")
                        .changed();
                if changed {
                    let mut timing = self.timing_draft.clone();
                    timing.auto_repeat = if repeat {
                        AutoRepeatPolicy::FollowTypematic
                    } else {
                        AutoRepeatPolicy::Ignore
                    };
                    timing.card_trailing_esc = if trailing {
                        TrailingEscPolicy::IfStarted
                    } else {
                        TrailingEscPolicy::Never
                    };
                    self.timing_draft = timing.clone();
                    self.timing_error = self.keys.set_timing(timing).err().map(|e| e.to_string());
                    if self.timing_error.is_none() {
                        self.timing_draft = self.keys.document.timing.clone();
                    }
                }
                if ui.button("載入校準結果…").clicked() {
                    self.job(Job::LoadTiming);
                }
                if ui.button("還原範本時間").clicked() {
                    if let Err(e) = self.keys.set_timing(template_keys().timing) {
                        self.log(e.to_string());
                    }
                    self.timing_draft = self.keys.document.timing.clone();
                    self.timing_error = None;
                }
            }
            _ => {
                ui.label("紀錄");
                if ui.button("開啟資料夾").clicked() {
                    let _ = self
                        .backend
                        .jobs
                        .send(Job::OpenExternal(self.logs_dir.clone()));
                }
                if ui.button("重新整理報告").clicked() {
                    let _ = self.backend.jobs.send(Job::Reports);
                }
                ui.separator();
                let reports = self.reports.clone();
                for p in reports {
                    if ui
                        .button(p.file_name().unwrap_or_default().to_string_lossy())
                        .on_hover_text(p.display().to_string())
                        .clicked()
                    {
                        let _ = self.backend.jobs.send(Job::OpenExternal(p));
                    }
                }
            }
        }
    }
    fn middle(&mut self, ui: &mut egui::Ui) {
        match self.tab {
            0 => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("點位按鍵清單").strong());
                    if ui.button("＋ 牌鍵").clicked() {
                        match self.points.add_card_item("新牌鍵".into()) {
                            Ok(id) => {
                                self.selection = Selection::Item(id);
                                self.capture(CaptureTarget::Item(id));
                            }
                            Err(e) => self.log_error(e.to_string()),
                        }
                    }
                });
                ui.separator();
                let analysis = analyze(&self.points.document, &self.keys.document);
                for id in sorted_item_ids(&self.points.document, &self.keys.document) {
                    let item = selected_item(&self.points.document, id).unwrap().clone();
                    let conflict = analysis.disabled_points.contains(&id);
                    let status = if conflict {
                        "重複：已停用"
                    } else if item.hotkey.is_none() {
                        "未設定熱鍵"
                    } else {
                        "啟用"
                    };
                    let position = if item.function == PointFunction::Card {
                        "手牌".into()
                    } else {
                        self.pixel_label(item.x.unwrap_or(0.), item.y.unwrap_or(0.))
                    };
                    ui.horizontal(|ui| {
                        let text = format!(
                            "{} · {}\n{} · {}",
                            item.name,
                            if item.function == PointFunction::Card {
                                "牌鍵"
                            } else {
                                "普通點位"
                            },
                            position,
                            status
                        );
                        let response = ui.add_sized(
                            [ui.available_width() - 90., 34.],
                            egui::Button::new(if conflict {
                                RichText::new(text).color(Color32::from_rgb(200, 65, 65))
                            } else {
                                RichText::new(text)
                            })
                            .selected(self.selection == Selection::Item(id)),
                        );
                        if response.clicked() {
                            self.selection = Selection::Item(id);
                        }
                        self.keycap(ui, item.hotkey, CaptureTarget::Item(id));
                    });
                }
            }
            1 => {
                ui.label(RichText::new("23 個預設按鍵").strong());
                ui.separator();
                let analysis = analyze(&self.points.document, &self.keys.document);
                for role in ["skill_", "spam_", "card_", ""]
                    .into_iter()
                    .flat_map(|prefix| {
                        PRESET_ROLES.into_iter().filter(move |r| {
                            if prefix.is_empty() {
                                !r.starts_with("skill_")
                                    && !r.starts_with("spam_")
                                    && !r.starts_with("card_")
                            } else {
                                r.starts_with(prefix)
                            }
                        })
                    })
                {
                    let p = self.keys.document.presets[role].clone();
                    let missing = self.points.document.mode == Mode::Six
                        && matches!(
                            role,
                            "skill_4" | "skill_5" | "spam_4" | "spam_5" | "card_4" | "card_5"
                        );
                    if matches!(role, "skill_1" | "spam_1" | "card_1" | "global_stop") {
                        ui.label(match role {
                            "skill_1" => "技能",
                            "spam_1" => "技能連點",
                            "card_1" => "卡幀開牌",
                            _ => "其他",
                        });
                    }
                    let conflict = analysis
                        .preset_errors
                        .iter()
                        .any(|e| e.to_string().contains(role));
                    ui.horizontal(|ui| {
                        let text = format!(
                            "{} · {}{}",
                            role_name(role),
                            if conflict {
                                "重複：已停用"
                            } else if p.enabled {
                                "啟用"
                            } else {
                                "停用"
                            },
                            if missing {
                                "（六人沒有此格）"
                            } else {
                                ""
                            }
                        );
                        let response = ui.add_sized(
                            [ui.available_width() - 105., 34.],
                            egui::Button::new(if conflict {
                                RichText::new(text).color(Color32::from_rgb(200, 65, 65))
                            } else {
                                RichText::new(text)
                            })
                            .selected(self.selection == Selection::Preset(role.into())),
                        );
                        if response.clicked() {
                            self.selection = Selection::Preset(role.into());
                        }
                        self.keycap(ui, p.hotkey, CaptureTarget::Preset(role.into()));
                    });
                }
            }
            2 => {
                ui.label(RichText::new("TimingProfile（ms）").strong());
                ui.separator();
                for (index, (name, value)) in timing_fields(&self.timing_draft).iter().enumerate() {
                    if row(
                        ui,
                        self.selection == Selection::Timing(index),
                        format!(
                            "{name}     {}",
                            value.map_or("不限".into(), |v| format!("{v:.3}"))
                        ),
                    )
                    .clicked()
                    {
                        self.selection = Selection::Timing(index);
                    }
                }
            }
            _ => {
                ui.label(RichText::new("事件清單").strong());
                ui.separator();
                for (index, text) in self.logs.clone().iter().enumerate().rev() {
                    if row_color(
                        ui,
                        self.selection == Selection::Log(index),
                        format!("{}  {}", index + 1, text.lines().next().unwrap_or_default()),
                        text.contains("問題："),
                    )
                    .clicked()
                    {
                        self.selection = Selection::Log(index);
                    }
                }
            }
        }
    }
    fn pixel_label(&self, x: f64, y: f64) -> String {
        self.monitor
            .map_or(format!("({x:.3}, {y:.3})，等待解析度"), |m| {
                format!(
                    "X {:.0}  Y {:.0} px",
                    x * f64::from(m[2]),
                    y * f64::from(m[3])
                )
            })
    }
    fn keycap(&mut self, ui: &mut egui::Ui, chord: Option<Chord>, target: CaptureTarget) {
        let text = if self.capture.as_ref().is_some_and(|c| c.target == target) {
            "按下按鍵…".into()
        } else {
            chord.map_or("— 設定熱鍵".into(), |c| c.to_string())
        };
        let analysis = analyze(&self.points.document, &self.keys.document);
        let (conflict, disabled) = match &target {
            CaptureTarget::Item(id) => (analysis.disabled_points.contains(id), chord.is_none()),
            CaptureTarget::Preset(role) => (
                analysis
                    .preset_errors
                    .iter()
                    .any(|e| e.to_string().contains(role)),
                !self.keys.document.presets[role].enabled,
            ),
            _ => (false, false),
        };
        let mut rich = RichText::new(text).monospace();
        if disabled {
            rich = rich.color(ui.visuals().weak_text_color());
        }
        if conflict {
            rich = rich.color(Color32::from_rgb(220, 70, 70));
        }
        let mut button = egui::Button::new(rich);
        if conflict {
            button = button
                .fill(Color32::from_rgba_unmultiplied(190, 40, 40, 40))
                .stroke(egui::Stroke::new(1., Color32::from_rgb(220, 70, 70)));
        }
        let response = ui.add(button);
        if chord.is_none() {
            let r = response.rect;
            let stroke = egui::Stroke::new(1., ui.visuals().weak_text_color());
            for (a, b) in [
                (r.left_top(), r.right_top()),
                (r.right_top(), r.right_bottom()),
                (r.right_bottom(), r.left_bottom()),
                (r.left_bottom(), r.left_top()),
            ] {
                let delta = b - a;
                let len = delta.length();
                let dir = delta / len;
                let mut at = 0.;
                while at < len {
                    ui.painter()
                        .line_segment([a + dir * at, a + dir * (at + 3.).min(len)], stroke);
                    at += 6.;
                }
            }
        }
        if response.clicked() {
            self.capture(target);
        }
    }
    fn details(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("詳細").strong());
        ui.separator();
        match self.selection.clone() {
            Selection::Candidate(index) => {
                if let Some(c) = self.candidates.get(index).cloned() {
                    ui.label(&c.name);
                    ui.monospace(self.pixel_label(c.position.x, c.position.y));
                    ui.label("來源：錄製／匯入");
                    if ui.button("綁成點位按鍵 →").clicked() {
                        match self.points.add_candidate(c) {
                            Ok(id) => {
                                self.candidates.remove(index);
                                self.selection = Selection::Item(id);
                                self.capture(CaptureTarget::Item(id));
                            }
                            Err(e) => self.log_error(e.to_string()),
                        }
                    }
                }
            }
            Selection::Item(id) => {
                if let Some(item) = selected_item(&self.points.document, id).cloned() {
                    let mut name = item.name.clone();
                    ui.label("名稱");
                    if ui.text_edit_singleline(&mut name).changed()
                        && let Err(e) = self.points.update_item(
                            id,
                            ItemPatch {
                                name: Some(name),
                                ..Default::default()
                            },
                        )
                    {
                        self.log(e.to_string());
                    }
                    ui.label("熱鍵");
                    self.keycap(ui, item.hotkey, CaptureTarget::Item(id));
                    if let Some(mut chord) = item.hotkey {
                        let mut exact = matches!(chord.mods, ModsMatch::Exact(_));
                        if ui.checkbox(&mut exact, "修飾鍵必須完全一致").changed() {
                            let mask = match chord.mods {
                                ModsMatch::Exact(m) | ModsMatch::AtLeast(m) => m,
                            };
                            chord.mods = if exact {
                                ModsMatch::Exact(mask)
                            } else {
                                ModsMatch::AtLeast(mask)
                            };
                            let _ = self.points.update_item(
                                id,
                                ItemPatch {
                                    hotkey: Some(Some(chord)),
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    ui.horizontal(|ui| {
                        ui.label("功能");
                        ui.label(if item.function == PointFunction::Card {
                            "牌鍵"
                        } else {
                            "普通點位"
                        });
                        if item.function == PointFunction::Point && ui.button("改為牌鍵").clicked()
                        {
                            let _ = self.points.update_item(
                                id,
                                ItemPatch {
                                    function: Some(PointFunction::Card),
                                    ..Default::default()
                                },
                            );
                        }
                    });
                    if item.function == PointFunction::Card
                        && ui.button("改為普通點位並錄製位置").clicked()
                    {
                        self.capture(CaptureTarget::ConvertToPoint(id));
                    }
                    let analysis = analyze(&self.points.document, &self.keys.document);
                    if analysis.disabled_points.contains(&id) {
                        for error in &analysis.reports {
                            ui.colored_label(Color32::from_rgb(200, 70, 70), error.to_string());
                        }
                    }
                    if item.function == PointFunction::Point {
                        let mut x = item.x.unwrap_or(0.);
                        let mut y = item.y.unwrap_or(0.);
                        if let Some(m) = self.monitor {
                            x *= f64::from(m[2]);
                            y *= f64::from(m[3]);
                            let mut changed = false;
                            ui.horizontal(|ui| {
                                ui.label("X（px）");
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut x)
                                            .speed(1.)
                                            .range(0.0..=f64::from(m[2] - 1)),
                                    )
                                    .changed();
                            });
                            ui.horizontal(|ui| {
                                ui.label("Y（px）");
                                changed |= ui
                                    .add(
                                        egui::DragValue::new(&mut y)
                                            .speed(1.)
                                            .range(0.0..=f64::from(m[3] - 1)),
                                    )
                                    .changed();
                            });
                            if changed
                                && let Err(e) = self.points.update_item(
                                    id,
                                    ItemPatch {
                                        position: Some(NormPoint {
                                            x: x / f64::from(m[2]),
                                            y: y / f64::from(m[3]),
                                        }),
                                        ..Default::default()
                                    },
                                )
                            {
                                self.log(e.to_string());
                            }
                        } else {
                            ui.label("等待遊戲解析度，暫不可輸入像素");
                            ui.monospace(self.pixel_label(x, y));
                        }
                        if ui.button("重錄位置").clicked() {
                            self.capture(CaptureTarget::ReRecord(id));
                        }
                        if ui.button("移回候選").clicked() {
                            match self.points.move_to_candidates(id) {
                                Ok(c) => {
                                    self.candidates.push(c);
                                    self.selection =
                                        Selection::Candidate(self.candidates.len() - 1);
                                }
                                Err(e) => self.log_error(e.to_string()),
                            }
                        }
                    } else {
                        self.hand_details(ui);
                    }
                    if ui.button("刪除這一項").clicked() {
                        let _ = self.points.delete_item(id);
                        self.selection = Selection::Item(0);
                    }
                }
            }
            Selection::Preset(role) => {
                let p = self.keys.document.presets[&role].clone();
                ui.label(role_name(&role));
                ui.monospace(&role);
                self.keycap(ui, p.hotkey, CaptureTarget::Preset(role.clone()));
                let mut enabled = p.enabled;
                let mut exact = p
                    .hotkey
                    .is_some_and(|h| matches!(h.mods, ModsMatch::Exact(_)));
                let mut changed = ui
                    .add_enabled(
                        role != "global_stop",
                        egui::Checkbox::new(&mut enabled, "啟用"),
                    )
                    .changed();
                changed |= ui.checkbox(&mut exact, "修飾鍵必須完全一致").changed();
                if changed && let Err(e) = self.keys.set_preset(&role, p.hotkey, enabled, exact) {
                    self.log(e.to_string());
                }
                ui.label(format!(
                    "預設：{}",
                    template_keys().presets[&role]
                        .hotkey
                        .map_or("—".into(), |c| c.to_string())
                ));
                if ui.button("恢復這一項").clicked() {
                    let _ = self.keys.reset_preset(&role);
                }
                for e in analyze(&self.points.document, &self.keys.document).preset_errors {
                    if e.to_string().contains(&role) {
                        ui.colored_label(Color32::from_rgb(200, 70, 70), e.to_string());
                    }
                }
            }
            Selection::Timing(index) => {
                let fields = timing_fields(&self.timing_draft);
                if let Some((name, value)) = fields.get(index) {
                    ui.monospace(*name);
                    ui.label("單位：ms；修改後改為未驗證");
                    ui.label(match index {
                        0 => "零按住時間需已驗證；修改時間會清除驗證",
                        1 => "滑鼠按住時間必須大於零",
                        3 | 7 => "週期必須大於零，連點週期需容納按住時間",
                        8..=14 => "期限容忍必須非負；第二個 Esc 不可放寬",
                        _ => "間隔必須非負",
                    });
                    let mut unlimited = value.is_none();
                    let mut number = value.unwrap_or(0.);
                    let mut changed = false;
                    if index >= 8 && index != 12 {
                        changed |= ui.checkbox(&mut unlimited, "不限制晚醒").changed();
                    }
                    changed |= ui
                        .add_enabled(
                            !unlimited,
                            egui::DragValue::new(&mut number)
                                .speed(0.1)
                                .range(0.0..=100_000.0),
                        )
                        .changed();
                    if changed {
                        let mut timing = self.timing_draft.clone();
                        set_timing_field(
                            &mut timing,
                            index,
                            if unlimited { None } else { Some(number) },
                        );
                        self.timing_draft = timing.clone();
                        self.timing_error =
                            self.keys.set_timing(timing).err().map(|e| e.to_string());
                        if self.timing_error.is_none() {
                            self.timing_draft = self.keys.document.timing.clone();
                        }
                    }
                    if let Some(error) = &self.timing_error {
                        ui.colored_label(Color32::RED, error);
                    }
                    let identity = &self.keys.document.timing.identity;
                    for value in [
                        &identity.game_version,
                        &identity.window_mode,
                        &identity.fps,
                        &identity.machine,
                        &identity.conditions,
                    ] {
                        ui.label(value);
                    }
                }
            }
            Selection::Log(index) => {
                if let Some(text) = self.logs.get(index) {
                    ui.label(text);
                }
            }
        }
    }
    fn hand_details(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("手牌（這台電腦）");
        ui.horizontal(|ui| {
            for (side, label) in [(CardUiSide::Right, "右"), (CardUiSide::Left, "左")] {
                if ui
                    .selectable_label(self.hand.document.card_ui_side == side, label)
                    .clicked()
                {
                    self.hand.set_card_ui_side(side);
                    self.store_hand();
                }
            }
        });
        let mut reverse = self.hand.document.slot_order == SlotOrder::RightToLeft;
        if ui.checkbox(&mut reverse, "格的順序：從右到左").changed() {
            self.hand.set_slot_order(if reverse {
                SlotOrder::RightToLeft
            } else {
                SlotOrder::LeftToRight
            });
            self.store_hand();
        }
        let mode = self.points.document.mode;
        let side = self.hand.document.card_ui_side;
        let r = self.hand.document.layout(mode);
        ui.label(if r.calibrated {
            "範圍已校準"
        } else {
            "範圍尚未校準"
        });
        let mut rect = r;
        let mut changed = false;
        let scale = self.monitor.map_or([1., 1., 1., 1.], |m| {
            [
                f64::from(m[2]),
                f64::from(m[2]),
                f64::from(m[3]),
                f64::from(m[3]),
            ]
        });
        let mut values = [
            rect.x[0] * scale[0],
            rect.x[1] * scale[1],
            rect.y[0] * scale[2],
            rect.y[1] * scale[3],
        ];
        for ((label, value), scale) in ["左", "右", "上", "下"]
            .into_iter()
            .zip(values.iter_mut())
            .zip(scale)
        {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{label}（{}）",
                    if self.monitor.is_some() {
                        "px"
                    } else {
                        "0–1"
                    }
                ));
                changed |= ui
                    .add(
                        egui::DragValue::new(value)
                            .speed(if scale > 1. { 1. } else { 0.001 })
                            .range(0.0..=scale),
                    )
                    .changed();
            });
        }
        if changed {
            rect.x = [values[0] / scale[0], values[1] / scale[1]];
            rect.y = [values[2] / scale[2], values[3] / scale[3]];
            rect.calibrated = true;
            match self.hand.set_hand_rect(mode, side, rect) {
                Ok(_) => self.store_hand(),
                Err(e) => self.log_error(e.to_string()),
            }
        }
        for slot in 0..usize::from(mode.slots()) {
            ui.horizontal(|ui| {
                ui.label(format!("第 {} 格", slot + 1));
                self.keycap(
                    ui,
                    Some(Chord {
                        key: self.hand.document.slot_keys[slot],
                        mods: ModsMatch::AtLeast(0),
                    }),
                    CaptureTarget::Slot(slot),
                );
            });
        }
        let mut repeat = self.hand.document.card_auto_repeat == AutoRepeatPolicy::FollowTypematic;
        if ui.checkbox(&mut repeat, "按住時自動重複").changed() {
            self.hand.set_card_auto_repeat(if repeat {
                AutoRepeatPolicy::FollowTypematic
            } else {
                AutoRepeatPolicy::Ignore
            });
            self.store_hand();
        }
        if ui.button("在遊戲上校準").clicked() {
            self.overlay_resume = self.wanted;
            self.job(Job::PrepareOverlay);
        }
        if ui.button("鏡像到另一邊").clicked() && self.hand.mirror(mode, side).is_ok() {
            self.store_hand();
        }
        if ui.button("還原預設範圍").clicked() && self.hand.reset_layout(mode, side).is_ok() {
            self.store_hand();
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            egui::Window::new("有未儲存的變更")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let kind = self.pending.as_ref().unwrap().kind;
                    ui.label(if kind == FileKind::Points {
                        "點位 JSON 尚未儲存"
                    } else {
                        "預設按鍵 JSON 尚未儲存"
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                (kind == FileKind::Points
                                    && !self.pending.as_ref().unwrap().closing)
                                    || (self.timing_error.is_none()
                                        && can_save_keys(
                                            &self.points.document,
                                            &self.keys.document,
                                        )),
                                egui::Button::new("儲存後切換"),
                            )
                            .clicked()
                        {
                            let pending = self.pending.take().unwrap();
                            self.next_job = Some(pending.job);
                            self.save(if pending.closing { None } else { Some(kind) });
                        }
                        if ui.button("不儲存").clicked() {
                            let pending = self.pending.take().unwrap();
                            if pending.closing {
                                self.quit();
                            } else {
                                self.job(pending.job);
                            }
                        }
                        if ui.button("取消").clicked() {
                            self.pending = None;
                        }
                    });
                });
        }
        if let Some((path, kind)) = self.close_prompt.clone() {
            egui::Window::new("關閉分頁或刪除檔案")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(path.display().to_string());
                    ui.label(
                        if (kind == FileKind::Points && self.points_dirty())
                            || (kind == FileKind::Keys && self.keys_dirty())
                        {
                            "此類文件有未儲存的變更"
                        } else {
                            "檔案保留在原路徑，或移到 .trash"
                        },
                    );
                    let count = self.settings.as_ref().map_or(0, |s| {
                        if kind == FileKind::Points {
                            s.point_tabs.len()
                        } else {
                            s.key_tabs.len()
                        }
                    });
                    ui.horizontal(|ui| {
                        for (trash, label) in [(false, "關閉分頁"), (true, "刪除檔案")] {
                            if ui
                                .add_enabled(count > 1, egui::Button::new(label))
                                .clicked()
                            {
                                self.close_prompt = None;
                                self.begin(kind, Job::Close(path.clone(), trash, self.wanted));
                            }
                        }
                        if ui.button("取消").clicked() {
                            self.close_prompt = None;
                        }
                    });
                    if count <= 1 {
                        ui.label("每列至少要留一個分頁");
                    }
                });
        }
        if let Some((path, reason)) = self.import.clone() {
            egui::Window::new("舊檔匯入")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(path.display().to_string());
                    if reason != "import" {
                        ui.label(&reason);
                    }
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.import_mode, Mode::Six, "六人");
                        ui.selectable_value(&mut self.import_mode, Mode::Ten, "十人");
                    });
                    ui.label("原始解析度（舊絕對座標才需要）");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.import_resolution[0])
                                .range(0.0..=100_000.0),
                        );
                        ui.label("×");
                        ui.add(
                            egui::DragValue::new(&mut self.import_resolution[1])
                                .range(0.0..=100_000.0),
                        );
                    });
                    if ui.button("轉換並開啟新版").clicked() {
                        let resolution = (self.import_resolution.iter().all(|v| *v > 0.))
                            .then_some(self.import_resolution);
                        self.import = None;
                        self.job(Job::PlanImport {
                            path,
                            mode: self.import_mode,
                            resolution,
                        });
                    }
                    if ui.button("取消").clicked() {
                        self.import = None;
                    }
                });
        }
    }
    fn quit(&mut self) {
        if !self.quitting {
            self.quitting = true;
            self.busy = true;
            self.backend.controller.cancel_capture();
            self.backend.controller.quit();
            let _ = self.backend.jobs.send(Job::Quit);
        }
    }
    fn close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.backend.controller.disable();
            self.wanted = false;
            if self.points_dirty() || self.keys_dirty() {
                let kind = if self.points_dirty() {
                    FileKind::Points
                } else {
                    FileKind::Keys
                };
                self.pending = Some(Pending {
                    kind,
                    job: Job::Quit,
                    closing: true,
                });
            } else {
                self.quit();
            }
        }
    }
    fn finish_overlay(&mut self, save: bool) {
        if let Some(overlay) = self.overlay.take() {
            if save {
                self.overlay_generation = Some(overlay.generation);
                match self.hand.set_hand_rect(
                    self.points.document.mode,
                    overlay.side,
                    HandRect {
                        calibrated: true,
                        ..overlay.rect
                    },
                ) {
                    Ok(_) => self.store_hand(),
                    Err(e) => self.log_error(e.to_string()),
                }
            } else if self.overlay_resume {
                self.backend.controller.resume(overlay.generation);
            }
            self.message = if save {
                "手牌範圍已送出儲存"
            } else {
                "校準已取消"
            }
            .into();
        }
    }
    fn draw_overlay(&mut self, ctx: &egui::Context) {
        if let Some(mut overlay) = self.overlay.take() {
            let ppp = ctx.input(|i| i.viewport().native_pixels_per_point.unwrap_or(1.));
            let m = overlay.monitor;
            let size = Vec2::new(m[2] as f32 / ppp, m[3] as f32 / ppp);
            let mut result = None;
            let mode = self.points.document.mode;
            let keys = self.hand.document.slot_keys;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("hand-calibration"),
                egui::ViewportBuilder::default()
                    .with_title("BAauto 手牌校準")
                    .with_position(egui::pos2(m[0] as f32 / ppp, m[1] as f32 / ppp))
                    .with_inner_size(size)
                    .with_decorations(false)
                    .with_transparent(true)
                    .with_active(false)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop)
                    .with_mouse_passthrough(
                        !overlay.centers_mode
                            && !overlay.dragging
                            && !overlay_hit(overlay.rect, self.observation.cursor, m),
                    ),
                |ui, _| {
                    let rect = ui.max_rect();
                    let norm_to_pos = |p: NormPoint| {
                        egui::pos2(
                            rect.left() + rect.width() * p.x as f32,
                            rect.top() + rect.height() * p.y as f32,
                        )
                    };
                    let pos_to_norm = |p: egui::Pos2| NormPoint {
                        x: f64::from((p.x - rect.left()) / rect.width()),
                        y: f64::from((p.y - rect.top()) / rect.height()),
                    };
                    let r = egui::Rect::from_min_max(
                        norm_to_pos(NormPoint {
                            x: overlay.rect.x[0],
                            y: overlay.rect.y[0],
                        }),
                        norm_to_pos(NormPoint {
                            x: overlay.rect.x[1],
                            y: overlay.rect.y[1],
                        }),
                    );
                    ui.painter().rect_stroke(
                        r,
                        0.,
                        egui::Stroke::new(2., Color32::from_rgb(45, 135, 235)),
                        egui::StrokeKind::Inside,
                    );
                    for slot in 0..mode.slots() {
                        let x = r.left() + r.width() * f32::from(slot) / f32::from(mode.slots());
                        if slot > 0 {
                            ui.painter().line_segment(
                                [egui::pos2(x, r.top()), egui::pos2(x, r.bottom())],
                                egui::Stroke::new(1., Color32::WHITE),
                            );
                        }
                        let center =
                            egui::pos2(x + r.width() / f32::from(mode.slots()) / 2., r.center().y);
                        ui.painter().text(
                            center,
                            egui::Align2::CENTER_CENTER,
                            format!("{}  {}", slot + 1, keys[slot as usize]),
                            egui::FontId::monospace(20.),
                            Color32::WHITE,
                        );
                    }
                    overlay.dragging = false;
                    if !overlay.centers_mode {
                        for (corner, p) in [
                            (Corner::TopLeft, r.left_top()),
                            (Corner::BottomRight, r.right_bottom()),
                            (Corner::TopRight, r.right_top()),
                            (Corner::BottomLeft, r.left_bottom()),
                            (Corner::Left, r.left_center()),
                            (Corner::Right, r.right_center()),
                            (Corner::Top, r.center_top()),
                            (Corner::Bottom, r.center_bottom()),
                        ] {
                            let hit = egui::Rect::from_center_size(
                                p,
                                match corner {
                                    Corner::Left | Corner::Right => {
                                        Vec2::new(20., (r.height() - 20.).max(20.))
                                    }
                                    Corner::Top | Corner::Bottom => {
                                        Vec2::new((r.width() - 20.).max(20.), 20.)
                                    }
                                    _ => Vec2::splat(20.),
                                },
                            );
                            ui.painter().rect_filled(
                                egui::Rect::from_center_size(p, Vec2::splat(12.)),
                                3.,
                                Color32::from_rgb(45, 135, 235),
                            );
                            let response = ui.interact(
                                hit,
                                ui.id().with(format!("corner-{corner:?}")),
                                egui::Sense::drag(),
                            );
                            overlay.dragging |= response.dragged();
                            if response.dragged()
                                && let Some(pos) = response.interact_pointer_pos()
                                && let Ok(next) =
                                    drag_corner(overlay.rect, corner, pos_to_norm(pos))
                            {
                                overlay.rect = next;
                            }
                        }
                    } else {
                        let response =
                            ui.interact(rect, ui.id().with("centers"), egui::Sense::click());
                        if response.clicked()
                            && let Some(pos) = response.interact_pointer_pos()
                        {
                            overlay.centers.push(pos_to_norm(pos));
                            if overlay.centers.len() == 2 {
                                if let Ok(r) = rect_from_centers(
                                    overlay.centers[0],
                                    overlay.centers[1],
                                    mode.slots(),
                                    overlay.rect.y[1] - overlay.rect.y[0],
                                ) {
                                    overlay.rect = r;
                                }
                                overlay.centers.clear();
                                overlay.centers_mode = false;
                            }
                        }
                    }
                    egui::Area::new("overlay-controls".into())
                        .fixed_pos(rect.left_top() + Vec2::splat(24.))
                        .show(ui.ctx(), |ui| {
                            egui::Frame::window(ui.style()).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("手牌校準");
                                    if ui.button("確認").clicked() {
                                        result = Some(true);
                                    }
                                    if ui.button("取消").clicked() {
                                        result = Some(false);
                                    }
                                    if ui.button("依序點第一格／最後一格中心").clicked()
                                    {
                                        overlay.centers_mode = true;
                                        overlay.centers.clear();
                                    }
                                    if ui.button("左／右預覽").clicked() {
                                        overlay.side = if overlay.side == CardUiSide::Right {
                                            CardUiSide::Left
                                        } else {
                                            CardUiSide::Right
                                        };
                                        let mut h = self.hand.document.clone();
                                        h.card_ui_side = overlay.side;
                                        overlay.rect = h.layout(mode);
                                    }
                                    if ui.button("鏡像到另一邊").clicked() {
                                        overlay.rect = mirror_rect(overlay.rect);
                                        overlay.side = if overlay.side == CardUiSide::Right {
                                            CardUiSide::Left
                                        } else {
                                            CardUiSide::Right
                                        };
                                    }
                                });
                            });
                        });
                    if ui.ctx().input(|i| {
                        i.key_pressed(egui::Key::Escape) || i.viewport().close_requested()
                    }) {
                        result = Some(false);
                    }
                },
            );
            let _ = self
                .backend
                .jobs
                .send(Job::PlaceOverlay("BAauto 手牌校準".into(), m));
            self.overlay = Some(overlay);
            if let Some(save) = result {
                self.finish_overlay(save);
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::from_hash_of("hand-calibration"),
                    egui::ViewportCommand::Close,
                );
            }
        }
        if self.capture.as_ref().is_some_and(|c| {
            c.ready
                && matches!(
                    c.target,
                    CaptureTarget::Point
                        | CaptureTarget::ReRecord(_)
                        | CaptureTarget::ConvertToPoint(_)
                )
        }) && let Some(m) = self.monitor
        {
            let ppp = ctx.input(|i| i.viewport().native_pixels_per_point.unwrap_or(1.));
            let cursor = self.observation.cursor;
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("record-point"),
                egui::ViewportBuilder::default()
                    .with_title("BAauto 錄點")
                    .with_position(egui::pos2(m[0] as f32 / ppp, m[1] as f32 / ppp))
                    .with_inner_size(Vec2::new(m[2] as f32 / ppp, m[3] as f32 / ppp))
                    .with_transparent(true)
                    .with_decorations(false)
                    .with_active(false)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop)
                    .with_mouse_passthrough(true),
                |ui, _| {
                    if let Some(p) = cursor {
                        let r = ui.max_rect();
                        let pos = egui::pos2(
                            r.left() + r.width() * p.x as f32,
                            r.top() + r.height() * p.y as f32,
                        );
                        ui.painter().text(
                            pos + Vec2::new(18., 18.),
                            egui::Align2::LEFT_TOP,
                            format!(
                                "X {:.0}  Y {:.0} — 點一下錄製 / Esc取消",
                                p.x * f64::from(m[2]),
                                p.y * f64::from(m[3])
                            ),
                            egui::FontId::monospace(16.),
                            Color32::WHITE,
                        );
                    }
                },
            );
            let _ = self
                .backend
                .jobs
                .send(Job::PlaceOverlay("BAauto 錄點".into(), m));
        } else {
            ctx.send_viewport_cmd_to(
                egui::ViewportId::from_hash_of("record-point"),
                egui::ViewportCommand::Close,
            );
        }
    }
}
impl eframe::App for App {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }
    fn logic(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll(ctx);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.frames += 1;
        self.close(&ctx);
        if self.frames.is_multiple_of(30)
            && self.qa.is_none()
            && let Some(size) = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()))
            && let Some(settings) = &mut self.settings
            && settings.ui.window_size != [size.x, size.y]
        {
            settings.ui.window_size = [size.x, size.y];
            let _ = self
                .backend
                .jobs
                .send(Job::Preferences(settings.ui.clone()));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.backend.controller.cancel_capture();
            self.capture = None;
            if self.overlay.is_some() {
                self.finish_overlay(false);
            }
        }
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            self.job(Job::Inspect(file.path().to_path_buf()));
        }
        egui::Panel::top("header").show(ui, |ui| self.header(ui));
        egui::Panel::bottom("footer").show(ui, |ui| self.footer(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            if self.busy || self.overlay.is_some() || self.pending.is_some() {
                ui.disable();
            }
            let available = ui.available_size();
            let widths = [
                available.x * 0.20,
                available.x * 0.43,
                available.x * 0.37 - 24.,
            ];
            ui.horizontal_top(|ui| {
                for (index, width) in widths.iter().enumerate() {
                    ui.allocate_ui_with_layout(
                        Vec2::new(*width, available.y),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(*width);
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                            egui::ScrollArea::vertical().id_salt(index).show(
                                ui,
                                |ui| match index {
                                    0 => self.list(ui),
                                    1 => self.middle(ui),
                                    _ => self.details(ui),
                                },
                            );
                        },
                    );
                    if index < 2 {
                        ui.separator();
                    }
                }
            });
        });
        self.dialogs(&ctx);
        self.draw_overlay(&ctx);
        if let Some(qa) = &mut self.qa
            && self.settings.is_some()
            && self.frames > 15
            && self.notice.as_ref().is_none_or(|n| n.expired())
            && !qa.requested
        {
            qa.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new((
                qa.dir.clone(),
                qa.dark,
                qa.scale,
                qa.tab,
            ))));
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event
                && let Some(qa) = &self.qa
            {
                let path = qa.dir.join(format!(
                    "{}-{}-{}pct.png",
                    if qa.dark { "dark" } else { "light" },
                    ["points", "keys", "timing", "logs"][qa.tab as usize],
                    (qa.scale * 100.) as u32
                ));
                let _ = self.backend.jobs.send(Job::Screenshot {
                    path,
                    width: image.size[0] as u32,
                    height: image.size[1] as u32,
                    rgba: image.pixels.iter().flat_map(|c| c.to_array()).collect(),
                });
                self.quit();
            }
        }
    }
    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        self.backend.controller.disable();
        self.backend.controller.cancel_capture();
        let _ = self.backend.jobs.send(Job::Quit);
    }
}
fn row(ui: &mut egui::Ui, selected: bool, text: String) -> egui::Response {
    row_color(ui, selected, text, false)
}
fn row_color(ui: &mut egui::Ui, selected: bool, text: String, problem: bool) -> egui::Response {
    let mut rich = RichText::new(text);
    if problem {
        rich = rich.color(Color32::from_rgb(200, 65, 65));
    }
    let response = ui.add_sized(
        [ui.available_width(), 34.],
        egui::Button::new(rich).selected(selected),
    );
    ui.add_space(1.);
    response
}
fn role_name(role: &str) -> String {
    match role {
        r if r.starts_with("skill_") => format!("技能 {}", r.trim_start_matches("skill_")),
        r if r.starts_with("spam_") => format!("技能連點 {}", r.trim_start_matches("spam_")),
        r if r.starts_with("card_") => format!("卡幀開牌 {}", r.trim_start_matches("card_")),
        "global_stop" => "緊急停止".into(),
        "record" => "錄製點位".into(),
        "pause" => "暫停".into(),
        "frame_fast" => "快速卡幀".into(),
        "frame_slow" => "慢速卡幀".into(),
        "auto" => "自動戰鬥".into(),
        "speed" => "戰鬥速度".into(),
        "click_spam" => "左鍵連點".into(),
        _ => role.into(),
    }
}
fn timing_fields(t: &TimingProfile) -> Vec<(&'static str, Option<f64>)> {
    vec![
        ("key_hold_ms", Some(t.key_hold_ms)),
        ("mouse_hold_ms", Some(t.mouse_hold_ms)),
        ("key_to_click_ms", Some(t.key_to_click_ms)),
        ("repeat_period_ms", Some(t.repeat_period_ms)),
        ("escape_gap_ms[0]", Some(t.escape_gap_ms[0])),
        ("escape_gap_ms[1]", Some(t.escape_gap_ms[1])),
        ("card_pre_esc_ms", Some(t.card_pre_esc_ms)),
        ("hold_maintain_period_ms", Some(t.hold_maintain_period_ms)),
        ("lateness.tap_start", t.allowed_lateness_ms.tap_start),
        ("lateness.repeat_click", t.allowed_lateness_ms.repeat_click),
        ("lateness.card_start", t.allowed_lateness_ms.card_start),
        ("lateness.card_click", t.allowed_lateness_ms.card_click),
        (
            "lateness.double_esc_second",
            Some(t.allowed_lateness_ms.double_esc_second),
        ),
        ("lateness.hold_start", t.allowed_lateness_ms.hold_start),
        (
            "lateness.card_key_start",
            t.allowed_lateness_ms.card_key_start,
        ),
    ]
}
fn set_timing_field(t: &mut TimingProfile, index: usize, value: Option<f64>) {
    match index {
        0 => t.key_hold_ms = value.unwrap_or(0.),
        1 => t.mouse_hold_ms = value.unwrap_or(0.),
        2 => t.key_to_click_ms = value.unwrap_or(0.),
        3 => t.repeat_period_ms = value.unwrap_or(0.),
        4 => t.escape_gap_ms[0] = value.unwrap_or(0.),
        5 => t.escape_gap_ms[1] = value.unwrap_or(0.),
        6 => t.card_pre_esc_ms = value.unwrap_or(0.),
        7 => t.hold_maintain_period_ms = value.unwrap_or(0.),
        8 => t.allowed_lateness_ms.tap_start = value,
        9 => t.allowed_lateness_ms.repeat_click = value,
        10 => t.allowed_lateness_ms.card_start = value,
        11 => t.allowed_lateness_ms.card_click = value,
        12 => t.allowed_lateness_ms.double_esc_second = value.unwrap_or(0.),
        13 => t.allowed_lateness_ms.hold_start = value,
        14 => t.allowed_lateness_ms.card_key_start = value,
        _ => {}
    }
}

// Native deadlines remain integer QPC ticks. Instant is used only for the portable UI preview.
struct UiDeadline {
    #[cfg(windows)]
    clock: baauto_win_platform::Clock,
    #[cfg(windows)]
    due: Ticks,
    #[cfg(not(windows))]
    due: std::time::Instant,
}
impl UiDeadline {
    fn new(seconds: u64) -> Self {
        #[cfg(windows)]
        {
            let clock = baauto_win_platform::Clock::new().expect("QPC available");
            Self {
                clock,
                due: clock.now().after(clock.rate.0 * seconds),
            }
        }
        #[cfg(not(windows))]
        {
            Self {
                due: std::time::Instant::now() + Duration::from_secs(seconds),
            }
        }
    }
    fn expired(&self) -> bool {
        #[cfg(windows)]
        {
            self.clock.now() >= self.due
        }
        #[cfg(not(windows))]
        {
            std::time::Instant::now() >= self.due
        }
    }
}
fn overlay_hit(r: HandRect, cursor: Option<NormPoint>, m: [i32; 4]) -> bool {
    cursor.is_some_and(|p| {
        let dx = 14. / f64::from(m[2]);
        let dy = 14. / f64::from(m[3]);
        let control = p.y * f64::from(m[3]) >= 20. && p.y * f64::from(m[3]) <= 115.;
        let vertical = (p.x - r.x[0]).abs().min((p.x - r.x[1]).abs()) <= dx
            && p.y >= r.y[0] - dy
            && p.y <= r.y[1] + dy;
        let horizontal = (p.y - r.y[0]).abs().min((p.y - r.y[1]).abs()) <= dy
            && p.x >= r.x[0] - dx
            && p.x <= r.x[1] + dx;
        control || vertical || horizontal
    })
}

fn state_color(state: &str) -> Color32 {
    if state.contains("故障") || state.contains("Faulted") {
        Color32::from_rgb(200, 65, 65)
    } else if state == "運作中" || state == "已停止" {
        Color32::from_rgb(45, 135, 75)
    } else {
        Color32::from_rgb(180, 110, 35)
    }
}
