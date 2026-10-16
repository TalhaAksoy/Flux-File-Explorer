//! Settings as a first-class page, not a modal: a slim category nav on
//! the left, a centered readable column on the right. Every control
//! applies immediately and persists via the debounced settings writer;
//! the page never blocks on the filesystem (font lists are cached).

use flux_settings::{Settings, Theme, ViewMode};

use crate::app::FluxApp;
use crate::glyphs::{self, Glyph};
use crate::theme::Palette;
use crate::widgets::{self, sp};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Category {
    #[default]
    Appearance,
    Typography,
    Files,
    Views,
    Terminal,
    Keys,
    Advanced,
    About,
}

impl Category {
    pub const ALL: &[Category] = &[
        Category::Appearance,
        Category::Typography,
        Category::Files,
        Category::Views,
        Category::Terminal,
        Category::Keys,
        Category::Advanced,
        Category::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Appearance => "Appearance",
            Category::Typography => "Typography",
            Category::Files => "Files & Folders",
            Category::Views => "File Views",
            Category::Terminal => "Terminal",
            Category::Keys => "Keyboard",
            Category::Advanced => "Advanced",
            Category::About => "About",
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Category::Appearance => Glyph::Sun,
            Category::Typography => Glyph::TypeAa,
            Category::Files => Glyph::Folder,
            Category::Views => Glyph::GridView,
            Category::Terminal => Glyph::Terminal,
            Category::Keys => Glyph::Keyboard,
            Category::Advanced => Glyph::Sliders,
            Category::About => Glyph::Info,
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Category::Appearance => "Theme, interface scale, and overall look.",
            Category::Typography => "Interface and terminal typefaces.",
            Category::Files => "Which entries are listed and how deletions behave.",
            Category::Views => "Default layout, density, sorting, and the preview panel.",
            Category::Terminal => "Embedded terminal typeface.",
            Category::Keys => "Keyboard shortcuts reference.",
            Category::Advanced => "Reset stored layout and preferences.",
            Category::About => "Version and build information.",
        }
    }
}

/// One searchable setting — `render` maps an id to its live control.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingId {
    Theme,
    UiScale,
    UiFont,
    ShowHidden,
    ShowSystem,
    ConfirmDelete,
    ViewMode,
    RowDensity,
    PreviewPanel,
    DirsFirst,
    SortKey,
    SortDir,
    MonoFont,
    TermFontSize,
}

struct SettingMeta {
    id: SettingId,
    cat: Category,
    title: &'static str,
    keywords: &'static str,
}

const REGISTRY: &[SettingMeta] = &[
    SettingMeta {
        id: SettingId::Theme,
        cat: Category::Appearance,
        title: "Theme",
        keywords: "dark light system appearance color",
    },
    SettingMeta {
        id: SettingId::UiScale,
        cat: Category::Appearance,
        title: "Interface scale",
        keywords: "zoom dpi size ui percent",
    },
    SettingMeta {
        id: SettingId::UiFont,
        cat: Category::Typography,
        title: "Interface font",
        keywords: "typeface sans text",
    },
    SettingMeta {
        id: SettingId::ShowHidden,
        cat: Category::Files,
        title: "Show hidden files",
        keywords: "dotfiles invisible",
    },
    SettingMeta {
        id: SettingId::ShowSystem,
        cat: Category::Files,
        title: "Show protected system files",
        keywords: "windows os guarded",
    },
    SettingMeta {
        id: SettingId::ConfirmDelete,
        cat: Category::Files,
        title: "Confirm permanent delete",
        keywords: "trash ask safety",
    },
    SettingMeta {
        id: SettingId::ViewMode,
        cat: Category::Views,
        title: "Default view",
        keywords: "grid gallery compact details list icons",
    },
    SettingMeta {
        id: SettingId::RowDensity,
        cat: Category::Views,
        title: "Row density",
        keywords: "compact comfortable spacing",
    },
    SettingMeta {
        id: SettingId::PreviewPanel,
        cat: Category::Views,
        title: "Preview panel",
        keywords: "sidebar metadata image",
    },
    SettingMeta {
        id: SettingId::DirsFirst,
        cat: Category::Views,
        title: "Folders first",
        keywords: "sort directories before files",
    },
    SettingMeta {
        id: SettingId::SortKey,
        cat: Category::Views,
        title: "Default sort column",
        keywords: "name size modified type order",
    },
    SettingMeta {
        id: SettingId::SortDir,
        cat: Category::Views,
        title: "Default sort direction",
        keywords: "ascending descending",
    },
    SettingMeta {
        id: SettingId::MonoFont,
        cat: Category::Terminal,
        title: "Terminal font",
        keywords: "mono nerd shell",
    },
    SettingMeta {
        id: SettingId::TermFontSize,
        cat: Category::Terminal,
        title: "Terminal font size",
        keywords: "pt points",
    },
];

