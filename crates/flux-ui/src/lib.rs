//! flux-ui: egui presentation layer.
//!
//! Threads own all filesystem work; `update` drains engine events and draws
//! the current snapshot. Repaints are event-driven (engine messages call
//! `ctx.request_repaint`), plus a slow tick while anything is loading so
//! spinners animate — no free-running 60fps loop.

pub mod app;
pub mod chrome;
pub mod dialogs;
pub mod format;
pub mod glyphs;
pub mod icon_cache;
pub mod menu;
pub mod pane;
pub mod theme;
pub mod view;

use std::path::PathBuf;
use std::time::Duration;

use egui::Key;

use app::{COMMANDS, Dialog, FluxApp};
use chrome::ChromeAction;

impl FluxApp {
    /// First-frame init: start enumerating the active tab.
    fn ensure_started(&mut self) {
        if self.current_tab().gen_id == 0 {
            let path = self.current_path();
            self.navigate(path);
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Always-on shortcuts, even while a field has focus.
        let i = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::P),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if i.0 {
            self.palette_open = !self.palette_open;
            self.palette_query.clear();
        }
        if i.1 {
            // Escape: close the topmost overlay, else clear filter.
            if self.palette_open {
                self.palette_open = false;
            } else if self.address_edit.is_some() {
                self.address_edit = None;
            } else if !matches!(self.dialog, Dialog::None) {
                self.dialog = Dialog::None;
            } else if !self.filter_text.is_empty() {
                self.filter_text.clear();
                self.set_filter(String::new());
            }
            return;
        }
        if self.palette_open || !matches!(self.dialog, Dialog::None) {
            return; // dialogs own the keyboard
        }
        if ctx.egui_wants_keyboard_input() {
            return; // a text field is focused
        }

        let cmd = egui::Modifiers::COMMAND;
        let shift = egui::Modifiers::SHIFT;
        type K = Key;
        let hits = ctx.input_mut(|i| {
            (
                i.consume_key(cmd, K::C),
                i.consume_key(cmd, K::X),
                i.consume_key(cmd, K::V),
                i.consume_key(cmd | shift, K::C),
                i.consume_key(cmd, K::A),
                i.consume_key(cmd, K::T),
                i.consume_key(cmd, K::W),
                i.consume_key(cmd, K::F),
                i.consume_key(cmd, K::L),
                i.consume_key(cmd, K::R),
                i.consume_key(cmd, K::H),
                i.consume_key(cmd | shift, K::N),
                i.consume_key(cmd | shift, K::Period),
                i.consume_key(egui::Modifiers::NONE, K::F2),
                i.consume_key(egui::Modifiers::NONE, K::F5),
                i.consume_key(egui::Modifiers::NONE, K::Backspace),
                i.consume_key(egui::Modifiers::ALT, K::ArrowLeft),
                i.consume_key(egui::Modifiers::ALT, K::ArrowRight),
                i.consume_key(egui::Modifiers::ALT, K::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, K::Delete),
                i.consume_key(shift, K::Delete),
                i.consume_key(egui::Modifiers::NONE, K::Enter),
                i.consume_key(egui::Modifiers::NONE, K::ArrowDown),
                i.consume_key(shift, K::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, K::ArrowUp),
                i.consume_key(shift, K::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, K::PageDown),
                i.consume_key(egui::Modifiers::NONE, K::PageUp),
                i.consume_key(egui::Modifiers::NONE, K::Home),
                i.consume_key(egui::Modifiers::NONE, K::End),
                i.consume_key(cmd, K::D),
            )
        });
        let (
            copy, cut, paste, copy_paths, select_all, new_tab, close_tab,
            focus_filter, focus_address, refresh_h, hidden_h, new_folder,
            hidden_mac, rename, refresh_f5, up_bs, back, fwd, up_alt,
            trash, delete_perm, enter, down, down_s, up_k, up_s, pgdn, pgup,
            home, end, duplicate,
        ) = hits;

        if new_tab {
            self.new_tab();
        }
        if close_tab {
            let a = self.active;
            self.close_tab(a);
        }
        if focus_filter {
            self.focus_filter = true;
        }
        if focus_address {
            self.address_edit = Some(self.current_path().display().to_string());
            self.focus_address = true;
        }
        if refresh_h || refresh_f5 {
            self.refresh();
        }
        if hidden_h || hidden_mac {
            self.toggle_hidden();
        }
        if new_folder {
            self.dialog = Dialog::NewFolder {
                input: "New Folder".into(),
            };
        }
        if up_bs || up_alt {
            self.go_up();
        }
        if back {
            self.go_back();
        }
        if fwd {
            self.go_forward();
        }
        if select_all {
            let tab = self.current_tab_mut();
            let s = tab.snap.read().unwrap();
            tab.sel.select_all(&s);
        }
        if copy {
            let items = self.actionable_paths();
            self.clipboard.items = items;
            self.clipboard.cut = false;
        }
        if cut {
            let items = self.actionable_paths();
            self.clipboard.items = items;
            self.clipboard.cut = true;
        }
        if paste {
            let dest = self.current_path();
            self.submit_copy(dest);
        }
        if duplicate {
            self.duplicate_selected();
        }
        if copy_paths {
            let text = self
                .actionable_paths()
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                self.pending_copy_text = Some(text);
            }
        }
        if trash {
            let items = self.actionable_paths();
            if !items.is_empty() {
                self.submit_trash(items);
            }
        }
        if delete_perm {
            let items = self.actionable_paths();
            if !items.is_empty() {
                self.dialog = Dialog::ConfirmPermanentDelete { items };
            }
        }
        if rename {
            let items = self.actionable_paths();
            match items.len() {
                0 => {}
                1 => {
                    self.dialog = Dialog::Rename {
                        input: items[0]
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        src: items[0].clone(),
                    };
                }
                _ => {
                    self.dialog = Dialog::BulkRename(app::BulkRenameState::new(items));
                }
            }
        }
        if enter {
            // Open focused row or first selected entry.
            let tab = self.current_tab();
            let target = {
                let s = tab.snap.read().unwrap();
                tab.sel
                    .focus
                    .and_then(|r| s.row_index(r))
                    .or_else(|| tab.sel.ids.iter().next().copied())
            };
            if let Some(idx) = target {
                self.open_entry(idx);
            }
        }
        // Arrow/Page/Home/End keyboard navigation.
        let page = {
            let tab = self.current_tab();
            let s = tab.snap.read().unwrap();
            s.view_len()
        };
        let step = if pgdn || pgup { 30 } else { 1 };
        let tab = self.current_tab_mut();
        let cur = tab.sel.focus.unwrap_or(0);
        let new_focus = if down || down_s {
            Some((cur + 1).min(page.saturating_sub(1)))
        } else if up_k || up_s {
            Some(cur.saturating_sub(1))
        } else if pgdn {
            Some((cur + step).min(page.saturating_sub(1)))
        } else if pgup {
            Some(cur.saturating_sub(step))
        } else if home {
            Some(0)
        } else if end {
            Some(page.saturating_sub(1))
        } else {
            None
        };
        if let Some(row) = new_focus {
            if page == 0 {
                return;
            }
            let tab = self.current_tab_mut();
            let s = tab.snap.read().unwrap();
            if down_s || up_s {
                tab.sel.extend_to(&s, row);
            } else {
                tab.sel.select_row(&s, row);
            }
            tab.scroll_to_row = Some(row);
        }
    }

    fn search_view(&mut self, ui: &mut egui::Ui) {
        let row_h = self.density.row_height();
        ui.horizontal(|ui| {
            if self.search_ui.running {
                ui.add(egui::Spinner::new().size(14.0));
                ui.weak(format!("Searching… {} found", self.search_ui.hits.len()));
            } else {
                ui.weak(format!("{} result(s)", self.search_ui.hits.len()));
            }
        });
        ui.separator();
        let hits = std::mem::take(&mut self.search_ui.hits);
        let mut open_dir: Option<PathBuf> = None;
        let mut open_file: Option<PathBuf> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_h, hits.len(), |ui, range| {
                let pal = crate::theme::Palette::of(&ui.style().visuals);
                let icons = self.icons.clone();
                for i in range {
                    let h = &hits[i];
                    ui.horizontal(|ui| {
                        let name = h
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        // Painted icon — emoji/font glyphs aren't reliable.
                        let kind = if h.is_dir {
                            flux_core::EntryKind::Dir
                        } else {
                            flux_core::EntryKind::File
                        };
                        let (ir, _) = ui.allocate_exact_size(
                            egui::Vec2::splat(row_h * 0.6),
                            egui::Sense::hover(),
                        );
                        let key = icon_cache::IconKey::for_entry_path(
                            kind,
                            &h.path,
                        );
                        match icons.peek(&key) {
                            Some(tex) => {
                                ui.painter().image(
                                    tex,
                                    ir,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    egui::Color32::WHITE,
                                );
                            }
                            None => view::paint_entry_icon(ui.painter(), ir, kind, &pal),
                        }
                        let resp = ui.selectable_label(false, name);
                        let path_str = h.path.display().to_string();
                        ui.weak(path_str);
                        if resp.double_clicked() {
                            if h.is_dir {
                                open_dir = Some(h.path.clone());
                            } else {
                                open_file = Some(h.path.clone());
                            }
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Copy path").clicked() {
                                ui.close();
                                self.pending_copy_text = Some(h.path.display().to_string());
                            }
                            if ui.button("Reveal").clicked() {
                                ui.close();
                                let _ = flux_platform::reveal_path(&h.path);
                            }
                        });
                    });
                }
            });
        self.search_ui.hits = hits;
        if let Some(p) = open_dir {
            self.navigate(p);
        }
        if let Some(p) = open_file {
            let _ = flux_platform::open_path(&p);
        }
    }

    fn palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }
        let mut close = false;
        let mut run: Option<&'static str> = None;
        egui::Window::new("Command Palette")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .show(ctx, |ui| {
                let resp = ui.add_sized(
                    [360.0, 24.0],
                    egui::TextEdit::singleline(&mut self.palette_query)
                        .hint_text("Type a command…"),
                );
                resp.request_focus();
                let q = self.palette_query.to_lowercase();
                let matches: Vec<&app::Command> = COMMANDS
                    .iter()
                    .filter(|c| q.is_empty() || c.name.to_lowercase().contains(&q))
                    .take(9)
                    .collect();
                for (i, c) in matches.iter().enumerate() {
                    let resp2 = ui.selectable_label(i == self.palette_sel, c.name);
                    if resp2.clicked() {
                        run = Some(c.id);
                    }
                }
                if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                    self.palette_sel = (self.palette_sel + 1).min(matches.len().saturating_sub(1));
                }
                if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                    self.palette_sel = self.palette_sel.saturating_sub(1);
                }
                if ui.input(|i| i.key_pressed(Key::Enter))
                    && let Some(c) = matches.get(self.palette_sel) {
                        run = Some(c.id);
                    }
            });
        if let Some(id) = run {
            self.run_command(id);
            close = true;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) || close {
            self.palette_open = false;
            self.palette_sel = 0;
        }
    }
}

