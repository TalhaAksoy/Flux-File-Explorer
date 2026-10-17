//! Modal dialogs and the row context menu. Rendered each frame on top of
//! the main UI; they mutate app state directly (dialogs are UI state).

use std::path::PathBuf;

use flux_operations::OpKind;
use flux_operations::conflict::{ConflictAction, ConflictDecision};

use crate::app::{Dialog, FluxApp};
use crate::format::format_size;

/// Modern card-style window: no title bar (the heading lives inside),
/// rounded corners + soft shadow come from the theme's window visuals.
fn card(ctx: &egui::Context, title: &'static str) -> egui::Window<'static> {
    egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .frame(
            egui::Frame::window(&ctx.style_of(ctx.theme()))
                .inner_margin(egui::Margin::symmetric(20, 16)),
        )
}

fn heading(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(17.0).strong());
    ui.add_space(2.0);
}

/// Right-aligned [primary][Cancel] footer. Returns (primary, cancel).
fn footer(ui: &mut egui::Ui, primary: &str, primary_enabled: bool) -> (bool, bool) {
    let mut clicked = (false, false);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_enabled {
                clicked.0 = crate::widgets::primary_button(ui, primary).clicked();
            } else {
                ui.add_enabled(
                    false,
                    egui::Button::new(primary).min_size(egui::vec2(88.0, 30.0)),
                );
            }
            ui.add_space(4.0);
            clicked.1 = crate::widgets::ghost_button(ui, "Cancel").clicked();
        });
    });
    clicked
}

