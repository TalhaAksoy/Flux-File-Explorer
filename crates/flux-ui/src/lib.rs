//! flux-ui: egui presentation layer.
//!
//! Threads own all filesystem work; `update` drains engine events and draws
//! the current snapshot. Repaints are event-driven (engine messages call
//! `ctx.request_repaint`), plus a slow tick while anything is loading so
//! spinners animate — no free-running 60fps loop.

pub mod app;
pub mod chrome;
pub mod dialogs;
pub mod file_icons;
pub mod format;
pub mod glyphs;
pub mod icon_cache;
pub mod icon_evidence;
pub mod menu;
pub mod pane;
pub mod preview;
pub mod rename_page;
pub mod settings_page;
pub mod terminal;
pub mod theme;
pub mod thumbs;
pub mod view;
pub mod widgets;

use std::path::PathBuf;
use std::time::Duration;

use egui::Key;

use app::{COMMANDS, Dialog, FluxApp};
use chrome::ChromeAction;

impl FluxApp {
    /// First-frame init: start enumerating the active tab.
    fn ensure_started(&mut self) {
        if self.current_dir().is_some_and(|d| d.gen_id == 0) {
            let Some(path) = self.current_path() else {
                return;
            };
            self.navigate(path);
        }
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        // Always-on shortcuts, even while a field has focus.
        let i = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::P),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
                i.consume_key(egui::Modifiers::ALT, Key::P),
            )
        });
        if i.0 {
            self.palette_open = !self.palette_open;
            self.palette_query.clear();
        }
        if i.2 {
            self.toggle_preview();
        }
        if i.1 {
            // Escape: close the topmost overlay first, then transient UI
            // state (drag, filter), then the selection itself — matching
            // Explorer/Finder layering.
            if self.palette_open {
                self.palette_open = false;
            } else if self.context_menu_pos.is_some() || self.bg_menu_pos.is_some() {
                self.context_menu_pos = None;
                self.bg_menu_pos = None;
            } else if self.address_edit.is_some() {
                self.address_edit = None;
            } else if !matches!(self.dialog, Dialog::None) {
                self.dialog = Dialog::None;
            } else if self.current_dir().is_none() {
                // Workspace tabs sit above the browser but below
                // overlays — Esc returns to the nearest directory tab
                // without closing the workspace (its state persists).
                self.save_settings();
                self.leave_workspace();
            } else if self.dragging.is_some() {
                self.dragging = None;
            } else if self.sidebar_drag.is_some() {
                self.sidebar_drag = None;
            } else if !self.filter_text.is_empty() {
                self.filter_text.clear();
                self.set_filter(String::new());
            } else if let Some(d) = self.current_dir_mut() {
                d.sel.clear();
            }
            return;
        }
        // Space rising-edge state — tracked ahead of every guard so a
        // key held through a dialog/text field can't produce a phantom
        // edge (or a missed press) when focus comes back.
        let space_down = ctx.input(|i| i.key_down(Key::Space));
        let space_edge = space_down && !self.space_was_down;
        self.space_was_down = space_down;

        if self.palette_open || !matches!(self.dialog, Dialog::None) {
            return; // dialogs own the keyboard
        }

        // Tab-level shortcuts work from ANY workspace — browser standard.
        {
            let cmd = egui::Modifiers::COMMAND;
            let shift = egui::Modifiers::SHIFT;
            let (next_tab, prev_tab, new_tab, close_tab) = ctx.input_mut(|i| {
                (
                    i.consume_key(cmd | shift, Key::Tab),
                    i.consume_key(cmd, Key::Tab),
                    i.consume_key(cmd, Key::T),
                    i.consume_key(cmd, Key::W),
                )
            });
            let n_tabs = self.tabs.len();
            if next_tab && n_tabs > 1 {
                self.activate_tab((self.active + 1) % n_tabs);
            }
            if prev_tab && n_tabs > 1 {
                self.activate_tab((self.active + n_tabs - 1) % n_tabs);
            }
            if new_tab {
                self.new_tab();
            }
            if close_tab {
                let a = self.active;
                self.close_tab(a);
            }
        }

        if self.current_dir().is_none() {
            return; // workspace pages own the remaining keys
        }
        if ctx.egui_wants_keyboard_input() {
            return; // a text field is focused
        }

        // Space = preview the hovered (or focused) file. Terminal input
        // is already drained, text fields returned above, dialogs and
        // workspace tabs returned earlier — reaching here means the
        // file list owns the keyboard. Key-repeat can't re-toggle:
        // space_edge only fires on the physical press.
        if space_edge
            && self.dragging.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Space))
        {
            self.space_preview();
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
                i.consume_key(egui::Modifiers::NONE, K::ArrowLeft),
                i.consume_key(egui::Modifiers::NONE, K::ArrowRight),
                i.consume_key(shift, K::ArrowLeft),
                i.consume_key(shift, K::ArrowRight),
                i.consume_key(egui::Modifiers::NONE, K::PageDown),
                i.consume_key(egui::Modifiers::NONE, K::PageUp),
                i.consume_key(egui::Modifiers::NONE, K::Home),
                i.consume_key(egui::Modifiers::NONE, K::End),
                i.consume_key(cmd, K::D),
                i.consume_key(cmd | shift, K::Z),
                i.consume_key(cmd, K::Z),
                i.consume_key(cmd, K::Y),
            )
        });
        let (
            copy,
            cut,
            paste,
            copy_paths,
            select_all,
            focus_filter,
            focus_address,
            refresh_h,
            hidden_h,
            new_folder,
            hidden_mac,
            rename,
            refresh_f5,
            up_bs,
            back,
            fwd,
            up_alt,
            trash,
            delete_perm,
            enter,
            down,
            down_s,
            up_k,
            up_s,
            left_k,
            right_k,
            left_s,
            right_s,
            pgdn,
            pgup,
            home,
            end,
            duplicate,
            redo_k,
            undo_k,
            redo_y,
        ) = hits;

        if focus_filter {
            self.focus_filter = true;
        }
        if focus_address && let Some(p) = self.current_path() {
            self.address_edit = Some(p.display().to_string());
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
        if select_all && let Some(tab) = self.current_dir_mut() {
            let s = tab.snap.read().unwrap();
            tab.sel.select_all(&s);
        }
        if copy {
            let items = self.actionable_paths();
            flux_platform::clipboard::write_files(&items, false);
            self.ext_clipboard = None;
            self.clipboard.items = items;
            self.clipboard.cut = false;
        }
        if cut {
            let items = self.actionable_paths();
            flux_platform::clipboard::write_files(&items, true);
            self.ext_clipboard = None;
            self.clipboard.items = items;
            self.clipboard.cut = true;
        }
        if paste && let Some(dest) = self.current_path() {
            self.submit_copy(dest);
        }
        if duplicate {
            self.duplicate_selected();
        }
        if undo_k {
            self.undo();
        }
        if redo_k || redo_y {
            self.redo();
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
                    self.open_bulk_rename(items);
                }
            }
        }
        if enter {
            // Open focused row or first selected entry.
            let target = self.current_dir().and_then(|tab| {
                let s = tab.snap.read().unwrap();
                tab.sel
                    .focus
                    .and_then(|r| s.row_index(r))
                    .or_else(|| tab.sel.ids.iter().next().copied())
            });
            if let Some(idx) = target {
                self.open_entry(idx);
            }
        }
        // Arrow/Page/Home/End keyboard navigation.
        let page = self
            .current_dir()
            .map(|tab| tab.snap.read().unwrap().view_len())
            .unwrap_or(0);
        let step = if pgdn || pgup { 30 } else { 1 };
        // Grid/Gallery navigate by cells: vertical moves jump a whole
        // cell-row, horizontal moves walk one entry at a time.
        let in_grid = matches!(
            self.settings.view_mode,
            flux_settings::ViewMode::Grid | flux_settings::ViewMode::Gallery
        );
        let vstep = if in_grid { self.grid_cols.max(1) } else { 1 };
        let cur = self
            .current_dir()
            .and_then(|tab| tab.sel.focus)
            .unwrap_or(0);
        let new_focus = if down || down_s {
            Some((cur + vstep).min(page.saturating_sub(1)))
        } else if up_k || up_s {
            Some(cur.saturating_sub(vstep))
        } else if in_grid && (right_k || right_s) {
            Some((cur + 1).min(page.saturating_sub(1)))
        } else if in_grid && (left_k || left_s) {
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
            if let Some(tab) = self.current_dir_mut() {
                let s = tab.snap.read().unwrap();
                if down_s || up_s || left_s || right_s {
                    tab.sel.extend_to(&s, row);
                } else {
                    tab.sel.select_row(&s, row);
                }
                tab.scroll_to_row = Some(row);
            }
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
        ui.add_space(2.0);
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
                        icons.paint(ui.painter(), ir, kind, &h.path, &pal);
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
                            crate::menu::begin(ui, 180.0);
                            if crate::menu::item_icon(ui, crate::glyphs::Glyph::Link, "Copy Path")
                                .clicked()
                            {
                                ui.close();
                                self.pending_copy_text = Some(h.path.display().to_string());
                            }
                            if crate::menu::item_icon(ui, crate::glyphs::Glyph::Folder, "Reveal")
                                .clicked()
                            {
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
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 96.0])
            .default_width(430.0)
            .show(ctx, |ui| {
                let pal = theme::Palette::of(&ui.style().visuals);
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_query)
                        .desired_width(f32::INFINITY)
                        .hint_text("Type a command…")
                        .margin(egui::Margin::symmetric(12, 8))
                        .font(egui::FontId::proportional(15.5)),
                );
                resp.request_focus();
                let q = self.palette_query.to_lowercase();
                let matches: Vec<&app::Command> = COMMANDS
                    .iter()
                    .filter(|c| q.is_empty() || c.name.to_lowercase().contains(&q))
                    .take(9)
                    .collect();
                if !matches.is_empty() {
                    ui.add_space(6.0);
                }
                for (i, c) in matches.iter().enumerate() {
                    let selected = i == self.palette_sel;
                    let w = ui.available_width();
                    let (rect, resp2) =
                        ui.allocate_exact_size(egui::vec2(w, 30.0), egui::Sense::click());
                    let fill = if selected {
                        pal.accent_fill
                    } else if resp2.hovered() {
                        pal.surface_hover
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    if fill != egui::Color32::TRANSPARENT {
                        ui.painter().rect_filled(
                            rect.shrink2(egui::vec2(2.0, 0.0)),
                            egui::CornerRadius::same(6),
                            fill,
                        );
                    }
                    ui.painter().text(
                        egui::pos2(rect.left() + 12.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        c.name,
                        egui::FontId::proportional(13.5),
                        pal.text_primary,
                    );
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
                    && let Some(c) = matches.get(self.palette_sel)
                {
                    run = Some(c.id);
                }
                if matches.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("No matching commands")
                            .color(pal.text_muted)
                            .size(12.0),
                    );
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("↑↓ navigate  ·  ↵ run  ·  esc close")
                        .color(pal.text_muted)
                        .size(10.5),
                );
            });
        if let Some(id) = run {
            self.run_command(id, ctx);
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
        self.sync_system_theme(ctx);
        self.pump(ctx);
    }

    // `frame` is only used on macOS/Windows (OS drag hand-off is cfg'd out
    // elsewhere) — underscore silences the unused-variable lint on Linux.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.ensure_started();
        // A focused embedded terminal owns the keyboard — drain key/text
        // events into PTY bytes before global shortcuts can see them.
        self.terminal_input(&ctx);
        self.handle_keys(&ctx);

        // OS file drop → copy into current dir. Dropping onto a workspace
        // tab has no destination — the payload is ignored, never faked.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty()
            && let Some(dest) = self.current_path()
        {
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
            .inner_margin(egui::Margin::symmetric(10, 4));
        let status_frame = egui::Frame::default()
            .fill(theme::chrome_fill(dark))
            .inner_margin(egui::Margin::symmetric(10, 5));
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
        if self.current_dir().is_some() {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(self.settings.sidebar_width)
                .size_range(120.0..=320.0)
                .show_separator_line(false)
                .frame(side_frame)
                .show(ui, |ui| {
                    chrome::sidebar(self, ui, &mut act);
                });
        }
        egui::Panel::bottom("status")
            .show_separator_line(false)
            .frame(status_frame)
            .show(ui, |ui| {
                chrome::status_bar(self, ui, &mut act);
            });
        // Docked terminal — created after the status panel so it stacks
        // directly above it, code-editor style.
        if self.terminal.is_some() {
            let term_frame = egui::Frame::default()
                .fill(theme::chrome_fill(dark))
                .inner_margin(egui::Margin::symmetric(10, 4));
            egui::Panel::bottom("terminal")
                .resizable(true)
                .size_range(120.0..=600.0)
                .default_size(200.0)
                .show_separator_line(false)
                .frame(term_frame)
                .show(ui, |ui| {
                    let pal = theme::Palette::of(&ui.style().visuals);
                    self.terminal_panel(ui, &pal);
                });
        }
        // Right-side preview pane (Alt+P) — after the side/bottom panels
        // so it shares the central column's full height. Suppressed on
        // workspace tabs — it's dead space beside Settings/Bulk Rename.
        if self.settings.show_preview && self.current_dir().is_some() {
            let prev_frame = egui::Frame::default()
                .fill(theme::chrome_fill(dark))
                .inner_margin(egui::Margin::symmetric(12, 10));
            egui::Panel::right("preview")
                .resizable(true)
                .size_range(180.0..=520.0)
                .default_size(320.0)
                .show_separator_line(false)
                .frame(prev_frame)
                .show(ui, |ui| {
                    preview::panel(self, ui);
                });
        }
        let mut row_action = view::RowAction::default();
        let central = egui::CentralPanel::default().show(ui, |ui| {
            if self.current_tab().settings().is_some() {
                settings_page::page(self, ui);
                None
            } else if self.current_tab().bulk_rename().is_some() {
                rename_page::page(self, ui);
                None
            } else if self.search_ui.active {
                self.search_view(ui);
                None
            } else {
                let row_act = view::file_table(self, ui);
                // Space-preview's hover resolution — pointer position
                // alone, never a selection change.
                self.hovered_path = row_act.hovered.and_then(|i| {
                    self.current_dir()
                        .and_then(|d| d.snap.read().unwrap().entry_path(i))
                });
                let had_row_ctx = row_act.context.is_some();
                view::apply_click(self, &row_act);
                if let Some(idx) = row_act.open {
                    self.open_entry(idx);
                }
                if let Some(idx) = row_act.open_new_tab {
                    let p = self
                        .current_dir()
                        .and_then(|d| d.snap.read().unwrap().entry_path(idx));
                    if let Some(p) = p
                        && p.is_dir()
                    {
                        self.new_tab_at(p);
                    }
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
                    let target = self.current_dir().and_then(|d| {
                        let s = d.snap.read().unwrap();
                        s.row(row).map(|(_, e)| s.path.join(&e.name))
                    });
                    if let Some(t) = target {
                        act.drop_target = Some(t);
                    }
                }
                if let Some((row, pos)) = row_act.context {
                    // Ensure the row is selected before showing its menu.
                    if let Some(tab) = self.current_dir_mut() {
                        let is_sel = {
                            let s = tab.snap.read().unwrap();
                            s.row_index(row).is_some_and(|i| tab.sel.contains(i))
                        };
                        if !is_sel {
                            let s = tab.snap.read().unwrap();
                            tab.sel.select_row(&s, row);
                        }
                    }
                    // Snapshot the OS clipboard once per menu open — the
                    // menu renders every frame and a live read would spawn
                    // a subprocess per frame on Linux.
                    self.ext_clipboard = flux_platform::clipboard::read_files();
                    self.context_menu_pos = Some(pos);
                }
                row_action = row_act;
                Some(had_row_ctx)
            }
        });
        // Steer the deferred-metadata engine toward the viewport: the
        // entry-index range rendered this frame is statted first.
        if let Some((lo, hi)) = row_action.visible_range {
            self.meta_engine.set_priority(lo, hi.saturating_add(1));
        }

        // Right-click on empty table space → background menu for the
        // current directory (Explorer's background menu equivalent).
        // Workspace tabs have no directory context — no menu.
        let central_rect = central.response.rect;
        let had_row_ctx = central.inner.unwrap_or(false);
        if !had_row_ctx
            && self.current_dir().is_some()
            && !self.search_ui.active
            && ctx.input(|i| i.pointer.secondary_clicked())
            && ctx
                .pointer_interact_pos()
                .is_some_and(|p| central_rect.contains(p))
        {
            self.ext_clipboard = flux_platform::clipboard::read_files();
            self.bg_menu_pos = ctx.pointer_interact_pos();
        }

        // Was a popup menu open this frame? Captured before the dismiss
        // checks below — a click that closed a menu must not also deselect.
        let menu_was_open = self.context_menu_pos.is_some() || self.bg_menu_pos.is_some();

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

        // Explorer/Finder behavior: a primary click on empty table space
        // (below the last row) clears the selection. Clicks that hit a row,
        // header, menu, or dialog produced an action above and are skipped.
        if row_action.click.is_none()
            && row_action.open.is_none()
            && row_action.open_new_tab.is_none()
            && row_action.context.is_none()
            && row_action.sort.is_none()
            && row_action.drag_start.is_none()
            && self.dragging.is_none()
            && !self.search_ui.active
            && !menu_was_open
            && matches!(self.dialog, Dialog::None)
            && !self.palette_open
            && ctx.input(|i| i.pointer.primary_clicked())
            && ctx
                .pointer_interact_pos()
                .is_some_and(|p| central_rect.contains(p))
            && let Some(d) = self.current_dir_mut()
        {
            d.sel.clear();
        }

        // --- Internal drag & drop ---
        if self.dragging.is_some() {
            // Translucent row-card ghost following the cursor.
            if let Some(pos) = ctx.pointer_interact_pos() {
                let (label, n, path, kind) = {
                    let d = self.dragging.as_ref().unwrap();
                    let first = d.items[0]
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (first, d.items.len(), d.items[0].clone(), d.kind)
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
                                icons.paint_cached(ui.painter(), ir, kind, &path, &pal);
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

            // Pointer released → drop on resolved target. The Bookmarks
            // zone wins: it pins dragged folders rather than moving them.
            if ctx.input(|i| i.pointer.primary_released()) {
                if act.bookmark_zone {
                    if let Some(drag) = self.dragging.take() {
                        let mut pinned = 0usize;
                        let mut skipped = 0usize;
                        for p in drag.items {
                            if p.is_dir() {
                                self.pin_to_sidebar(p);
                                pinned += 1;
                            } else {
                                skipped += 1;
                            }
                        }
                        if skipped > 0 && pinned == 0 {
                            self.toast("Only folders can be bookmarked".to_string());
                        }
                    }
                } else {
                    let copy = ctx.input(|i| i.modifiers.alt);
                    match act.drop_target.take() {
                        Some(target) => self.apply_drop(target, copy),
                        None => self.dragging = None,
                    }
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
                            _frame,
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
        if act.toggle_search && self.current_dir().is_some() {
            self.search_ui.active = !self.search_ui.active;
            if self.search_ui.active {
                self.focus_filter = true;
            }
        }
        if act.toggle_density {
            self.density = self.density.next();
        }
        if act.toggle_theme {
            self.toggle_theme(&ctx);
        }
        if let Some(id) = act.cancel_op {
            self.ops.cancel(id);
        }
        if act.paste
            && let Some(dest) = self.current_path()
        {
            self.submit_copy(dest);
        }

        // Debounced recursive search trigger — directory tabs only.
        if self.current_dir().is_some()
            && self.search_ui.active
            && self.search_ui.dirty
            && self.search_ui.last_edit.elapsed() > Duration::from_millis(350)
        {
            self.search_ui.dirty = false;
            let text = self.filter_text.clone();
            self.search_ui.hits.clear();
            if !text.is_empty()
                && let Some(root) = self.current_path()
            {
                self.search_engine.search(flux_search::SearchQuery {
                    root,
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

        // Flush debounced settings writes (column resizes etc.). While a
        // write is pending, wake once after the quiet window — otherwise
        // an idle UI would never get the frame that flushes.
        self.flush_settings_debounce();
        if let Some(t) = self.settings_dirty_at {
            ctx.request_repaint_after(Duration::from_millis(1250).saturating_sub(t.elapsed()));
        }

        // Slow tick while anything streams so spinners/progress animate.
        let loading = self
            .current_dir()
            .and_then(|d| d.snap.read().ok().map(|s| s.state.is_loading()))
            .unwrap_or(false)
            || self
                .current_tab()
                .bulk_rename()
                .is_some_and(|b| b.plan_stale);
        if loading || !self.ops.active_ops().is_empty() || self.search_ui.active {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        // ── Screenshot harness (dev/QA only, off unless FLUX_SHOT_OUT) ──
        if self.shot.is_some() {
            // Switch to the target screen once the directory has loaded.
            if self.shot.as_ref().is_some_and(|s| !s.screen_done) {
                let which = self.shot.as_ref().unwrap().screen.clone();
                let ok = match which.as_str() {
                    "settings" | "typography" | "files" | "views" | "keys" | "advanced"
                    | "about" => {
                        self.open_settings();
                        if let Some(state) = self.current_tab_mut().settings_mut() {
                            state.category = match which.as_str() {
                                "typography" => settings_page::Category::Typography,
                                "files" => settings_page::Category::Files,
                                "views" => settings_page::Category::Views,
                                "keys" => settings_page::Category::Keys,
                                "advanced" => settings_page::Category::Advanced,
                                "about" => settings_page::Category::About,
                                _ => settings_page::Category::Appearance,
                            };
                        }
                        true
                    }
                    "terminal" => {
                        self.terminal = terminal::EmbeddedTerm::spawn(
                            self.last_dir_path().unwrap_or_default(),
                            &ctx,
                        );
                        true
                    }
                    "rename" => {
                        let paths: Vec<PathBuf> = self
                            .current_dir()
                            .map(|d| {
                                let s = d.snap.read().unwrap();
                                (0..s.view_len().min(60))
                                    .filter_map(|i| s.row(i).map(|(_, e)| s.path.join(&e.name)))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if paths.is_empty() {
                            false
                        } else {
                            self.open_bulk_rename(paths);
                            true
                        }
                    }
                    _ => true,
                };
                if ok && let Some(sc) = &mut self.shot {
                    sc.screen_done = true;
                }
            }
            // Optional context-menu capture — open once the screen is
            // up so the menu renders for the final frames.
            // Preview capture — pin the named file once its entry has
            // enumerated, opening the details panel the way Space does.
            if self
                .shot
                .as_ref()
                .is_some_and(|s| s.preview_name.is_some() && !s.preview_done)
            {
                let want = self
                    .shot
                    .as_ref()
                    .and_then(|s| s.preview_name.clone())
                    .unwrap();
                let found = self.current_dir().and_then(|d| {
                    let s = d.snap.read().unwrap();
                    (0..s.view_len()).find_map(|i| {
                        s.row(i)
                            .filter(|(_, e)| e.name.to_string_lossy() == want)
                            .and_then(|(idx, _)| s.entry_path(idx))
                    })
                });
                if let Some(p) = found {
                    self.settings.show_preview = true;
                    self.preview_pin = Some(p);
                    self.pin_anchor = self.focused_path();
                    if let Some(sc) = &mut self.shot {
                        sc.preview_done = true;
                    }
                }
            }
            let menu = self
                .shot
                .as_ref()
                .and_then(|s| s.screen_done.then(|| s.menu.clone()).flatten());
            match menu.as_deref() {
                Some("row") if self.context_menu_pos.is_none() => {
                    if let Some(tab) = self.current_dir_mut() {
                        let has = tab.snap.read().unwrap().view_len() > 0;
                        if has {
                            let s = tab.snap.read().unwrap();
                            tab.sel.select_row(&s, 0);
                        }
                    }
                    self.ext_clipboard = flux_platform::clipboard::read_files();
                    self.context_menu_pos = Some(egui::pos2(320.0, 160.0));
                }
                Some("bg") if self.bg_menu_pos.is_none() => {
                    self.ext_clipboard = flux_platform::clipboard::read_files();
                    self.bg_menu_pos = Some(egui::pos2(320.0, 160.0));
                }
                _ => {}
            }
            if let Some(sc) = &mut self.shot
                && sc.screen_done
            {
                if let Some(size) = sc.size
                    && sc.frames < 10
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::from(size)));
                }
                sc.frames += 1;
                if sc.frames >= sc.wait && !sc.requested {
                    sc.requested = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::UserData::default(),
                    ));
                }
            }
            // Keep frames flowing while waiting for the target screen /
            // the screenshot reply; also during capture so events arrive.
            ctx.request_repaint();
            let mut shot_img: Option<std::sync::Arc<egui::ColorImage>> = None;
            ctx.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Screenshot { image, .. } = ev {
                        shot_img = Some(image.clone());
                    }
                }
            });
            if let Some(img) = shot_img
                && let Some(sc) = self.shot.take()
            {
                let (w, h) = (img.width() as u32, img.height() as u32);
                let buf: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
                if let Some(rgba) = image::RgbaImage::from_raw(w, h, buf)
                    && let Err(e) = rgba.save(&sc.out)
                {
                    eprintln!("flux: screenshot save failed: {e}");
                }
                eprintln!("flux icons: {:?}", self.icons.stats());
                eprintln!("flux: screenshot → {}", sc.out.display());
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_settings();
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}
}

/// Launch the app. `start_dir` overrides the restored session path.
pub fn run(start_dir: Option<PathBuf>) -> eframe::Result<()> {
    let shot_size = app::shot_size_env();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Flux Files")
            .with_inner_size(shot_size.unwrap_or([1060.0, 680.0]))
            .with_min_inner_size([560.0, 360.0]),
        renderer: eframe::Renderer::Glow,
        // QA must neither inherit nor overwrite the user's egui panel
        // geometry. FLUX_SETTINGS_PATH only isolates Flux preferences.
        persistence_path: std::env::var_os("FLUX_SHOT_OUT")
            .map(|out| PathBuf::from(out).with_extension("egui.ron")),
        persist_window: std::env::var_os("FLUX_SHOT_OUT").is_none(),
        ..Default::default()
    };
    eframe::run_native(
        "Flux Files",
        options,
        Box::new(|cc| {
            let mut app = FluxApp::new();
            app.icons.set_ctx(cc.egui_ctx.clone());
            app.thumbs.set_ctx(cc.egui_ctx.clone());
            if let Some(d) = start_dir
                .or_else(|| std::env::var_os("FLUX_SHOT_DIR").map(PathBuf::from))
                .filter(|d| d.is_dir())
            {
                app.navigate(d);
            }
            // Screenshot QA overrides — theme/size are env-driven so a
            // script can capture every combination without a config file.
            if let Ok(t) = std::env::var("FLUX_SHOT_THEME") {
                app.settings.theme = if t == "light" {
                    flux_settings::Theme::Light
                } else {
                    flux_settings::Theme::Dark
                };
                app.dark = t != "light";
            }
            if std::env::var_os("FLUX_SHOT_NO_PREVIEW").is_some() {
                app.settings.show_preview = false;
            }
            if let Ok(scale) = std::env::var("FLUX_SHOT_SCALE")
                && let Ok(scale) = scale.parse::<f32>()
                && scale.is_finite()
            {
                app.settings.ui_scale = scale.clamp(0.75, 2.0);
            }
            if let Ok(v) = std::env::var("FLUX_SHOT_VIEW") {
                app.settings.view_mode = match v.as_str() {
                    "grid" => flux_settings::ViewMode::Grid,
                    "gallery" => flux_settings::ViewMode::Gallery,
                    "compact" => flux_settings::ViewMode::Compact,
                    _ => flux_settings::ViewMode::List,
                };
            }
            cc.egui_ctx.set_fonts(theme::load_fonts(
                &app.settings.ui_font,
                &app.settings.mono_font,
            ));
            crate::theme::prewarm_font_scan();
            cc.egui_ctx.set_zoom_factor(app.settings.ui_scale);
            // Zoom shrinks the logical space — re-issue the requested
            // logical size so the physical window grows to match.
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1060.0, 680.0)));
            if app.settings.theme == flux_settings::Theme::System {
                app.dark = matches!(cc.egui_ctx.system_theme(), Some(egui::Theme::Dark));
            }
            theme::apply_theme(&cc.egui_ctx, app.dark);
            Ok(Box::new(app))
        }),
    )
}
