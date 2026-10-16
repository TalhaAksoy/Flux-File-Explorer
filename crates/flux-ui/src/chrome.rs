//! Window chrome: toolbar, breadcrumb, sidebar, tab bar, status bar.
//! Pure rendering + intents; the app executes them.

use std::path::PathBuf;

use crate::app::FluxApp;
use crate::format::{format_count, format_size};
use crate::glyphs;

#[derive(Default)]
pub struct ChromeAction {
    pub navigate: Option<PathBuf>,
    pub back: bool,
    pub forward: bool,
    pub up: bool,
    pub refresh: bool,
    pub go_home: bool,
    pub new_tab: bool,
    pub close_tab: Option<usize>,
    pub activate_tab: Option<usize>,
    pub toggle_hidden: bool,
    pub toggle_search: bool,
    pub toggle_density: bool,
    pub toggle_theme: bool,
    pub open_terminal: bool,
    pub new_folder: bool,
    pub new_file: bool,
    pub open_palette: bool,
    pub set_filter: Option<String>,
    pub run_search: Option<String>,
    pub clear_search: bool,
    pub paste: bool,
    pub cancel_op: Option<u64>,
    /// Directory widget currently hovered while a file drag is active —
    /// the drop target on release.
    pub drop_target: Option<PathBuf>,
    /// Sidebar context menu → open this path in a new tab.
    pub new_tab_at: Option<PathBuf>,
    /// Pointer is over the Bookmarks drop zone while dragging — a release
    /// pins the dragged folders instead of moving them anywhere.
    pub bookmark_zone: bool,
}

/// During a drag, mark `resp` as a drop target for `dir` when hovered and
/// paint a highlight.
fn drop_target(
    app: &FluxApp,
    ui: &egui::Ui,
    resp: &egui::Response,
    dir: &std::path::Path,
    act: &mut ChromeAction,
) {
    // `contains_pointer`, not `hovered`: egui reports `hovered` only on the
    // dragged widget while a pointer button is down, so drop zones must use
    // the pointer-position test to glow and register during the drag.
    if app.dragging.is_some() && resp.contains_pointer() {
        act.drop_target = Some(dir.to_path_buf());
        glyphs::glow_drop_target(ui.painter(), resp.rect, ui.visuals().selection.stroke.color);
    }
}

/// While a sidebar row is being reordered, check whether `resp` is the
/// hovered row: paint the accent insertion line at the nearer edge and
/// return the index the dragged row should be moved to. `member` gates it
/// to the section the dragged item actually belongs to.
fn reorder_zone(
    app: &FluxApp,
    ui: &egui::Ui,
    resp: &egui::Response,
    row_idx: usize,
    member: bool,
) -> Option<usize> {
    if app.sidebar_drag.is_none() || !member || !resp.contains_pointer() {
        return None;
    }
    let y = ui
        .input(|i| i.pointer.latest_pos())
        .map(|p| p.y)
        .unwrap_or_default();
    let top = y < resp.rect.center().y;
    let edge = if top {
        resp.rect.top()
    } else {
        resp.rect.bottom()
    };
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(resp.rect.left() + 4.0, edge - 1.0),
            egui::pos2(resp.rect.right() - 4.0, edge + 1.0),
        ),
        1.0,
        ui.visuals().selection.stroke.color,
    );
    Some(if top { row_idx } else { row_idx + 1 })
}