/// Registry entries whose title/keywords match `query` (case-insensitive,
/// all words must match somewhere).
fn matching(query: &str) -> Vec<&'static SettingMeta> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    REGISTRY
        .iter()
        .filter(|m| {
            q.split_whitespace().all(|w| {
                m.title.to_lowercase().contains(w)
                    || m.keywords.contains(w)
                    || m.cat.label().to_lowercase().contains(w)
            })
        })
        .collect()
}

/// The full page — rendered inside the central panel when
/// `app.screen == Screen::Settings`.
pub fn page(app: &mut FluxApp, ui: &mut egui::Ui) {
    let pal = Palette::of(&ui.style().visuals);
    let ctx = ui.ctx().clone();

    // ── Header: back affordance + title + search ──
    ui.add_space(sp::MD);
    ui.horizontal(|ui| {
        if back_button(ui, &pal, "Back to files (Esc)").clicked() {
            app.save_settings();
            app.close_screen();
            return;
        }
        ui.add_space(sp::SM);
        ui.label(
            egui::RichText::new("Settings")
                .color(pal.text_primary)
                .size(22.0)
                .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let width = ui.available_width().clamp(120.0, 240.0);
            if let crate::app::Screen::Settings(sui) = &mut app.screen {
                let r = widgets::text_field(ui, &mut sui.query, "Search settings", width);
                if r.changed() {
                    r.request_focus();
                }
            }
        });
    });
    ui.add_space(sp::MD);

    let Some(sui) = (match &mut app.screen {
        crate::app::Screen::Settings(s) => Some(s),
        _ => None,
    }) else {
        return;
    };
    let query = sui.query.clone();
    let category = sui.category;
    let searching = !query.trim().is_empty();

    let narrow = ui.available_width() < 720.0;
    if narrow {
        // Compact form: category picker on top instead of a nav column.
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("settings_cat")
                .width(200.0)
                .selected_text(egui::RichText::new(category.label()).size(13.5))
                .show_ui(ui, |ui| {
                    for c in Category::ALL {
                        if ui.selectable_label(*c == category, c.label()).clicked()
                            && let crate::app::Screen::Settings(s) = &mut app.screen
                        {
                            s.category = *c;
                        }
                    }
                });
        });
        ui.add_space(sp::SM);
        widgets::row_divider(ui, &pal);
        content(app, ui, &ctx, &pal, category, &query, searching);
        return;
    }

    // Wide layout: split the remaining region manually. `ui.horizontal`
    // children only get `interact_size.y` of height until they grow, so a
    // plain horizontal would leave both columns vertically compressed.
    let avail = ui.available_rect_before_wrap();
    ui.allocate_rect(avail, egui::Sense::hover());
    let nav_w = 190.0_f32.min(avail.width() * 0.35);
    let sep_x = avail.left() + nav_w + 5.0;
    let nav_rect = egui::Rect::from_min_size(avail.min, egui::vec2(nav_w, avail.height()));
    let content_rect = egui::Rect::from_min_max(egui::pos2(sep_x + 5.0, avail.top()), avail.max);

    // ── Category nav ──
    let mut nav_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(nav_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("setnav")
        .show(&mut nav_ui, |ui| {
            for c in Category::ALL {
                let selected = !searching && *c == category;
                let resp = nav_row(ui, &pal, *c, selected);
                if resp.clicked()
                    && let crate::app::Screen::Settings(s) = &mut app.screen
                {
                    s.category = *c;
                    s.query.clear();
                }
            }
        });

    ui.painter().vline(
        sep_x,
        avail.top()..=avail.bottom(),
        egui::Stroke::new(1.0, pal.border_soft),
    );

    // ── Content column, centered, capped width ──
    let avail_w = content_rect.width();
    let col_w = (avail_w - sp::XXL * 2.0).clamp(320.0, 880.0).min(avail_w);
    let x_off = ((avail_w - col_w) / 2.0).max(0.0);
    let col_rect = egui::Rect::from_min_size(
        egui::pos2(content_rect.left() + x_off, content_rect.top()),
        egui::vec2(col_w, content_rect.height()),
    );
    let mut col_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(col_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    content(app, &mut col_ui, &ctx, &pal, category, &query, searching);
}

/// Chevron-back + label used as the page's exit affordance.
fn back_button(ui: &mut egui::Ui, pal: &Palette, tip: &str) -> egui::Response {
    let size = egui::vec2(34.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let resp = resp.on_hover_text(tip);
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(7), pal.surface_hover);
    }
    // Flip the Forward arrow for a left-pointing chevron-free look:
    // reuse the Back arrow glyph (already a left arrow).
    glyphs::paint_glyph(
        ui.painter(),
        rect.shrink(6.0),
        Glyph::Back,
        pal.text_primary,
    );
    resp
}

