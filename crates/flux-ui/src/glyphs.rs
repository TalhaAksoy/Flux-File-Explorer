//! Vector-painted UI glyphs — the bundled egui font lacks arrow/symbol
//! coverage, so toolbar icons are drawn as strokes instead of text.
//! Every glyph renders identically on every platform.

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Back,
    Forward,
    Up,
    Refresh,
    Search,
    Plus,
    Close,
    /// Volume / drive.
    Disk,
}

/// Draw `g` centered inside `rect` with the given color.
pub fn paint_glyph(p: &egui::Painter, rect: Rect, g: Glyph, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let w = (r * 0.22).max(1.2);
    let st = Stroke::new(w, color);
    match g {
        Glyph::Back | Glyph::Forward => {
            let dir = if g == Glyph::Back { -1.0 } else { 1.0 };
            let tip = Pos2::new(c.x + dir * r * 0.72, c.y);
            let tail = Pos2::new(c.x - dir * r * 0.72, c.y);
            p.line_segment([tail, tip], st);
            // Arrowhead: two strokes leaning back from the tip.
            for dy in [-1.0f32, 1.0] {
                p.line_segment(
                    [
                        tip,
                        Pos2::new(tip.x - dir * r * 0.55, tip.y + dy * r * 0.5),
                    ],
                    st,
                );
            }
        }
        Glyph::Up => {
            let tip = Pos2::new(c.x, c.y - r * 0.72);
            let tail = Pos2::new(c.x, c.y + r * 0.72);
            p.line_segment([tail, tip], st);
            for dx in [-1.0f32, 1.0] {
                p.line_segment(
                    [tip, Pos2::new(tip.x + dx * r * 0.5, tip.y + r * 0.55)],
                    st,
                );
            }
        }
        Glyph::Refresh => {
            // 280° arc with an arrowhead at its end.
            let n = 24;
            let (a0, a1) = (0.5f32, 0.5 + std::f32::consts::TAU * 0.78);
            let pts: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let a = a0 + (a1 - a0) * i as f32 / n as f32;
                    Pos2::new(c.x + a.cos() * r * 0.7, c.y + a.sin() * r * 0.7)
                })
                .collect();
            p.add(egui::Shape::line(pts.clone(), st));
            let tip = *pts.last().unwrap();
            let tan = {
                let a = a1;
                Vec2::new(-a.sin(), a.cos()).normalized()
            };
            let nrm = Vec2::new(tan.y, -tan.x);
            for s in [-1.0f32, 1.0] {
                p.line_segment(
                    [
                        tip,
                        tip - tan * r * 0.5 + nrm * s * r * 0.32,
                    ],
                    st,
                );
            }
        }
        Glyph::Search => {
            let cr = r * 0.55;
            let cc = Pos2::new(c.x - r * 0.14, c.y - r * 0.14);
            p.circle_stroke(cc, cr, st);
            let d = Vec2::angled(std::f32::consts::FRAC_PI_4) * cr;
            p.line_segment([cc + d, cc + d * 1.75], st);
        }
        Glyph::Plus => {
            p.line_segment(
                [
                    Pos2::new(c.x - r * 0.6, c.y),
                    Pos2::new(c.x + r * 0.6, c.y),
                ],
                st,
            );
            p.line_segment(
                [
                    Pos2::new(c.x, c.y - r * 0.6),
                    Pos2::new(c.x, c.y + r * 0.6),
                ],
                st,
            );
        }
        Glyph::Close => {
            for (dx, dy) in [(-1.0f32, -1.0f32), (1.0, -1.0)] {
                p.line_segment(
                    [
                        Pos2::new(c.x + dx * r * 0.55, c.y + dy * r * 0.55),
                        Pos2::new(c.x - dx * r * 0.55, c.y - dy * r * 0.55),
                    ],
                    st,
                );
            }
        }
        Glyph::Disk => {
            // Drive: wide rounded body + slot line + activity dot.
            let body = Rect::from_center_size(
                c,
                Vec2::new(r * 1.5, r * 0.9),
            );
            p.rect_filled(body, CornerRadius::same(2), color.gamma_multiply(0.25));
            p.rect_stroke(body, CornerRadius::same(2), st, StrokeKind::Inside);
            p.line_segment(
                [
                    Pos2::new(body.left() + r * 0.2, body.bottom() - r * 0.28),
                    Pos2::new(body.right() - r * 0.2, body.bottom() - r * 0.28),
                ],
                st,
            );
            p.circle_filled(
                Pos2::new(body.right() - r * 0.32, body.top() + r * 0.28),
                w * 0.7,
                color,
            );
        }
    }
}