/// Render all pending dialogs. Consumes `app.dialog` when a dialog closes.
pub fn show(app: &mut FluxApp, ctx: &egui::Context) {
    ops_log_window(app, ctx);
    // Take the dialog out so dialog bodies can call back into `app` freely.
    let mut dialog = std::mem::replace(&mut app.dialog, Dialog::None);
    // Modal scrim: dims the app behind the card and swallows stray clicks
    // so dialogs behave modally without blocking window input handling.
    if !matches!(dialog, Dialog::None) {
        let dark = ctx.theme() == egui::Theme::Dark;
        let r = ctx.content_rect();
        egui::Area::new(egui::Id::new("dialog_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(r.min)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_clip_rect(r);
                // Clicking the scrim does not dismiss — dialogs carry
                // their own explicit Cancel; a missed click mustn't
                // discard a half-typed name.
                let _ = ui.interact(r, egui::Id::new("scrim_hit"), egui::Sense::click());
                ui.painter().rect_filled(
                    r,
                    0,
                    egui::Color32::from_black_alpha(if dark { 110 } else { 60 }),
                );
            });
    }
    // Directory-targeted dialogs keep the last real path — they can only
    // be raised on a dir tab, so `last_dir_path` is always `Some` here.
    let cur_dir = app.last_dir_path().unwrap_or_default();
    let mut close = false;
    match &mut dialog {
        Dialog::None => {}
        Dialog::Rename { src, input } => {
            let src_c = src.clone();
            let mut do_rename = false;
            card(ctx, "Rename").default_width(360.0).show(ctx, |ui| {
                heading(ui, "Rename");
                ui.label(
                    egui::RichText::new(src_c.display().to_string())
                        .weak()
                        .small(),
                );
                ui.add_space(8.0);
                let resp = ui.add(
                    egui::TextEdit::singleline(input)
                        .desired_width(320.0)
                        .margin(egui::Margin::symmetric(12, 9))
                        .font(egui::TextStyle::Heading),
                );
                resp.request_focus();
                let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (primary, cancel) = footer(ui, "Rename", !input.trim().is_empty());
                if primary || enter {
                    do_rename = true;
                }
                if cancel {
                    close = true;
                }
            });
            if do_rename {
                let name = input.trim();
                if !name.is_empty() && name != src_c.file_name().unwrap_or_default() {
                    let dst = src_c.with_file_name(name);
                    app.ops.submit(
                        OpKind::Rename {
                            from: src_c.clone(),
                            to: dst,
                        },
                        flux_operations::ConflictPolicy::Ask,
                    );
                }
                close = true;
            }
        }
        Dialog::NewFolder { input } => {
            let mut ok = false;
            card(ctx, "New Folder")
                .default_width(360.0)
                .show(ctx, |ui| {
                    heading(ui, "New Folder");
                    ui.label(
                        egui::RichText::new(format!("in {}", cur_dir.display()))
                            .weak()
                            .small(),
                    );
                    ui.add_space(8.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(input)
                            .desired_width(320.0)
                            .margin(egui::Margin::symmetric(12, 9))
                            .hint_text("Folder name"),
                    );
                    r.request_focus();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let (primary, cancel) = footer(ui, "Create", !input.trim().is_empty());
                    ok = primary || enter;
                    if cancel {
                        close = true;
                    }
                });
            if ok {
                app.ops.submit(
                    OpKind::Mkdir {
                        path: cur_dir.join(input.trim()),
                    },
                    flux_operations::ConflictPolicy::Ask,
                );
                close = true;
            }
        }
        Dialog::NewFile { input } => {
            let mut ok = false;
            card(ctx, "New File").default_width(360.0).show(ctx, |ui| {
                heading(ui, "New File");
                ui.label(
                    egui::RichText::new(format!("in {}", cur_dir.display()))
                        .weak()
                        .small(),
                );
                ui.add_space(8.0);
                let r = ui.add(
                    egui::TextEdit::singleline(input)
                        .desired_width(320.0)
                        .margin(egui::Margin::symmetric(12, 9))
                        .hint_text("File name"),
                );
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (primary, cancel) = footer(ui, "Create", !input.trim().is_empty());
                ok = primary || enter;
                if cancel {
                    close = true;
                }
            });
            if ok {
                app.ops.submit(
                    OpKind::CreateFile {
                        path: cur_dir.join(input.trim()),
                    },
                    flux_operations::ConflictPolicy::Ask,
                );
                close = true;
            }
        }
        Dialog::ConfirmPermanentDelete { items } => {
            let n = items.len();
            let mut confirm = false;
            card(ctx, "Delete Permanently")
                .default_width(400.0)
                .show(ctx, |ui| {
                    heading(ui, "Delete Permanently?");
                    ui.label(format!(
                        "{n} item{} will be permanently deleted. This cannot be undone.",
                        if n == 1 { "" } else { "s" }
                    ));
                    if n <= 8 {
                        ui.add_space(6.0);
                        for p in items.iter() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "  {}",
                                    p.file_name()
                                        .map(|f| f.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| p.display().to_string())
                                ))
                                .weak()
                                .small(),
                            );
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::widgets::danger_button(ui, "Delete").clicked() {
                                confirm = true;
                            }
                            ui.add_space(4.0);
                            if crate::widgets::secondary_button(ui, "Cancel").clicked() {
                                close = true;
                            }
                        });
                    });
                });
            if confirm {
                let items = std::mem::take(items);
                app.ops.submit(
                    OpKind::Delete { items },
                    flux_operations::ConflictPolicy::SkipAll,
                );
                close = true;
            }
        }
        Dialog::ConfirmTrashUnsupported { items } => {
            let n = items.len();
            let mut yes = false;
            card(ctx, "Trash Unavailable")
                .default_width(400.0)
                .show(ctx, |ui| {
                    heading(ui, "Trash Unavailable");
                    ui.label(format!(
                        "Trash is unavailable for {n} item{}. Permanently delete them instead?",
                        if n == 1 { "" } else { "s" }
                    ));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::widgets::danger_button(ui, "Delete Permanently").clicked() {
                                yes = true;
                            }
                            ui.add_space(4.0);
                            if crate::widgets::secondary_button(ui, "Cancel").clicked() {
                                close = true;
                            }
                        });
                    });
                });
            if yes {
                let items = std::mem::take(items);
                app.ops.submit(
                    OpKind::Delete { items },
                    flux_operations::ConflictPolicy::SkipAll,
                );
                close = true;
            }
        }
        Dialog::Conflict { query, apply_all } => {
            let src = query.source.clone();
            let dst = query.dest.clone();
            let qid = query.op_id;
            let mut decision: Option<ConflictAction> = None;
            // Escape can't just close — an unanswered query stalls the op.
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                decision = Some(ConflictAction::CancelOp);
            }
            card(ctx, "Item Already Exists")
                .default_width(440.0)
                .show(ctx, |ui| {
                    let pal = crate::theme::Palette::of(&ui.style().visuals);
                    heading(ui, "An item with this name already exists");
                    // The destination in a quiet card — what the user is
                    // deciding about; the source path stays muted.
                    egui::Frame::default()
                        .fill(pal.field_inset)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(12, 9))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(
                                    dst.file_name()
                                        .map(|f| f.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| dst.display().to_string()),
                                )
                                .strong()
                                .size(14.0),
                            );
                            ui.label(
                                egui::RichText::new(
                                    dst.parent()
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default(),
                                )
                                .weak()
                                .small(),
                            );
                            ui.label(
                                egui::RichText::new(format!("Incoming: {}", src.display()))
                                    .weak()
                                    .small(),
                            );
                        });
                    ui.add_space(6.0);
                    ui.checkbox(apply_all, "Apply to all remaining conflicts");
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::widgets::primary_button(ui, "Replace").clicked() {
                                decision = Some(ConflictAction::Overwrite);
                            }
                            if crate::widgets::secondary_button(ui, "Keep Both").clicked() {
                                decision = Some(ConflictAction::KeepBoth);
                            }
                            if crate::widgets::ghost_button(ui, "Skip").clicked() {
                                decision = Some(ConflictAction::Skip);
                            }
                            if crate::widgets::ghost_button(ui, "Cancel").clicked() {
                                decision = Some(ConflictAction::CancelOp);
                            }
                        });
                    });
                });
            if let Some(action) = decision {
                app.ops.answer_conflict(
                    qid,
                    ConflictDecision {
                        action,
                        apply_all: *apply_all,
                    },
                );
                close = true;
            }
        }
        Dialog::Properties { path, meta } => {
            let p = path.clone();
            card(ctx, "Properties")
                .resizable(true)
                .min_size([340.0, 0.0])
                .show(ctx, |ui| {
                    heading(ui, "Properties");
                    ui.add_space(6.0);
                    properties_body(ui, &p, meta, ctx, &app.icons);
                    let (_p, cancel) = footer(ui, "Close", true);
                    if _p || cancel {
                        close = true;
                    }
                });
        }
        Dialog::ConfirmCloseTab { idx } => {
            let idx = *idx;
            let mut confirmed = false;
            card(ctx, "Close Tab")
                .default_width(400.0)
                .show(ctx, |ui| {
                    heading(ui, "Discard pending renames?");
                    ui.label(
                        "This Bulk Rename tab has a plan that hasn't been applied. Closing it discards the plan — no files will be changed.",
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::widgets::danger_button(ui, "Close Tab").clicked() {
                                confirmed = true;
                            }
                            ui.add_space(4.0);
                            if crate::widgets::secondary_button(ui, "Keep Open").clicked() {
                                close = true;
                            }
                        });
                    });
                });
            if confirmed {
                app.force_close_tab(idx);
                close = true;
            }
        }
    }
    // Escape dismisses any open dialog.
    if !close && !matches!(dialog, Dialog::None) && ctx.input(|i| i.key_pressed(egui::Key::Escape))
    {
        close = true;
    }
    if !close {
        app.dialog = dialog;
    }
}