/// A nav row: icon + label, selected pill, comfortable hit target.
fn nav_row(ui: &mut egui::Ui, pal: &Palette, cat: Category, selected: bool) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 34.0), egui::Sense::click());
    let pill = rect.shrink2(egui::vec2(2.0, 2.0));
    if selected {
        ui.painter()
            .rect_filled(pill, egui::CornerRadius::same(8), pal.accent_fill);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(pill, egui::CornerRadius::same(8), pal.surface_hover);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 18.0, rect.center().y),
        egui::Vec2::splat(16.0),
    );
    glyphs::paint_glyph(
        ui.painter(),
        icon_rect,
        cat.glyph(),
        if selected {
            pal.accent
        } else {
            pal.text_secondary
        },
    );
    ui.painter().text(
        egui::pos2(rect.left() + 38.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        cat.label(),
        egui::FontId::proportional(13.5),
        if selected {
            pal.text_primary
        } else {
            pal.text_secondary
        },
    );
    resp
}

fn content(
    app: &mut FluxApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    pal: &Palette,
    category: Category,
    query: &str,
    searching: bool,
) {
    egui::ScrollArea::vertical()
        .id_salt("settings_content")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(sp::XS);
            if searching {
                // Search mode: every matching control, live, grouped under
                // its category. No fake navigation-only results.
                let hits = matching(query);
                if hits.is_empty() {
                    widgets::empty_state(
                        ui,
                        pal,
                        |p, r, c| glyphs::paint_glyph(p, r, Glyph::Search, c),
                        "No settings match",
                        Some("Try a different search term"),
                    );
                    return;
                }
                ui.label(
                    egui::RichText::new(format!("{} matching settings", hits.len()))
                        .color(pal.text_muted)
                        .size(12.0),
                );
                let mut last_cat: Option<Category> = None;
                for m in hits {
                    if last_cat != Some(m.cat) {
                        widgets::section_heading(ui, pal, m.cat.label(), None);
                        last_cat = Some(m.cat);
                    }
                    setting_row(app, ui, ctx, pal, m.id);
                }
                return;
            }
            ui.label(
                egui::RichText::new(category.label())
                    .color(pal.text_primary)
                    .size(19.0)
                    .strong(),
            );
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(category.blurb())
                    .color(pal.text_muted)
                    .size(12.5),
            );
            match category {
                Category::Appearance => appearance(app, ui, ctx, pal),
                Category::Typography => typography(app, ui, ctx, pal),
                Category::Files => files(app, ui, ctx, pal),
                Category::Views => views(app, ui, ctx, pal),
                Category::Terminal => terminal(app, ui, ctx, pal),
                Category::Keys => keys(ui, pal),
                Category::Advanced => advanced(app, ui, ctx, pal),
                Category::About => about(app, ui, pal),
            }
        });
}

