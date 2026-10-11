//! Look-and-feel for the three-column layout in docs/pages/BAauto-新介面Demo.html.
//! Pure drawing helpers: no App state, no file or IPC work.
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Layout, Margin, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};

/// Color tokens copied from the demo page (light / dark).
#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,
    pub soft: Color32,
    pub field: Color32,
    pub line: Color32,
    pub line2: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_ink: Color32,
    pub sel: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub bad_bg: Color32,
}
const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}
pub const LIGHT: Pal = Pal {
    bg: hex(0xFFFFFF),
    soft: hex(0xF5F6F8),
    field: hex(0xFFFFFF),
    line: hex(0xD8DCE1),
    line2: hex(0xECEEF1),
    ink: hex(0x1C1F23),
    muted: hex(0x646A73),
    faint: hex(0x9AA0A8),
    accent: hex(0x0067C0),
    accent_ink: hex(0xFFFFFF),
    sel: hex(0xE1EEFA),
    good: hex(0x16833A),
    warn: hex(0x946200),
    bad: hex(0xC42B1C),
    bad_bg: hex(0xFDECEC),
};
pub const DARK: Pal = Pal {
    bg: hex(0x222326),
    soft: hex(0x292B2F),
    field: hex(0x1A1B1E),
    line: hex(0x3C3F45),
    line2: hex(0x303237),
    ink: hex(0xE9EAEC),
    muted: hex(0xA8ADB5),
    faint: hex(0x747982),
    accent: hex(0x5AB4F5),
    accent_ink: hex(0x06223A),
    sel: hex(0x1E3850),
    good: hex(0x5CC27A),
    warn: hex(0xE3B341),
    bad: hex(0xFF8A80),
    bad_bg: hex(0x3B2324),
};
impl Pal {
    pub fn of(ui: &Ui) -> Self {
        if ui.visuals().dark_mode { DARK } else { LIGHT }
    }
}

pub const BODY: f32 = 13.5;
pub const SMALL: f32 = 12.;
pub const MONO: f32 = 12.5;
pub const ROW_H: f32 = 34.;
pub const GAP: f32 = 14.;

pub fn install_style(ctx: &egui::Context) {
    for (theme, p) in [(egui::Theme::Light, LIGHT), (egui::Theme::Dark, DARK)] {
        ctx.style_mut_of(theme, |s| {
            s.interaction.selectable_labels = false;
            s.spacing.item_spacing = vec2(8., 6.);
            s.spacing.button_padding = vec2(12., 5.);
            s.spacing.interact_size.y = 28.;
            s.spacing.combo_width = 120.;
            for (style, size, family) in [
                (egui::TextStyle::Body, BODY, egui::FontFamily::Proportional),
                (
                    egui::TextStyle::Button,
                    BODY,
                    egui::FontFamily::Proportional,
                ),
                (
                    egui::TextStyle::Small,
                    SMALL,
                    egui::FontFamily::Proportional,
                ),
                (
                    egui::TextStyle::Monospace,
                    MONO,
                    egui::FontFamily::Monospace,
                ),
                (
                    egui::TextStyle::Heading,
                    15.,
                    egui::FontFamily::Proportional,
                ),
            ] {
                s.text_styles.insert(style, FontId::new(size, family));
            }
            let v = &mut s.visuals;
            v.panel_fill = p.bg;
            v.window_fill = p.bg;
            v.window_stroke = Stroke::new(1., p.line);
            v.window_corner_radius = CornerRadius::same(10);
            v.extreme_bg_color = p.field;
            v.text_edit_bg_color = Some(p.field);
            v.faint_bg_color = p.soft;
            v.hyperlink_color = p.accent;
            v.error_fg_color = p.bad;
            v.warn_fg_color = p.warn;
            v.selection.bg_fill = p.sel;
            v.selection.stroke = Stroke::new(1., p.ink);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1., p.ink);
            v.widgets.noninteractive.bg_stroke = Stroke::new(1., p.line);
            for w in [
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.corner_radius = CornerRadius::same(4);
                w.fg_stroke = Stroke::new(1., p.ink);
                w.expansion = 0.;
            }
            v.widgets.inactive.bg_fill = p.field;
            v.widgets.inactive.weak_bg_fill = p.field;
            v.widgets.inactive.bg_stroke = Stroke::new(1., p.line);
            v.widgets.hovered.bg_fill = p.soft;
            v.widgets.hovered.weak_bg_fill = p.soft;
            v.widgets.hovered.bg_stroke = Stroke::new(1., p.faint);
            v.widgets.active.bg_fill = p.sel;
            v.widgets.active.weak_bg_fill = p.sel;
            v.widgets.active.bg_stroke = Stroke::new(1., p.accent);
            v.widgets.open.bg_fill = p.soft;
            v.widgets.open.weak_bg_fill = p.soft;
            v.widgets.open.bg_stroke = Stroke::new(1., p.accent);
        });
    }
}

