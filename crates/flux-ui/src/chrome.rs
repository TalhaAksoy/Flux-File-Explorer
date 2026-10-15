//! Window chrome: toolbar, breadcrumb, sidebar, tab bar, status bar.
//! Pure rendering + intents; the app executes them.

use std::path::PathBuf;


use crate::app::FluxApp;
use crate::format::{format_count, format_size};

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
}

/// Top toolbar: nav buttons, breadcrumb / editable path, filter field.
pub fn toolbar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    ui.horizontal(|ui| {
        let tab = app.current_tab();
        let can_back = tab.history.can_back();
        let can_fwd = tab.history.can_forward();
        if ui.add_enabled(can_back, egui::Button::new("←")).on_hover_text("Back (Alt+←)").clicked() {
            act.back = true;
        }
        if ui.add_enabled(can_fwd, egui::Button::new("→")).on_hover_text("Forward (Alt+→)").clicked() {
            act.forward = true;
        }
        if ui.button("↑").on_hover_text("Up (Backspace)").clicked() {
            act.up = true;
        }
        if ui.button("⟳").on_hover_text("Refresh (F5)").clicked() {
            act.refresh = true;
        }
        ui.separator();

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
            egui::ScrollArea::horizontal()
                .id_salt("crumbs")
                .max_width(ui.available_width() - 260.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for c in crumbs {
                            let label = c
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| c.display().to_string());
                            if ui.link(label).clicked() {
                                act.navigate = Some(c.clone());
                            }
                            ui.label(egui::RichText::new("›").weak());
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
            if ui
                .selectable_label(app.search_ui.active, "⇲")
                .on_hover_text("Toggle recursive search")
                .clicked()
            {
                act.toggle_search = true;
            }
        });
    });
}

/// Left sidebar: places, bookmarks, volumes.
pub fn sidebar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    ui.add_space(4.0);
    egui::ScrollArea::vertical().id_salt("sidebar").show(ui, |ui| {
        ui.label(egui::RichText::new("Places").weak().small());
        for (label, path) in app.known.clone() {
            if ui.selectable_label(false, label).clicked() {
                act.navigate = Some(path);
            }
        }
        if !app.settings.recents.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Recent").weak().small());
            for p in app.settings.recents.iter().take(6).cloned().collect::<Vec<_>>() {
                let label = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string());
                if ui
                    .selectable_label(false, label)
                    .on_hover_text(p.display().to_string())
                    .clicked()
                {
                    act.navigate = Some(p);
                }
            }
        }
        if !app.settings.bookmarks.is_empty() {
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Bookmarks").weak().small());
            for p in app.settings.bookmarks.clone() {
                let label = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string());
                if ui.selectable_label(false, label).clicked() {
                    act.navigate = Some(p);
                }
            }
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Volumes").weak().small());
        for v in app.volumes.clone() {
            let label = if v.removable {
                format!("⏏ {}", v.label)
            } else {
                v.label.clone()
            };
            if ui.selectable_label(false, label).clicked() {
                act.navigate = Some(v.mount);
            }
        }
    });
}

/// Tab strip above the table.
pub fn tab_bar(app: &mut FluxApp, ui: &mut egui::Ui, act: &mut ChromeAction) {
    ui.horizontal(|ui| {
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
                format!("◌ {title}")
            } else {
                title
            };
            let resp = ui.selectable_label(selected, label);
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
        if ui.button("＋").on_hover_text("New Tab (⌘T)").clicked() {
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

    ui.horizontal(|ui| {
        let item_label = if loading {
            format!("{} items (loading…)", format_count(view_len))
        } else {
            format!("{} items", format_count(view_len))
        };
        ui.label(&item_label);
        if sel_count > 0 {
            ui.weak("·");
            ui.label(format!("{} selected", format_count(sel_count)));
            if sel_bytes > 0 {
                ui.weak("·");
                ui.label(format_size(sel_bytes));
            }
        }
        if let Some(fs) = app.free_space {
            ui.weak("·");
            ui.weak(format!("{} free", format_size(fs.available)));
        }
        // Toast.
        if let Some((msg, t)) = &app.status_msg
            && t.elapsed() < std::time::Duration::from_secs(6) {
                ui.weak("·");
                ui.label(egui::RichText::new(msg).italics());
            }
        // Active op progress on the right.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for op in app.ops.active_ops() {
                let p = op.progress_snapshot();
                let frac = p.fraction();
                ui.horizontal(|ui| {
                    if ui.small_button("✕").on_hover_text("Cancel").clicked() {
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
