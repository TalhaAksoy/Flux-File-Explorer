//! Design tokens + theme application. One semantic palette drives every
//! surface: closely related neutral tones, translucent accents, no hard
//! borders. Surfaces separate by ~4-6 value steps, not lines.

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

/// Semantic design tokens — the single source of truth for color.
/// `Palette::dark()`/`light()` return cohesive sets; every UI element
/// reads from these, never ad-hoc literals.
pub struct Palette {
    // Accents & states
    pub accent: Color32,
    /// Translucent accent tint — row selection, focused chrome.
    pub accent_fill: Color32,
    pub danger: Color32,
    pub warn: Color32,
    // Surfaces (darkest → lightest in dark mode; reversed in light)
    pub bg_window: Color32,
    pub bg_sidebar: Color32,
    pub bg_content: Color32,
    /// Popups, dialogs, active tabs — floats above content.
    pub bg_elevated: Color32,
    /// Text fields: slightly recessed so they read as input zones.
    pub field_inset: Color32,
    /// Alternating row stripe — barely a step off content.
    pub stripe: Color32,
    pub surface_hover: Color32,
    pub surface_active: Color32,
    pub surface_selected: Color32,
    // Type hierarchy
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    // File glyphs (fallbacks when no native icon)
    pub dir_icon: Color32,
    pub file_icon: Color32,
    pub link_icon: Color32,
    /// Last-resort hairline — only where a boundary is functional.
    pub border_soft: Color32,
}

impl Palette {
    pub fn dark() -> Self {
        let accent = Color32::from_rgb(80, 140, 235);
        Self {
            accent,
            accent_fill: accent.gamma_multiply(0.24),
            danger: Color32::from_rgb(229, 72, 77),
            warn: Color32::from_rgb(226, 175, 75),
            bg_window: Color32::from_rgb(23, 25, 28),
            bg_sidebar: Color32::from_rgb(28, 30, 34),
            bg_content: Color32::from_rgb(32, 34, 38),
            bg_elevated: Color32::from_rgb(38, 41, 46),
            field_inset: Color32::from_rgb(26, 28, 32),
            stripe: Color32::from_rgb(35, 37, 42),
            surface_hover: Color32::from_rgb(42, 45, 50),
            surface_active: Color32::from_rgb(48, 51, 57),
            surface_selected: Color32::from_rgb(48, 57, 67),
            text_primary: Color32::from_rgb(232, 234, 237),
            text_secondary: Color32::from_rgb(150, 155, 164),
            text_muted: Color32::from_rgb(116, 122, 132),
            dir_icon: Color32::from_rgb(96, 165, 250),
            file_icon: Color32::from_rgb(148, 156, 170),
            link_icon: Color32::from_rgb(180, 140, 230),
            border_soft: Color32::from_rgb(50, 53, 59).gamma_multiply(0.55),
        }
    }

    pub fn light() -> Self {
        let accent = Color32::from_rgb(40, 105, 215);
        Self {
            accent,
            accent_fill: accent.gamma_multiply(0.14),
            danger: Color32::from_rgb(198, 52, 48),
            warn: Color32::from_rgb(160, 110, 20),
            bg_window: Color32::from_rgb(245, 246, 248),
            bg_sidebar: Color32::from_rgb(240, 241, 244),
            bg_content: Color32::from_rgb(250, 250, 251),
            bg_elevated: Color32::from_rgb(255, 255, 255),
            field_inset: Color32::from_rgb(242, 243, 246),
            stripe: Color32::from_rgb(246, 247, 249),
            surface_hover: Color32::from_rgb(235, 237, 241),
            surface_active: Color32::from_rgb(228, 231, 236),
            surface_selected: Color32::from_rgb(226, 234, 243),
            text_primary: Color32::from_rgb(34, 37, 43),
            text_secondary: Color32::from_rgb(108, 114, 125),
            text_muted: Color32::from_rgb(154, 160, 170),
            dir_icon: Color32::from_rgb(45, 125, 210),
            file_icon: Color32::from_rgb(110, 118, 132),
            link_icon: Color32::from_rgb(140, 95, 200),
            border_soft: Color32::from_rgb(214, 217, 224),
        }
    }

    pub fn of(visuals: &Visuals) -> Self {
        if visuals.dark_mode {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

/// Fill for the top/bottom chrome — the window tone itself; the toolbar
/// merges into the title surface rather than floating above the list.
pub fn chrome_fill(dark: bool) -> Color32 {
    if dark {
        Palette::dark()
    } else {
        Palette::light()
    }
    .bg_window
}

/// Sidebar fill — its own token, a half-step toward content.
pub fn sidebar_fill(dark: bool) -> Color32 {
    if dark {
        Palette::dark()
    } else {
        Palette::light()
    }
    .bg_sidebar
}

fn widget(
    corner: u8,
    weak_bg: Color32,
    bg: Color32,
    fg: Color32,
    stroke: Color32,
) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak_bg,
        bg_stroke: Stroke::new(1.0, stroke),
        corner_radius: CornerRadius::same(corner),
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    }
}

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let p = if dark {
        Palette::dark()
    } else {
        Palette::light()
    };
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.panel_fill = p.bg_content;
    v.window_fill = p.bg_elevated;
    v.extreme_bg_color = p.field_inset;
    v.faint_bg_color = p.stripe;
    v.text_edit_bg_color = Some(p.field_inset);
    v.weak_text_color = Some(p.text_secondary);
    v.override_text_color = None;
    v.hyperlink_color = p.accent;
    v.error_fg_color = p.danger;
    v.warn_fg_color = p.warn;
    v.selection.bg_fill = p.accent_fill;
    v.selection.stroke = Stroke::new(1.0, p.accent);