/// Dispatch one live setting row (used both by category pages and search).
fn setting_row(
    app: &mut FluxApp,
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    pal: &Palette,
    id: SettingId,
) {
    match id {
        SettingId::Theme => theme_row(app, ui, ctx, pal),
        SettingId::UiScale => scale_row(app, ui, ctx, pal),
        SettingId::UiFont => widgets::setting_row(
            ui,
            pal,
            "Interface font",
            Some("Used for all labels, menus, and file names."),
            |ui| {
                if let Some(k) = font_picker(
                    ui,
                    pal,
                    "ui_font",
                    &app.settings.ui_font,
                    crate::theme::UI_FONTS,
                ) {
                    app.settings.ui_font = k;
                    ctx.set_fonts(crate::theme::load_fonts(
                        &app.settings.ui_font,
                        &app.settings.mono_font,
                    ));
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            },
        ),
        SettingId::MonoFont => widgets::setting_row(
            ui,
            pal,
            "Terminal font",
            Some("Monospace face for the embedded terminal. Bundled Nerd Fonts always work."),
            |ui| {
                if let Some(k) = font_picker(
                    ui,
                    pal,
                    "mono_font",
                    &app.settings.mono_font,
                    crate::theme::MONO_FONTS,
                ) {
                    app.settings.mono_font = k;
                    ctx.set_fonts(crate::theme::load_fonts(
                        &app.settings.ui_font,
                        &app.settings.mono_font,
                    ));
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            },
        ),
        SettingId::TermFontSize => {
            widgets::setting_row(ui, pal, "Terminal font size", None, |ui| {
                let r = ui.add(
                    egui::Slider::new(&mut app.settings.terminal_font_size, 9.0..=20.0)
                        .suffix(" pt")
                        .fixed_decimals(1),
                );
                if r.changed() {
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            })
        }
        SettingId::ShowHidden => widgets::setting_row(
            ui,
            pal,
            "Show hidden files",
            Some(
                "Dotfiles and entries carrying the platform hidden attribute. Shown dimmed. (Ctrl+H)",
            ),
            |ui| {
                let mut v = app.settings.show_hidden;
                if widgets::switch(ui, &mut v).changed() {
                    app.toggle_hidden();
                }
            },
        ),
        SettingId::ShowSystem => widgets::setting_row(
            ui,
            pal,
            "Show protected system files",
            Some(if cfg!(windows) {
                "Entries marked both hidden and system (FILE_ATTRIBUTE_SYSTEM). Shown dimmed; not recommended for everyday browsing."
            } else {
                "Only has an effect on Windows, where entries marked hidden+system are filtered separately. No-op here."
            }),
            |ui| {
                let mut v = app.settings.show_system_files;
                if widgets::switch(ui, &mut v).changed() {
                    app.set_show_system_files(v);
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            },
        ),
        SettingId::ConfirmDelete => widgets::setting_row(
            ui,
            pal,
            "Confirm permanent delete",
            Some("Ask before Shift+Delete permanently removes items."),
            |ui| {
                let mut v = app.settings.confirm_permanent_delete;
                if widgets::switch(ui, &mut v).changed() {
                    app.settings.confirm_permanent_delete = v;
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            },
        ),
        SettingId::ViewMode => widgets::setting_row(
            ui,
            pal,
            "Default view",
            Some("Presentation used for new folders."),
            |ui| {
                let cur = app.settings.view_mode;
                if let Some(v) = widgets::segmented(
                    ui,
                    pal,
                    &[
                        (ViewMode::List, "Details"),
                        (ViewMode::Compact, "Compact"),
                        (ViewMode::Grid, "Grid"),
                        (ViewMode::Gallery, "Gallery"),
                    ],
                    cur,
                ) {
                    app.set_view_mode(v);
                }
            },
        ),
        SettingId::RowDensity => widgets::setting_row(
            ui,
            pal,
            "Row density",
            Some("Height of rows in the file list."),
            |ui| {
                let cur = app.density;
                if let Some(v) = widgets::segmented(
                    ui,
                    pal,
                    &[
                        (crate::theme::Density::Compact, "Compact"),
                        (crate::theme::Density::Standard, "Standard"),
                        (crate::theme::Density::Comfortable, "Comfortable"),
                    ],
                    cur,
                ) {
                    app.density = v;
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            },
        ),
        SettingId::PreviewPanel => widgets::setting_row(
            ui,
            pal,
            "Preview panel",
            Some("Right-side preview and metadata pane. (Alt+P)"),
            |ui| {
                let mut v = app.settings.show_preview;
                if widgets::switch(ui, &mut v).changed() {
                    app.toggle_preview();
                }
            },
        ),
        SettingId::DirsFirst => widgets::setting_row(
            ui,
            pal,
            "Folders first",
            Some("Group directories above files regardless of sort column."),
            |ui| {
                let mut v = app.settings.dirs_first;
                if widgets::switch(ui, &mut v).changed() {
                    app.settings.dirs_first = v;
                    let cur = {
                        let s = app.current_tab().snap.read().unwrap();
                        s.sort
                    };
                    let mut spec = cur;
                    spec.dirs_first = v;
                    app.set_sort(spec);
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            },
        ),
        SettingId::SortKey => widgets::setting_row(
            ui,
            pal,
            "Default sort column",
            Some("Sort order applied to new folders."),
            |ui| {
                let cur = app.settings.sort_key.clone();
                if let Some(v) = widgets::segmented(
                    ui,
                    pal,
                    &[
                        ("name", "Name"),
                        ("size", "Size"),
                        ("modified", "Modified"),
                        ("kind", "Type"),
                    ],
                    cur.as_str(),
                ) {
                    app.settings.sort_key = v.to_string();
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            },
        ),
        SettingId::SortDir => widgets::setting_row(ui, pal, "Default sort direction", None, |ui| {
            let desc = app.settings.sort_desc;
            if let Some(v) =
                widgets::segmented(ui, pal, &[(false, "Ascending"), (true, "Descending")], desc)
            {
                app.settings.sort_desc = v;
                app.settings_dirty_at = Some(std::time::Instant::now());
            }
        }),
    }
}

// ── Category bodies ──

fn appearance(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    theme_row(app, ui, ctx, pal);
    widgets::row_divider(ui, pal);
    scale_row(app, ui, ctx, pal);
    widgets::row_divider(ui, pal);
    widgets::setting_row(
        ui,
        pal,
        "Density",
        Some("Row height in the file list."),
        |ui| {
            let cur = app.density;
            if let Some(v) = widgets::segmented(
                ui,
                pal,
                &[
                    (crate::theme::Density::Compact, "Compact"),
                    (crate::theme::Density::Standard, "Standard"),
                    (crate::theme::Density::Comfortable, "Comfortable"),
                ],
                cur,
            ) {
                app.density = v;
                app.settings_dirty_at = Some(std::time::Instant::now());
            }
        },
    );
}

fn theme_row(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    widgets::setting_row(
        ui,
        pal,
        "Theme",
        Some("System follows the OS light/dark preference."),
        |ui| {
            let cur = app.settings.theme;
            if let Some(v) = widgets::segmented(
                ui,
                pal,
                &[
                    (Theme::Light, "Light"),
                    (Theme::Dark, "Dark"),
                    (Theme::System, "System"),
                ],
                cur,
            ) {
                app.settings.theme = v;
                app.dark = match v {
                    Theme::System => ctx.system_theme().is_some_and(|t| t == egui::Theme::Dark),
                    Theme::Dark => true,
                    Theme::Light => false,
                };
                crate::theme::apply_theme(ctx, app.dark);
                app.settings_dirty_at = Some(std::time::Instant::now());
            }
        },
    );
}

fn scale_row(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    widgets::setting_row(
        ui,
        pal,
        "Interface scale",
        Some("Zoom applied to the whole interface. The window grows to compensate."),
        |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut app.settings.ui_scale, 0.8..=1.5).show_value(false));
                ui.label(
                    egui::RichText::new(format!("{:.0}%", app.settings.ui_scale * 100.0))
                        .color(pal.text_secondary)
                        .size(12.5),
                );
                if widgets::secondary_button(ui, "Reset").clicked() {
                    app.settings.ui_scale = 1.0;
                    app.apply_ui_scale(ctx);
                    app.settings_dirty_at = Some(std::time::Instant::now());
                }
            });
        },
    );
    // Commit the zoom once the pointer releases — live zoom resizes this
    // very page under the cursor, so dragging only updates the number.
    if app.settings.ui_scale != ctx.zoom_factor() && !ui.input(|i| i.pointer.primary_down()) {
        app.apply_ui_scale(ctx);
        app.settings_dirty_at = Some(std::time::Instant::now());
    }
}

fn typography(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    setting_row(app, ui, ctx, pal, SettingId::UiFont);
    widgets::row_divider(ui, pal);
    widgets::section_heading(
        ui,
        pal,
        "Sample",
        Some("How the interface font renders at each level."),
    );
    // Clean sample card: heading / body / filename / metadata / mono.
    egui::Frame::default()
        .fill(pal.field_inset)
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Documents")
                    .size(20.0)
                    .strong()
                    .color(pal.text_primary),
            );
            ui.add_space(sp::XS);
            ui.label(
                egui::RichText::new("12 items · report.pdf selected")
                    .size(13.5)
                    .color(pal.text_secondary),
            );
            ui.add_space(sp::SM);
            ui.horizontal(|ui| {
                let (ir, _) = ui.allocate_exact_size(egui::Vec2::splat(15.0), egui::Sense::hover());
                crate::view::paint_entry_icon(ui.painter(), ir, flux_core::EntryKind::Dir, pal);
                ui.label(egui::RichText::new("Photos Library").size(13.5));
            });
            ui.horizontal(|ui| {
                let (ir, _) = ui.allocate_exact_size(egui::Vec2::splat(15.0), egui::Sense::hover());
                crate::view::paint_entry_icon(ui.painter(), ir, flux_core::EntryKind::File, pal);
                ui.label(egui::RichText::new("notes.txt").size(13.5));
                ui.label(
                    egui::RichText::new("2.4 KB · modified today")
                        .size(11.5)
                        .color(pal.text_muted),
                );
            });
            ui.add_space(sp::SM);
            ui.label(
                egui::RichText::new("$ ls ~/Projects — terminal sample")
                    .monospace()
                    .size(12.0)
                    .color(pal.text_secondary),
            );
        });
    ui.add_space(sp::SM);
    ui.label(
        egui::RichText::new("Terminal typeface lives under the Terminal section.")
            .color(pal.text_muted)
            .size(11.5),
    );
}