/// A sidebar-style row: small painted glyph + label, full-width clickable.
/// `selected` draws the calm "current location" pill. Returns the response.
pub fn icon_row(
    ui: &mut egui::Ui,
    paint: impl FnOnce(&egui::Painter, Rect, Color32),
    label: &str,
    selected: bool,
) -> egui::Response {
    let h = ui.spacing().interact_size.y;
    let w = ui.available_width();
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::click());
    let pal = crate::theme::Palette::of(&ui.style().visuals);
    let hovered = resp.hovered() && !selected;
    // Pill insets from the panel edge — the fill breathes around the
    // row content rather than hugging the sidebar walls.
    let pill = rect.shrink2(Vec2::new(4.0, 2.0));
    if selected {
        ui.painter().rect_filled(pill, CornerRadius::same(7), pal.surface_selected);
    } else if hovered {
        ui.painter().rect_filled(pill, CornerRadius::same(7), pal.surface_hover);
    }
    let text_col = if selected {
        pal.text_primary
    } else {
        pal.text_secondary
    };
    let ir = Rect::from_center_size(
        Pos2::new(rect.left() + h * 0.62, rect.center().y),
        Vec2::splat(h * 0.56),
    );
    paint(ui.painter(), ir, if selected { pal.text_primary } else { text_col });
    ui.painter().text(
        Pos2::new(rect.left() + h * 1.14, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        text_col,
    );
    resp
}

/// Small filled triangle for sort headers (`up`=ascending).
pub fn paint_triangle(p: &egui::Painter, rect: Rect, up: bool, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let (a, b, tip) = if up {
        (
            Pos2::new(c.x - r, c.y + r * 0.6),
            Pos2::new(c.x + r, c.y + r * 0.6),
            Pos2::new(c.x, c.y - r * 0.8),
        )
    } else {
        (
            Pos2::new(c.x - r, c.y - r * 0.6),
            Pos2::new(c.x + r, c.y - r * 0.6),
            Pos2::new(c.x, c.y + r * 0.8),
        )
    };
    p.add(egui::Shape::convex_polygon(
        vec![a, b, tip],
        color,
        Stroke::NONE,
    ));
}

/// A square toolbar button showing a painted glyph; honors hover/disabled
/// styling through the widget visuals so it looks native to the theme.
pub fn icon_button(
    ui: &mut egui::Ui,
    g: Glyph,
    tip: &str,
    enabled: bool,
) -> egui::Response {
    let size = ui.spacing().interact_size.y;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), sense);
    let resp = if enabled {
        resp.on_hover_text(tip)
    } else {
        resp.on_disabled_hover_text(tip)
    };
    let vis = ui.style().interact(&resp);
    if enabled && (resp.hovered() || resp.has_focus()) {
        ui.painter().rect(
            rect,
            vis.corner_radius,
            vis.weak_bg_fill,
            vis.bg_stroke,
            StrokeKind::Inside,
        );
    }
    let col = if enabled {
        vis.fg_stroke.color
    } else {
        ui.visuals().weak_text_color()
    };
    paint_glyph(ui.painter(), rect.shrink(size * 0.22), g, col);
    resp
}

/// Toggle-style icon button — glyph turns accent + bg shows when `on`.
pub fn icon_toggle(ui: &mut egui::Ui, g: Glyph, tip: &str, on: bool) -> egui::Response {
    let size = ui.spacing().interact_size.y;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
    let resp = resp.on_hover_text(tip);
    let vis = ui.style().interact(&resp);
    let accent = ui.visuals().selection.stroke.color;
    if on {
        ui.painter().rect(
            rect,
            vis.corner_radius,
            accent.gamma_multiply(0.18),
            Stroke::new(1.0, accent.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter().rect(
            rect,
            vis.corner_radius,
            vis.weak_bg_fill,
            vis.bg_stroke,
            StrokeKind::Inside,
        );
    }
    let col = if on { accent } else { vis.fg_stroke.color };
    paint_glyph(ui.painter(), rect.shrink(size * 0.22), g, col);
    resp
}

/// Bright drop-target glow: translucent accent fill + crisp accent stroke.
/// Painted on whatever layer the caller's painter is on.
pub fn glow_drop_target(p: &egui::Painter, rect: Rect, accent: Color32) {
    p.rect_filled(
        rect,
        CornerRadius::same(3),
        accent.gamma_multiply(0.22),
    );
    p.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.5, accent),
        StrokeKind::Inside,
    );
}
