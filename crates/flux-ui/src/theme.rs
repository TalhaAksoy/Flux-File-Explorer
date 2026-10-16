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
/// Build the font stack from the user's picks: explicit UI/mono choices
/// go to the front of their families; the OS face remains the fallback.
/// Fonts compiled into the binary — they always resolve, no download
/// needed. All are OFL/UFL licensed (see `fonts/LICENSES.md`).
fn bundled_bytes(key: &str) -> Option<&'static [u8]> {
    Some(match key {
        "jetbrains-nerd" => include_bytes!("../fonts/JetBrainsMonoNerdFont-Regular.ttf"),
        "meslo-nerd" => include_bytes!("../fonts/MesloLGSNerdFont-Regular.ttf"),
        "jetbrains" => include_bytes!("../fonts/JetBrainsMono-Regular.ttf"),
        "fira-sans" => include_bytes!("../fonts/FiraSans-Regular.ttf"),
        "ubuntu" => include_bytes!("../fonts/Ubuntu-Regular.ttf"),
        _ => return None,
    })
}

/// True when the choice's bytes ship inside the binary.
pub fn is_bundled(key: &str) -> bool {
    matches!(
        key,
        "jetbrains-nerd" | "meslo-nerd" | "jetbrains" | "fira-sans" | "ubuntu"
    )
}

/// egui family names under which bundled fonts are always registered —
/// lets pickers render each font name in its own typeface for free
/// (the bytes are `&'static`, `from_static` copies nothing).
fn preview_family(key: &str) -> egui::FontFamily {
    egui::FontFamily::Name(format!("flux-font-{key}").into())
}

/// The family a bundled font key renders in, if registered.
pub fn bundled_family(key: &str) -> Option<egui::FontFamily> {
    is_bundled(key).then(|| preview_family(key))
}

pub fn load_fonts(ui_key: &str, mono_key: &str) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    // Register every bundled font under its own preview family — the
    // bytes live in the binary, so this is pointer + len, zero copy.
    for key in [
        "jetbrains-nerd",
        "meslo-nerd",
        "jetbrains",
        "fira-sans",
        "ubuntu",
    ] {
        if let Some(bytes) = bundled_bytes(key) {
            let name = preview_family(key).to_string();
            defs.font_data.insert(
                name.clone(),
                std::sync::Arc::new(egui::FontData::from_static(bytes)),
            );
            defs.families
                .entry(preview_family(key))
                .or_default()
                .push(name);
        }
    }
    let ui_bytes = font_choice_bytes(ui_key, UI_FONTS).or_else(system_font_bytes);
    if let Some(bytes) = ui_bytes {
        let data = egui::FontData::from_owned(bytes);
        defs.font_data
            .insert("ui-main".into(), std::sync::Arc::new(data));
        defs.families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "ui-main".into());
    }
    // Terminal default is the bundled Nerd Font — file icons work out
    // of the box, no font install needed.
    let mono_bytes = if mono_key.is_empty() {
        bundled_bytes("jetbrains-nerd").map(<[u8]>::to_vec)
    } else {
        font_choice_bytes(mono_key, MONO_FONTS)
    };
    if let Some(bytes) = mono_bytes {
        let data = egui::FontData::from_owned(bytes);
        defs.font_data
            .insert("mono-main".into(), std::sync::Arc::new(data));
        defs.families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "mono-main".into());
    }
    defs
}

/// A selectable font — `patterns` are tried in order against normalized
/// file stems (lowercase, alnum only); `exclude` rejects style variants.
pub struct FontChoice {
    pub key: &'static str,
    pub label: &'static str,
    pub patterns: &'static [&'static str],
    pub exclude: &'static [&'static str],
    pub nerd: bool,
}

const STYLE_WORDS: &[&str] = &[
    "bold",
    "italic",
    "oblique",
    "light",
    "medium",
    "thin",
    "black",
    "semi",
    "extra",
    "demi",
    "hairline",
    "condensed",
    "display",
    "mono",
];

pub const UI_FONTS: &[FontChoice] = &[
    FontChoice {
        key: "",
        label: "System default",
        patterns: &[],
        exclude: &[],
        nerd: false,
    },
    FontChoice {
        key: "fira-sans",
        label: "Fira Sans",
        patterns: &["firasansregular", "firasans"],
        exclude: STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "ubuntu",
        label: "Ubuntu",
        patterns: &["ubuntur", "ubunturegular", "ubuntu"],
        exclude: STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "inter",
        label: "Inter",
        patterns: &["inter"],
        exclude: STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "roboto",
        label: "Roboto",
        patterns: &["robotoregular", "roboto"],
        exclude: STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "noto",
        label: "Noto Sans",
        patterns: &["notosansregular", "notosans"],
        exclude: STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "segoe",
        label: "Segoe UI",
        patterns: &["segoeui"],
        exclude: STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "cantarell",
        label: "Cantarell",
        patterns: &["cantarell"],
        exclude: STYLE_WORDS,
        nerd: false,
    },
];

