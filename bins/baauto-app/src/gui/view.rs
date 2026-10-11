//! View layer for the agreed C layout (docs/pages/BAauto-新介面Demo.html):
//! two JSON file rows, pill tabs, list + details columns, save bar and status bar.
//! Only rendering lives here; every action reuses the existing App commands.
use super::*;
use crate::ui_kit::{self as k, ChipKind, Pal};
use eframe::egui::{Align, CornerRadius, Layout, Margin, Stroke, vec2};

const CAND_COLS: [f32; 2] = [0., 92.];
const ITEM_COLS: [f32; 5] = [0., 74., 66., 88., 40.];
const PRESET_COLS: [f32; 3] = [0., 110., 170.];
const LOG_COLS: [f32; 2] = [34., 0.];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lvl {
    Ok,
    Bad,
    Off,
}
struct Status {
    lvl: Lvl,
    msg: String,
}
impl Status {
    fn color(&self, p: &Pal) -> Color32 {
        match self.lvl {
            Lvl::Ok => p.good,
            Lvl::Bad => p.bad,
            Lvl::Off => p.faint,
        }
    }
    fn chip(&self, has_key: bool) -> ChipKind {
        if !has_key {
            ChipKind::Empty
        } else {
            match self.lvl {
                Lvl::Ok => ChipKind::Normal,
                Lvl::Bad => ChipKind::Bad,
                Lvl::Off => ChipKind::Off,
            }
        }
    }
}

pub(super) struct TimingMeta {
    pub name: &'static str,
    pub json: &'static str,
    pub purpose: &'static str,
    pub rule: &'static str,
}
pub(super) const TIMING: [TimingMeta; 9] = [
    TimingMeta {
        name: "按鍵按住",
        json: "key_hold_ms",
        purpose: "技能、AUTO、加速、暫停、牌鍵送出時，按下到放開的時間。",
        rule: "0 只有已驗證的時間能用。",
    },
    TimingMeta {
        name: "點擊按住",
        json: "mouse_hold_ms",
        purpose: "連點、開牌裡左鍵按下到放開的時間。",
        rule: "要大於 0。",
    },
    TimingMeta {
        name: "技能鍵到點擊",
        json: "key_to_click_ms",
        purpose: "技能連點：技能鍵放開後，多久點左鍵。",
        rule: "",
    },
    TimingMeta {
        name: "連點週期",
        json: "repeat_period_ms",
        purpose: "連點一輪開始到下一輪開始。",
        rule: "不能小於「按鍵按住＋技能鍵到點擊＋點擊按住」。",
    },
    TimingMeta {
        name: "快卡幀 Esc 間隔",
        json: "escape_gap_ms[0]",
        purpose: "卡幀暫停（快）兩次 Esc 之間的間隔。第二個 Esc 一定會送。",
        rule: "",
    },
    TimingMeta {
        name: "慢卡幀 Esc 間隔",
        json: "escape_gap_ms[1]",
        purpose: "卡幀暫停（慢）兩次 Esc 之間的間隔。第二個 Esc 一定會送。",
        rule: "",
    },
    TimingMeta {
        name: "開牌前等待",
        json: "card_pre_esc_ms",
        purpose: "卡幀開牌：第一個 Esc 之後，多久開始送數字鍵。",
        rule: "",
    },
    TimingMeta {
        name: "點位維持週期",
        json: "hold_maintain_period_ms",
        purpose: "普通點位按住時，多久把游標拉回點位一次。",
        rule: "要大於 0。",
    },
    TimingMeta {
        name: "牌鍵回位等待",
        json: "card_return_wait_ms",
        purpose: "牌鍵點完位置後，游標在那裡停多久才回到你的位置。太短的話遊戲可能讀不到點擊位置。",
        rule: "不超過「連點週期－點擊按住」時，牌鍵和連點一樣快。",
    },
];

fn role_of(path: &str) -> Option<&str> {
    path.strip_prefix("$.presets.")?.strip_suffix(".hotkey")
}
fn other_of(reason: &str) -> &str {
    reason.strip_prefix("conflicts with ").unwrap_or(reason)
}

