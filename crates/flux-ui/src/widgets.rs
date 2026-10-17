//! Shared component kit — the design system's vocabulary. Every screen
//! (settings page, rename workspace, dialogs, chrome) builds from these
//! primitives so interaction states and spacing stay consistent.
//!
//! Buttons: `primary` = accent fill (the one thing a user should press),
//! `secondary` = quiet surface fill, `ghost` = borderless until hover,
//! `danger` = destructive fill. All share one size + corner language.

use egui::{
    Align2, Button, Color32, CornerRadius, CursorIcon, Pos2, Rect, Response, RichText, Sense,
    Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{Palette, metrics};

/// Spacing scale (logical px): micro / compact / related / normal / section / major.
pub mod sp {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
}

const BTN_H: f32 = metrics::CONTROL;
const BTN_RADIUS: u8 = 7;

fn sized(btn: Button<'_>, min_w: f32) -> Button<'_> {
    btn.corner_radius(CornerRadius::same(BTN_RADIUS))
        .min_size(Vec2::new(min_w, BTN_H))
}

/// Accent-filled primary action — the single obvious choice in a dialog.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let accent = ui.visuals().selection.stroke.color;
    ui.add(sized(
        Button::new(RichText::new(label).color(Color32::WHITE).size(13.5)).fill(accent),
        88.0,
    ))
}

/// Quiet filled secondary action.
pub fn secondary_button(ui: &mut Ui, label: &str) -> Response {
    let pal = Palette::of(&ui.style().visuals);
    ui.add(sized(
        Button::new(RichText::new(label).size(13.5)).fill(pal.surface_hover),
        88.0,
    ))
}

/// Borderless secondary action — rests transparent, surfaces on hover.
pub fn ghost_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(sized(Button::new(RichText::new(label).size(13.5)), 72.0))
}

/// Destructive primary — danger fill; use only for irreversible actions.
pub fn danger_button(ui: &mut Ui, label: &str) -> Response {
    let pal = Palette::of(&ui.style().visuals);
    ui.add(sized(
        Button::new(RichText::new(label).color(Color32::WHITE).size(13.5)).fill(pal.danger),
        88.0,
    ))
}

/// iOS-style switch: 34×20 pill with a sliding knob. Clearer than a
/// checkbox for preferences that apply immediately.
///
/// `id` must be stable and unique (e.g. `"settings.hidden_files"`) —
/// anonymous child UIs share `Ui::id`, so an unsalted switch id could
/// alias another control's widget state.
pub fn switch(ui: &mut Ui, id: &'static str, on: &mut bool) -> Response {
    let pal = Palette::of(&ui.style().visuals);
    let (_, rect) = ui.allocate_space(Vec2::new(36.0, 20.0));
    let mut resp = ui.interact(rect, ui.id().with(("switch", id)), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, id)
    });
    if resp.clicked() && ui.is_enabled() {
        *on = !*on;
        resp.mark_changed();
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(3.0),
            CornerRadius::same(12),
            Stroke::new(1.5, pal.accent),
            StrokeKind::Outside,
        );
    }
    let anim = ui
        .ctx()
        .animate_bool_with_time(resp.id, *on, ui.style().animation_time);
    let track = if *on { pal.accent } else { pal.surface_active };
    let track = track.gamma_multiply(0.5 + anim * 0.5);
    let hover = resp.hovered();
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(10),
        if hover {
            track.gamma_multiply(1.1)
        } else {
            track
        },
    );
    let knob_r = 7.0;
    let knob_x = rect.left() + 3.0 + knob_r + (rect.width() - 6.0 - knob_r * 2.0) * anim;
    ui.painter()
        .circle_filled(Pos2::new(knob_x, rect.center().y), knob_r, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

/// One settings row: name + optional description on the left (wrapping),
/// control pinned to the right. At narrow widths the control drops onto
/// its own line rather than overlapping the description.
pub fn setting_row(
    ui: &mut Ui,
    pal: &Palette,
    title: &str,
    desc: Option<&str>,
    add_control: impl FnOnce(&mut Ui),
) {
    ui.add_space(sp::SM);
    let narrow = ui.available_width() < 620.0;
    if narrow {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = sp::XS;
            ui.label(
                RichText::new(title)
                    .color(pal.text_primary)
                    .size(metrics::BODY),
            );
            if let Some(d) = desc {
                ui.label(
                    RichText::new(d)
                        .color(pal.text_secondary)
                        .size(metrics::SECONDARY),
                );
            }
            ui.add_space(2.0);
            add_control(ui);
        });
    } else {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width() * 0.55, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        RichText::new(title)
                            .color(pal.text_primary)
                            .size(metrics::BODY),
                    );
                    if let Some(d) = desc {
                        ui.label(
                            RichText::new(d)
                                .color(pal.text_secondary)
                                .size(metrics::SECONDARY),
                        );
                    }
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                add_control(ui);
            });
        });
    }
}