fn files(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    setting_row(app, ui, ctx, pal, SettingId::ShowHidden);
    widgets::row_divider(ui, pal);
    setting_row(app, ui, ctx, pal, SettingId::ShowSystem);
    widgets::row_divider(ui, pal);
    setting_row(app, ui, ctx, pal, SettingId::ConfirmDelete);
}

fn views(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    setting_row(app, ui, ctx, pal, SettingId::ViewMode);
    widgets::row_divider(ui, pal);
    setting_row(app, ui, ctx, pal, SettingId::RowDensity);
    widgets::row_divider(ui, pal);
    setting_row(app, ui, ctx, pal, SettingId::PreviewPanel);
    widgets::section_heading(ui, pal, "Sorting", None);
    setting_row(app, ui, ctx, pal, SettingId::SortKey);
    widgets::row_divider(ui, pal);
    setting_row(app, ui, ctx, pal, SettingId::SortDir);
    widgets::row_divider(ui, pal);
    setting_row(app, ui, ctx, pal, SettingId::DirsFirst);
}

fn terminal(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    setting_row(app, ui, ctx, pal, SettingId::MonoFont);
    widgets::row_divider(ui, pal);
    setting_row(app, ui, ctx, pal, SettingId::TermFontSize);
    widgets::section_heading(ui, pal, "Sample", None);
    egui::Frame::default()
        .fill(egui::Color32::from_black_alpha(160))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("~/Projects/flux $ ls\nREADME.md  crates  target\n$ _")
                    .monospace()
                    .size(app.settings.terminal_font_size)
                    .color(egui::Color32::from_rgb(200, 205, 212)),
            );
        });
}

