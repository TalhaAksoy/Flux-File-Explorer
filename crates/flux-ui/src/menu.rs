//! Modern context-menu kit: rounded popup, full-width flat items with an
//! inset rounded hover, right-aligned shortcut hints, native submenus.
//!
//! The host `egui::Area` MUST be tagged `.kind(egui::UiKind::Menu)` —
//! `SubMenuButton`'s state lookup walks the stack for a Menu root.

use egui::containers::menu::{self, SubMenuButton};
use egui::{Button, Color32, CornerRadius, Frame, Margin, Response, RichText, Style, Ui, Vec2};

/// Rounded, softly shadowed popup surface.
pub fn popup_frame(style: &Style) -> Frame {
    Frame::popup(style)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(5))
}

/// Apply egui's flat menu styling + tight rhythm inside a menu Area.
/// Call once at the top of the menu closure. `width` is a hard bound —
/// inside an `Area`, `available_width` is the rest of the screen, so
/// without clamping the menu would stretch to the window edge.
pub fn begin(ui: &mut Ui, width: f32) {
    menu::menu_style(ui.style_mut());
    {
        let st = ui.style_mut();
        st.spacing.item_spacing = Vec2::new(0.0, 1.0);
        st.spacing.button_padding = Vec2::new(10.0, 5.0);
    }
    ui.set_min_width(width);
    ui.set_max_width(width);
}

/// Full-width flat menu item — rounded hover highlight, left-aligned label.
pub fn item(ui: &mut Ui, label: &str) -> Response {
    item_full(ui, label, None, true, None, false)
}

/// Item with a small painted glyph in the icon column.
pub fn item_icon(ui: &mut Ui, icon: crate::glyphs::Glyph, label: &str) -> Response {
    item_full(ui, label, None, true, Some(icon), false)
}

/// Icon item with a right-aligned shortcut hint.
pub fn item_icon_sc(ui: &mut Ui, icon: crate::glyphs::Glyph, label: &str, sc: &str) -> Response {
    item_full(ui, label, Some(sc), true, Some(icon), false)
}

/// Destructive item — danger-colored label + icon.
pub fn item_danger(ui: &mut Ui, label: &str, sc: &str) -> Response {
    item_full(
        ui,
        label,
        Some(sc).filter(|s| !s.is_empty()),
        true,
        Some(crate::glyphs::Glyph::Trash),
        true,
    )
}

/// Full-width item with a right-aligned shortcut hint (e.g. "Cmd+C").
pub fn item_shortcut(ui: &mut Ui, label: &str, shortcut: &str) -> Response {
    item_full(ui, label, Some(shortcut), true, None, false)
}

/// Item that greys out when `enabled` is false (e.g. Paste w/ empty clip).
pub fn item_enabled(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    item_full(ui, label, None, enabled, None, false)
}

/// Shortcut item that greys out when `enabled` is false.
pub fn item_sc_enabled(ui: &mut Ui, label: &str, shortcut: &str, enabled: bool) -> Response {
    item_full(ui, label, Some(shortcut), enabled, None, false)
}

fn item_full(
    ui: &mut Ui,
    label: &str,
    sc: Option<&str>,
    enabled: bool,
    icon: Option<crate::glyphs::Glyph>,
    danger: bool,
) -> Response {
    let w = ui.available_width();
    // Left offset reserves a checkmark/icon column.
    let mut b = Button::new(RichText::new(""))
        .min_size(Vec2::new(w, 30.0))
        .frame_when_inactive(false);
    if let Some(s) = sc {
        b = b.right_text(RichText::new(s).weak().small());
    }
    let r = ui.add_enabled(enabled, b);
    let label_x = r.rect.left() + 26.0;
    let pal = crate::theme::Palette::of(&ui.style().visuals);
    let fg = if !enabled {
        ui.visuals().weak_text_color()
    } else if danger {
        pal.danger
    } else {
        ui.visuals().text_color()
    };
    if let Some(g) = icon {
        let ir = egui::Rect::from_center_size(
            egui::pos2(r.rect.left() + 13.0, r.rect.center().y),
            Vec2::splat(14.0),
        );
        crate::glyphs::paint_glyph(
            ui.painter(),
            ir,
            g,
            if enabled {
                fg
            } else {
                ui.visuals().weak_text_color()
            },
        );
    }
    ui.painter().text(
        egui::pos2(label_x, r.rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        fg,
    );
    r
}

/// Item with a painted checkmark when `checked` — no font dependency.
pub fn item_check(ui: &mut Ui, label: &str, checked: bool) -> Response {
    let r = item_full(ui, label, None, true, None, false);
    if checked {
        let accent = ui.visuals().selection.stroke.color;
        let center = egui::pos2(r.rect.left() + 13.0, r.rect.center().y);
        let p = ui.painter();
        p.line_segment(
            [center + Vec2::new(-3.0, 0.5), center + Vec2::new(-1.0, 2.5)],
            egui::Stroke::new(1.6, accent),
        );
        p.line_segment(
            [center + Vec2::new(-1.0, 2.5), center + Vec2::new(3.0, -2.5)],
            egui::Stroke::new(1.6, accent),
        );
    }
    r
}

/// A nested submenu with a full-width trigger and a painted "›" arrow.
/// The label is painted at the same x as item labels so the leading
/// icon column lines up across rows.
pub fn submenu<R>(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui) -> R) {
    let w = ui.available_width();
    let btn = Button::new(RichText::new(""))
        .min_size(Vec2::new(w, 30.0))
        .frame_when_inactive(false)
        .right_text(RichText::new("›").weak());
    let (r, inner) = SubMenuButton::from_button(btn).ui(ui, |sub| {
        menu::menu_style(sub.style_mut());
        {
            let st = sub.style_mut();
            st.spacing.item_spacing = Vec2::new(0.0, 1.0);
            st.spacing.button_padding = Vec2::new(10.0, 5.0);
        }
        sub.set_min_width(180.0);
        sub.set_max_width(210.0);
        add_contents(sub)
    });
    ui.painter().text(
        egui::pos2(r.rect.left() + 26.0, r.rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        ui.visuals().text_color(),
    );
    let _ = inner;
}

/// Subtle full-width divider — softer than `ui.separator()`.
pub fn sep(ui: &mut Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 7.0), egui::Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment(
        [
            egui::pos2(rect.left() + 8.0, y),
            egui::pos2(rect.right() - 8.0, y),
        ],
        egui::Stroke::new(1.0, ui.visuals().weak_text_color().gamma_multiply(0.25)),
    );
}

/// Small right-aligned weak label used for section headers inside menus.
pub fn section(ui: &mut Ui, label: &str) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 16.0), egui::Sense::hover());
    ui.painter().text(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        ui.visuals().weak_text_color(),
    );
}

/// Accent-filled primary action button (dialogs).
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let accent = ui.visuals().selection.stroke.color;
    ui.add(
        Button::new(RichText::new(label).color(Color32::WHITE).size(14.0))
            .fill(accent)
            .corner_radius(CornerRadius::same(7))
            .min_size(Vec2::new(88.0, 32.0)),
    )
}

/// Quiet secondary button (dialogs).
pub fn ghost_button(ui: &mut Ui, label: &str) -> Response {
    ui.add(
        Button::new(label)
            .corner_radius(CornerRadius::same(7))
            .min_size(Vec2::new(88.0, 32.0)),
    )
}