pub fn text(s: impl Into<String>, color: Color32, size: f32) -> RichText {
    RichText::new(s).color(color).size(size)
}

/// Single-line label that never wraps; long text is cut with an ellipsis and the
/// full text is available on hover.
pub fn cell(ui: &mut Ui, rich: RichText) -> Response {
    ui.add(egui::Label::new(rich).truncate())
}

/// Plain text button in the accent color (the demo's `.lnk`).
pub fn link(ui: &mut Ui, s: &str) -> Response {
    link_colored(ui, s, Pal::of(ui).accent, true)
}
pub fn danger_link(ui: &mut Ui, s: &str) -> Response {
    link_colored(ui, s, Pal::of(ui).bad, true)
}
pub fn link_colored(ui: &mut Ui, s: &str, color: Color32, enabled: bool) -> Response {
    let p = Pal::of(ui);
    let color = if enabled { color } else { p.faint };
    let galley = ui
        .painter()
        .layout_no_wrap(s.to_owned(), FontId::proportional(BODY), color);
    let (rect, r) = ui.allocate_exact_size(
        vec2(galley.size().x, galley.size().y.max(22.)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let pos = pos2(rect.left(), rect.center().y - galley.size().y / 2.);
    if r.hovered() && enabled {
        let y = pos.y + galley.size().y - 1.;
        ui.painter().line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1., color),
        );
    }
    ui.painter().galley(pos, galley, color);
    if enabled {
        r.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        r
    }
}

/// Bordered button (the demo's `.btn`); `primary` uses the accent fill.
pub fn button(ui: &mut Ui, s: &str, primary: bool, enabled: bool) -> Response {
    let p = Pal::of(ui);
    let mut b = egui::Button::new(if primary {
        RichText::new(s).color(p.accent_ink).strong()
    } else {
        RichText::new(s)
    })
    .min_size(vec2(0., 28.))
    .corner_radius(CornerRadius::same(6));
    if primary {
        b = b.fill(p.accent).stroke(Stroke::new(1., p.accent));
    }
    ui.add_enabled(enabled, b)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    Normal,
    Empty,
    Bad,
    Off,
    Capturing,
}
/// Keycap (the demo's `.chip`): mono text, thicker bottom edge, dashed when empty.
pub fn chip(ui: &mut Ui, s: &str, kind: ChipKind, big: bool) -> Response {
    let p = Pal::of(ui);
    let (fg, fill, stroke) = match kind {
        ChipKind::Normal => (p.ink, p.field, p.line),
        ChipKind::Empty => (p.faint, p.field, p.line),
        ChipKind::Bad => (p.bad, p.bad_bg, p.bad),
        ChipKind::Off => (p.faint, p.field, p.line),
        ChipKind::Capturing => (p.ink, p.sel, p.accent),
    };
    let font = if matches!(kind, ChipKind::Empty | ChipKind::Capturing) {
        FontId::proportional(if big { 13. } else { SMALL })
    } else {
        FontId::monospace(if big { 13. } else { SMALL })
    };
    let galley = ui.painter().layout_no_wrap(s.to_owned(), font, fg);
    let h = if big { 32. } else { 26. };
    let w = (galley.size().x + 18.).max(if big { 64. } else { 34. });
    let (rect, r) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let stroke = if r.hovered() && kind != ChipKind::Bad {
        p.accent
    } else {
        stroke
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(6),
        fill,
        Stroke::NONE,
        StrokeKind::Inside,
    );
    if kind == ChipKind::Empty {
        dashed_rect(painter, rect.shrink(0.5), Stroke::new(1., stroke));
    } else {
        painter.rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1., stroke),
            StrokeKind::Inside,
        );
        if kind != ChipKind::Capturing {
            // Thicker bottom edge like a physical key.
            painter.line_segment(
                [
                    pos2(rect.left() + 4., rect.bottom() - 1.),
                    pos2(rect.right() - 4., rect.bottom() - 1.),
                ],
                Stroke::new(2., stroke),
            );
        }
    }
    painter.galley(
        rect.center() - galley.size() / 2. - vec2(0., 0.5),
        galley,
        fg,
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn dashed_rect(painter: &egui::Painter, r: Rect, stroke: Stroke) {
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
            painter.line_segment([a + dir * at, a + dir * (at + 3.).min(len)], stroke);
            at += 6.;
        }
    }
}