/// Async slot for the Properties dialog's metadata — `None` while the
/// background stat is in flight.
pub type PropsMeta = std::sync::Arc<std::sync::Mutex<Option<Result<std::fs::Metadata, String>>>>;

fn properties_body(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    meta: &mut Option<PropsMeta>,
    ctx: &egui::Context,
    icons: &crate::icon_cache::IconCache,
) {
    // Kick off the stat once; the worker wakes the UI when it lands.
    let slot = meta.get_or_insert_with(|| {
        let slot = PropsMeta::default();
        let s2 = slot.clone();
        let p = path.to_path_buf();
        let c2 = ctx.clone();
        std::thread::spawn(move || {
            let r = std::fs::metadata(&p).map_err(|e| e.to_string());
            *s2.lock().unwrap() = Some(r);
            c2.request_repaint();
        });
        slot
    });
    let md = slot.lock().unwrap().clone();
    let kind = match &md {
        Some(Ok(md)) if md.is_dir() => flux_core::EntryKind::Dir,
        _ => flux_core::EntryKind::File,
    };
    let pal = crate::theme::Palette::of(ui.visuals());
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(32.0), egui::Sense::hover());
        icons.paint(ui.painter(), rect, kind, path, &pal);
        ui.label(icons.file_type(kind, path).info().label);
    });
    egui::Grid::new("props").num_columns(2).show(ui, |ui| {
        ui.label("Name");
        ui.label(
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        ui.end_row();
        ui.label("Path");
        ui.label(path.display().to_string());
        ui.end_row();
        match &md {
            None => {
                ui.label("…");
                ui.weak("Reading metadata…");
                ui.end_row();
            }
            Some(Ok(md)) => {
                ui.label("Size");
                ui.label(if md.is_dir() {
                    "—".to_string()
                } else {
                    format_size(md.len())
                });
                ui.end_row();
                ui.label("Modified");
                ui.label(crate::format::format_mtime(md.modified().ok()));
                ui.end_row();
                ui.label("Read-only");
                ui.label(if md.permissions().readonly() {
                    "Yes"
                } else {
                    "No"
                });
                ui.end_row();
            }
            Some(Err(e)) => {
                ui.label("Error");
                ui.label(e);
                ui.end_row();
            }
        }
    });
}
/// Filesystem facts the row menu needs to decide which items to show.
/// Computed once per menu instance and cached on the app —
/// `has_real_dir` alone is one `symlink_metadata` per selected item, so
/// re-statting every frame would stall the UI on large selections and
/// slow filesystems.
#[derive(Clone, Copy)]
pub struct RowFlags {
    pub single_dir: bool,
    pub single_file: bool,
    pub single_zip: bool,
    pub has_real_dir: bool,
}