/// Section heading inside a settings category: real title, optional muted
/// description — replaces the old uppercase 10px eyebrows.
pub fn section_heading(ui: &mut Ui, pal: &Palette, title: &str, desc: Option<&str>) {
    ui.add_space(sp::XL);
    ui.label(
        RichText::new(title)
            .color(pal.text_primary)
            .size(metrics::SECTION)
            .strong(),
    );
    if let Some(d) = desc {
        ui.add_space(2.0);
        ui.label(
            RichText::new(d)
                .color(pal.text_secondary)
                .size(metrics::SECONDARY),
        );
    }
    ui.add_space(sp::SM);
}

/// A soft 1px divider inset from the edges — separates row groups without
/// drawing a hard rule across the whole surface.
pub fn row_divider(ui: &mut Ui, pal: &Palette) {
    ui.add_space(sp::XS);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
    ui.painter().hline(
        rect.left() + 4.0..=rect.right() - 4.0,
        rect.center().y,
        Stroke::new(1.0, pal.border_soft),
    );
    ui.add_space(sp::XS);
}

/// Segmented control (macOS-style): one inset pill per option inside a
/// recessed track. `options` are (label, value); returns the clicked value.
///
/// `id` MUST be a stable, unique identifier for the control
/// (e.g. `"settings.theme"`) — egui gives anonymous child UIs the same
/// `Ui::id`, so deriving interaction ids from `ui.id()` alone makes
/// every segmented control on a page share ids and cross-fire clicks.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    pal: &Palette,
    id: &'static str,
    options: &[(T, &str)],
    current: T,
) -> Option<T> {
    let mut picked = None;
    let h = metrics::SEGMENT;
    let pad = 3.0;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap(
                    (*l).to_string(),
                    egui::FontId::proportional(13.0),
                    pal.text_primary,
                )
                .size()
                .x
                + 24.0
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + pad * 2.0 + (options.len() as f32 - 1.0) * 2.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, h), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), pal.field_inset);
    let mut x = rect.left() + pad;
    for (i, (val, label)) in options.iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(x, rect.top() + pad),
            Vec2::new(widths[i], h - pad * 2.0),
        );
        let resp = ui.interact(cell, ui.id().with(("seg", id, i)), Sense::click());
        let on = *val == current;
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                on,
                *label,
            )
        });
        if on {
            ui.painter().rect(
                cell,
                CornerRadius::same(6),
                pal.bg_elevated,
                Stroke::new(1.0, pal.border_soft),
                StrokeKind::Inside,
            );
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(cell, CornerRadius::same(6), pal.surface_hover);
        }
        ui.painter().text(
            cell.center(),
            Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(13.0),
            if on {
                pal.text_primary
            } else {
                pal.text_secondary
            },
        );
        if resp.has_focus() {
            ui.painter().rect_stroke(
                cell,
                CornerRadius::same(6),
                Stroke::new(1.5, pal.accent),
                StrokeKind::Inside,
            );
        }
        if resp.clicked() && ui.is_enabled() {
            picked = Some(*val);
        }
        x += widths[i] + 2.0;
    }
    picked
}

/// Centered empty-state block: painted icon, title, optional hint/action.
pub fn empty_state(
    ui: &mut Ui,
    pal: &Palette,
    paint: impl FnOnce(&egui::Painter, Rect, Color32),
    title: &str,
    hint: Option<&str>,
) {
    let w = ui.available_width();
    ui.allocate_ui_with_layout(
        Vec2::new(w, (ui.available_height() - 80.0).clamp(140.0, 300.0)),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.add_space(ui.available_height() * 0.32);
            ui.vertical_centered(|ui| {
                let (ir, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                paint(ui.painter(), ir, pal.text_muted);
                ui.add_space(sp::MD);
                ui.label(RichText::new(title).color(pal.text_secondary).size(14.0));
                if let Some(h) = hint {
                    ui.add_space(sp::XS);
                    ui.label(RichText::new(h).color(pal.text_muted).size(12.0));
                }
            });
        },
    );
}

/// Styled single-line input: recessed field, comfy margin.
pub fn text_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .desired_width(width)
            .margin(egui::Margin::symmetric(10, 7))
            .hint_text(RichText::new(hint).weak()),
    )
}

/// Small status pill: colored dot + label. `kind`: "ok" | "warn" | "err" | "info".
/// Paint single-line text within a strict width, with Unicode-safe ellipsis.
/// Uses egui's text layout rather than repeatedly measuring every prefix.
pub fn elided_text(
    painter: &egui::Painter,
    pos: Pos2,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width.max(0.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    let y = pos.y - galley.size().y * 0.5;
    painter.galley(Pos2::new(pos.x, y), galley, color);
}

pub fn status_chip(ui: &mut Ui, pal: &Palette, kind: &str, label: &str) {
    let col = match kind {
        "ok" => pal.success,
        "warn" => pal.warn,
        "err" => pal.danger,
        _ => pal.text_secondary,
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let (ir, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
        ui.painter().circle_filled(ir.center(), 3.0, col);
        ui.label(RichText::new(label).color(pal.text_secondary).size(12.0));
    });
}