/// Segmented control (the demo's `.seg`). Returns the clicked index.
pub fn seg(ui: &mut Ui, options: &[&str], selected: usize, height: f32) -> Option<usize> {
    let p = Pal::of(ui);
    let font = FontId::proportional(if height < 28. { 12.5 } else { BODY });
    let widths: Vec<f32> = options
        .iter()
        .map(|s| {
            ui.painter()
                .layout_no_wrap((*s).to_owned(), font.clone(), p.ink)
                .size()
                .x
                + 22.
        })
        .collect();
    let (rect, _) = ui.allocate_exact_size(vec2(widths.iter().sum(), height), Sense::hover());
    let mut clicked = None;
    let mut x = rect.left();
    for (i, w) in widths.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, height));
        let resp = ui
            .interact(r, ui.id().with(("seg", options[0], i)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = i == selected;
        let fill = if on {
            p.sel
        } else if resp.hovered() {
            p.soft
        } else {
            p.field
        };
        let radius = CornerRadius {
            nw: if i == 0 { 6 } else { 0 },
            sw: if i == 0 { 6 } else { 0 },
            ne: if i + 1 == options.len() { 6 } else { 0 },
            se: if i + 1 == options.len() { 6 } else { 0 },
        };
        ui.painter().rect_filled(r, radius, fill);
        if i > 0 {
            ui.painter().line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1., p.line),
            );
        }
        let color = if on { p.ink } else { p.muted };
        let galley = ui
            .painter()
            .layout_no_wrap(options[i].to_owned(), font.clone(), color);
        ui.painter()
            .galley(r.center() - galley.size() / 2., galley, color);
        if on {
            // Fake bold: second pass offset by a fraction of a pixel.
            let galley = ui
                .painter()
                .layout_no_wrap(options[i].to_owned(), font.clone(), color);
            ui.painter().galley(
                r.center() - galley.size() / 2. + vec2(0.4, 0.),
                galley,
                color,
            );
        }
        if resp.clicked() {
            clicked = Some(i);
        }
        x += w;
    }
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1., p.line),
        StrokeKind::Inside,
    );
    clicked
}

/// Pill tab (the demo's `.ctabs button`).
pub fn pill(ui: &mut Ui, s: &str, selected: bool, dot: bool) -> Response {
    let p = Pal::of(ui);
    let color = if selected { p.bg } else { p.muted };
    let galley = ui
        .painter()
        .layout_no_wrap(s.to_owned(), FontId::proportional(BODY), color);
    let extra = if dot { 11. } else { 0. };
    let (rect, r) =
        ui.allocate_exact_size(vec2(galley.size().x + 24. + extra, 30.), Sense::click());
    let fill = if selected {
        p.ink
    } else if r.hovered() {
        p.soft
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(15), fill);
    let pos = pos2(rect.left() + 12., rect.center().y - galley.size().y / 2.);
    let w = galley.size().x;
    ui.painter().galley(pos, galley, color);
    if dot {
        ui.painter()
            .circle_filled(pos2(pos.x + w + 8., rect.center().y), 3., p.bad);
    }
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// iOS-like switch (the demo's `.sw`).
pub fn switch(ui: &mut Ui, on: &mut bool, enabled: bool) -> Response {
    let p = Pal::of(ui);
    let (rect, mut r) = ui.allocate_exact_size(
        vec2(30., 18.),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    let t = ui.ctx().animate_bool(r.id, *on);
    let fill = if *on { p.accent } else { p.line };
    let fill = if enabled {
        fill
    } else {
        fill.gamma_multiply(0.5)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    let x = egui::lerp(rect.left() + 9.0..=rect.right() - 9., t);
    ui.painter()
        .circle_filled(pos2(x, rect.center().y), 7., Color32::WHITE);
    r
}

/// Colored dot followed by text (state indicators). Works in any layout direction.
pub fn dot_text(ui: &mut Ui, s: &str, color: Color32, size: f32, strong: bool) {
    let font = FontId::proportional(size);
    let galley = ui
        .painter()
        .layout_no_wrap(s.to_owned(), font.clone(), color);
    let (rect, _) = ui.allocate_exact_size(vec2(14. + galley.size().x, 22.), Sense::hover());
    ui.painter()
        .circle_filled(pos2(rect.left() + 4., rect.center().y), 4., color);
    let pos = pos2(rect.left() + 14., rect.center().y - galley.size().y / 2.);
    if strong {
        let again = ui.painter().layout_no_wrap(s.to_owned(), font, color);
        ui.painter().galley(pos + vec2(0.45, 0.), again, color);
    }
    ui.painter().galley(pos, galley, color);
}

/// Pane header row: bold title, faint count, then right-aligned extras.
pub fn pane_header(
    ui: &mut Ui,
    title: &str,
    count: &str,
    count_color: Option<Color32>,
    right: impl FnOnce(&mut Ui),
) {
    let p = Pal::of(ui);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 34.),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(text(title, p.ink, BODY + 0.5).strong());
            if !count.is_empty() {
                ui.label(text(count, count_color.unwrap_or(p.faint), SMALL));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), right);
        },
    );
}