impl eframe::App for FluxApp {
    /// Non-UI work: drain engine events, watcher, op events.
    /// Called every frame (and on wakeups) before `ui`.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ensure_started();
        self.handle_keys(&ctx);

        // OS file drop → copy into current dir.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            let dest = self.current_path();
            self.ops.submit(
                flux_operations::OpKind::Copy {
                    sources: dropped,
                    dest_dir: dest,
                },
                flux_operations::ConflictPolicy::Ask,
            );
        }
        if let Some(text) = self.pending_copy_text.take() {
            ctx.copy_text(text);
        }

        let mut act = ChromeAction::default();
        // Zones separate by tonal step, not ruled lines: sidebar recessed,
        // chrome raised, content neutral. `show_separator_line(false)` removes
        // the hard 1px edge (resize line still appears while dragging).
        let dark = ctx.style_of(ctx.theme()).visuals.dark_mode;
        let chrome_frame = egui::Frame::default()
            .fill(theme::chrome_fill(dark))
            .inner_margin(egui::Margin::symmetric(10, 7));
        let tabs_frame = egui::Frame::default()
            .fill(theme::chrome_fill(dark))
            .inner_margin(egui::Margin::symmetric(10, 2));
        let status_frame = egui::Frame::default()
            .fill(theme::chrome_fill(dark))
            .inner_margin(egui::Margin::symmetric(10, 4));
        let side_frame = egui::Frame::default()
            .fill(theme::sidebar_fill(dark))
            .inner_margin(egui::Margin::symmetric(8, 6));
        egui::Panel::top("toolbar")
            .show_separator_line(false)
            .frame(chrome_frame)
            .show(ui, |ui| {
                chrome::toolbar(self, ui, &mut act);
            });
        egui::Panel::top("tabs")
            .show_separator_line(false)
            .frame(tabs_frame)
            .show(ui, |ui| {
                chrome::tab_bar(self, ui, &mut act);
            });
        egui::Panel::left("sidebar")
            .resizable(true)
            .default_size(self.settings.sidebar_width)
            .size_range(120.0..=320.0)
            .show_separator_line(false)
            .frame(side_frame)
            .show(ui, |ui| {
                chrome::sidebar(self, ui, &mut act);
            });
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(status_frame)
            .show(ui, |ui| {
                chrome::status_bar(self, ui, &mut act);
            });
        let central = egui::CentralPanel::default().show(ui, |ui| {
            if self.search_ui.active {
                self.search_view(ui);
                None
            } else {
                let row_act = view::file_table(self, ui);
                let had_row_ctx = row_act.context.is_some();
                view::apply_click(self, &row_act);
                if let Some(idx) = row_act.open {
                    self.open_entry(idx);
                }
                if let Some(spec) = row_act.sort {
                    self.set_sort(spec);
                }
                if row_act.retry {
                    self.refresh();
                }
                if let Some(row) = row_act.drag_start {
                    self.start_drag_row(row);
                }
                if let Some(row) = row_act.drop_row {
                    let target = {
                        let s = self.current_tab().snap.read().unwrap();
                        s.row(row).map(|(_, e)| s.path.join(&e.name))
                    };
                    if let Some(t) = target {
                        act.drop_target = Some(t);
                    }
                }
                if let Some((row, pos)) = row_act.context {
                    // Ensure the row is selected before showing its menu.
                    let tab = self.current_tab_mut();
                    let is_sel = {
                        let s = tab.snap.read().unwrap();
                        s.row_index(row).is_some_and(|i| tab.sel.contains(i))
                    };
                    if !is_sel {
                        let s = tab.snap.read().unwrap();
                        tab.sel.select_row(&s, row);
                    }
                    self.context_menu_pos = Some(pos);
                }
                Some(had_row_ctx)
            }
        });

        // Right-click on empty table space → background menu for the
        // current directory (Explorer's background menu equivalent).
        let central_rect = central.response.rect;
        let had_row_ctx = central.inner.unwrap_or(false);
        if !had_row_ctx
            && !self.search_ui.active
            && ctx.input(|i| i.pointer.secondary_clicked())
            && ctx
                .pointer_interact_pos()
                .is_some_and(|p| central_rect.contains(p))
        {
            self.bg_menu_pos = ctx.pointer_interact_pos();
        }

        // Row context menu overlay; persists until a click outside it.
        if let Some(pos) = self.context_menu_pos {
            let items = self.actionable_paths();
            let inner = egui::Area::new(egui::Id::new("row_ctx"))
                .order(egui::Order::Foreground)
                .kind(egui::UiKind::Menu)
                .fixed_pos(pos)
                .show(&ctx, |ui| {
                    menu::popup_frame(ui.style()).show(ui, |ui| {
                        dialogs::row_context_menu(self, ui, &items);
                    });
                });
            let menu_rect = inner.response.rect;
            let clicked_elsewhere = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !menu_rect.contains(p))
            });
            // A release inside the menu means an item was clicked — dismiss.
            let item_chosen = ctx.input(|i| {
                i.pointer.primary_released()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| menu_rect.contains(p))
            });
            if clicked_elsewhere || item_chosen {
                self.context_menu_pos = None;
            }
        }

        // Background (empty-area) context menu — same persistence pattern.
        if let Some(pos) = self.bg_menu_pos {
            let inner = egui::Area::new(egui::Id::new("bg_ctx"))
                .order(egui::Order::Foreground)
                .kind(egui::UiKind::Menu)
                .fixed_pos(pos)
                .show(&ctx, |ui| {
                    menu::popup_frame(ui.style()).show(ui, |ui| {
                        dialogs::bg_context_menu(self, ui);
                    });
                });
            let menu_rect = inner.response.rect;
            let clicked_elsewhere = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| !menu_rect.contains(p))
            });
            let item_chosen = ctx.input(|i| {
                i.pointer.primary_released()
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|p| menu_rect.contains(p))
            });
            if clicked_elsewhere || item_chosen {
                self.bg_menu_pos = None;
            }
        }

        // --- Internal drag & drop ---
        if self.dragging.is_some() {
            // Translucent row-card ghost following the cursor.
            if let Some(pos) = ctx.pointer_interact_pos() {
                let (label, n, key, kind) = {
                    let d = self.dragging.as_ref().unwrap();
                    let first = d.items[0]
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (first, d.items.len(), d.key.clone(), d.kind)
                };
                let copy_hint = ctx.input(|i| i.modifiers.alt);
                let icons = self.icons.clone();
                let pal = crate::theme::Palette::of(&ctx.style_of(ctx.theme()).visuals);
                egui::Area::new(egui::Id::new("drag_ghost"))
                    .order(egui::Order::Tooltip)
                    .interactable(false)
                    .fixed_pos(pos + egui::vec2(14.0, 10.0))
                    .show(&ctx, |ui| {
                        ui.set_opacity(0.78);
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (ir, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(16.0),
                                    egui::Sense::hover(),
                                );
                                match icons.peek(&key) {
                                    Some(tex) => {
                                        ui.painter().image(
                                            tex,
                                            ir,
                                            egui::Rect::from_min_max(
                                                egui::pos2(0.0, 0.0),
                                                egui::pos2(1.0, 1.0),
                                            ),
                                            egui::Color32::WHITE,
                                        );
                                    }
                                    None => view::paint_entry_icon(
                                        ui.painter(),
                                        ir,
                                        kind,
                                        &pal,
                                    ),
                                }
                                ui.label(&label);
                                if n > 1 {
                                    ui.weak(format!("+{}", n - 1));
                                }
                                if copy_hint {
                                    // Finder-style green ⊕ "copy" badge.
                                    let (br, _) = ui.allocate_exact_size(
                                        egui::Vec2::splat(13.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().circle_filled(
                                        br.center(),
                                        6.5,
                                        egui::Color32::from_rgb(52, 199, 89),
                                    );
                                    glyphs::paint_glyph(
                                        ui.painter(),
                                        br.shrink(4.0),
                                        glyphs::Glyph::Plus,
                                        egui::Color32::WHITE,
                                    );
                                }
                            });
                        });
                    });
                ctx.request_repaint();
            }

            // Pointer released → drop on resolved target.
            if ctx.input(|i| i.pointer.any_released()) {
                let copy = ctx.input(|i| i.modifiers.alt);
                match act.drop_target.take() {
                    Some(target) => self.apply_drop(target, copy),
                    None => self.dragging = None,
                }
            }

            // Pointer left the window while dragging → hand off to the OS.
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            if self.dragging.as_ref().is_some_and(|d| !d.handed_to_os) {
                let pos = ctx.pointer_interact_pos();
                let outside = pos.is_none_or(|p| !ctx.content_rect().contains(p));
                if outside {
                    let drag = self.dragging.take().unwrap();
                    let items = drag.items.clone();
                    let preview = items.first().cloned().map(drag::Image::File);
                    if let Some(preview) = preview {
                        let _ = drag::start_drag(
                            frame,
                            drag::DragItem::Files(items),
                            preview,
                            |_result, _pos| {},
                            drag::Options::default(),
                        );
                    }
                }
            }
        }

        // Apply chrome intents.
        if let Some(p) = act.navigate {
            if p.is_dir() {
                self.navigate(p);
            } else {
                self.toast(format!("Not a directory: {}", p.display()));
            }
        }
        if act.back {
            self.go_back();
        }
        if act.forward {
            self.go_forward();
        }
        if act.up {
            self.go_up();
        }
        if act.refresh {
            self.refresh();
        }
        if act.new_tab {
            self.new_tab();
        }
        if let Some(p) = act.new_tab_at {
            self.new_tab_at(p);
        }
        if let Some(i) = act.close_tab {
            self.close_tab(i);
        }
        if let Some(i) = act.activate_tab {
            self.activate_tab(i);
        }
        if let Some(text) = act.set_filter {
            self.set_filter(text);
        }
        if act.toggle_hidden {
            self.toggle_hidden();
        }
        if act.toggle_search {
            self.search_ui.active = !self.search_ui.active;
            if self.search_ui.active {
                self.focus_filter = true;
            }
        }
        if act.toggle_density {
            self.density = self.density.next();
        }
        if act.toggle_theme {
            self.dark = !self.dark;
            theme::apply_theme(&ctx, self.dark);
        }
        if let Some(id) = act.cancel_op {
            self.ops.cancel(id);
        }
        if act.paste {
            let dest = self.current_path();
            self.submit_copy(dest);
        }

        // Debounced recursive search trigger.
        if self.search_ui.active
            && self.search_ui.dirty
            && self.search_ui.last_edit.elapsed() > Duration::from_millis(350)
        {
            self.search_ui.dirty = false;
            let text = self.filter_text.clone();
            self.search_ui.hits.clear();
            if !text.is_empty() {
                self.search_engine.search(flux_search::SearchQuery {
                    root: self.current_path(),
                    text,
                    case_sensitive: false,
                    regex: false,
                    include_hidden: self.settings.show_hidden,
                    max_depth: None,
                    max_results: 20_000,
                });
            }
        }

        dialogs::show(self, &ctx);
        self.palette(&ctx);

        // Slow tick while anything streams so spinners/progress animate.
        let loading = self
            .current_tab()
            .snap
            .read()
            .map(|s| s.state.is_loading())
            .unwrap_or(false);
        if loading || !self.ops.active_ops().is_empty() || self.search_ui.active {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
}

/// Launch the app. `start_dir` overrides the restored session path.
pub fn run(start_dir: Option<PathBuf>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Flux Files")
            .with_inner_size([1060.0, 680.0])
            .with_min_inner_size([560.0, 360.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Flux Files",
        options,
        Box::new(|cc| {
            let mut app = FluxApp::new();
            app.icons.set_ctx(cc.egui_ctx.clone());
            if let Some(d) = start_dir.filter(|d| d.is_dir()) {
                app.navigate(d);
            }
            theme::apply_theme(&cc.egui_ctx, app.dark);
            Ok(Box::new(app))
        }),
    )
}
