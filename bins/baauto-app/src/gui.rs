//! GUI edits memory through config::editor and sends commands; no injection or synchronous file/IPC work.
use crate::{backend::*, state::*};
use baauto_config::{editor::*, store::*, *};
use baauto_protocol::{capture::*, ipc::EngineToApp};
use eframe::egui::{self, Color32, Vec2};
use std::{path::PathBuf, time::Duration};
mod view;
use view::margin;
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
        crate::ui_kit::install_style(&cc.egui_ctx);
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
            tab: qa.as_ref().map_or(0, |v| v.3 % 4),
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
                hotkey_warning: None,
                can_arm: true,
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
    /// Offline preview only: show the occupied-hotkey banner without a supervisor.
    pub fn demo_hotkey_warning(&mut self) {
        self.observation.hotkey_warning = Some(
            "緊急停止鍵 Ctrl+F9 被其他程式佔用，現在不能啟用。請到「預設按鍵」改成別的組合鍵並儲存。"
                .into(),
        );
        self.observation.can_arm = false;
    }
    fn select_tab(&mut self) {
        let qa_card = self.qa.as_ref().is_some_and(|q| q.tab == 4);
        self.selection = match self.tab {
            0 if qa_card => Selection::Item(
                self.points
                    .document
                    .items
                    .iter()
                    .find(|p| p.function == PointFunction::Card)
                    .map_or(0, |p| p.id),
            ),
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
        let bg = crate::ui_kit::Pal::of(ui).bg;
        egui::Panel::top("header")
            .frame(egui::Frame::new().fill(bg).inner_margin(margin(16, 8, 8)))
            .show(ui, |ui| self.header(ui));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(bg).inner_margin(margin(16, 1, 1)))
            .show(ui, |ui| self.status_bar(ui));
        if self.save_bar_needed() {
            let soft = crate::ui_kit::Pal::of(ui).soft;
            egui::Panel::bottom("savebar")
                .frame(egui::Frame::new().fill(soft).inner_margin(margin(16, 6, 6)))
                .show(ui, |ui| self.save_bar(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg).inner_margin(margin(16, 12, 14)))
            .show(ui, |ui| {
                if self.busy || self.overlay.is_some() || self.pending.is_some() {
                    ui.disable();
                }
                self.main_area(ui);
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
                    ["points", "keys", "timing", "logs", "points-card"][qa.tab as usize],
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
fn role_name(role: &str) -> String {
    match role {
        r if r.starts_with("skill_") => format!("技能 {}", r.trim_start_matches("skill_")),
        r if r.starts_with("spam_") => format!("連點 {}", r.trim_start_matches("spam_")),
        r if r.starts_with("card_") => format!("開牌 {}", r.trim_start_matches("card_")),
        "global_stop" => "緊急停止".into(),
        "record" => "錄製點位".into(),
        "pause" => "普通暫停".into(),
        "frame_fast" => "卡幀暫停（快）".into(),
        "frame_slow" => "卡幀暫停（慢）".into(),
        "auto" => "AUTO".into(),
        "speed" => "加速".into(),
        "click_spam" => "滑鼠連點".into(),
        _ => role.into(),
    }
}
fn role_description(role: &str) -> String {
    let n = role.rsplit('_').next().unwrap_or_default();
    match role {
        r if r.starts_with("skill_") => format!("送出 {n} 一次。"),
        r if r.starts_with("spam_") => format!("按住時一直重複「送出 {n}，再點一下左鍵」。"),
        r if r.starts_with("card_") => {
            format!("按下先送 Esc，按住時重複「送出 {n}＋左鍵」，放開再送 Esc。")
        }
        "global_stop" => "停止全部動作並放開所有按鍵，任何時候都有效。只能用鍵盤組合鍵。".into(),
        "record" => "開始錄製點位（不限遊戲在前景）。只能用鍵盤組合鍵。".into(),
        "pause" => "送出 Esc。".into(),
        "frame_fast" => "Esc，等快卡幀間隔，再 Esc。".into(),
        "frame_slow" => "Esc，等慢卡幀間隔，再 Esc。".into(),
        "auto" => "送出 a。".into(),
        "speed" => "送出 s。".into(),
        "click_spam" => "按住時一直點左鍵。".into(),
        _ => String::new(),
    }
}
fn timing_fields(t: &TimingProfile) -> [(&'static str, f64); 8] {
    [
        ("key_hold_ms", t.key_hold_ms),
        ("mouse_hold_ms", t.mouse_hold_ms),
        ("key_to_click_ms", t.key_to_click_ms),
        ("repeat_period_ms", t.repeat_period_ms),
        ("escape_gap_ms[0]", t.escape_gap_ms[0]),
        ("escape_gap_ms[1]", t.escape_gap_ms[1]),
        ("card_pre_esc_ms", t.card_pre_esc_ms),
        ("hold_maintain_period_ms", t.hold_maintain_period_ms),
    ]
}
fn set_timing_field(t: &mut TimingProfile, index: usize, value: f64) {
    match index {
        0 => t.key_hold_ms = value,
        1 => t.mouse_hold_ms = value,
        2 => t.key_to_click_ms = value,
        3 => t.repeat_period_ms = value,
        4 => t.escape_gap_ms[0] = value,
        5 => t.escape_gap_ms[1] = value,
        6 => t.card_pre_esc_ms = value,
        7 => t.hold_maintain_period_ms = value,
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