fn keys(ui: &mut egui::Ui, pal: &Palette) {
    use crate::format::MOD_KEY as CMD;
    let groups: &[(&str, &[(&str, String)])] = &[
        (
            "Navigation",
            &[
                ("Back / Forward", "Alt+← / Alt+→".into()),
                ("Up one level", "Backspace".into()),
                ("Focus address bar", format!("{CMD}+L")),
                ("New / close tab", format!("{CMD}+T / {CMD}+W")),
                ("Switch tab", format!("{CMD}+Tab")),
            ],
        ),
        (
            "Files",
            &[
                ("Copy / Cut / Paste", format!("{CMD}+C / {CMD}+X / {CMD}+V")),
                ("Duplicate", format!("{CMD}+D")),
                ("Rename", "F2".into()),
                ("Move to trash", "Del".into()),
                ("Delete permanently", "Shift+Del".into()),
                ("Undo / redo", format!("{CMD}+Z / {CMD}+Shift+Z")),
            ],
        ),
        (
            "View",
            &[
                ("Toggle hidden files", format!("{CMD}+H or {CMD}+Shift+.")),
                ("Preview panel", "Alt+P".into()),
                ("Filter / search", format!("{CMD}+F")),
                ("Refresh", "F5".into()),
                ("Command palette", format!("{CMD}+Shift+P")),
            ],
        ),
    ];
    for (title, rows) in groups {
        widgets::section_heading(ui, pal, title, None);
        for (action, keys) in *rows {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(*action)
                        .color(pal.text_secondary)
                        .size(13.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Keyboard chips: mono text on an inset cap.
                    egui::Frame::default()
                        .fill(pal.field_inset)
                        .corner_radius(egui::CornerRadius::same(5))
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(keys)
                                    .monospace()
                                    .size(11.5)
                                    .color(pal.text_primary),
                            );
                        });
                });
            });
            ui.add_space(2.0);
        }
    }
}