/// Top toolbar: nav buttons, breadcrumb / editable path, filter field.
pub fn toolbar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    // Toolbar chrome runs larger than the file list — primary navigation
    // deserves bigger hit targets and text. Scoped to this Ui only.
    {
        let st = ui.style_mut();
        st.spacing.interact_size.y = 32.0;
        st.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        st.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
    }
    ui.horizontal(|ui| {
        // Click empty toolbar space to edit the path directly. Registered
        // FIRST so real widgets (nav buttons, crumbs, search) — registered
        // later — win egui's last-registered-wins hit test over their rects.
        let crumb_edit = ui.interact(
            ui.max_rect(),
            egui::Id::new("crumb_edit"),
            egui::Sense::click(),
        );
        let tab = app.current_tab();
        let can_back = tab.history.can_back();
        let can_fwd = tab.history.can_forward();
        let pal = crate::theme::Palette::of(&ui.style().visuals);
        // Section plates — a faint elevated surface behind each control
        // group so toolbar regions read as units, not a wall of icons.
        let plate = egui::Frame::default()
            .fill(pal.bg_elevated)
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::same(2));
        plate.show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if glyphs::icon_button(ui, glyphs::Glyph::Back, "Back (Alt+Left)", can_back).clicked() {
                act.back = true;
            }
            if glyphs::icon_button(ui, glyphs::Glyph::Forward, "Forward (Alt+Right)", can_fwd)
                .clicked()
            {
                act.forward = true;
            }
            if glyphs::icon_button(ui, glyphs::Glyph::Up, "Up (Backspace)", true).clicked() {
                act.up = true;
            }
            if glyphs::icon_button(ui, glyphs::Glyph::Refresh, "Refresh (F5)", true).clicked() {
                act.refresh = true;
            }
        });
        ui.add_space(6.0);

        // Breadcrumb or editable path field.
        if let Some(text) = &mut app.address_edit {
            let id = egui::Id::new("address_bar");
            let resp = ui.add_sized(
                [(ui.available_width() - 320.0).max(80.0), 26.0],
                egui::TextEdit::singleline(text)
                    .margin(egui::Margin::symmetric(10, 4))
                    .id(id),
            );
            if app.focus_address {
                app.focus_address = false;
                resp.request_focus();
            }
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                act.navigate = Some(PathBuf::from(text.trim()));
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.address_edit = None;
            }
        } else {
            let path = app.current_path();
            let mut crumbs = Vec::new();
            let mut cur = Some(path.as_path());
            while let Some(p) = cur {
                crumbs.push(p.to_path_buf());
                cur = p.parent();
            }
            crumbs.reverse();
            let crumb_plate = egui::Frame::default()
                .fill(pal.field_inset)
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(6, 1));
            crumb_plate.show(ui, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("crumbs")
                    .max_width((ui.available_width() - 320.0).max(60.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let n = crumbs.len();
                            for (i, c) in crumbs.into_iter().enumerate() {
                                let label = c
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| c.display().to_string());
                                let last = i == n - 1;
                                // Ghost crumb: quiet text, pill hover, accent
                                // only for the current directory.
                                let size = egui::vec2(
                                    ui.painter()
                                        .layout_no_wrap(
                                            label.clone(),
                                            egui::FontId::proportional(14.0),
                                            pal.text_secondary,
                                        )
                                        .size()
                                        .x
                                        + 14.0,
                                    26.0,
                                );
                                let (rect, resp) =
                                    ui.allocate_exact_size(size, egui::Sense::click());
                                if resp.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        egui::CornerRadius::same(6),
                                        pal.surface_hover,
                                    );
                                }
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    &label,
                                    egui::FontId::proportional(14.0),
                                    if last {
                                        pal.text_primary
                                    } else {
                                        pal.text_secondary
                                    },
                                );
                                drop_target(app, ui, &resp, &c, act);
                                if resp.clicked() {
                                    act.navigate = Some(c.clone());
                                }
                                if !last {
                                    ui.painter().text(
                                        egui::pos2(rect.right() + 7.0, rect.center().y),
                                        egui::Align2::CENTER_CENTER,
                                        "›",
                                        egui::FontId::proportional(13.0),
                                        pal.text_muted,
                                    );
                                    ui.add_space(10.0);
                                }
                            }
                        });
                    });
            });
            // The background interact was registered before the buttons —
            // it only wins clicks that land on empty space.
            if crumb_edit.clicked() {
                app.address_edit = Some(path.display().to_string());
                app.focus_address = true;
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if glyphs::icon_button(ui, glyphs::Glyph::Gear, "Settings", true).clicked() {
                app.open_settings();
            }
            ui.add_space(4.0);
            // Search mode toggle + filter/search box.
            let resp = ui.add_sized(
                [200.0, 26.0],
                egui::TextEdit::singleline(&mut app.filter_text)
                    .margin(egui::Margin::symmetric(10, 4))
                    .hint_text(if app.search_ui.active {
                        "Search subfolders…"
                    } else {
                        "Filter…"
                    })
                    .id(egui::Id::new("filter_box")),
            );
            if app.focus_filter {
                app.focus_filter = false;
                resp.request_focus();
            }
            if resp.changed() {
                if app.search_ui.active {
                    app.search_ui.last_edit = std::time::Instant::now();
                    app.search_ui.dirty = true;
                } else {
                    act.set_filter = Some(app.filter_text.clone());
                }
            }
            if glyphs::icon_toggle(
                ui,
                glyphs::Glyph::Search,
                "Toggle recursive search",
                app.search_ui.active,
            )
            .clicked()
            {
                act.toggle_search = true;
            }
        });
    });
}

