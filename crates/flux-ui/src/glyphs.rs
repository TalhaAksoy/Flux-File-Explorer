//! Vector-painted UI glyphs — the bundled egui font lacks arrow/symbol
//! coverage, so toolbar icons are drawn as strokes instead of text.
//! Every glyph renders identically on every platform.

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    /// Shell prompt (❯_) — embedded terminal.
    Terminal,
    /// Open in external app (box with arrow exiting NE).
    External,
    /// Cogwheel — settings.
    Gear,
    /// Side panel with a content column — preview pane.
    Preview,
    /// Clock face — operation history.
    History,
    /// Sun/disc — appearance & theme settings.
    Sun,
    /// "Aa" text — typography settings.
    TypeAa,
    /// Folder silhouette — file/folder settings + menu icons.
    Folder,
    /// Four cells — view-mode/grid settings.
    GridView,
    /// Keyboard outline — shortcut list.
    Keyboard,
    /// Three fader lines — advanced settings.
    Sliders,
    /// Three horizontal bars of decreasing length — sort order.
    Sort,
    /// Three evenly spaced rules — list/row density.
    Density,
    /// Circled i — about/properties.
    Info,
    /// Scissors — cut.
    Cut,
    /// Two overlapping sheets — copy.
    Copy,
    /// Clipboard — paste.
    Paste,
    /// Pencil — rename/edit.
    Pencil,
    /// Trash can — destructive delete.
    Trash,
    /// Archive box — compress.
    Archive,
    /// Linked chain — copy path / link.
    Link,
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
                    [tip, Pos2::new(tip.x - dir * r * 0.55, tip.y + dy * r * 0.5)],
                    st,
                );
            }
        }
        Glyph::Up => {
            let tip = Pos2::new(c.x, c.y - r * 0.72);
            let tail = Pos2::new(c.x, c.y + r * 0.72);
            p.line_segment([tail, tip], st);
            for dx in [-1.0f32, 1.0] {
                p.line_segment([tip, Pos2::new(tip.x + dx * r * 0.5, tip.y + r * 0.55)], st);
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
                p.line_segment([tip, tip - tan * r * 0.5 + nrm * s * r * 0.32], st);
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
                [Pos2::new(c.x - r * 0.6, c.y), Pos2::new(c.x + r * 0.6, c.y)],
                st,
            );
            p.line_segment(
                [Pos2::new(c.x, c.y - r * 0.6), Pos2::new(c.x, c.y + r * 0.6)],
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
        Glyph::Preview => {
            // Framed pane split vertically — list side + preview side.
            let body = Rect::from_center_size(c, Vec2::new(r * 1.5, r * 1.15));
            p.rect_stroke(body, CornerRadius::same(2), st, StrokeKind::Inside);
            let x = body.left() + body.width() * 0.6;
            p.line_segment(
                [
                    Pos2::new(x, body.top() + r * 0.14),
                    Pos2::new(x, body.bottom() - r * 0.14),
                ],
                st,
            );
        }
        Glyph::Disk => {
            // Drive: wide rounded body + slot line + activity dot.
            let body = Rect::from_center_size(c, Vec2::new(r * 1.5, r * 0.9));
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
        Glyph::Terminal => {
            // ❯_ — chevron at left, underscore prompt at lower right.
            let tip = Pos2::new(c.x - r * 0.05, c.y - r * 0.15);
            p.line_segment([Pos2::new(c.x - r * 0.6, c.y - r * 0.7), tip], st);
            p.line_segment([tip, Pos2::new(c.x - r * 0.6, c.y + r * 0.4)], st);
            p.line_segment(
                [
                    Pos2::new(c.x + r * 0.05, c.y + r * 0.55),
                    Pos2::new(c.x + r * 0.6, c.y + r * 0.55),
                ],
                st,
            );
        }
        Glyph::External => {
            // Rounded box, lower-left — with an arrow leaving its top-right.
            let b =
                Rect::from_center_size(c + egui::vec2(-r * 0.15, r * 0.15), Vec2::splat(r * 1.15));
            p.rect_stroke(b, CornerRadius::same(2), st, StrokeKind::Inside);
            let to = Pos2::new(c.x + r * 0.72, c.y - r * 0.72);
            p.line_segment([Pos2::new(c.x - r * 0.05, c.y + r * 0.05), to], st);
            for d in [egui::vec2(-0.45, 0.0), egui::vec2(0.0, 0.45)] {
                p.line_segment([to, to + d * r], st);
            }
        }
        Glyph::Gear => {
            // Hub circle + 8 teeth spokes.
            p.circle_stroke(c, r * 0.34, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = egui::vec2(a.cos(), a.sin());
                p.line_segment([c + d * (r * 0.5), c + d * (r * 0.78)], st);
            }
        }
        Glyph::History => {
            // Clock: circular face + hands to ~12 and ~4 o'clock.
            p.circle_stroke(c, r * 0.78, st);
            p.line_segment([c, c + egui::vec2(0.0, -r * 0.5)], st);
            p.line_segment([c, c + egui::vec2(r * 0.38, r * 0.22)], st);
        }
        Glyph::Sun => {
            p.circle_stroke(c, r * 0.42, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = egui::vec2(a.cos(), a.sin());
                p.line_segment([c + d * (r * 0.62), c + d * (r * 0.82)], st);
            }
        }
        Glyph::TypeAa => {
            // Painted "Aa" — the one glyph that's intentionally text.
            p.text(
                c + egui::vec2(-r * 0.1, 0.0),
                egui::Align2::CENTER_CENTER,
                "Aa",
                egui::FontId::proportional(r * 1.15),
                color,
            );
        }
        Glyph::Folder => {
            let (w, h) = (rect.width(), rect.height());
            let dark = color;
            let tab = Rect::from_min_size(rect.min, Vec2::new(w * 0.44, h * 0.34));
            p.rect_filled(tab, 1.5, dark);
            let back = Rect::from_min_max(
                Pos2::new(rect.left() + w * 0.4, rect.top() + h * 0.12),
                Pos2::new(rect.right(), rect.top() + h * 0.42),
            );
            p.rect_filled(back, 1.5, dark);
            let body = Rect::from_min_max(Pos2::new(rect.left(), rect.top() + h * 0.32), rect.max);
            p.rect_filled(body, 2.0, dark.gamma_multiply(0.75));
        }
        Glyph::GridView => {
            let cell = r * 0.62;
            for dy in [-1.0f32, 1.0] {
                for dx in [-1.0f32, 1.0] {
                    let cr = Rect::from_center_size(
                        c + egui::vec2(dx * (cell * 0.62), dy * (cell * 0.62)),
                        Vec2::splat(cell),
                    );
                    p.rect_stroke(cr, CornerRadius::same(1), st, StrokeKind::Inside);
                }
            }
        }
        Glyph::Keyboard => {
            let body = Rect::from_center_size(c, Vec2::new(r * 1.6, r * 1.05));
            p.rect_stroke(body, CornerRadius::same(2), st, StrokeKind::Inside);
            // Spacebar + a few key dots.
            p.line_segment(
                [
                    Pos2::new(body.left() + r * 0.3, body.bottom() - r * 0.24),
                    Pos2::new(body.right() - r * 0.3, body.bottom() - r * 0.24),
                ],
                st,
            );
            for i in -1..=1 {
                p.circle_filled(
                    Pos2::new(c.x + i as f32 * r * 0.45, body.top() + r * 0.32),
                    w * 0.6,
                    color,
                );
            }
        }
        Glyph::Sliders => {
            for (i, knob) in [-0.3f32, 0.25, -0.05].iter().enumerate() {
                let y = c.y + (i as f32 - 1.0) * r * 0.5;
                p.line_segment(
                    [Pos2::new(c.x - r * 0.72, y), Pos2::new(c.x + r * 0.72, y)],
                    st,
                );
                p.circle_filled(Pos2::new(c.x + knob * r * 1.4, y), w * 1.1, color);
            }
        }
        Glyph::Sort => {
            // Bars shrinking top→bottom, like a funnel.
            for i in 0..3 {
                let y = c.y + (i as f32 - 1.0) * r * 0.52;
                let half = r * (0.72 - i as f32 * 0.22);
                p.line_segment([Pos2::new(c.x - half, y), Pos2::new(c.x + half, y)], st);
            }
        }
        Glyph::Density => {
            for i in 0..3 {
                let y = c.y + (i as f32 - 1.0) * r * 0.52;
                p.line_segment(
                    [Pos2::new(c.x - r * 0.72, y), Pos2::new(c.x + r * 0.72, y)],
                    st,
                );
            }
        }
        Glyph::Info => {
            p.circle_stroke(c, r * 0.75, st);
            p.circle_filled(Pos2::new(c.x, c.y - r * 0.36), w * 0.9, color);
            p.line_segment(
                [
                    Pos2::new(c.x, c.y - r * 0.05),
                    Pos2::new(c.x, c.y + r * 0.45),
                ],
                st,
            );
        }
        Glyph::Cut => {
            // Scissors: two finger rings + open blades crossing at center.
            p.circle_stroke(c + egui::vec2(-r * 0.42, r * 0.45), r * 0.24, st);
            p.circle_stroke(c + egui::vec2(r * 0.42, r * 0.45), r * 0.24, st);
            p.line_segment(
                [
                    c + egui::vec2(-r * 0.3, r * 0.3),
                    c + egui::vec2(r * 0.55, -r * 0.55),
                ],
                st,
            );
            p.line_segment(
                [
                    c + egui::vec2(r * 0.3, r * 0.3),
                    c + egui::vec2(-r * 0.55, -r * 0.55),
                ],
                st,
            );
        }
        Glyph::Copy => {
            // Two overlapping sheets — back sheet top-right, front bottom-left.
            let back =
                Rect::from_center_size(c + egui::vec2(r * 0.22, -r * 0.22), Vec2::splat(r * 0.95));
            p.rect_stroke(back, CornerRadius::same(2), st, StrokeKind::Inside);
            let front =
                Rect::from_center_size(c + egui::vec2(-r * 0.22, r * 0.22), Vec2::splat(r * 0.95));
            p.rect_stroke(front, CornerRadius::same(2), st, StrokeKind::Inside);
        }
        Glyph::Paste => {
            let body = Rect::from_center_size(c, Vec2::new(r * 1.15, r * 1.4));
            p.rect_stroke(body, CornerRadius::same(2), st, StrokeKind::Inside);
            let clip =
                Rect::from_center_size(Pos2::new(c.x, body.top()), Vec2::new(r * 0.55, r * 0.28));
            p.rect_filled(clip, 1.0, color);
            for i in 0..2 {
                let y = body.top() + r * (0.55 + i as f32 * 0.4);
                p.line_segment(
                    [
                        Pos2::new(body.left() + r * 0.22, y),
                        Pos2::new(body.right() - r * 0.22, y),
                    ],
                    st,
                );
            }
        }
        Glyph::Pencil => {
            // Diagonal pencil: body line + sharpened tip.
            let a = c + egui::vec2(-r * 0.55, r * 0.55);
            let b = c + egui::vec2(r * 0.35, -r * 0.35);
            p.line_segment([a, b], Stroke::new(w * 1.6, color));
            let tip = c + egui::vec2(r * 0.62, -r * 0.62);
            p.line_segment([b, tip], st);
            p.add(egui::Shape::convex_polygon(
                vec![
                    b + egui::vec2(-r * 0.08, r * 0.28),
                    b + egui::vec2(r * 0.28, -r * 0.08),
                    tip,
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Trash => {
            let body = Rect::from_min_max(
                Pos2::new(c.x - r * 0.5, c.y - r * 0.38),
                Pos2::new(c.x + r * 0.5, c.y + r * 0.68),
            );
            p.rect_stroke(body, CornerRadius::same(2), st, StrokeKind::Inside);
            // Lid + handle.
            p.line_segment(
                [
                    Pos2::new(c.x - r * 0.65, c.y - r * 0.45),
                    Pos2::new(c.x + r * 0.65, c.y - r * 0.45),
                ],
                st,
            );
            p.line_segment(
                [
                    Pos2::new(c.x - r * 0.2, c.y - r * 0.62),
                    Pos2::new(c.x + r * 0.2, c.y - r * 0.62),
                ],
                st,
            );
            for dx in [-0.2f32, 0.2] {
                p.line_segment(
                    [
                        Pos2::new(c.x + dx * r, c.y - r * 0.15),
                        Pos2::new(c.x + dx * r, c.y + r * 0.5),
                    ],
                    Stroke::new(w * 0.8, color),
                );
            }
        }
        Glyph::Archive => {
            let body = Rect::from_center_size(c, Vec2::new(r * 1.45, r * 1.15));
            p.rect_stroke(body, CornerRadius::same(2), st, StrokeKind::Inside);
            // Lid seam + handle slot.
            p.line_segment(
                [
                    Pos2::new(body.left(), body.top() + r * 0.32),
                    Pos2::new(body.right(), body.top() + r * 0.32),
                ],
                st,
            );
            p.line_segment(
                [
                    Pos2::new(c.x - r * 0.2, body.top() + r * 0.6),
                    Pos2::new(c.x + r * 0.2, body.top() + r * 0.6),
                ],
                Stroke::new(w * 1.1, color),
            );
        }
        Glyph::Link => {
            // Chain link: two rings on a diagonal, joined by the middle bar.
            p.circle_stroke(c + egui::vec2(-r * 0.34, r * 0.34), r * 0.34, st);
            p.circle_stroke(c + egui::vec2(r * 0.34, -r * 0.34), r * 0.34, st);
            p.line_segment(
                [
                    c + egui::vec2(-r * 0.1, r * 0.1),
                    c + egui::vec2(r * 0.1, -r * 0.1),
                ],
                st,
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
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::click_and_drag());
    let pal = crate::theme::Palette::of(&ui.style().visuals);
    let hovered = resp.hovered() && !selected;
    // Pill insets from the panel edge — the fill breathes around the
    // row content rather than hugging the sidebar walls.
    let pill = rect.shrink2(Vec2::new(4.0, 2.0));
    if selected {
        ui.painter()
            .rect_filled(pill, CornerRadius::same(7), pal.surface_selected);
    } else if hovered {
        ui.painter()
            .rect_filled(pill, CornerRadius::same(7), pal.surface_hover);
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
    paint(
        ui.painter(),
        ir,
        if selected { pal.text_primary } else { text_col },
    );
    ui.painter().text(
        Pos2::new(rect.left() + h * 1.1, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.0),
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
pub fn icon_button(ui: &mut egui::Ui, g: Glyph, tip: &str, enabled: bool) -> egui::Response {
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
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click());
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

/// Toggle-style icon button that can be disabled — same visuals as
/// `icon_toggle` but greyed out and unclickable when `enabled` is false.
pub fn icon_toggle_enabled(
    ui: &mut egui::Ui,
    g: Glyph,
    tip: &str,
    on: bool,
    enabled: bool,
) -> egui::Response {
    if enabled {
        return icon_toggle(ui, g, tip, on);
    }
    let size = ui.spacing().interact_size.y;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let resp = resp.on_disabled_hover_text(tip);
    paint_glyph(
        ui.painter(),
        rect.shrink(size * 0.22),
        g,
        ui.visuals().weak_text_color(),
    );
    resp
}

/// Bright drop-target glow: translucent accent fill + crisp accent stroke.
/// Painted on whatever layer the caller's painter is on.
pub fn glow_drop_target(p: &egui::Painter, rect: Rect, accent: Color32) {
    p.rect_filled(rect, CornerRadius::same(3), accent.gamma_multiply(0.22));
    p.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.5, accent),
        StrokeKind::Inside,
    );
}