fn advanced(app: &mut FluxApp, ui: &mut egui::Ui, ctx: &egui::Context, pal: &Palette) {
    widgets::setting_row(
        ui,
        pal,
        "Column widths",
        Some("Restore the default column widths for the details view."),
        |ui| {
            if widgets::secondary_button(ui, "Reset").clicked() {
                app.settings.column_widths = Settings::default().column_widths;
                app.settings_dirty_at = Some(std::time::Instant::now());
            }
        },
    );
    widgets::row_divider(ui, pal);
    widgets::section_heading(
        ui,
        pal,
        "Reset everything",
        Some(
            "Restores every preference to its default — theme, fonts, view modes, sidebar, sort. Open tabs stay open.",
        ),
    );
    let armed = matches!(&app.screen, crate::app::Screen::Settings(s) if s.reset_armed);
    ui.horizontal(|ui| {
        if !armed {
            if widgets::secondary_button(ui, "Reset all settings…").clicked()
                && let crate::app::Screen::Settings(s) = &mut app.screen
            {
                s.reset_armed = true;
            }
        } else {
            ui.label(
                egui::RichText::new("Reset all preferences?")
                    .color(pal.danger)
                    .size(13.0),
            );
            if widgets::danger_button(ui, "Reset").clicked() {
                let keep_tabs = app.settings.tabs.clone();
                let keep_bms = app.settings.bookmarks.clone();
                app.settings = Settings::default();
                app.settings.tabs = keep_tabs;
                app.settings.bookmarks = keep_bms;
                app.density = crate::theme::Density::Standard;
                app.dark = ctx.system_theme().is_some_and(|t| t == egui::Theme::Dark);
                ctx.set_fonts(crate::theme::load_fonts("", ""));
                app.apply_ui_scale(ctx);
                crate::theme::apply_theme(ctx, app.dark);
                app.save_settings();
                if let crate::app::Screen::Settings(s) = &mut app.screen {
                    s.reset_armed = false;
                }
            }
            if widgets::ghost_button(ui, "Cancel").clicked()
                && let crate::app::Screen::Settings(s) = &mut app.screen
            {
                s.reset_armed = false;
            }
        }
    });
}