/// Left sidebar: places, bookmarks, volumes.
pub fn sidebar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    // Taller rows — comfortable hit targets, the pill gets real padding.
    ui.style_mut().spacing.interact_size.y = 32.0;
    ui.add_space(6.0);
    let pal = crate::theme::Palette::of(&ui.style().visuals);
    let current = app.current_path();
    let folder_icon = |p: &egui::Painter, r: egui::Rect, _c: egui::Color32| {
        crate::view::paint_entry_icon(p, r, flux_core::EntryKind::Dir, &pal)
    };
    let section = |ui: &mut egui::Ui, label: &str| {
        ui.add_space(8.0);
        ui.label(egui::RichText::new(label).color(pal.text_muted).size(11.0));
    };
    egui::ScrollArea::vertical()
        .id_salt("sidebar")
        .show(ui, |ui| {
            section(ui, "Places");
            let mut places_insert: Option<usize> = None;
            let places = app.ordered_places();
            for (i, (label, path)) in places.into_iter().enumerate() {
                let resp = glyphs::icon_row(ui, folder_icon, label, path == current);
                resp.context_menu(|ui| {
                    crate::menu::begin(ui, 180.0);
                    if crate::menu::item_icon(ui, glyphs::Glyph::External, "Open").clicked() {
                        act.navigate = Some(path.clone());
                        ui.close();
                    }
                    if crate::menu::item_icon(ui, glyphs::Glyph::Plus, "Open in New Tab").clicked()
                    {
                        act.new_tab_at = Some(path.clone());
                        ui.close();
                    }
                });
                drop_target(app, ui, &resp, &path, act);
                if app.dragging.is_none()
                    && app.sidebar_drag.is_none()
                    && resp.drag_started_by(egui::PointerButton::Primary)
                {
                    app.sidebar_drag = Some(crate::app::SidebarDrag {
                        path: path.clone(),
                        section: crate::app::SidebarSection::Places,
                    });
                }
                let member = app
                    .sidebar_drag
                    .as_ref()
                    .is_some_and(|d| d.section == crate::app::SidebarSection::Places);
                if let Some(idx) = reorder_zone(app, ui, &resp, i, member) {
                    places_insert = Some(idx);
                }
                if resp.middle_clicked() {
                    act.new_tab_at = Some(path.clone());
                }
                if resp.clicked() {
                    act.navigate = Some(path);
                }
            }
            // Bookmarks — always rendered (even empty): the section is the
            // drop zone for pinning dragged folders, and the header carries
            // the "Add Folder…" context action.
            let mut add_bookmark = false;
            let mut bm_insert: Option<usize> = None;
            ui.add_space(8.0);
            let hdr_lbl = ui.label(
                egui::RichText::new("Bookmarks")
                    .color(pal.text_muted)
                    .size(11.0),
            );
            // The header strip spans the full panel width — a small text
            // label would be a frustrating drop target otherwise.
            let hdr = ui.interact(
                egui::Rect::from_min_size(
                    hdr_lbl.rect.min,
                    egui::vec2(ui.available_width(), hdr_lbl.rect.height()),
                ),
                ui.id().with("bookmarks_header"),
                egui::Sense::click(),
            );
            hdr.context_menu(|ui| {
                crate::menu::begin(ui, 180.0);
                if crate::menu::item_icon(ui, glyphs::Glyph::Plus, "Add Folder…").clicked() {
                    add_bookmark = true;
                    ui.close();
                }
            });
            if app.dragging.is_some() && hdr.contains_pointer() {
                act.bookmark_zone = true;
                glyphs::glow_drop_target(
                    ui.painter(),
                    hdr.rect.expand2(egui::Vec2::new(0.0, 3.0)),
                    ui.visuals().selection.stroke.color,
                );
            }
            if app.settings.bookmarks.is_empty() {
                // Empty-state hint doubles as the drop zone + add menu.
                let (rect, resp) = ui.allocate_exact_size(
                    egui::Vec2::new(ui.available_width(), 28.0),
                    egui::Sense::click(),
                );
                // Ellipsize the hint if the sidebar is too narrow for it —
                // never let text bleed past the panel edge.
                const HINT: &str = "Drop folders here";
                let font = egui::FontId::proportional(11.5);
                let mut n = HINT.chars().count();
                let mut hint = HINT.to_string();
                loop {
                    let w = ui
                        .painter()
                        .layout_no_wrap(hint.clone(), font.clone(), pal.text_muted)
                        .rect
                        .width();
                    if w <= rect.width() - 12.0 || n <= 2 {
                        break;
                    }
                    n -= 1;
                    hint = format!("{}…", HINT.chars().take(n).collect::<String>().trim_end());
                }
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    hint,
                    font,
                    pal.text_muted,
                );
                resp.context_menu(|ui| {
                    crate::menu::begin(ui, 180.0);
                    if crate::menu::item_icon(ui, glyphs::Glyph::Plus, "Add Folder…").clicked() {
                        add_bookmark = true;
                        ui.close();
                    }
                });
                if resp.clicked() {
                    add_bookmark = true;
                }
                if app.dragging.is_some() && resp.contains_pointer() {
                    act.bookmark_zone = true;
                    glyphs::glow_drop_target(
                        ui.painter(),
                        rect,
                        ui.visuals().selection.stroke.color,
                    );
                }
            } else {
                let mut unpin: Option<PathBuf> = None;
                for (i, p) in app.settings.bookmarks.clone().into_iter().enumerate() {
                    let label = p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string());
                    let resp = glyphs::icon_row(ui, folder_icon, &label, p == current)
                        .on_hover_text(p.display().to_string());
                    resp.context_menu(|ui| {
                        crate::menu::begin(ui, 180.0);
                        if crate::menu::item_icon(ui, glyphs::Glyph::External, "Open").clicked() {
                            act.navigate = Some(p.clone());
                            ui.close();
                        }
                        if crate::menu::item_icon(ui, glyphs::Glyph::Plus, "Open in New Tab")
                            .clicked()
                        {
                            act.new_tab_at = Some(p.clone());
                            ui.close();
                        }
                        crate::menu::sep(ui);
                        if crate::menu::item_danger(ui, "Remove from Sidebar", "").clicked() {
                            unpin = Some(p.clone());
                            ui.close();
                        }
                    });
                    drop_target(app, ui, &resp, &p, act);
                    if app.dragging.is_none()
                        && app.sidebar_drag.is_none()
                        && resp.drag_started_by(egui::PointerButton::Primary)
                    {
                        app.sidebar_drag = Some(crate::app::SidebarDrag {
                            path: p.clone(),
                            section: crate::app::SidebarSection::Bookmarks,
                        });
                    }
                    let member = app
                        .sidebar_drag
                        .as_ref()
                        .is_some_and(|d| d.section == crate::app::SidebarSection::Bookmarks);
                    if let Some(idx) = reorder_zone(app, ui, &resp, i, member) {
                        bm_insert = Some(idx);
                    }
                    if resp.middle_clicked() {
                        act.new_tab_at = Some(p.clone());
                    }
                    if resp.clicked() {
                        act.navigate = Some(p);
                    }
                }
                if let Some(p) = unpin {
                    app.unpin_from_sidebar(&p);
                }
                // Trailing "add" row — permanent click target for the picker
                // and a generous pin drop zone once the list has entries.
                let plus = |p: &egui::Painter, r: egui::Rect, c: egui::Color32| {
                    p.text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        "+",
                        egui::FontId::proportional(r.height() * 0.8),
                        c,
                    );
                };
                let resp = glyphs::icon_row(ui, plus, "Add Bookmark…", false)
                    .on_hover_text("Choose a folder, or drop folders here");
                resp.context_menu(|ui| {
                    crate::menu::begin(ui, 180.0);
                    if crate::menu::item_icon(ui, glyphs::Glyph::Plus, "Add Folder…").clicked() {
                        add_bookmark = true;
                        ui.close();
                    }
                });
                if resp.clicked() {
                    add_bookmark = true;
                }
                if app.dragging.is_some() && resp.contains_pointer() {
                    act.bookmark_zone = true;
                    glyphs::glow_drop_target(
                        ui.painter(),
                        resp.rect,
                        ui.visuals().selection.stroke.color,
                    );
                }
                // During a bookmark reorder the add-row doubles as the
                // "append at end" target — it sits at the list bottom.
                let member = app
                    .sidebar_drag
                    .as_ref()
                    .is_some_and(|d| d.section == crate::app::SidebarSection::Bookmarks);
                if let Some(idx) =
                    reorder_zone(app, ui, &resp, app.settings.bookmarks.len(), member)
                {
                    bm_insert = Some(idx);
                }
            }
            if add_bookmark {
                match flux_platform::pick_folder() {
                    flux_platform::PickResult::Picked(p) => app.pin_to_sidebar(p),
                    flux_platform::PickResult::Unavailable => {
                        app.toast("No folder picker available".to_string())
                    }
                    flux_platform::PickResult::Cancelled => {}
                }
            }
            section(ui, "Volumes");
            for v in app.volumes.clone() {
                let resp = glyphs::icon_row(
                    ui,
                    |p, r, c| glyphs::paint_glyph(p, r, glyphs::Glyph::Disk, c),
                    &v.label,
                    v.mount == current,
                )
                .on_hover_text(v.mount.display().to_string());
                drop_target(app, ui, &resp, &v.mount, act);
                if resp.middle_clicked() {
                    act.new_tab_at = Some(v.mount.clone());
                }
                if resp.clicked() {
                    act.navigate = Some(v.mount);
                }
            }
            // Commit an in-flight sidebar reorder on primary release.
            // Escape cancels via the global cascade in `handle_keys` —
            // consume_key removes the event before this frame reaches us.
            if let Some(d) = app.sidebar_drag.as_ref() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if ui.input(|i| i.pointer.primary_released()) {
                    let d_path = d.path.clone();
                    app.sidebar_drag = None;
                    if let Some(idx) = bm_insert {
                        app.move_bookmark(&d_path, idx);
                    } else if let Some(idx) = places_insert {
                        app.move_place(&d_path, idx);
                    }
                }
            }
        });
}

