//! Visual identity: restrained, dense, fast. Short hovers, no gratuitous
//! animation.

use egui::{Color32, CornerRadius, Stroke, Style, Visuals};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Density {
    Compact,
    Standard,
    Comfortable,
}

impl Density {
    pub fn row_height(self) -> f32 {
        match self {
            Density::Compact => 20.0,
            Density::Standard => 24.0,
            Density::Comfortable => 30.0,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Density::Compact => Density::Standard,
            Density::Standard => Density::Comfortable,
            Density::Comfortable => Density::Compact,
        }
    }
}

pub struct Palette {
    pub accent: Color32,
    pub accent_soft: Color32,
    pub dir_icon: Color32,
    pub file_icon: Color32,
    pub link_icon: Color32,
    pub error: Color32,
    pub warn: Color32,
}

impl Palette {
    pub fn of(visuals: &Visuals) -> Self {
        if visuals.dark_mode {
            Self {
                accent: Color32::from_rgb(80, 140, 255),
                accent_soft: Color32::from_rgb(60, 100, 190),
                dir_icon: Color32::from_rgb(96, 165, 250),
                file_icon: Color32::from_rgb(148, 156, 170),
                link_icon: Color32::from_rgb(180, 140, 230),
                error: Color32::from_rgb(235, 87, 87),
                warn: Color32::from_rgb(235, 180, 60),
            }
        } else {
            Self {
                accent: Color32::from_rgb(35, 100, 220),
                accent_soft: Color32::from_rgb(210, 225, 250),
                dir_icon: Color32::from_rgb(45, 125, 210),
                file_icon: Color32::from_rgb(110, 118, 132),
                link_icon: Color32::from_rgb(140, 95, 200),
                error: Color32::from_rgb(200, 45, 45),
                warn: Color32::from_rgb(180, 130, 20),
            }
        }
    }
}

/// Fill for the top/bottom chrome — a subtle step *lighter* than the
/// content background, so zones read as raised surface rather than being
/// separated by ruled lines.
pub fn chrome_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(33, 35, 41)
    } else {
        Color32::from_rgb(251, 252, 255)
    }
}

/// Sidebar fill — one step *recessed* below the content background.
pub fn sidebar_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(22, 23, 27)
    } else {
        Color32::from_rgb(240, 242, 247)
    }
}

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark {
        let mut v = Visuals::dark();
        v.panel_fill = Color32::from_rgb(26, 27, 31);
        v.window_fill = Color32::from_rgb(31, 33, 38);
        v.extreme_bg_color = Color32::from_rgb(20, 21, 24);
        v.faint_bg_color = Color32::from_rgb(35, 37, 43);
        v.selection.bg_fill = Color32::from_rgb(50, 85, 160);
        v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(110, 160, 255));
        v.hyperlink_color = Color32::from_rgb(110, 165, 255);
        v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(212, 216, 224);
        // Softer widget outlines — field borders shouldn't scream.
        v.widgets.noninteractive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(54, 56, 63));
        // Translucent accent selection — modern tint, not a solid slab.
        v.selection.bg_fill = Color32::from_rgb(80, 140, 255).gamma_multiply(0.28);
        v
    } else {
        let mut v = Visuals::light();
        v.panel_fill = Color32::from_rgb(247, 248, 250);
        v.window_fill = Color32::from_rgb(252, 253, 255);
        v.extreme_bg_color = Color32::from_rgb(238, 240, 244);
        v.selection.bg_fill = Color32::from_rgb(60, 115, 220).gamma_multiply(0.16);
        v.selection.stroke = Stroke::new(1.0, Color32::from_rgb(60, 115, 220));
        v.widgets.noninteractive.bg_stroke =
            Stroke::new(1.0, Color32::from_rgb(219, 222, 229));
        v
    };
    // Floating surfaces: rounded, softly shadowed — the 2025 desktop look.
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 90 } else { 40 }),
    };
    visuals.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 55 }),
    };
    let mut style = Style {
        visuals,
        ..Style::default()
    };
    // Tighter, denser defaults — this is a productivity tool.
    style.spacing.item_spacing = egui::vec2(7.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
    style.spacing.menu_margin = egui::Margin::same(7);
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    for w in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
        &mut style.visuals.widgets.noninteractive,
    ] {
        w.corner_radius = CornerRadius::same(6);
    }
    let theme = if dark { egui::Theme::Dark } else { egui::Theme::Light };
    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
}