/// Bordered list area (the demo's `.list`) filling `height`.
pub fn list_box<R>(ui: &mut Ui, id: &str, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Pal::of(ui);
    let width = ui.available_width();
    egui::Frame::new()
        .fill(p.field)
        .stroke(Stroke::new(1., p.line))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(1))
        .show(ui, |ui| {
            ui.set_width(width - 2.);
            ui.set_min_height(height - 2.);
            ui.set_max_height(height - 2.);
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .max_height(height - 2.)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    add(ui)
                })
                .inner
        })
        .inner
}

/// Right-hand details panel (the demo's `.insp`).
pub fn inspector<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Pal::of(ui);
    let width = ui.available_width();
    egui::Frame::new()
        .fill(p.soft)
        .stroke(Stroke::new(1., p.line))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(width - 30.);
            ui.set_min_height(height - 30.);
            egui::ScrollArea::vertical()
                .id_salt("inspector")
                .auto_shrink([false, false])
                .max_height(height - 30.)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 10.;
                    add(ui)
                })
                .inner
        })
        .inner
}

/// Column layout for a table row. `0.0` marks the single flexible column.
pub fn columns(rect: Rect, spec: &[f32], pad: f32, gap: f32) -> Vec<Rect> {
    let fixed: f32 = spec.iter().sum();
    let gaps = gap * (spec.len().saturating_sub(1)) as f32;
    let flex = (rect.width() - 2. * pad - fixed - gaps).max(20.);
    let mut x = rect.left() + pad;
    spec.iter()
        .map(|w| {
            let w = if *w == 0. { flex } else { *w };
            let r = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
            x += w + gap;
            r
        })
        .collect()
}

/// Sticky-looking header row of a table.
pub fn table_head(ui: &mut Ui, spec: &[f32], labels: &[&str]) {
    let p = Pal::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: 7,
            ne: 7,
            sw: 0,
            se: 0,
        },
        p.soft,
    );
    line_below(ui, rect, p.line2);
    for (r, s) in columns(rect, spec, 10., 8.).into_iter().zip(labels) {
        ui.painter().text(
            pos2(r.left(), r.center().y),
            Align2::LEFT_CENTER,
            *s,
            FontId::proportional(SMALL),
            p.faint,
        );
    }
}

/// Group label row inside a table (the demo's `.row.grp`).
pub fn group_row(ui: &mut Ui, s: &str) {
    let p = Pal::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, p.soft);
    line_below(ui, rect, p.line2);
    ui.painter().text(
        pos2(rect.left() + 10., rect.center().y),
        Align2::LEFT_CENTER,
        s,
        FontId::proportional(SMALL),
        p.muted,
    );
}

/// Clickable table row. Returns the row response and one child `Ui` per column.
pub fn table_row(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    spec: &[f32],
    selected: bool,
    height: f32,
) -> (Response, Vec<Ui>) {
    let p = Pal::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let r = ui.interact(rect, ui.id().with(("row", id)), Sense::click());
    let fill = if selected {
        p.sel
    } else if r.hovered() {
        p.soft
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    line_below(ui, rect, p.line2);
    let cells = columns(rect, spec, 10., 8.)
        .into_iter()
        .map(|c| {
            ui.new_child(
                UiBuilder::new()
                    .max_rect(c)
                    .layout(Layout::left_to_right(Align::Center)),
            )
        })
        .collect();
    (r, cells)
}
fn line_below(ui: &Ui, rect: Rect, color: Color32) {
    ui.painter().line_segment(
        [
            pos2(rect.left(), rect.bottom() - 0.5),
            pos2(rect.right(), rect.bottom() - 0.5),
        ],
        Stroke::new(1., color),
    );
}

/// Centered faint text for empty lists and empty inspectors.
pub fn empty_note(ui: &mut Ui, s: &str) {
    let p = Pal::of(ui);
    ui.add_space(22.);
    ui.vertical_centered(|ui| {
        ui.label(text(s, p.faint, 12.5));
    });
}

/// Label/value grid row in the inspector (the demo's `.f`).
pub fn field<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = Pal::of(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.;
        let (rect, _) = ui.allocate_exact_size(vec2(52., 28.), Sense::hover());
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(BODY),
            p.muted,
        );
        ui.add_space(2.);
        add(ui)
    })
    .inner
}