    // Widget states: resting controls are borderless ghosts; interaction
    // reveals a soft surface fill. Focused fields take an accent edge.
    let ws = &mut v.widgets;
    ws.inactive = widget(
        6,
        Color32::TRANSPARENT,
        p.field_inset,
        p.text_primary,
        p.border_soft,
    );
    ws.hovered = widget(
        6,
        p.surface_hover,
        p.surface_hover,
        p.text_primary,
        p.border_soft,
    );
    ws.active = widget(
        6,
        p.surface_active,
        p.surface_active,
        p.text_primary,
        p.border_soft,
    );
    ws.open = widget(
        6,
        p.surface_hover,
        p.surface_hover,
        p.text_primary,
        p.border_soft,
    );
    ws.noninteractive = widget(
        6,
        Color32::TRANSPARENT,
        p.field_inset,
        p.text_primary,
        Color32::TRANSPARENT,
    );

    v.menu_corner_radius = CornerRadius::same(10);
    v.window_corner_radius = CornerRadius::same(12);
    v.popup_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 90 } else { 40 }),
    };
    v.window_shadow = egui::Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 55 }),
    };

    let mut style = Style {
        visuals: v,
        ..Style::default()
    };
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.menu_margin = egui::Margin::same(7);
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    // Typography scale — quiet, readable, no marketing-size headings.
    let st = &mut style.text_styles;
    st.insert(egui::TextStyle::Body, egui::FontId::proportional(13.5));
    st.insert(egui::TextStyle::Button, egui::FontId::proportional(13.5));
    st.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
    st.insert(egui::TextStyle::Heading, egui::FontId::proportional(16.5));
    st.insert(egui::TextStyle::Monospace, egui::FontId::monospace(12.5));

    let theme = if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
}

/// System-native UI font, best-effort per platform. Falls back silently
/// to egui's bundled fonts if nothing on the list parses/loads.
pub fn load_system_fonts() -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    if let Some(bytes) = system_font_bytes() {
        let data = egui::FontData::from_owned(bytes);
        defs.font_data
            .insert("system-ui".into(), std::sync::Arc::new(data));
        defs.families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system-ui".into());
    }
    defs
}

fn system_font_bytes() -> Option<Vec<u8>> {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/HelveticaNeue.ttc",
        ]
    } else if cfg!(target_os = "windows") {
        &[
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\segoeuivf.ttf",
        ]
    } else {
        // Fast paths for the Debian/Ubuntu layout; other distros are
        // covered by the directory scan below.
        &[
            "/usr/share/fonts/truetype/inter/Inter-Regular.otf",
            "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto-sans/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        ]
    };
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            return Some(bytes);
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    return scan_font_dirs();
    #[allow(unreachable_code)]
    None
}

/// Linux distros disagree on font paths (`truetype/` vs `dejavu-sans-fonts/`
/// vs `google-noto/`), so walk the standard roots and pick a preferred
/// family by file name. Variable fonts ("Name[wght].ttf", "*-VF.otf") are
/// skipped — the rasterizer can reject them and epaint panics on bad data.
#[cfg(all(unix, not(target_os = "macos")))]
fn scan_font_dirs() -> Option<Vec<u8>> {
    const PREFERRED: &[&str] = &[
        "inter-regular",
        "notosans-regular",
        "adwaitasans",
        "cantarell",
        "dejavusans",
        "liberationsans-regular",
        "ubuntu-r",
    ];
    // Style variants make poor UI fonts — "dejavusans" also matches
    // "dejavusans-bold", so name a regular face, not just the family.
    const NON_REGULAR: &[&str] = &[
        "bold",
        "italic",
        "oblique",
        "light",
        "medium",
        "thin",
        "black",
        "condensed",
        "mono",
        "semibold",
        "extra",
        "display",
        "hairline",
        "demi",
    ];
    let mut roots = vec![
        std::path::PathBuf::from("/usr/share/fonts"),
        std::path::PathBuf::from("/usr/local/share/fonts"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        roots.push(std::path::Path::new(&home).join(".local/share/fonts"));
        roots.push(std::path::Path::new(&home).join(".fonts"));
    }
    let mut files = Vec::new();
    for root in &roots {
        collect_font_files(root, 0, &mut files);
    }
    for want in PREFERRED {
        for f in &files {
            let stem = f
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let variable = stem.contains('[') || stem.ends_with("-vf") || stem.ends_with("_vf");
            let styled = NON_REGULAR.iter().any(|s| stem.contains(s));
            if !variable
                && !styled
                && stem.contains(want)
                && let Ok(b) = std::fs::read(f)
            {
                return Some(b);
            }
        }
    }
    None
}

#[cfg(all(unix, not(target_os = "macos")))]
fn collect_font_files(dir: &std::path::Path, depth: u8, out: &mut Vec<std::path::PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_font_files(&p, depth + 1, out);
        } else if p.extension().is_some_and(|x| {
            ["ttf", "otf", "ttc", "otc"]
                .iter()
                .any(|ext| x.eq_ignore_ascii_case(ext))
        }) {
            out.push(p);
        }
    }
}