/// Tab strip above the table — pill surfaces, not boxed browser tabs.
pub fn tab_bar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    {
        let st = ui.style_mut();
        st.spacing.interact_size.y = 26.0;
    }
    let pal = crate::theme::Palette::of(&ui.style().visuals);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let n = app.tabs.len();
        let mut close_idx = None;
        for i in 0..n {
            let title = app.tabs[i].title();
            let selected = i == app.active;
            let loading = app.tabs[i]
                .snap
                .read()
                .map(|s| s.state.is_loading())
                .unwrap_or(false);
            let label = if loading {
                format!("… {title}")
            } else {
                title
            };
            // Measure the label; close × occupies the right slot on the
            // active/hovered tab.
            let text_w = ui
                .painter()
                .layout_no_wrap(
                    label.clone(),
                    egui::FontId::proportional(13.0),
                    pal.text_primary,
                )
                .size()
                .x;
            let show_x = n > 1;
            let w = text_w + 22.0 + if show_x { 14.0 } else { 0.0 };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
            let fill = if selected {
                pal.bg_elevated
            } else if resp.hovered() {
                pal.surface_hover
            } else {
                egui::Color32::TRANSPARENT
            };
            if fill != egui::Color32::TRANSPARENT {
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(7), fill);
            }
            ui.painter().text(
                egui::pos2(rect.left() + 11.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &label,
                egui::FontId::proportional(13.0),
                if selected {
                    pal.text_primary
                } else {
                    pal.text_secondary
                },
            );
            // × close — active tab always, others on hover.
            if show_x {
                let xr = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - 13.0, rect.center().y),
                    egui::Vec2::splat(14.0),
                );
                let xresp = ui.interact(xr, egui::Id::new("tabx").with(i), egui::Sense::click());
                let xcol = if xresp.hovered() {
                    pal.text_primary
                } else {
                    pal.text_muted
                };
                if selected || xresp.hovered() || resp.hovered() {
                    glyphs::paint_glyph(ui.painter(), xr.shrink(3.0), glyphs::Glyph::Close, xcol);
                }
                if xresp.clicked() {
                    close_idx = Some(i);
                }
            }
            if resp.middle_clicked() && n > 1 {
                close_idx = Some(i);
            }
            if resp.clicked() {
                act.activate_tab = Some(i);
            }
            resp.context_menu(|ui| {
                crate::menu::begin(ui, 180.0);
                if crate::menu::item_icon(ui, glyphs::Glyph::Copy, "Duplicate Tab").clicked() {
                    act.new_tab = true;
                    ui.close();
                }
                if crate::menu::item_icon(ui, glyphs::Glyph::Close, "Close").clicked() {
                    close_idx = Some(i);
                    ui.close();
                }
            });
        }
        if let Some(i) = close_idx {
            act.close_tab = Some(i);
        }
        let new_tab_tip = format!("New Tab ({}+T)", crate::format::MOD_KEY);
        if glyphs::icon_button(ui, glyphs::Glyph::Plus, &new_tab_tip, true).clicked() {
            act.new_tab = true;
        }
    });
}

