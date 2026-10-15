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
}

/// During a drag, mark `resp` as a drop target for `dir` when hovered and
/// paint a highlight.
fn drop_target(app: &FluxApp, ui: &egui::Ui, resp: &egui::Response, dir: &std::path::Path, act: &mut ChromeAction) {
    if app.dragging.is_some() && resp.hovered() {
        act.drop_target = Some(dir.to_path_buf());
        glyphs::glow_drop_target(
            ui.painter(),
            resp.rect,
            ui.visuals().selection.stroke.color,
        );
    }
}

/// Top toolbar: nav buttons, breadcrumb / editable path, filter field.
pub fn toolbar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    // Toolbar chrome runs larger than the file list — primary navigation
    // deserves bigger hit targets and text. Scoped to this Ui only.
    {
        let st = ui.style_mut();
        st.spacing.interact_size.y = 30.0;
        st.text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        st.text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.5));
    }
    ui.horizontal(|ui| {
        let tab = app.current_tab();
        let can_back = tab.history.can_back();
        let can_fwd = tab.history.can_forward();
        if glyphs::icon_button(ui, glyphs::Glyph::Back, "Back (Alt+Left)", can_back).clicked() {
            act.back = true;
        }
        if glyphs::icon_button(ui, glyphs::Glyph::Forward, "Forward (Alt+Right)", can_fwd).clicked() {
            act.forward = true;
        }
        if glyphs::icon_button(ui, glyphs::Glyph::Up, "Up (Backspace)", true).clicked() {
            act.up = true;
        }
        if glyphs::icon_button(ui, glyphs::Glyph::Refresh, "Refresh (F5)", true).clicked() {
            act.refresh = true;
        }
        ui.add_space(8.0);

        // Breadcrumb or editable path field.
        if let Some(text) = &mut app.address_edit {
            let id = egui::Id::new("address_bar");
            let resp = ui.add_sized(
                [ui.available_width() - 260.0, 22.0],
                egui::TextEdit::singleline(text).id(id),
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
            let pal = crate::theme::Palette::of(&ui.style().visuals);
            egui::ScrollArea::horizontal()
                .id_salt("crumbs")
                .max_width(ui.available_width() - 260.0)
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
                                if last { pal.text_primary } else { pal.text_secondary },
                            );
                            drop_target(app, ui, &resp, &c, act);
                            if resp.clicked() {
                                act.navigate = Some(c.clone());
                            }
                            if !last {
                                ui.painter().text(
                                    egui::pos2(
                                        rect.right() + 7.0,
                                        rect.center().y,
                                    ),
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
            // Click the breadcrumb area to edit the path directly.
            if ui.interact(ui.max_rect(), egui::Id::new("crumb_edit"), egui::Sense::click()).clicked() {
                app.address_edit = Some(path.display().to_string());
                app.focus_address = true;
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Search mode toggle + filter/search box.
            let resp = ui.add_sized(
                [190.0, 22.0],
                egui::TextEdit::singleline(&mut app.filter_text)
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
    ui.style_mut().spacing.interact_size.y = 28.0;
    ui.add_space(6.0);
    let pal = crate::theme::Palette::of(&ui.style().visuals);
    let current = app.current_path();
    let folder_icon =
        |p: &egui::Painter, r: egui::Rect, _c: egui::Color32| {
            crate::view::paint_entry_icon(p, r, flux_core::EntryKind::Dir, &pal)
        };
    let section = |ui: &mut egui::Ui, label: &str| {
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(label)
                .color(pal.text_muted)
                .size(11.0),
        );
    };
    egui::ScrollArea::vertical().id_salt("sidebar").show(ui, |ui| {
        section(ui, "Places");
        for (label, path) in app.known.clone() {
            let resp = glyphs::icon_row(ui, folder_icon, label, path == current);
            resp.context_menu(|ui| {
                if ui.button("Open").clicked() {
                    act.navigate = Some(path.clone());
                    ui.close();
                }
                if ui.button("Open in New Tab").clicked() {
                    act.new_tab_at = Some(path.clone());
                    ui.close();
                }
            });
            drop_target(app, ui, &resp, &path, act);
            if resp.clicked() {
                act.navigate = Some(path);
            }
        }
        if !app.settings.recents.is_empty() {
            section(ui, "Recent");
            for p in app.settings.recents.iter().take(6).cloned().collect::<Vec<_>>() {
                let label = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string());
                let resp = glyphs::icon_row(ui, folder_icon, &label, p == current)
                    .on_hover_text(p.display().to_string());
                resp.context_menu(|ui| {
                    if ui.button("Open").clicked() {
                        act.navigate = Some(p.clone());
                        ui.close();
                    }
                    if ui.button("Open in New Tab").clicked() {
                        act.new_tab_at = Some(p.clone());
                        ui.close();
                    }
                });
                drop_target(app, ui, &resp, &p, act);
                if resp.clicked() {
                    act.navigate = Some(p);
                }
            }
        }
        if !app.settings.bookmarks.is_empty() {
            section(ui, "Bookmarks");
            let mut unpin: Option<PathBuf> = None;
            for p in app.settings.bookmarks.clone() {
                let label = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string());
                let resp = glyphs::icon_row(ui, folder_icon, &label, p == current)
                    .on_hover_text(p.display().to_string());
                resp.context_menu(|ui| {
                    if ui.button("Open").clicked() {
                        act.navigate = Some(p.clone());
                        ui.close();
                    }
                    if ui.button("Open in New Tab").clicked() {
                        act.new_tab_at = Some(p.clone());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Remove from Sidebar").clicked() {
                        unpin = Some(p.clone());
                        ui.close();
                    }
                });
                drop_target(app, ui, &resp, &p, act);
                if resp.clicked() {
                    act.navigate = Some(p);
                }
            }
            if let Some(p) = unpin {
                app.unpin_from_sidebar(&p);
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
            if resp.clicked() {
                act.navigate = Some(v.mount);
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
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
            let fill = if selected {
                pal.bg_elevated
            } else if resp.hovered() {
                pal.surface_hover
            } else {
                egui::Color32::TRANSPARENT
            };
            if fill != egui::Color32::TRANSPARENT {
                ui.painter().rect_filled(rect, egui::CornerRadius::same(7), fill);
            }
            ui.painter().text(
                egui::pos2(rect.left() + 11.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &label,
                egui::FontId::proportional(13.0),
                if selected { pal.text_primary } else { pal.text_secondary },
            );
            // × close — active tab always, others on hover.
            if show_x {
                let xr = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - 13.0, rect.center().y),
                    egui::Vec2::splat(14.0),
                );
                let xresp = ui.interact(
                    xr,
                    egui::Id::new("tabx").with(i),
                    egui::Sense::click(),
                );
                let xcol = if xresp.hovered() {
                    pal.text_primary
                } else {
                    pal.text_muted
                };
                if selected || xresp.hovered() || resp.hovered() {
                    glyphs::paint_glyph(
                        ui.painter(),
                        xr.shrink(3.0),
                        glyphs::Glyph::Close,
                        xcol,
                    );
                }
                if xresp.clicked() {
                    close_idx = Some(i);
                }
            }
            if resp.clicked() {
                act.activate_tab = Some(i);
            }
            resp.context_menu(|ui| {
                if ui.button("Close").clicked() {
                    close_idx = Some(i);
                    ui.close();
                }
                if ui.button("Duplicate Tab").clicked() {
                    act.new_tab = true;
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
    let tab = app.current_tab();
    let (view_len, total, sel_count, sel_bytes, loading) = {
        let s = tab.snap.read().unwrap();
        let sel_bytes: u64 = if tab.sel.len() <= 50_000 {
            tab.sel
                .ids
                .iter()
                .filter_map(|&i| s.entry(i))
                .map(|e| if e.size == flux_core::UNKNOWN_SIZE { 0 } else { e.size })
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
            && t.elapsed() < std::time::Duration::from_secs(6) {
                dot(ui);
                ui.label(
                    egui::RichText::new(msg).italics().color(pal.text_secondary).size(11.5),
                );
            }
        // Active op progress on the right.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                    ui.label(egui::RichText::new(text).small());
                });
            }
        });
    });
}