fn about(_app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette) {
    ui.horizontal(|ui| {
        let (ir, _) = ui.allocate_exact_size(egui::Vec2::splat(52.0), egui::Sense::hover());
        crate::view::paint_entry_icon(ui.painter(), ir, flux_core::EntryKind::Dir, pal);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Flux Files")
                    .size(20.0)
                    .strong()
                    .color(pal.text_primary),
            );
            ui.label(
                egui::RichText::new(format!(
                    "Version {} · {} {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
                .size(12.0)
                .color(pal.text_muted),
            );
        });
    });
    ui.add_space(sp::SM);
    ui.label(
        egui::RichText::new(
            "A native file manager written in Rust with egui — fast on huge directories, \
             careful with your files.",
        )
        .size(13.0)
        .color(pal.text_secondary),
    );
}

// ── Font picker ──

/// Combo picker for a `FontChoice` list: bundled fonts render in their own
/// typeface, system picks are filtered to installed ones, and a memory-
/// backed filter box keeps long lists navigable.
fn font_picker(
    ui: &mut egui::Ui,
    pal: &Palette,
    id: &str,
    key: &str,
    list: &'static [crate::theme::FontChoice],
) -> Option<String> {
    let mono = std::ptr::eq(list, crate::theme::MONO_FONTS);
    let cur = if let Some(rest) = key.strip_prefix("sys:") {
        crate::theme::system_fonts(mono)
            .iter()
            .find(|f| f.path == std::path::Path::new(rest))
            .map(|f| f.label.clone())
            .unwrap_or_else(|| {
                std::path::Path::new(rest)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Custom".into())
            })
    } else {
        list.iter()
            .find(|c| c.key == key)
            .map(|c| c.label.to_string())
            .unwrap_or_else(|| list[0].label.to_string())
    };
    let mut picked: Option<String> = None;
    let filter_id = ui.id().with(("font_filter", id));
    egui::ComboBox::from_id_salt(("font", id))
        .width(250.0)
        .selected_text(egui::RichText::new(cur).size(13.0))
        .show_ui(ui, |ui| {
            ui.set_min_width(300.0);
            let mut q: String = ui.data_mut(|d| d.get_temp(filter_id)).unwrap_or_default();
            let fr = widgets::text_field(ui, &mut q, "Filter fonts…", f32::INFINITY);
            if fr.changed() {
                ui.data_mut(|d| d.insert_temp(filter_id, q.clone()));
            }
            if ui.memory(|m| m.focused().is_none()) {
                fr.request_focus();
            }
            let needle = q.trim().to_lowercase();
            let group = |ui: &mut egui::Ui, text: &str| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(text).size(10.5).color(pal.text_muted));
            };
            let mut pick = |ui: &mut egui::Ui,
                            k: String,
                            text: String,
                            family: Option<egui::FontFamily>,
                            enabled: bool| {
                if !needle.is_empty() && !text.to_lowercase().contains(&needle) {
                    return;
                }
                let rt = match family {
                    Some(f) => egui::RichText::new(text).family(f),
                    None => egui::RichText::new(text),
                };
                if ui
                    .add_enabled(enabled, egui::Button::selectable(key == k, rt))
                    .clicked()
                {
                    picked = Some(k);
                }
            };
            pick(ui, String::new(), list[0].label.to_string(), None, true);
            group(ui, "Bundled");
            for (c, _) in crate::theme::installed_fonts(list)
                .iter()
                .filter(|(c, _)| crate::theme::is_bundled(c.key))
            {
                pick(
                    ui,
                    c.key.to_string(),
                    c.label.to_string(),
                    crate::theme::bundled_family(c.key),
                    true,
                );
            }
            group(ui, "System");
            for (c, inst) in crate::theme::installed_fonts(list)
                .iter()
                .filter(|(c, i)| !crate::theme::is_bundled(c.key) && !c.patterns.is_empty() && *i)
            {
                let _ = inst;
                pick(ui, c.key.to_string(), c.label.to_string(), None, true);
            }
            let curated: Vec<String> = list
                .iter()
                .map(|c| crate::theme::norm_label(c.label))
                .collect();
            for f in crate::theme::system_fonts(mono) {
                if curated.contains(&crate::theme::norm_label(&f.label)) {
                    continue;
                }
                pick(
                    ui,
                    format!("sys:{}", f.path.display()),
                    f.label.clone(),
                    None,
                    true,
                );
            }
        });
    picked
}