impl RowFlags {
    fn compute(items: &[PathBuf]) -> Self {
        let first = items.first();
        let single = items.len() == 1;
        let single_file = single && first.is_some_and(|p| p.is_file());
        Self {
            single_dir: single && first.is_some_and(|p| p.is_dir()),
            single_file,
            single_zip: single_file
                && first.is_some_and(|p| {
                    p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
                }),
            has_real_dir: items.iter().any(|p| {
                std::fs::symlink_metadata(p)
                    .map(|m| m.is_dir() && !m.file_type().is_symlink())
                    .unwrap_or(false)
            }),
        }
    }
}

fn row_flags(app: &mut FluxApp, items: &[PathBuf]) -> RowFlags {
    use std::hash::{Hash, Hasher};
    let mut h = rustc_hash::FxHasher::default();
    items.hash(&mut h);
    let key = h.finish();
    if let Some((k, f)) = app.row_menu_flags
        && k == key
    {
        return f;
    }
    let f = RowFlags::compute(items);
    app.row_menu_flags = Some((key, f));
    f
}

/// Context menu for a file row. `items`: paths the action applies to.
pub fn row_context_menu(app: &mut FluxApp, ui: &mut egui::Ui, items: &[PathBuf]) {
    use crate::format::MOD_KEY as CMD;
    use crate::menu as m;
    m::begin(ui, 224.0);
    let single = items.len() == 1;
    let first = items.first().cloned();
    let flags = row_flags(app, items);
    let single_dir = flags.single_dir;
    use crate::glyphs::Glyph;
    let clipboard_ready = !app.clipboard.items.is_empty() || app.ext_clipboard.is_some();
    if m::item_icon(ui, Glyph::External, "Open").clicked() {
        ui.close();
        if let Some(p) = first.clone() {
            if p.is_dir() {
                app.navigate(p);
            } else {
                let _ = flux_platform::open_path(&p);
            }
        }
    }
    if single_dir {
        let p = first.clone().unwrap();
        if m::item_icon(ui, Glyph::Plus, "Open in New Tab").clicked() {
            ui.close();
            app.new_tab_at(p.clone());
        }
        if m::item_icon(ui, Glyph::Terminal, "Open Terminal Here").clicked() {
            ui.close();
            let _ = flux_platform::open_in_terminal(&p);
        }
    }
    #[cfg(windows)]
    if single && flags.single_file && m::item_icon(ui, Glyph::External, "Open With…").clicked() {
        ui.close();
        if let Some(p) = first.clone()
            && let Err(e) = flux_platform::open_with_dialog(&p)
        {
            app.toast(format!("Open With failed: {e}"));
        }
    }
    #[cfg(not(windows))]
    if single && flags.single_file {
        let target = first.clone().unwrap();
        let apps = app.open_with_apps(&target);
        m::submenu_icon(ui, Glyph::External, "Open With", |ui| {
            if apps.is_empty() {
                ui.weak("No applications found");
            }
            for a in &apps {
                let label = if a.is_default {
                    format!("{} (default)", a.name)
                } else {
                    a.name.clone()
                };
                if m::item_icon(ui, Glyph::External, &label).clicked() {
                    ui.close();
                    if let Err(e) = flux_platform::open_with(&target, a) {
                        app.toast(format!("Could not open with {}: {e}", a.name));
                    }
                }
            }
        });
    }
    if single && m::item_icon(ui, Glyph::Folder, "Reveal in File Manager").clicked() {
        ui.close();
        if let Some(p) = first.clone() {
            let _ = flux_platform::reveal_path(&p);
        }
    }
    m::sep(ui);
    if m::item_icon_sc(ui, Glyph::Cut, "Cut", &format!("{CMD}+X")).clicked() {
        ui.close();
        flux_platform::clipboard::write_files(items, true);
        app.ext_clipboard = None;
        app.clipboard.items = items.to_vec();
        app.clipboard.cut = true;
    }
    if m::item_icon_sc(ui, Glyph::Copy, "Copy", &format!("{CMD}+C")).clicked() {
        ui.close();
        flux_platform::clipboard::write_files(items, false);
        app.ext_clipboard = None;
        app.clipboard.items = items.to_vec();
        app.clipboard.cut = false;
    }
    if single_dir
        && clipboard_ready
        && m::item_icon_sc(ui, Glyph::Paste, "Paste Into Folder", &format!("{CMD}+V")).clicked()
    {
        ui.close();
        app.submit_copy(first.clone().unwrap());
    }
    if m::item_icon_sc(ui, Glyph::Copy, "Duplicate", &format!("{CMD}+D")).clicked() {
        ui.close();
        app.ops.submit(
            flux_operations::OpKind::Duplicate {
                items: items.to_vec(),
            },
            flux_operations::ConflictPolicy::SkipAll,
        );
    }
    if m::item_icon(ui, Glyph::Archive, "Compress to .zip").clicked() {
        ui.close();
        app.compress_items(items.to_vec());
    }
    // Single .zip → safe extraction (zip-slip guarded, staged when the
    // target dir is new).
    if flags.single_zip {
        let archive = first.clone().unwrap();
        let parent = archive
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let stem = archive
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "extracted".into());
        let label = format!("Extract to \"{stem}/\"");
        if m::item_icon(ui, Glyph::Archive, &label).clicked() {
            ui.close();
            // Reserve a free sibling dir name; extract still guards races.
            let mut dest = parent.join(&stem);
            for n in 2..1000 {
                if !dest.exists() {
                    break;
                }
                dest = parent.join(format!("{stem} {n}"));
            }
            app.extract_archive(archive.clone(), dest);
        }
        if m::item_icon(ui, Glyph::Archive, "Extract Here").clicked() {
            ui.close();
            app.extract_archive(archive.clone(), parent);
        }
    }
    // Opt-in recursive folder sizes — Dolphin/Explorer-Properties style.
    // Only real dirs qualify (symlinked dirs show their link, not the
    // target's content).
    if flags.has_real_dir && m::item_icon(ui, Glyph::Disk, "Calculate Folder Size").clicked() {
        ui.close();
        app.calculate_dir_sizes(items);
    }
    m::sep(ui);
    let rename_sc = if single { "F2" } else { "" };
    if m::item_icon_sc(
        ui,
        Glyph::Pencil,
        if single {
            "Rename…"
        } else {
            "Rename… (batch)"
        },
        rename_sc,
    )
    .clicked()
    {
        ui.close();
        if single {
            if let Some(p) = first.clone() {
                app.dialog = Dialog::Rename {
                    input: p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    src: p,
                };
            }
        } else {
            app.open_bulk_rename(items.to_vec());
        }
    }
    if single_dir
        && !app.settings.bookmarks.contains(first.as_ref().unwrap())
        && m::item_icon(ui, Glyph::Plus, "Add to Sidebar").clicked()
    {
        ui.close();
        app.pin_to_sidebar(first.clone().unwrap());
    }
    m::sep(ui);
    if m::item_icon_sc(ui, Glyph::Trash, "Move to Trash", "Del").clicked() {
        ui.close();
        app.submit_trash(items.to_vec());
    }
    if m::item_danger(ui, "Delete Permanently…", "Shift+Del").clicked() {
        ui.close();
        app.dialog = Dialog::ConfirmPermanentDelete {
            items: items.to_vec(),
        };
    }
    m::sep(ui);
    if m::item_icon_sc(ui, Glyph::Link, "Copy Path", &format!("Shift+{CMD}+C")).clicked() {
        ui.close();
        app.pending_copy_text = Some(
            items
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if single && m::item_icon(ui, Glyph::Copy, "Copy Name").clicked() {
        ui.close();
        if let Some(p) = first.clone() {
            app.pending_copy_text = Some(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    if single
        && let Some(p) = first.clone()
        && m::item_icon(ui, Glyph::Info, "Properties").clicked()
    {
        ui.close();
        app.dialog = Dialog::Properties {
            path: p,
            meta: None,
        };
    }
}

/// Context menu for empty space in the file list — actions on the *current*
/// directory itself (Explorer's background menu equivalent).
pub fn bg_context_menu(app: &mut FluxApp, ui: &mut egui::Ui) {
    use crate::format::MOD_KEY as CMD;
    use crate::menu as m;
    m::begin(ui, 200.0);
    use crate::glyphs::Glyph;
    if m::item_icon_sc(ui, Glyph::Folder, "New Folder…", &format!("Shift+{CMD}+N")).clicked() {
        ui.close();
        app.dialog = Dialog::NewFolder {
            input: String::new(),
        };
    }
    if m::item_icon(ui, Glyph::Plus, "New File…").clicked() {
        ui.close();
        app.dialog = Dialog::NewFile {
            input: String::new(),
        };
    }
    let has_clip = !app.clipboard.items.is_empty() || app.ext_clipboard.is_some();
    if m::item_icon_sc_enabled(ui, Glyph::Paste, "Paste", &format!("{CMD}+V"), has_clip).clicked() {
        ui.close();
        if let Some(dest) = app.current_path() {
            app.submit_copy(dest);
        }
    }
    m::sep(ui);
    // Sort submenu — same affordance as column headers.
    let cur = app
        .current_dir()
        .map(|d| d.snap.read().unwrap().sort)
        .unwrap_or_default();
    m::submenu_icon(ui, Glyph::Sort, "Sort By", |ui| {
        for (label, key) in [
            ("Name", flux_core::SortKey::Name),
            ("Size", flux_core::SortKey::Size),
            ("Modified", flux_core::SortKey::Modified),
            ("Type", flux_core::SortKey::Kind),
        ] {
            if m::item_check(ui, label, cur.key == key).clicked() {
                ui.close();
                app.set_sort(flux_core::SortSpec::for_header_click(&cur, key));
            }
        }
        m::sep(ui);
        let asc = cur.dir == flux_core::SortDir::Asc;
        if m::item_check(ui, "Ascending", asc).clicked() {
            ui.close();
            let mut s = cur;
            s.dir = flux_core::SortDir::Asc;
            app.set_sort(s);
        }
        if m::item_check(ui, "Descending", !asc).clicked() {
            ui.close();
            let mut s = cur;
            s.dir = flux_core::SortDir::Desc;
            app.set_sort(s);
        }
    });
    let cur_mode = app.settings.view_mode;
    m::submenu_icon(ui, Glyph::GridView, "View", |ui| {
        for (label, mode) in [
            ("Details", flux_settings::ViewMode::List),
            ("Compact List", flux_settings::ViewMode::Compact),
            ("Grid", flux_settings::ViewMode::Grid),
            ("Gallery", flux_settings::ViewMode::Gallery),
        ] {
            if m::item_check(ui, label, cur_mode == mode).clicked() {
                ui.close();
                app.set_view_mode(mode);
            }
        }
    });
    let cur_density = app.density;
    m::submenu_icon(ui, Glyph::Density, "Row Density", |ui| {
        for (label, d) in [
            ("Compact", crate::theme::Density::Compact),
            ("Standard", crate::theme::Density::Standard),
            ("Comfortable", crate::theme::Density::Comfortable),
        ] {
            if m::item_check(ui, label, cur_density == d).clicked() {
                ui.close();
                app.density = d;
            }
        }
    });
    m::sep(ui);
    if m::item_icon_sc(ui, Glyph::Refresh, "Refresh", "F5").clicked() {
        ui.close();
        app.refresh();
    }
    if m::item_icon(ui, Glyph::Terminal, "Open Terminal Here").clicked() {
        ui.close();
        if let Some(p) = app.current_path() {
            let _ = flux_platform::open_in_terminal(&p);
        }
    }
    m::sep(ui);
    if m::item_icon(ui, Glyph::Info, "Properties").clicked() {
        ui.close();
        if let Some(p) = app.current_path() {
            app.dialog = Dialog::Properties {
                path: p,
                meta: None,
            };
        }
    }
}

/// Operation-history window: every op this session (newest first), with
/// live progress for running ones and a state badge for finished ones.
/// Toggled from the status bar; Esc/close button dismisses.
fn ops_log_window(app: &mut FluxApp, ctx: &egui::Context) {
    if !app.show_ops_log {
        return;
    }
    let mut open = true;
    egui::Window::new("Operations")
        .open(&mut open)
        .default_width(430.0)
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            let ops = app.ops.all_ops();
            if ops.is_empty() {
                ui.weak("No operations yet this session.");
                return;
            }
            if !app.undo_stack.is_empty()
                && ui
                    .button(format!("Undo: {}", app.undo_stack.last().unwrap().label))
                    .clicked()
            {
                app.undo();
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let pal = crate::theme::Palette::of(&ui.style().visuals);
                    for op in ops.iter().rev() {
                        let p = op.progress_snapshot();
                        ui.horizontal(|ui| {
                            let col = match &p.state {
                                flux_operations::OpState::Queued
                                | flux_operations::OpState::Discovering
                                | flux_operations::OpState::Running => pal.accent,
                                flux_operations::OpState::WaitingConflict => pal.warn,
                                flux_operations::OpState::Done => pal.success,
                                flux_operations::OpState::DoneWithErrors => pal.warn,
                                flux_operations::OpState::Cancelled => pal.text_muted,
                                flux_operations::OpState::Failed(_) => pal.danger,
                            };
                            let (dot, _) = ui.allocate_exact_size(
                                egui::vec2(14.0, ui.spacing().interact_size.y),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(dot.center(), 3.5, col);
                            ui.label(&op.label);
                            if p.state.is_active()
                                && let Some(frac) = p.fraction()
                            {
                                ui.weak(format!("{:.0}%", frac * 100.0));
                            }
                            if let flux_operations::OpState::Failed(m) = &p.state {
                                ui.weak(m);
                            }
                        });
                        if p.error_count > 0
                            && let Some(e) = p.errors.first()
                        {
                            ui.indent(op.id as usize, |ui| {
                                ui.label(egui::RichText::new(e).weak().small());
                            });
                        }
                    }
                });
        });
    if !open {
        app.show_ops_log = false;
    }
}