impl App {
    /// Physical size of the game's monitor (from the engine) or, before the game
    /// has been seen, of the monitor this window is on.
    fn resolution(&self, ctx: &egui::Context) -> Option<([f64; 2], bool)> {
        if let Some(m) = self.monitor {
            return Some(([f64::from(m[2]), f64::from(m[3])], true));
        }
        let ppp = ctx.pixels_per_point();
        ctx.input(|i| {
            let size = i.viewport().monitor_size?;
            Some((
                [
                    f64::from(size.x * ppp).round(),
                    f64::from(size.y * ppp).round(),
                ],
                false,
            ))
        })
    }
    fn px(&self, ctx: &egui::Context, x: f64, y: f64) -> String {
        match self.resolution(ctx) {
            Some(([w, h], _)) => format!("{:.0}, {:.0}", x * w, y * h),
            None => format!("{x:.3}, {y:.3}"),
        }
    }
    fn item_status(&self, a: &ConflictAnalysis, item: &PointItem) -> Status {
        if item.hotkey.is_none() {
            return Status {
                lvl: Lvl::Off,
                msg: "沒有熱鍵，不會觸發".into(),
            };
        }
        if item.x.is_none() || item.y.is_none() {
            return Status {
                lvl: Lvl::Bad,
                msg: "需要錄點位，停用".into(),
            };
        }
        if a.disabled_points.contains(&item.id) {
            let path = format!("$.items[id={}].hotkey", item.id);
            let other = a
                .reports
                .iter()
                .find(|e| e.path == path)
                .map(|e| other_of(&e.reason).to_owned())
                .unwrap_or_default();
            let who = if PRESET_ROLES.contains(&other.as_str()) {
                format!("預設按鍵「{}」", role_name(&other))
            } else {
                format!("「{other}」")
            };
            return Status {
                lvl: Lvl::Bad,
                msg: format!("和{who}重複，停用"),
            };
        }
        Status {
            lvl: Lvl::Ok,
            msg: "啟用".into(),
        }
    }
    fn preset_status(&self, a: &ConflictAnalysis, role: &str) -> Status {
        let p = &self.keys.document.presets[role];
        if p.hotkey.is_none() {
            return Status {
                lvl: Lvl::Off,
                msg: "沒有熱鍵，停用".into(),
            };
        }
        if !p.enabled {
            return Status {
                lvl: Lvl::Off,
                msg: "停用".into(),
            };
        }
        let others: Vec<String> = a
            .preset_errors
            .iter()
            .filter(|e| role_of(&e.path) == Some(role))
            .map(|e| role_name(other_of(&e.reason)))
            .collect();
        if !others.is_empty() {
            return Status {
                lvl: Lvl::Bad,
                msg: format!("和「{}」重複", others.join("、")),
            };
        }
        let slot = role
            .rsplit('_')
            .next()
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|_| {
                role.starts_with("skill_") || role.starts_with("spam_") || role.starts_with("card_")
            });
        match slot {
            Some(n) if n > self.points.document.mode.slots() => Status {
                lvl: Lvl::Ok,
                msg: format!("啟用（六人沒有第 {n} 格）"),
            },
            _ => Status {
                lvl: Lvl::Ok,
                msg: "啟用".into(),
            },
        }
    }
    fn timing_checks(&self) -> [Option<String>; 9] {
        let t = &self.timing_draft;
        let values = timing_fields(t);
        let mut out: [Option<String>; 9] = Default::default();
        for (i, (_, v)) in values.iter().enumerate() {
            if !v.is_finite() || *v < 0. {
                out[i] = Some("要填 0 以上的數字".into());
            }
        }
        if out[0].is_none() && t.key_hold_ms == 0. {
            out[0] = Some("0 只有已驗證的時間能用".into());
        }
        if out[1].is_none() && t.mouse_hold_ms <= 0. {
            out[1] = Some("要大於 0".into());
        }
        let need = t.key_hold_ms + t.key_to_click_ms + t.mouse_hold_ms;
        if out[3].is_none() && t.repeat_period_ms < need {
            out[3] = Some(format!("至少要 {need}"));
        }
        if out[7].is_none() && t.hold_maintain_period_ms <= 0. {
            out[7] = Some("要大於 0".into());
        }
        out
    }

    // ---------- top ----------
    pub(super) fn header(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        ui.horizontal(|ui| {
            ui.set_height(32.);
            ui.label(k::text("BAauto", p.ink, 14.5).strong());
            if self.preview {
                ui.label(k::text("離線預覽", p.faint, k::SMALL));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let faulted = self.observation.state.contains("故障");
                let label = if self.wanted {
                    "停止"
                } else if faulted {
                    "重新啟用"
                } else {
                    "啟用"
                };
                let enabled = self.wanted
                    || (!self.busy
                        && self.capture.is_none()
                        && self.overlay.is_none()
                        && self.observation.can_arm);
                if k::button(ui, label, false, enabled)
                    .on_hover_text(format!("緊急停止：{}", self.stop_key()))
                    .clicked()
                {
                    if self.wanted {
                        self.emergency_stop();
                    } else {
                        self.wanted = true;
                        self.backend.controller.enable();
                    }
                }
                ui.add_space(4.);
                let state = self.observation.state.clone();
                k::dot_text(ui, &state, state_color(ui, &state), k::BODY, true);
                ui.add_space(8.);
                let dark = ui.visuals().dark_mode;
                if k::link_colored(ui, if dark { "淺色" } else { "深色" }, p.faint, true)
                    .on_hover_text("切換淺色／深色")
                    .clicked()
                {
                    ui.ctx().set_theme(if dark {
                        egui::ThemePreference::Light
                    } else {
                        egui::ThemePreference::Dark
                    });
                    if let Some(settings) = &mut self.settings {
                        settings.ui.theme = Some(!dark);
                        let _ = self
                            .backend
                            .jobs
                            .send(Job::Preferences(settings.ui.clone()));
                    }
                }
            });
        });
        if let Some(warning) = self.observation.hotkey_warning.clone() {
            self.hotkey_banner(ui, &warning);
        }
        ui.add_space(2.);
        for kind in [FileKind::Points, FileKind::Keys] {
            ui.add_enabled_ui(!self.busy && self.overlay.is_none(), |ui| {
                self.file_row(ui, kind)
            });
        }
        ui.add_space(6.);
        self.tabs(ui);
    }
    fn hotkey_banner(&mut self, ui: &mut egui::Ui, warning: &str) {
        let p = Pal::of(ui);
        ui.add_space(4.);
        egui::Frame::new()
            .fill(p.bad_bg)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(margin(12, 6, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                for line in warning.lines() {
                    ui.label(k::text(line, p.bad, k::BODY));
                }
                if k::link(ui, "前往「預設按鍵」改鍵").clicked() {
                    let role = if self.observation.can_arm {
                        "record"
                    } else {
                        "global_stop"
                    };
                    self.tab = 1;
                    self.selection = Selection::Preset(role.into());
                    if let Some(settings) = &mut self.settings {
                        settings.ui.selected_tab = self.tab;
                        let _ = self
                            .backend
                            .jobs
                            .send(Job::Preferences(settings.ui.clone()));
                    }
                }
            });
    }
    pub(super) fn emergency_stop(&mut self) {
        self.wanted = false;
        self.overlay_resume = false;
        self.backend.controller.disable();
        self.backend.controller.cancel_capture();
        self.capture = None;
        if self.overlay.is_some() {
            self.finish_overlay(false);
        }
    }
    fn stop_key(&self) -> String {
        self.keys.document.presets["global_stop"]
            .hotkey
            .map_or("未設定".into(), |c| c.to_string())
    }
    fn file_row(&mut self, ui: &mut egui::Ui, kind: FileKind) {
        let p = Pal::of(ui);
        let Some(s) = self.settings.clone() else {
            ui.horizontal(|ui| {
                ui.set_height(36.);
                ui.spinner();
            });
            return;
        };
        let (paths, current, dirty) = if kind == FileKind::Points {
            (s.point_tabs, s.current_points, self.points_dirty())
        } else {
            (s.key_tabs, s.current_keys, self.keys_dirty())
        };
        ui.horizontal(|ui| {
            ui.set_height(36.);
            let (rect, _) = ui.allocate_exact_size(vec2(64., 30.), egui::Sense::hover());
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                if kind == FileKind::Points {
                    "點位"
                } else {
                    "預設按鍵"
                },
                egui::FontId::proportional(k::BODY),
                p.muted,
            );
            // File tabs, each closable.
            let tabs_width: f32 = paths
                .iter()
                .map(|path| {
                    k::file_tab_width(ui, &self.tab_label(&paths, path, *path == current && dirty))
                })
                .sum::<f32>()
                + 2.;
            let max = (ui.available_width() * 0.46).max(160.);
            let strip_w = tabs_width.min(max);
            let (strip, _) = ui.allocate_exact_size(vec2(strip_w, 30.), egui::Sense::hover());
            ui.painter()
                .rect_filled(strip, CornerRadius::same(8), p.field);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(strip)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            child.set_clip_rect(strip.shrink(1.));
            let mut draw = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.;
                    for (i, path) in paths.iter().enumerate() {
                        let label = self.tab_label(&paths, path, *path == current && dirty);
                        let missing = self.missing.contains(path);
                        let (open, close) = k::file_tab(
                            ui,
                            &label,
                            *path == current,
                            missing,
                            (i == 0, i + 1 == paths.len()),
                        );
                        let hover = format!(
                            "{}{}",
                            path.display(),
                            if missing {
                                "\n找不到檔案，點一下重新指定位置"
                            } else {
                                ""
                            }
                        );
                        if open.on_hover_text(hover).clicked() {
                            if missing {
                                self.reassign = Some(path.clone());
                                self.job(Job::DialogOpen);
                            } else if *path != current {
                                self.begin(kind, Job::OpenAs(path.clone(), kind, self.wanted));
                            }
                        }
                        if close.on_hover_text("關閉這個分頁").clicked() {
                            self.close_prompt = Some((path.clone(), kind));
                        }
                    }
                });
            };
            if tabs_width > max {
                egui::ScrollArea::horizontal()
                    .id_salt(format!("files-{kind:?}"))
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(&mut child, |ui| draw(ui));
            } else {
                draw(&mut child);
            }
            ui.painter().rect_stroke(
                strip,
                CornerRadius::same(8),
                Stroke::new(1., p.line),
                egui::StrokeKind::Inside,
            );
            ui.add_space(2.);
            if k::button(ui, "新增", false, true).clicked() {
                self.begin(kind, Job::New(kind, self.wanted));
            }
            if k::button(ui, "開啟…", false, true)
                .on_hover_text("任何位置的 JSON 都可以；也可以直接把檔案拖進視窗")
                .clicked()
            {
                self.job(Job::DialogOpen);
            }
            if kind == FileKind::Points {
                let selected = usize::from(self.points.document.mode == Mode::Ten);
                if let Some(i) = k::seg(ui, &["六人", "十人"], selected, 28.) {
                    self.points
                        .set_mode(if i == 0 { Mode::Six } else { Mode::Ten });
                }
            }
            let links = 150.;
            let path_width = (ui.available_width() - links).max(40.);
            ui.allocate_ui_with_layout(
                vec2(path_width, 30.),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(path_width);
                    k::cell(
                        ui,
                        egui::RichText::new(current.display().to_string())
                            .monospace()
                            .size(12.)
                            .color(p.faint),
                    );
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if k::link(ui, "重新載入").clicked() {
                    self.begin(kind, Job::Reload(kind, self.wanted));
                }
                ui.add_space(4.);
                let can = kind == FileKind::Points || self.timing_error.is_none();
                if k::link_colored(ui, "另存新檔…", p.accent, can).clicked() {
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
    fn tab_label(&self, paths: &[PathBuf], path: &PathBuf, dirty: bool) -> String {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let same = paths
            .iter()
            .filter(|p| {
                p.file_name().map(|n| n.to_string_lossy().to_lowercase())
                    == path.file_name().map(|n| n.to_string_lossy().to_lowercase())
            })
            .count()
            > 1;
        let parent = if same {
            let par = path
                .parent()
                .and_then(|p| p.file_name())
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let short: String = par.chars().take(10).collect();
            format!(
                " · {short}{}",
                if par.chars().count() > 10 { "…" } else { "" }
            )
        } else {
            String::new()
        };
        format!(
            "{name}{parent}{}{}",
            if dirty { " *" } else { "" },
            if self.missing.contains(path) {
                "（找不到）"
            } else {
                ""
            }
        )
    }
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        let analysis = analyze(&self.points.document, &self.keys.document);
        ui.horizontal(|ui| {
            ui.set_height(32.);
            ui.spacing_mut().item_spacing.x = 4.;
            for (tab, name) in ["點位", "預設按鍵", "時間", "紀錄"].iter().enumerate() {
                let dot = tab == 1 && !analysis.preset_errors.is_empty();
                if k::pill(ui, name, self.tab == tab as u8, dot).clicked() && self.tab != tab as u8
                {
                    self.tab = tab as u8;
                    self.select_tab();
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
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(s) = &self.settings
                    && self.tab < 3
                {
                    let (kind, path) = if self.tab == 0 {
                        ("點位 JSON", &s.current_points)
                    } else {
                        ("預設按鍵 JSON", &s.current_keys)
                    };
                    ui.label(k::text(
                        format!(
                            "存在{kind}：{}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        p.faint,
                        k::SMALL,
                    ));
                }
            });
        });
    }

    // ---------- bottom ----------
    pub(super) fn save_bar_needed(&self) -> bool {
        self.points_dirty() || self.keys_dirty()
    }
    pub(super) fn save_bar(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        let analysis = analyze(&self.points.document, &self.keys.document);
        let mut blocks = Vec::new();
        if !analysis.preset_errors.is_empty() {
            blocks.push("預設按鍵有重複");
        }
        if self.timing_error.is_some() || self.timing_checks().iter().any(Option::is_some) {
            blocks.push("時間設定有錯");
        }
        let keys_blocked = self.keys_dirty() && !blocks.is_empty();
        let can_save = !keys_blocked
            && self.timing_error.is_none()
            && (!self.keys_dirty() || can_save_keys(&self.points.document, &self.keys.document));
        ui.horizontal(|ui| {
            ui.set_height(32.);
            if keys_blocked {
                ui.label(k::text(
                    format!("預設按鍵 JSON 不能儲存：{}", blocks.join("、")),
                    p.bad,
                    k::BODY,
                ));
            } else {
                let mut names = Vec::new();
                if self.points_dirty() {
                    names.push(self.current_name(FileKind::Points));
                }
                if self.keys_dirty() {
                    names.push(self.current_name(FileKind::Keys));
                }
                ui.spacing_mut().item_spacing.x = 0.;
                for (i, n) in names.iter().enumerate() {
                    if i > 0 {
                        ui.label("、");
                    }
                    ui.label(egui::RichText::new(n).monospace().strong().color(p.ink));
                }
                ui.label(" 有未儲存的變更");
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.;
                if k::button(ui, "儲存", true, can_save && !self.busy).clicked() {
                    self.save(None);
                }
                if k::button(ui, "還原", false, !self.busy).clicked() {
                    self.points = PointSetEditor::new(self.saved_points.clone());
                    self.keys = KeysEditor::new(self.saved_keys.clone());
                    self.timing_draft = self.saved_keys.timing.clone();
                    self.timing_error = None;
                }
            });
        });
    }
    fn current_name(&self, kind: FileKind) -> String {
        self.settings
            .as_ref()
            .map(|s| {
                if kind == FileKind::Points {
                    &s.current_points
                } else {
                    &s.current_keys
                }
            })
            .and_then(|p| p.file_name())
            .map_or_else(
                || {
                    if kind == FileKind::Points {
                        "點位 JSON".into()
                    } else {
                        "預設按鍵 JSON".into()
                    }
                },
                |n| n.to_string_lossy().into_owned(),
            )
    }
    pub(super) fn status_bar(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.set_height(28.);
            ui.spacing_mut().item_spacing.x = 16.;
            let small = |s: String, c: Color32| k::text(s, c, k::SMALL);
            if self.busy {
                ui.spinner();
                ui.label(small("停止並處理檔案中…".into(), p.ink));
            } else if self.capture.is_some()
                || self.overlay.is_some()
                || self.notice.as_ref().is_some_and(|n| !n.expired())
            {
                let c = if self.message.contains("問題：") {
                    p.bad
                } else {
                    p.ink
                };
                k::cell(ui, small(self.message.clone(), c));
            } else {
                let state = &self.observation.state;
                let (c, t) = if state.contains("故障") {
                    (p.bad, "● 引擎故障")
                } else if state == "運作中" {
                    (p.good, "● 引擎正常")
                } else {
                    (p.muted, "● 引擎停止")
                };
                ui.label(small(t.into(), c));
                ui.label(if self.observation.foreground {
                    small("遊戲在前景".into(), p.muted)
                } else {
                    small("遊戲不在前景".into(), p.warn)
                });
                if self.observation.warning {
                    ui.label(small(
                        "遊戲不是全螢幕 16:9，點位和牌鍵會不準".into(),
                        p.warn,
                    ));
                } else if let Some(([w, h], exact)) = self.resolution(&ctx) {
                    ui.label(small(
                        if exact {
                            format!("全螢幕 {w}×{h}（16:9）")
                        } else {
                            format!("螢幕 {w}×{h}")
                        },
                        p.muted,
                    ));
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
                    .and_then(|c| slot_at(&hand.layout(mode), mode.slots(), hand.slot_order, c));
                ui.label(small(
                    format!(
                        "游標：{}",
                        slot.map_or("不在手牌上".into(), |n| format!("第 {} 格", n + 1))
                    ),
                    p.muted,
                ));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(small(format!("{} 緊急停止", self.stop_key()), p.muted));
            });
        });
    }

    // ---------- main ----------
    pub(super) fn main_area(&mut self, ui: &mut egui::Ui) {
        let h = ui.available_height();
        let w = ui.available_width();
        let plan: Vec<f32> = match self.tab {
            0 => vec![210., w - 210. - 290. - 2. * k::GAP, 290.],
            3 => vec![w - 320. - k::GAP, 320.],
            _ => vec![w - 290. - k::GAP, 290.],
        };
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = k::GAP;
            for (i, width) in plan.iter().enumerate() {
                let width = width.max(120.);
                ui.allocate_ui_with_layout(vec2(width, h), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    ui.set_height(h);
                    match (self.tab, i) {
                        (0, 0) => self.candidates_pane(ui, h),
                        (0, 1) => self.items_pane(ui, h),
                        (0, _) => k::inspector(ui, h, |ui| self.point_details(ui)),
                        (1, 0) => self.presets_pane(ui, h),
                        (1, _) => k::inspector(ui, h, |ui| self.preset_details(ui)),
                        (2, 0) => self.timing_pane(ui, h),
                        (2, _) => k::inspector(ui, h, |ui| self.timing_details(ui)),
                        (_, 0) => self.log_pane(ui, h),
                        _ => self.reports_pane(ui, h),
                    }
                });
            }
        });
    }

    // ---------- points ----------
    fn candidates_pane(&mut self, ui: &mut egui::Ui, h: f32) {
        let p = Pal::of(ui);
        let recording = self
            .capture
            .as_ref()
            .is_some_and(|c| c.target == CaptureTarget::Point);
        let mut record = false;
        k::pane_header(
            ui,
            "候選點位",
            &format!("{} 個", self.candidates.len()),
            None,
            |ui| {
                let label = if recording {
                    "錄製中…（Esc 取消）"
                } else {
                    "＋ 錄製"
                };
                let b = egui::Button::new(if recording {
                    egui::RichText::new(label).color(p.accent)
                } else {
                    egui::RichText::new(label)
                })
                .min_size(vec2(0., 28.));
                let b = if recording {
                    b.stroke(Stroke::new(1., p.accent))
                } else {
                    b
                };
                if ui.add_enabled(!recording, b).clicked() {
                    record = true;
                }
            },
        );
        if record {
            self.capture(CaptureTarget::Point);
        }
        let ctx = ui.ctx().clone();
        let candidates = self.candidates.clone();
        let selected = self.selection.clone();
        let mut pick = None;
        k::list_box(ui, "cands", h - 34. - 34., |ui| {
            if candidates.is_empty() {
                k::empty_note(ui, "按「錄製」，到遊戲上點一下\n就會出現在這裡。");
            }
            for (i, c) in candidates.iter().enumerate() {
                let (r, mut cells) = k::table_row(
                    ui,
                    ("cand", i),
                    &CAND_COLS,
                    selected == Selection::Candidate(i),
                    k::ROW_H,
                );
                k::cell(&mut cells[0], k::text(&c.name, p.ink, k::BODY));
                cells[1].with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(k::text(
                        self.px(&ctx, c.position.x, c.position.y),
                        p.muted,
                        k::SMALL,
                    ));
                });
                if r.clicked() {
                    pick = Some(i);
                }
            }
        });
        if let Some(i) = pick {
            self.selection = Selection::Candidate(i);
        }
        ui.add_space(8.);
        if k::link(ui, "從舊檔／MuMu 匯入…").clicked() {
            self.job(Job::DialogImport);
        }
    }
    fn items_pane(&mut self, ui: &mut egui::Ui, h: f32) {
        let p = Pal::of(ui);
        let mut add_card = false;
        k::pane_header(
            ui,
            "點位按鍵清單",
            &format!("{} 個", self.points.document.items.len()),
            None,
            |ui| {
                if k::link(ui, "＋ 牌鍵").clicked() {
                    add_card = true;
                }
            },
        );
        if add_card {
            match self.points.add_card_item("新牌鍵".into()) {
                Ok(id) => {
                    self.selection = Selection::Item(id);
                    self.capture(CaptureTarget::Item(id));
                }
                Err(e) => self.log_error(e.to_string()),
            }
        }
        let ctx = ui.ctx().clone();
        let analysis = analyze(&self.points.document, &self.keys.document);
        let ids = sorted_item_ids(&self.points.document, &self.keys.document);
        let mut pick = None;
        let mut cap = None;
        k::list_box(ui, "items", h - 34., |ui| {
            k::table_head(ui, &ITEM_COLS, &["名稱", "熱鍵", "功能", "位置", "狀態"]);
            if ids.is_empty() {
                k::empty_note(ui, "選一個候選帶入成普通點位，或按「＋ 牌鍵」。");
            }
            for id in ids {
                let Some(item) = selected_item(&self.points.document, id).cloned() else {
                    continue;
                };
                let st = self.item_status(&analysis, &item);
                let (r, mut cells) = k::table_row(
                    ui,
                    ("item", id),
                    &ITEM_COLS,
                    self.selection == Selection::Item(id),
                    k::ROW_H,
                );
                k::cell(
                    &mut cells[0],
                    k::text(
                        if item.name.is_empty() {
                            "（未命名）"
                        } else {
                            &item.name
                        },
                        p.ink,
                        k::BODY,
                    ),
                );
                let target = CaptureTarget::Item(id);
                if self.keycap(
                    &mut cells[1],
                    item.hotkey,
                    target.clone(),
                    st.chip(item.hotkey.is_some()),
                    false,
                ) {
                    cap = Some(target);
                }
                k::tag(
                    &mut cells[2],
                    if item.function == PointFunction::Card {
                        "牌鍵"
                    } else {
                        "普通點位"
                    },
                    item.function == PointFunction::Card,
                );
                k::cell(
                    &mut cells[3],
                    k::text(
                        match item.x.zip(item.y) {
                            Some((x, y)) => self.px(&ctx, x, y),
                            None => "未錄製".to_owned(),
                        },
                        p.muted,
                        k::SMALL,
                    ),
                );
                cells[4]
                    .label(k::text(
                        match st.lvl {
                            Lvl::Ok => "啟用",
                            Lvl::Bad => "停用",
                            Lvl::Off => "未設",
                        },
                        st.color(&p),
                        k::SMALL,
                    ))
                    .on_hover_text(&st.msg);
                if r.clicked() {
                    pick = Some(id);
                }
            }
        });
        if let Some(id) = pick {
            self.selection = Selection::Item(id);
        }
        if let Some(t) = cap {
            if let CaptureTarget::Item(id) = t {
                self.selection = Selection::Item(id);
            }
            self.capture(t);
        }
    }
    /// Keycap for a hotkey; returns true when the user asks to capture a new key.
    fn keycap(
        &self,
        ui: &mut egui::Ui,
        chord: Option<Chord>,
        target: CaptureTarget,
        kind: ChipKind,
        big: bool,
    ) -> bool {
        let capturing = self.capture.as_ref().is_some_and(|c| c.target == target);
        let text = if capturing {
            "按下按鍵…".to_owned()
        } else {
            chord.map_or_else(
                || {
                    if big {
                        "點一下設定".to_owned()
                    } else {
                        "—".to_owned()
                    }
                },
                |c| c.to_string(),
            )
        };
        let kind = if capturing { ChipKind::Capturing } else { kind };
        k::chip(ui, &text, kind, big)
            .on_hover_text("點一下，然後按下要用的按鍵（Esc 取消、Backspace 清除）")
            .clicked()
    }
    fn point_details(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        let ctx = ui.ctx().clone();
        match self.selection.clone() {
            Selection::Candidate(index) => {
                let Some(c) = self.candidates.get(index).cloned() else {
                    k::empty_note(ui, "選一個候選或點位按鍵，這裡會顯示它的設定。");
                    return;
                };
                ui.label(k::text(&c.name, p.ink, 15.).strong());
                let res = self.resolution(&ctx);
                for (label, v, size) in [
                    ("X", c.position.x, res.map(|r| r.0[0])),
                    ("Y", c.position.y, res.map(|r| r.0[1])),
                ] {
                    k::field(ui, label, |ui| {
                        ui.label(match size {
                            Some(s) => format!("{:.0}", v * s),
                            None => format!("{v:.3}"),
                        });
                    });
                }
                k::field(ui, "來源", |ui| {
                    ui.label(k::text("錄製／匯入", p.muted, k::BODY));
                });
                if k::button(ui, "帶入點位按鍵清單 →", true, true).clicked() {
                    match self.points.add_candidate(c) {
                        Ok(id) => {
                            self.candidates.remove(index);
                            self.selection = Selection::Item(id);
                            self.capture(CaptureTarget::Item(id));
                        }
                        Err(e) => self.log_error(e.to_string()),
                    }
                }
                ui.label(k::text(
                    "會變成一個普通點位，接著按下要用的熱鍵。",
                    p.muted,
                    k::SMALL,
                ));
                self.actions(ui, |this, ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if k::danger_link(ui, "刪除候選").clicked() {
                            this.candidates.remove(index);
                            this.selection = Selection::Item(
                                this.points.document.items.first().map_or(0, |i| i.id),
                            );
                        }
                    });
                });
            }
            Selection::Item(id) => {
                let Some(item) = selected_item(&self.points.document, id).cloned() else {
                    k::empty_note(ui, "選一個候選或點位按鍵，這裡會顯示它的設定。");
                    return;
                };
                let analysis = analyze(&self.points.document, &self.keys.document);
                let st = self.item_status(&analysis, &item);
                ui.label(
                    k::text(
                        if item.name.is_empty() {
                            "（未命名）"
                        } else {
                            &item.name
                        },
                        p.ink,
                        15.,
                    )
                    .strong(),
                );
                k::field(ui, "名稱", |ui| {
                    let mut name = item.name.clone();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut name)
                                .desired_width(f32::INFINITY)
                                .margin(vec2(8., 6.)),
                        )
                        .changed()
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
                });
                let mut cap = false;
                k::field(ui, "熱鍵", |ui| {
                    cap = self.keycap(
                        ui,
                        item.hotkey,
                        CaptureTarget::Item(id),
                        st.chip(item.hotkey.is_some()),
                        true,
                    );
                });
                if cap {
                    self.capture(CaptureTarget::Item(id));
                }
                if let Some(mut chord) = item.hotkey {
                    k::field(ui, "", |ui| {
                        let mut exact = matches!(chord.mods, ModsMatch::Exact(_));
                        if ui
                            .checkbox(&mut exact, k::text("修飾鍵必須完全一致", p.ink, k::SMALL))
                            .changed()
                        {
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
                    });
                }
                let mut to_card = false;
                let mut to_point = false;
                k::field(ui, "功能", |ui| {
                    let current = item.function == PointFunction::Card;
                    egui::ComboBox::from_id_salt(("fn", id))
                        .width(ui.available_width())
                        .selected_text(if current { "牌鍵" } else { "普通點位" })
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(!current, "普通點位").clicked() && current {
                                to_point = true;
                            }
                            if ui.selectable_label(current, "牌鍵").clicked() && !current {
                                to_card = true;
                            }
                        });
                });
                if to_card {
                    let _ = self.points.update_item(
                        id,
                        ItemPatch {
                            function: Some(PointFunction::Card),
                            ..Default::default()
                        },
                    );
                }
                if to_point {
                    if item.x.is_some() && item.y.is_some() {
                        let _ = self.points.update_item(
                            id,
                            ItemPatch {
                                function: Some(PointFunction::Point),
                                ..Default::default()
                            },
                        );
                    } else {
                        // A point needs a position: record it on the game screen.
                        self.capture(CaptureTarget::ConvertToPoint(id));
                    }
                }
                match item.x.zip(item.y) {
                    Some((x, y)) => self.position_fields(ui, &ctx, id, x, y),
                    None => {
                        let mut record = false;
                        k::field(ui, "位置", |ui| {
                            record = k::button(ui, "錄製點位", true, true).clicked();
                        });
                        if record {
                            self.capture(CaptureTarget::ReRecord(id));
                        }
                    }
                }
                k::field(ui, "狀態", |ui| {
                    ui.label(k::text(&st.msg, st.color(&p), k::SMALL));
                });
                if item.function == PointFunction::Point {
                    let res = self
                        .resolution(&ctx)
                        .map_or("".into(), |([w, h], _)| format!("（目前 {w}×{h}）"));
                    ui.label(k::text(
                        format!("按住熱鍵：游標移到這個點並按住左鍵；放開熱鍵就放開。X、Y 是全螢幕遊戲畫面上的位置{res}；換解析度不用重錄。"),
                        p.muted,
                        k::SMALL,
                    ));
                    self.actions(ui, |this, ui| {
                        if k::link(ui, "重錄位置").clicked() {
                            this.capture(CaptureTarget::ReRecord(id));
                        }
                        ui.add_space(6.);
                        if k::link(ui, "移回候選").clicked() {
                            match this.points.move_to_candidates(id) {
                                Ok(c) => {
                                    this.candidates.push(c);
                                    this.selection =
                                        Selection::Candidate(this.candidates.len() - 1);
                                }
                                Err(e) => this.log_error(e.to_string()),
                            }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if k::danger_link(ui, "刪除").clicked() {
                                this.delete_item(id);
                            }
                        });
                    });
                } else {
                    ui.label(k::text(
                        "按住熱鍵：游標在手牌第幾張，就送那一格的按鍵，再點一下這個位置，游標接著回到原處；一直重複到放開。游標不在手牌上時暫停。速度跟連點共用「時間」分頁的設定。",
                        p.muted,
                        k::SMALL,
                    ));
                    self.hand_details(ui);
                    self.actions(ui, |this, ui| {
                        if item.x.is_some() && k::link(ui, "重錄位置").clicked() {
                            this.capture(CaptureTarget::ReRecord(id));
                        }
                        ui.add_space(6.);
                        let mode = this.points.document.mode;
                        let side = this.hand.document.card_ui_side;
                        if k::link(ui, "鏡像到另一邊").clicked()
                            && this.hand.mirror(mode, side).is_ok()
                        {
                            this.store_hand();
                        }
                        ui.add_space(6.);
                        if k::link(ui, "還原預設").clicked()
                            && this.hand.reset_layout(mode, side).is_ok()
                        {
                            this.store_hand();
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if k::danger_link(ui, "刪除").clicked() {
                                this.delete_item(id);
                            }
                        });
                    });
                }
            }
            _ => k::empty_note(ui, "選一個候選或點位按鍵，這裡會顯示它的設定。"),
        }
    }
    fn delete_item(&mut self, id: u32) {
        let _ = self.points.delete_item(id);
        self.selection = Selection::Item(self.points.document.items.first().map_or(0, |i| i.id));
    }
    fn actions(&mut self, ui: &mut egui::Ui, add: impl FnOnce(&mut Self, &mut egui::Ui)) {
        ui.add_space(2.);
        k::hr(ui);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.;
            add(self, ui)
        });
    }
    fn position_fields(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, id: u32, x: f64, y: f64) {
        let (scale, decimals, speed) = match self.resolution(ctx) {
            Some(([w, h], _)) => ([w, h], 0, 1.),
            None => ([1., 1.], 3, 0.001),
        };
        let mut v = [x * scale[0], y * scale[1]];
        let mut changed = false;
        for (i, label) in ["X", "Y"].into_iter().enumerate() {
            k::field(ui, label, |ui| {
                changed |= k::num(ui, &mut v[i], speed, 0.0..=scale[i], decimals, false).changed();
            });
        }
        if changed
            && let Err(e) = self.points.update_item(
                id,
                ItemPatch {
                    position: Some(NormPoint {
                        x: v[0] / scale[0],
                        y: v[1] / scale[1],
                    }),
                    ..Default::default()
                },
            )
        {
            self.log(e.to_string());
        }
    }
    fn hand_details(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        let ctx = ui.ctx().clone();
        let mode = self.points.document.mode;
        let side = self.hand.document.card_ui_side;
        k::hr(ui);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.;
            ui.label(
                k::text(
                    if mode == Mode::Ten {
                        "手牌（十人 5 格）"
                    } else {
                        "手牌（六人 3 格）"
                    },
                    p.muted,
                    12.5,
                )
                .strong(),
            );
            ui.label(k::text(
                "這台電腦的設定，所有點位檔共用，改了馬上生效",
                p.faint,
                11.5,
            ));
        });
        k::field(ui, "位置", |ui| {
            let selected = usize::from(side == CardUiSide::Left);
            if let Some(i) = k::seg(ui, &["右邊", "左邊"], selected, 28.) {
                self.hand.set_card_ui_side(if i == 0 {
                    CardUiSide::Right
                } else {
                    CardUiSide::Left
                });
                self.store_hand();
            }
        });
        k::field(ui, "順序", |ui| {
            let rtl = self.hand.document.slot_order == SlotOrder::RightToLeft;
            let mut pick = None;
            egui::ComboBox::from_id_salt("slot-order")
                .width(ui.available_width())
                .selected_text(if rtl {
                    "右到左是第 1 格起"
                } else {
                    "左到右是第 1 格起"
                })
                .show_ui(ui, |ui| {
                    if ui.selectable_label(!rtl, "左到右是第 1 格起").clicked() {
                        pick = Some(SlotOrder::LeftToRight);
                    }
                    if ui.selectable_label(rtl, "右到左是第 1 格起").clicked() {
                        pick = Some(SlotOrder::RightToLeft);
                    }
                });
            if let Some(o) = pick
                && self.hand.set_slot_order(o)
            {
                self.store_hand();
            }
        });
        let r = self.hand.document.layout(mode);
        let (scale, decimals, speed) = match self.resolution(&ctx) {
            Some(([w, h], _)) => ([w, h], 0, 1.),
            None => ([1., 1.], 3, 0.001),
        };
        let mut v = [
            r.x[0] * scale[0],
            r.y[0] * scale[1],
            r.x[1] * scale[0],
            r.y[1] * scale[1],
        ];
        let valid = r.x[1] > r.x[0] && r.y[1] > r.y[0];
        let mut changed = false;
        for (label, a, b) in [("左上", 0, 1), ("右下", 2, 3)] {
            k::field(ui, label, |ui| {
                ui.spacing_mut().item_spacing.x = 6.;
                changed |= k::num(ui, &mut v[a], speed, 0.0..=scale[0], decimals, !valid).changed();
                changed |= k::num(ui, &mut v[b], speed, 0.0..=scale[1], decimals, !valid).changed();
            });
        }
        if changed {
            let rect = HandRect {
                x: [v[0] / scale[0], v[2] / scale[0]],
                y: [v[1] / scale[1], v[3] / scale[1]],
                calibrated: true,
            };
            match self.hand.set_hand_rect(mode, side, rect) {
                Ok(_) => self.store_hand(),
                Err(_) => self.log_error("手牌範圍：右下要比左上大".into()),
            }
        }
        let mut slot_cap = None;
        k::field(ui, "每格", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4., 4.);
                for slot in 0..usize::from(mode.slots()) {
                    ui.label(k::text(format!("{}", slot + 1), p.muted, k::SMALL));
                    let chord = Chord {
                        key: self.hand.document.slot_keys[slot],
                        mods: ModsMatch::AtLeast(0),
                    };
                    if self.keycap(
                        ui,
                        Some(chord),
                        CaptureTarget::Slot(slot),
                        ChipKind::Normal,
                        false,
                    ) {
                        slot_cap = Some(slot);
                    }
                    ui.add_space(4.);
                }
            });
        });
        if let Some(slot) = slot_cap {
            self.capture(CaptureTarget::Slot(slot));
        }
        k::field(ui, "校準", |ui| {
            let (c, t) = if !valid {
                (p.bad, "右下要比左上大")
            } else if r.calibrated {
                (p.good, "已校準")
            } else {
                (p.warn, "預設值，還沒校準")
            };
            ui.label(k::text(t, c, k::SMALL));
            if k::link(ui, "在遊戲上校準").clicked() {
                self.overlay_resume = self.wanted;
                self.job(Job::PrepareOverlay);
            }
        });
    }

    // ---------- presets ----------
    fn presets_pane(&mut self, ui: &mut egui::Ui, h: f32) {
        let p = Pal::of(ui);
        let analysis = analyze(&self.points.document, &self.keys.document);
        let conflicts = PRESET_ROLES
            .iter()
            .filter(|r| self.preset_status(&analysis, r).lvl == Lvl::Bad)
            .count();
        let mut reset = false;
        k::pane_header(
            ui,
            "預設按鍵",
            &if conflicts > 0 {
                format!("{conflicts} 個重複")
            } else {
                format!("{} 個", PRESET_ROLES.len())
            },
            (conflicts > 0).then_some(p.bad),
            |ui| {
                if k::link(ui, "恢復全部預設").clicked() {
                    reset = true;
                }
            },
        );
        if reset {
            self.keys.reset_all();
        }
        let mut pick = None;
        let mut cap = None;
        k::list_box(ui, "presets", h - 34., |ui| {
            k::table_head(ui, &PRESET_COLS, &["功能", "熱鍵", "狀態"]);
            for (group, roles) in preset_groups() {
                k::group_row(ui, group);
                for role in roles {
                    let st = self.preset_status(&analysis, role);
                    let preset = self.keys.document.presets[role].clone();
                    let (r, mut cells) = k::table_row(
                        ui,
                        ("preset", role),
                        &PRESET_COLS,
                        self.selection == Selection::Preset(role.into()),
                        k::ROW_H,
                    );
                    k::cell(
                        &mut cells[0],
                        k::text(
                            role_name(role),
                            if st.lvl == Lvl::Off { p.faint } else { p.ink },
                            k::BODY,
                        ),
                    );
                    let target = CaptureTarget::Preset(role.into());
                    let has = preset.hotkey.is_some();
                    let capturing = self.capture.as_ref().is_some_and(|c| c.target == target);
                    let text = if capturing {
                        "按下按鍵…".to_owned()
                    } else {
                        preset.hotkey.map_or("未設定".into(), |c| c.to_string())
                    };
                    if k::chip(
                        &mut cells[1],
                        &text,
                        if capturing {
                            ChipKind::Capturing
                        } else {
                            st.chip(has)
                        },
                        false,
                    )
                    .clicked()
                    {
                        cap = Some(target);
                    }
                    k::cell(&mut cells[2], k::text(&st.msg, st.color(&p), k::SMALL));
                    if r.clicked() {
                        pick = Some(role);
                    }
                }
            }
        });
        if let Some(role) = pick {
            self.selection = Selection::Preset(role.into());
        }
        if let Some(t) = cap {
            if let CaptureTarget::Preset(role) = &t {
                self.selection = Selection::Preset(role.clone());
            }
            self.capture(t);
        }
    }
    fn preset_details(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        let Selection::Preset(role) = self.selection.clone() else {
            k::empty_note(ui, "選一個預設按鍵。");
            return;
        };
        let analysis = analyze(&self.points.document, &self.keys.document);
        let st = self.preset_status(&analysis, &role);
        let preset = self.keys.document.presets[&role].clone();
        ui.label(k::text(role_name(&role), p.ink, 15.).strong());
        ui.label(k::text(role_description(&role), p.muted, k::SMALL));
        let mut cap = false;
        k::field(ui, "熱鍵", |ui| {
            cap = self.keycap(
                ui,
                preset.hotkey,
                CaptureTarget::Preset(role.clone()),
                st.chip(preset.hotkey.is_some()),
                true,
            );
        });
        if cap {
            self.capture(CaptureTarget::Preset(role.clone()));
        }
        let mut enabled = preset.enabled;
        let mut exact = preset
            .hotkey
            .is_some_and(|h| matches!(h.mods, ModsMatch::Exact(_)));
        let mut changed = false;
        k::field(ui, "啟用", |ui| {
            if role == "global_stop" {
                ui.label(k::text("一定啟用", p.muted, k::SMALL));
            } else {
                changed |= k::switch(ui, &mut enabled, true).changed();
            }
        });
        k::field(ui, "", |ui| {
            changed |= ui
                .add_enabled(
                    preset.hotkey.is_some(),
                    egui::Checkbox::new(&mut exact, k::text("修飾鍵必須完全一致", p.ink, k::SMALL)),
                )
                .changed();
        });
        if changed && let Err(e) = self.keys.set_preset(&role, preset.hotkey, enabled, exact) {
            self.log(e.to_string());
        }
        k::field(ui, "狀態", |ui| {
            ui.label(k::text(&st.msg, st.color(&p), k::SMALL));
        });
        let default = template_keys().presets[&role]
            .hotkey
            .map_or("—".into(), |c| c.to_string());
        self.actions(ui, |this, ui| {
            ui.label(k::text(format!("預設：{default}"), p.muted, k::SMALL));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if k::link(ui, "恢復這一項").clicked() {
                    let _ = this.keys.reset_preset(&role);
                }
            });
        });
    }

    // ---------- timing ----------
    fn timing_pane(&mut self, ui: &mut egui::Ui, h: f32) {
        let p = Pal::of(ui);
        k::pane_header(ui, "時間", "ms", None, |_| {});
        let checks = self.timing_checks();
        let values = timing_fields(&self.timing_draft);
        let mut edit = None;
        let mut pick = None;
        let width = ui.available_width();
        let cols = [0., 92., ((width - 92. - 40.) * 0.48).max(80.)];
        k::list_box(ui, "timing", h - 34., |ui| {
            k::table_head(ui, &cols, &["項目", "數值", "檢查"]);
            for (index, (meta, (_, value))) in TIMING.iter().zip(values.iter()).enumerate() {
                let (r, mut cells) = k::table_row(
                    ui,
                    ("tm", index),
                    &cols,
                    self.selection == Selection::Timing(index),
                    k::ROW_H,
                );
                k::cell(&mut cells[0], k::text(meta.name, p.ink, k::BODY));
                let mut v = *value;
                if k::num(
                    &mut cells[1],
                    &mut v,
                    1.,
                    0.0..=100_000.0,
                    2,
                    checks[index].is_some(),
                )
                .changed()
                {
                    edit = Some((index, v));
                }
                match &checks[index] {
                    Some(e) => k::cell(&mut cells[2], k::text(e, p.bad, k::SMALL)),
                    None => k::cell(&mut cells[2], k::text("—", p.faint, k::SMALL)),
                };
                if r.clicked() {
                    pick = Some(index);
                }
            }
        });
        if let Some(i) = pick {
            self.selection = Selection::Timing(i);
        }
        if let Some((index, value)) = edit {
            self.selection = Selection::Timing(index);
            self.edit_timing(index, value);
        }
    }
    fn edit_timing(&mut self, index: usize, value: f64) {
        let mut timing = self.timing_draft.clone();
        set_timing_field(&mut timing, index, value);
        self.timing_draft = timing.clone();
        self.timing_error = self.keys.set_timing(timing).err().map(|e| e.to_string());
        if self.timing_error.is_none() {
            self.timing_draft = self.keys.document.timing.clone();
        }
    }
    fn timing_details(&mut self, ui: &mut egui::Ui) {
        let p = Pal::of(ui);
        let t = self.keys.document.timing.clone();
        ui.label(k::text("這組時間", p.ink, 15.).strong());
        k::field(ui, "驗證", |ui| {
            if t.verified {
                ui.label(k::text("已驗證", p.good, k::SMALL));
            } else {
                ui.label(k::text("未驗證", p.warn, k::SMALL));
            }
        });
        k::field(ui, "來源", |ui| {
            ui.label(k::text(
                match t.source {
                    ProfileSource::ImportedLegacy => "範本／舊版匯入",
                    ProfileSource::Calibrated => "校準結果",
                    ProfileSource::Manual => "手動填寫",
                },
                p.muted,
                k::SMALL,
            ));
        });
        if !t.verified {
            ui.label(k::text(
                "可以用，但不保證是最快、最穩的設定。跑完校準後載入結果就會變成已驗證；之後再改任何數值，又會變回未驗證。",
                p.muted,
                k::SMALL,
            ));
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.;
            if k::link(ui, "載入校準結果…").clicked() {
                self.job(Job::LoadTiming);
            }
            if k::link(ui, "還原範本值").clicked() {
                if let Err(e) = self.keys.set_timing(template_keys().timing) {
                    self.log(e.to_string());
                }
                self.timing_draft = self.keys.document.timing.clone();
                self.timing_error = None;
            }
        });
        let mut repeat = self.timing_draft.auto_repeat == AutoRepeatPolicy::FollowTypematic;
        let mut trailing = self.timing_draft.card_trailing_esc == TrailingEscPolicy::IfStarted;
        let changed = ui
            .checkbox(
                &mut repeat,
                k::text("按住技能鍵時跟著鍵盤自動重複", p.ink, k::SMALL),
            )
            .changed()
            | ui.checkbox(
                &mut trailing,
                k::text("卡幀開牌放開時再送一次 Esc", p.ink, k::SMALL),
            )
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
        let Selection::Timing(index) = self.selection else {
            return;
        };
        let Some(meta) = TIMING.get(index) else {
            return;
        };
        k::hr(ui);
        ui.label(k::text(meta.name, p.ink, 15.).strong());
        ui.label(
            egui::RichText::new(meta.json)
                .monospace()
                .size(11.5)
                .color(p.faint),
        );
        ui.label(k::text(meta.purpose, p.ink, k::SMALL));
        if !meta.rule.is_empty() {
            ui.label(k::text(meta.rule, p.muted, k::SMALL));
        }
        if let Some(e) = &self.timing_checks()[index] {
            ui.label(k::text(e, p.bad, k::SMALL));
        } else if let Some(e) = &self.timing_error {
            ui.label(k::text(format!("時間設定有錯：{e}"), p.bad, k::SMALL));
        }
        if t.verified {
            let id = &t.identity;
            ui.label(k::text(
                format!(
                    "校準條件：{} · {} · {} · {}",
                    id.game_version, id.window_mode, id.fps, id.machine
                ),
                p.faint,
                11.5,
            ));
        }
    }

    // ---------- log ----------
    fn log_pane(&mut self, ui: &mut egui::Ui, h: f32) {
        let p = Pal::of(ui);
        k::pane_header(ui, "事件", &format!("{} 筆", self.logs.len()), None, |_| {});
        let logs = self.logs.clone();
        let mut pick = None;
        k::list_box(ui, "logs", h - 34., |ui| {
            if logs.is_empty() {
                k::empty_note(ui, "還沒有事件。");
            }
            for (index, line) in logs.iter().enumerate().rev() {
                let (r, mut cells) = k::table_row(
                    ui,
                    ("log", index),
                    &LOG_COLS,
                    self.selection == Selection::Log(index),
                    32.,
                );
                cells[0].label(k::text(format!("{}", index + 1), p.faint, k::SMALL));
                let (text, qpc) = split_qpc(line);
                let problem = text.contains("問題：") || text.contains("故障");
                cells[1].spacing_mut().item_spacing.x = 8.;
                let (dot, _) = cells[1].allocate_exact_size(vec2(7., 7.), egui::Sense::hover());
                cells[1].painter().circle_filled(
                    dot.center(),
                    3.5,
                    if problem { p.bad } else { p.faint },
                );
                let resp = k::cell(&mut cells[1], k::text(text, p.ink, k::BODY));
                if let Some(q) = qpc {
                    resp.on_hover_text(format!("QPC {q}"));
                }
                if r.clicked() {
                    pick = Some(index);
                }
            }
        });
        if let Some(i) = pick {
            self.selection = Selection::Log(i);
        }
    }
    fn reports_pane(&mut self, ui: &mut egui::Ui, h: f32) {
        let p = Pal::of(ui);
        let mut open_dir = false;
        k::pane_header(ui, "報告與故障紀錄", "", None, |ui| {
            if k::link(ui, "重新整理").clicked() {
                let _ = self.backend.jobs.send(Job::Reports);
            }
            ui.add_space(6.);
            if k::link(ui, "開啟資料夾").clicked() {
                open_dir = true;
            }
        });
        if open_dir {
            let _ = self
                .backend
                .jobs
                .send(Job::OpenExternal(self.logs_dir.clone()));
        }
        let reports = self.reports.clone();
        k::list_box(ui, "reports", h - 34., |ui| {
            if reports.is_empty() {
                k::empty_note(ui, "還沒有報告。校準、量測或故障時會出現在這裡。");
            }
            for (i, path) in reports.iter().enumerate() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let kind = if name.starts_with("cal") {
                    "校準"
                } else if name.starts_with("fault") {
                    "故障"
                } else if name.starts_with("latency") || name.starts_with("bench") {
                    "量測"
                } else if name.starts_with("receiver") {
                    "接收器"
                } else {
                    "紀錄"
                };
                let (_, mut cells) = k::table_row(ui, ("rep", i), &[0., 40.], false, 44.);
                cells[0].vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    k::cell(
                        ui,
                        egui::RichText::new(name.as_ref())
                            .monospace()
                            .size(12.)
                            .color(p.ink),
                    );
                    ui.label(k::text(kind, p.muted, k::SMALL));
                });
                if k::link(&mut cells[1], "開啟")
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    let _ = self.backend.jobs.send(Job::OpenExternal(path.clone()));
                }
            }
        });
    }
}