const MONO_STYLE_WORDS: &[&str] = &[
    "bold",
    "italic",
    "oblique",
    "light",
    "thin",
    "black",
    "extra",
    "hairline",
    "medium",
    "propo",
    "condensed",
    "display",
    "semi",
    "demi",
    "retina",
];

/// Bundled fonts first — they always work out of the box; then system
/// Nerd Fonts (they carry the glyphs file managers actually use).
pub const MONO_FONTS: &[FontChoice] = &[
    FontChoice {
        key: "",
        label: "JetBrainsMono Nerd Font (default)",
        patterns: &[],
        exclude: &[],
        nerd: true,
    },
    FontChoice {
        key: "jetbrains-nerd",
        label: "JetBrainsMono Nerd Font",
        patterns: &[
            "jetbrainsmononerdfontmono",
            "jetbrainsmononerdfont",
            "jetbrainsmonoregularnerdfont",
        ],
        exclude: MONO_STYLE_WORDS,
        nerd: true,
    },
    FontChoice {
        key: "meslo-nerd",
        label: "MesloLGS Nerd Font",
        patterns: &["meslolgsnerdfont", "meslolgsnf", "meslolgs"],
        exclude: MONO_STYLE_WORDS,
        nerd: true,
    },
    FontChoice {
        key: "jetbrains",
        label: "JetBrains Mono",
        patterns: &["jetbrainsmono"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "firacode-nerd",
        label: "FiraCode Nerd Font",
        patterns: &["firacodenerdfontmono", "firacodenerdfont"],
        exclude: MONO_STYLE_WORDS,
        nerd: true,
    },
    FontChoice {
        key: "hack-nerd",
        label: "Hack Nerd Font",
        patterns: &["hacknerdfontmono", "hacknerdfont"],
        exclude: MONO_STYLE_WORDS,
        nerd: true,
    },
    FontChoice {
        key: "cascadia-nerd",
        label: "CascadiaCode Nerd Font",
        patterns: &["caskaydiamononerdfont", "caskaydiacodenerdfont"],
        exclude: MONO_STYLE_WORDS,
        nerd: true,
    },
    FontChoice {
        key: "firacode",
        label: "Fira Code",
        patterns: &["firacode"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "cascadia",
        label: "Cascadia Mono",
        patterns: &["cascadiamono", "caskaydiamono"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "sf-mono",
        label: "SF Mono",
        patterns: &["sfnsmono", "sfmono"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "menlo",
        label: "Menlo",
        patterns: &["menlo"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "consolas",
        label: "Consolas",
        patterns: &["consola"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "dejavu-mono",
        label: "DejaVu Sans Mono",
        patterns: &["dejavusansmono"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "ubuntu-mono",
        label: "Ubuntu Mono",
        patterns: &["ubuntumono"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
    FontChoice {
        key: "liberation-mono",
        label: "Liberation Mono",
        patterns: &["liberationmono"],
        exclude: MONO_STYLE_WORDS,
        nerd: false,
    },
];

/// Case-insensitive comparison key for deduping curated labels against
/// family names parsed out of font files.
pub fn norm_label(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn norm_stem(s: &str) -> String {
    norm_label(s)
}

/// Warm the font-file + system-family caches off the UI thread.
pub fn prewarm_font_scan() {
    std::thread::spawn(|| {
        let _ = font_files();
        let _ = system_fonts(false);
        let _ = system_fonts(true);
    });
}

static FONT_FILES: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();

/// All font files under the platform's standard roots — scanned once.
fn font_files() -> &'static [std::path::PathBuf] {
    FONT_FILES
        .get_or_init(|| {
            let mut files = Vec::new();
            for root in font_roots() {
                collect_font_files(&root, 0, &mut files);
            }
            // Deterministic order — first-pattern-match shouldn't depend
            // on readdir luck.
            files.sort();
            files
        })
        .as_slice()
}

fn font_roots() -> Vec<std::path::PathBuf> {
    let mut roots = Vec::new();
    #[cfg(target_os = "macos")]
    {
        roots.push("/System/Library/Fonts".into());
        roots.push("/System/Library/Fonts/Supplemental".into());
        roots.push("/Library/Fonts".into());
        if let Ok(home) = std::env::var("HOME") {
            roots.push(std::path::Path::new(&home).join("Library/Fonts"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        let win = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        roots.push(std::path::Path::new(&win).join("Fonts"));
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            roots.push(std::path::Path::new(&local).join("Microsoft/Windows/Fonts"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        roots.push("/usr/share/fonts".into());
        roots.push("/usr/local/share/fonts".into());
        if let Ok(home) = std::env::var("HOME") {
            roots.push(std::path::Path::new(&home).join(".local/share/fonts"));
            roots.push(std::path::Path::new(&home).join(".fonts"));
        }
    }
    roots
}

/// The installed file for a choice, if any (None = sentinel/default or
/// not installed). Pattern order in the table is the priority order.
pub fn installed_font_file(choice: &FontChoice) -> Option<std::path::PathBuf> {
    for pat in choice.patterns {
        let pat = norm_stem(pat);
        let mut fallback: Option<std::path::PathBuf> = None;
        for f in font_files() {
            let Some(raw) = f.file_stem().map(|s| s.to_string_lossy()) else {
                continue;
            };
            // Variable fonts can be rejected by the rasterizer.
            if raw.contains('[')
                || raw.ends_with("-vf")
                || raw.ends_with("_vf")
                || raw.ends_with("-VF")
                || raw.ends_with("_VF")
            {
                continue;
            }
            let stem = norm_stem(&raw);
            if !stem.contains(&pat) || choice.exclude.iter().any(|e| stem.contains(&norm_stem(e))) {
                continue;
            }
            // An explicit "regular" face wins over a bare stem (Menlo,
            // SFNSMono, consola…) and its unknown suffixes.
            if stem.contains("regular") {
                return Some(f.clone());
            }
            fallback.get_or_insert_with(|| f.clone());
        }
        if fallback.is_some() {
            return fallback;
        }
    }
    None
}

/// Choices that resolve to a real file — for pickers that only offer
/// what's actually installed.
/// Cached per-choice install flags — the file scan happens once per list,
/// not per frame while a picker is open.
fn installed_flags(list: &'static [FontChoice]) -> &'static [bool] {
    static UI: std::sync::OnceLock<Vec<bool>> = std::sync::OnceLock::new();
    static MONO: std::sync::OnceLock<Vec<bool>> = std::sync::OnceLock::new();
    let slot = if std::ptr::eq(list, UI_FONTS) {
        &UI
    } else {
        &MONO
    };
    slot.get_or_init(|| {
        list.iter()
            .map(|c| c.patterns.is_empty() || is_bundled(c.key) || installed_font_file(c).is_some())
            .collect()
    })
}

pub fn installed_fonts(list: &'static [FontChoice]) -> Vec<(&'static FontChoice, bool)> {
    list.iter()
        .zip(installed_flags(list))
        .map(|(c, i)| (c, *i))
        .collect()
}

fn font_choice_bytes(key: &str, list: &[FontChoice]) -> Option<Vec<u8>> {
    if let Some(b) = bundled_bytes(key) {
        return Some(b.to_vec());
    }
    if let Some(path) = key.strip_prefix("sys:") {
        return std::fs::read(path).ok();
    }
    let choice = list.iter().find(|c| c.key == key)?;
    let path = installed_font_file(choice)?;
    std::fs::read(path).ok()
}

/// A font family discovered in the platform font dirs by parsing its
/// `name` table — beyond the curated pick list.
pub struct SystemFont {
    pub label: String,
    pub path: std::path::PathBuf,
    pub mono: bool,
}

/// All font families installed on the system, regular faces only.
/// Variable fonts, TTC collections, WOFF containers and fonts whose
/// name table can't be parsed are skipped.
pub fn system_fonts(mono: bool) -> &'static [SystemFont] {
    static PROPORTIONAL: std::sync::OnceLock<Vec<SystemFont>> = std::sync::OnceLock::new();
    static MONO: std::sync::OnceLock<Vec<SystemFont>> = std::sync::OnceLock::new();
    let slot = if mono { &MONO } else { &PROPORTIONAL };
    slot.get_or_init(|| {
        let mut by_family: std::collections::BTreeMap<String, SystemFont> =
            std::collections::BTreeMap::new();
        for f in font_files() {
            let Ok(data) = std::fs::read(f) else {
                continue;
            };
            let Some((family, subfamily, is_mono)) = ttf_meta(&data) else {
                continue;
            };
            if is_mono != mono {
                continue;
            }
            let regular = subfamily.is_empty()
                || ["regular", "book", "roman", "normal", "plain"]
                    .iter()
                    .any(|w| subfamily.contains(w));
            let entry = SystemFont {
                label: family.clone(),
                path: f.clone(),
                mono,
            };
            if regular {
                // Always (re)insert: replaces a non-regular face or the
                // previous regular — deterministic given the sorted scan.
                by_family.insert(family.clone(), entry);
            } else {
                by_family.entry(family).or_insert(entry);
            }
        }
        by_family.into_values().collect()
    })
    .as_slice()
}

/// Minimal sfnt reader: family/subfamily from `name`, pitch from `post`.
/// Returns None for TTC/WOFF/variable fonts — all rasterizer risks.
fn ttf_meta(data: &[u8]) -> Option<(String, String, bool)> {
    if data.len() < 12 || &data[..4] != b"\0\x01\0\0" && &data[..4] != b"true" {
        return None;
    }
    let num = u16::from_be_bytes(data[4..6].try_into().ok()?) as usize;
    let (mut name, mut post) = (None, None);
    for i in 0..num {
        let rec = 12 + i * 16;
        if data.len() < rec + 16 {
            return None;
        }
        let off = u32::from_be_bytes(data[rec + 8..rec + 12].try_into().ok()?) as usize;
        let len = u32::from_be_bytes(data[rec + 12..rec + 16].try_into().ok()?) as usize;
        match &data[rec..rec + 4] {
            b"name" => name = Some((off, len)),
            b"post" => post = Some((off, len)),
            b"fvar" => return None,
            _ => {}
        }
    }
    let (off, len) = name?;
    let table = data.get(off..off + len)?;
    if table.len() < 6 {
        return None;
    }
    let count = u16::from_be_bytes(table[2..4].try_into().ok()?) as usize;
    let storage = u16::from_be_bytes(table[4..6].try_into().ok()?) as usize;
    // Best record per platform: prefer Windows (3) UTF-16 names.
    let pick = |want: u16| -> Option<String> {
        let mut mac_fallback = None;
        for i in 0..count {
            let r = 6 + i * 12;
            let rec = table.get(r..r + 12)?;
            let platform = u16::from_be_bytes(rec[0..2].try_into().ok()?);
            let name_id = u16::from_be_bytes(rec[6..8].try_into().ok()?);
            let slen = u16::from_be_bytes(rec[8..10].try_into().ok()?) as usize;
            let soff = u16::from_be_bytes(rec[10..12].try_into().ok()?) as usize;
            if name_id != want {
                continue;
            }
            let raw = table.get(storage + soff..storage + soff + slen)?;
            if platform == 3 {
                // UTF-16BE
                return String::from_utf16(
                    &raw.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|c| u16::from_be_bytes(*c))
                        .collect::<Vec<_>>(),
                )
                .ok();
            }
            mac_fallback.get_or_insert_with(|| raw.iter().map(|&b| b as char).collect::<String>());
        }
        mac_fallback
    };
    let family = pick(16).or_else(|| pick(1))?.trim().to_string();
    if family.is_empty() {
        return None;
    }
    let subfamily = pick(17)
        .or_else(|| pick(2))
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    // post.isFixedPitch is unreliable alone — Nerd Font patching resets
    // it on many builds — so the family name is a second signal.
    let pitched = post
        .and_then(|(o, l)| data.get(o..o + l))
        .is_some_and(|t| t.len() >= 16 && u32::from_be_bytes(t[12..16].try_into().unwrap()) != 0);
    let fname = family.to_lowercase();
    let mono = pitched || fname.contains("mono") || fname.contains("nerd");
    Some((family, subfamily, mono))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every embedded font must be a static sfnt whose name and pitch
    /// tables parse — bad bytes would panic epaint at first paint.
    #[test]
    fn bundled_fonts_are_valid_static_sfnts() {
        for (key, mono) in [
            ("jetbrains-nerd", true),
            ("meslo-nerd", true),
            ("jetbrains", true),
            ("fira-sans", false),
            ("ubuntu", false),
        ] {
            let bytes = bundled_bytes(key).expect("bundled");
            let (family, _sub, is_mono) =
                ttf_meta(bytes).unwrap_or_else(|| panic!("{key} failed name-table parse"));
            assert!(!family.is_empty(), "{key}: empty family");
            assert_eq!(is_mono, mono, "{key}: wrong pitch class ({family})");
        }
    }
}