/// Thin separator line with the inspector's soft color.
pub fn hr(ui: &mut Ui) {
    let p = Pal::of(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.), Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1., p.line2),
    );
}

/// Full-width horizontal rule for panel edges.
pub fn edge(ui: &Ui, rect: Rect, top: bool) {
    let p = Pal::of(ui);
    let y = if top { rect.top() } else { rect.bottom() };
    ui.painter().line_segment(
        [pos2(rect.left(), y), pos2(rect.right(), y)],
        Stroke::new(1., p.line),
    );
}

/// Small number box (the demo's `input.n`).
pub fn num(
    ui: &mut Ui,
    value: &mut f64,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    decimals: usize,
    bad: bool,
) -> Response {
    let p = Pal::of(ui);
    ui.scope(|ui| {
        if bad {
            let w = &mut ui.visuals_mut().widgets;
            w.inactive.bg_stroke = Stroke::new(1., p.bad);
            w.hovered.bg_stroke = Stroke::new(1., p.bad);
        }
        ui.add_sized(
            vec2(66., 28.),
            egui::DragValue::new(value)
                .speed(speed)
                .range(range)
                .min_decimals(0)
                .max_decimals(decimals),
        )
    })
    .inner
}

/// Small rounded tag (the demo's `.fn`).
pub fn tag(ui: &mut Ui, s: &str, highlight: bool) {
    let p = Pal::of(ui);
    let color = if highlight { p.ink } else { p.muted };
    let galley = ui
        .painter()
        .layout_no_wrap(s.to_owned(), FontId::proportional(SMALL), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + vec2(12., 4.), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(4),
        if highlight { p.sel } else { p.soft },
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, color);
}

const TAB_FONT: f32 = 12.5;
pub fn file_tab_width(ui: &Ui, label: &str) -> f32 {
    let g = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::monospace(TAB_FONT),
        Color32::WHITE,
    );
    g.size().x + 24. + 22.
}
/// One file tab inside the bordered tab strip: name plus a close cross.
/// Returns (name response, close response).
pub fn file_tab(
    ui: &mut Ui,
    label: &str,
    current: bool,
    missing: bool,
    (first, last): (bool, bool),
) -> (Response, Response) {
    let p = Pal::of(ui);
    let color = if missing {
        p.warn
    } else if current {
        p.ink
    } else {
        p.muted
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), FontId::monospace(TAB_FONT), color);
    let name_w = galley.size().x + 24.;
    let (rect, _) = ui.allocate_exact_size(vec2(name_w + 22., 30.), Sense::hover());
    let name_rect = Rect::from_min_size(rect.min, vec2(name_w, rect.height()));
    let x_rect = Rect::from_min_max(pos2(name_rect.right(), rect.top()), rect.max);
    let open = ui
        .interact(name_rect, ui.id().with(("ftab", label)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let close = ui
        .interact(x_rect, ui.id().with(("ftab-x", label)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if current {
        p.sel
    } else if open.hovered() || close.hovered() {
        p.soft
    } else {
        Color32::TRANSPARENT
    };
    let r = |on: bool| if on { 7 } else { 0 };
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: r(first),
            sw: r(first),
            ne: r(last),
            se: r(last),
        },
        fill,
    );
    if !first {
        ui.painter().line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1., p.line),
        );
    }
    let pos = pos2(
        name_rect.left() + 12.,
        rect.center().y - galley.size().y / 2.,
    );
    if current {
        // Bold for the open file: draw twice with a sub-pixel offset.
        let again =
            ui.painter()
                .layout_no_wrap(label.to_owned(), FontId::monospace(TAB_FONT), color);
        ui.painter().galley(pos + vec2(0.45, 0.), again, color);
    }
    ui.painter().galley(pos, galley, color);
    ui.painter().text(
        pos2(x_rect.left() + 4., rect.center().y),
        Align2::LEFT_CENTER,
        "×",
        FontId::proportional(14.),
        if close.hovered() { p.bad } else { p.faint },
    );
    (open, close)
}