/// Bottom status bar: counts, selection size, free space, op progress.
pub fn status_bar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    {
        let st = ui.style_mut();
        st.spacing.interact_size.y = 26.0;
    }
    let tab = app.current_tab();
    let (view_len, total, sel_count, sel_bytes, loading) = {
        let s = tab.snap.read().unwrap();
        let sel_bytes: u64 = if tab.sel.len() <= 50_000 {
            tab.sel
                .ids
                .iter()
                .filter_map(|&i| s.entry(i))
                .map(|e| {
                    if e.size == flux_core::UNKNOWN_SIZE {
                        0
                    } else {
                        e.size
                    }
                })
                .sum()
        } else {
            0
        };
        (
            s.view_len(),
            s.entry_count(),
            tab.sel.len(),
            sel_bytes,
            s.state.is_loading(),
        )
    };
    let _ = total;

    let pal = crate::theme::Palette::of(&ui.style().visuals);
    let quiet = |ui: &mut egui::Ui, s: String| {
        ui.label(egui::RichText::new(s).color(pal.text_secondary).size(11.5));
    };
    let dot = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new("·").color(pal.text_muted));
    };
    ui.horizontal(|ui| {
        let item_label = if loading {
            format!("{} items (loading…)", format_count(view_len))
        } else {
            format!("{} items", format_count(view_len))
        };
        quiet(ui, item_label);
        if sel_count > 0 {
            dot(ui);
            quiet(ui, format!("{} selected", format_count(sel_count)));
            if sel_bytes > 0 {
                dot(ui);
                quiet(ui, format_size(sel_bytes));
            }
        }
        if let Some(fs) = app.free_space {
            dot(ui);
            quiet(ui, format!("{} free", format_size(fs.available)));
        }
        // Toast.
        if let Some((msg, t)) = &app.status_msg
            && t.elapsed() < std::time::Duration::from_secs(6)
        {
            dot(ui);
            ui.label(
                egui::RichText::new(msg)
                    .italics()
                    .color(pal.text_secondary)
                    .size(11.5),
            );
        }
        // Right side: tool buttons, then active-op progress to their left.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if glyphs::icon_toggle(
                ui,
                glyphs::Glyph::Terminal,
                "Toggle terminal panel",
                app.terminal.is_some(),
            )
            .clicked()
            {
                if app.terminal.is_some() {
                    app.terminal = None;
                } else {
                    let dir = app.current_path();
                    match crate::terminal::EmbeddedTerm::spawn(dir, &ui.ctx().clone()) {
                        Some(t) => app.terminal = Some(t),
                        None => app.toast("Could not open a terminal".to_string()),
                    }
                }
            }
            if glyphs::icon_toggle(
                ui,
                glyphs::Glyph::Preview,
                "Toggle preview panel (Alt+P)",
                app.settings.show_preview,
            )
            .clicked()
            {
                app.toggle_preview();
            }
            if glyphs::icon_toggle(
                ui,
                glyphs::Glyph::History,
                "Operation history / undo (Ctrl+Z)",
                app.show_ops_log,
            )
            .clicked()
            {
                app.show_ops_log = !app.show_ops_log;
            }
            for op in app.ops.active_ops() {
                let p = op.progress_snapshot();
                let frac = p.fraction();
                ui.horizontal(|ui| {
                    if glyphs::icon_button(ui, glyphs::Glyph::Close, "Cancel", true).clicked() {
                        act.cancel_op = Some(op.id);
                    }
                    let bar = egui::ProgressBar::new(frac.unwrap_or(0.0))
                        .desired_width(120.0)
                        .animate(frac.is_none());
                    ui.add(bar);
                    let mut text = op.label.clone();
                    if let Some(bps) = p.throughput_bps() {
                        text.push_str(&format!(" · {}/s", format_size(bps as u64)));
                    }
                    ui.label(
                        egui::RichText::new(text)
                            .color(pal.text_secondary)
                            .size(11.5),
                    );
                });
            }
        });
    });
}