impl App {
    // ---------- dialogs ----------
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(kind) = self.pending.as_ref().map(|p| p.kind) {
            let closing = self.pending.as_ref().is_some_and(|p| p.closing);
            let can_save = (kind == FileKind::Points && !closing)
                || (self.timing_error.is_none()
                    && can_save_keys(&self.points.document, &self.keys.document));
            let name = self.current_name(kind);
            let mut choice = None;
            let esc = dialog(ctx, "unsaved", "有未儲存的變更", |ui, p| {
                ui.label(k::text(
                    format!(
                        "{name}（{}）還沒儲存。",
                        if kind == FileKind::Points {
                            "點位 JSON"
                        } else {
                            "預設按鍵 JSON"
                        }
                    ),
                    p.muted,
                    k::BODY,
                ));
                buttons(ui, |ui| {
                    if k::button(
                        ui,
                        if closing {
                            "儲存後關閉"
                        } else {
                            "儲存後切換"
                        },
                        true,
                        can_save,
                    )
                    .clicked()
                    {
                        choice = Some(0);
                    }
                    if k::button(ui, "不儲存", false, true).clicked() {
                        choice = Some(1);
                    }
                    if k::button(ui, "取消", false, true).clicked() {
                        choice = Some(2);
                    }
                });
            });
            if esc && choice.is_none() {
                choice = Some(2);
            }
            match choice {
                Some(0) => {
                    let pending = self.pending.take().unwrap();
                    self.next_job = Some(pending.job);
                    self.save(if pending.closing { None } else { Some(kind) });
                }
                Some(1) => {
                    let pending = self.pending.take().unwrap();
                    if pending.closing {
                        self.quit();
                    } else {
                        self.job(pending.job);
                    }
                }
                Some(_) => self.pending = None,
                None => {}
            }
        }
        if let Some((path, kind)) = self.close_prompt.clone() {
            let dirty = (kind == FileKind::Points && self.points_dirty())
                || (kind == FileKind::Keys && self.keys_dirty());
            let count = self.settings.as_ref().map_or(0, |s| {
                if kind == FileKind::Points {
                    s.point_tabs.len()
                } else {
                    s.key_tabs.len()
                }
            });
            let mut choice = None;
            let title = format!(
                "關閉「{}」",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            let esc = dialog(ctx, "close-tab", &title, |ui, p| {
                ui.label(
                    egui::RichText::new(path.display().to_string())
                        .monospace()
                        .size(12.)
                        .color(p.muted),
                );
                if dirty {
                    ui.label(k::text(
                        "這個檔案有未儲存的變更，會先問你要不要儲存。",
                        p.warn,
                        k::SMALL,
                    ));
                }
                if count <= 1 {
                    ui.label(k::text(
                        "每一列至少要留一個分頁，先開啟或新增另一個檔案再關。",
                        p.bad,
                        k::SMALL,
                    ));
                } else {
                    ui.label(k::text(
                        "關閉分頁：檔案還在原本的位置。\n刪除檔案：移到同一個資料夾的 .trash，需要時可以拿回來。",
                        p.muted,
                        k::SMALL,
                    ));
                }
                buttons(ui, |ui| {
                    if k::button(ui, "取消", false, true).clicked() {
                        choice = Some(0);
                    }
                    if ui
                        .add_enabled(
                            count > 1,
                            egui::Button::new(egui::RichText::new("刪除檔案").color(p.bad))
                                .min_size(vec2(0., 28.)),
                        )
                        .clicked()
                    {
                        choice = Some(2);
                    }
                    if k::button(ui, "關閉分頁", true, count > 1).clicked() {
                        choice = Some(1);
                    }
                });
            });
            if esc && choice.is_none() {
                choice = Some(0);
            }
            match choice {
                Some(0) => self.close_prompt = None,
                Some(c) => {
                    self.close_prompt = None;
                    self.begin(kind, Job::Close(path.clone(), c == 2, self.wanted));
                }
                None => {}
            }
        }
        if let Some((path, reason)) = self.import.clone() {
            let mut choice = None;
            let esc = dialog(ctx, "import", "轉換舊版檔案", |ui, p| {
                ui.label(
                    egui::RichText::new(path.display().to_string())
                        .monospace()
                        .size(12.)
                        .color(p.muted),
                );
                if reason != "import" {
                    ui.label(k::text(&reason, p.ink, k::SMALL));
                }
                ui.label(k::text(
                    "轉換後另存成新檔，放在原檔旁邊，原檔不會被改。",
                    p.muted,
                    k::SMALL,
                ));
                k::field(ui, "編成", |ui| {
                    let selected = usize::from(self.import_mode == Mode::Ten);
                    if let Some(i) = k::seg(ui, &["六人", "十人"], selected, 28.) {
                        self.import_mode = if i == 0 { Mode::Six } else { Mode::Ten };
                    }
                });
                k::field(ui, "解析度", |ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.import_resolution[0]).range(0.0..=100_000.0),
                    );
                    ui.label("×");
                    ui.add(
                        egui::DragValue::new(&mut self.import_resolution[1]).range(0.0..=100_000.0),
                    );
                });
                ui.label(k::text(
                    "只有舊的絕對座標需要填原本的解析度。",
                    p.faint,
                    k::SMALL,
                ));
                buttons(ui, |ui| {
                    if k::button(ui, "取消", false, true).clicked() {
                        choice = Some(false);
                    }
                    if k::button(ui, "轉換並開啟", true, true).clicked() {
                        choice = Some(true);
                    }
                });
            });
            if esc && choice.is_none() {
                choice = Some(false);
            }
            match choice {
                Some(true) => {
                    let resolution = (self.import_resolution.iter().all(|v| *v > 0.))
                        .then_some(self.import_resolution);
                    self.import = None;
                    self.job(Job::PlanImport {
                        path,
                        mode: self.import_mode,
                        resolution,
                    });
                }
                Some(false) => self.import = None,
                None => {}
            }
        }
    }
}

/// Centered modal with a dimmed backdrop (the demo's `.dlg`). Returns true when the
/// user pressed Esc or clicked outside, which callers treat as cancel.
fn dialog(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    add: impl FnOnce(&mut egui::Ui, &Pal),
) -> bool {
    let dark = ctx.theme() == egui::Theme::Dark;
    let p = if dark { k::DARK } else { k::LIGHT };
    let modal = egui::Modal::new(egui::Id::new(("dialog", id)))
        .backdrop_color(Color32::from_black_alpha(if dark { 140 } else { 96 }))
        .frame(
            egui::Frame::new()
                .fill(p.bg)
                .stroke(Stroke::new(1., p.line))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(16)),
        );
    modal
        .show(ctx, |ui| {
            ui.set_width(460.);
            ui.spacing_mut().item_spacing.y = 8.;
            ui.label(k::text(title, p.ink, 15.).strong());
            add(ui, &p);
        })
        .should_close()
}
fn buttons(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(6.);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.;
        add(ui)
    });
}

fn split_qpc(line: &str) -> (&str, Option<&str>) {
    if let Some(rest) = line.strip_prefix("QPC ")
        && let Some((q, text)) = rest.split_once("  ")
    {
        return (text, Some(q));
    }
    (line, None)
}

pub(super) fn preset_groups() -> [(&'static str, Vec<&'static str>); 4] {
    let pick = |f: &dyn Fn(&str) -> bool| PRESET_ROLES.iter().copied().filter(|r| f(r)).collect();
    [
        ("技能", pick(&|r| r.starts_with("skill_"))),
        ("技能連點", pick(&|r| r.starts_with("spam_"))),
        ("卡幀開牌", pick(&|r| r.starts_with("card_"))),
        (
            "其他",
            pick(&|r| {
                !r.starts_with("skill_") && !r.starts_with("spam_") && !r.starts_with("card_")
            }),
        ),
    ]
}

pub(super) fn state_color(ui: &egui::Ui, state: &str) -> Color32 {
    let p = Pal::of(ui);
    if state.contains("故障") || state.contains("Faulted") {
        p.bad
    } else if state == "運作中" {
        p.good
    } else if state == "已停止" {
        p.faint
    } else {
        p.warn
    }
}

pub(super) fn margin(x: i8, top: i8, bottom: i8) -> Margin {
    Margin {
        left: x,
        right: x,
        top,
        bottom,
    }
}
