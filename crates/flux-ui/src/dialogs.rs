//! Modal dialogs and the row context menu. Rendered each frame on top of
//! the main UI; they mutate app state directly (dialogs are UI state).

use std::path::PathBuf;

use flux_operations::conflict::{ConflictAction, ConflictDecision};
use flux_operations::{OpKind, RenameRule, RenameStatus};

use crate::app::{BulkRenameState, Dialog, FluxApp};
use crate::format::format_size;

/// Modern card-style window: no title bar (the heading lives inside),
/// rounded corners + soft shadow come from the theme's window visuals.
fn card(title: &'static str) -> egui::Window<'static> {
    egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
}

fn heading(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(16.0).strong());
}

/// Right-aligned [primary][Cancel] footer. Returns (primary, cancel).
fn footer(ui: &mut egui::Ui, primary: &str, primary_enabled: bool) -> (bool, bool) {
    let mut clicked = (false, false);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_enabled {
                clicked.0 = crate::menu::primary_button(ui, primary).clicked();
            } else {
                ui.add_enabled(false, egui::Button::new(primary));
            }
            ui.add_space(4.0);
            clicked.1 = crate::menu::ghost_button(ui, "Cancel").clicked();
        });
    });
    clicked
}

/// Render all pending dialogs. Consumes `app.dialog` when a dialog closes.
pub fn show(app: &mut FluxApp, ctx: &egui::Context) {
    // Take the dialog out so dialog bodies can call back into `app` freely.
    let mut dialog = std::mem::replace(&mut app.dialog, Dialog::None);
    let cur_dir = app.current_path();
    let mut close = false;
    match &mut dialog {
        Dialog::None => {}
        Dialog::Rename { src, input } => {
            let src_c = src.clone();
            let mut do_rename = false;
            card("Rename").default_width(360.0).show(ctx, |ui| {
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
                let enter = resp.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (primary, cancel) =
                    footer(ui, "Rename", !input.trim().is_empty());
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
            card("New Folder").default_width(360.0).show(ctx, |ui| {
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
                let enter = r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (primary, cancel) =
                    footer(ui, "Create", !input.trim().is_empty());
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
            card("New File").default_width(360.0).show(ctx, |ui| {
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
                let enter = r.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let (primary, cancel) =
                    footer(ui, "Create", !input.trim().is_empty());
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
            card("Delete Permanently").default_width(380.0).show(ctx, |ui| {
                heading(ui, "Delete Permanently?");
                ui.add_space(4.0);
                ui.label(format!(
                    "Permanently delete {n} item(s)? This cannot be undone."
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Delete")
                                        .color(egui::Color32::WHITE)
                                        .size(14.0),
                                )
                                .fill(ui.visuals().error_fg_color)
                                .corner_radius(egui::CornerRadius::same(7))
                                .min_size(egui::vec2(88.0, 32.0)),
                            )
                            .clicked()
                        {
                            confirm = true;
                        }
                        ui.add_space(4.0);
                        if crate::menu::ghost_button(ui, "Cancel").clicked() {
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
            card("Trash unavailable").default_width(380.0).show(ctx, |ui| {
                heading(ui, "Trash Unavailable");
                ui.add_space(4.0);
                ui.label(format!(
                    "Trash is unavailable for {n} item(s). Permanently delete them instead?"
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Delete permanently")
                                        .color(egui::Color32::WHITE)
                                        .size(14.0),
                                )
                                .fill(ui.visuals().error_fg_color)
                                .corner_radius(egui::CornerRadius::same(7))
                                .min_size(egui::vec2(88.0, 32.0)),
                            )
                            .clicked()
                        {
                            yes = true;
                        }
                        ui.add_space(4.0);
                        if crate::menu::ghost_button(ui, "Cancel").clicked() {
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
            card("Item already exists").default_width(420.0).show(ctx, |ui| {
                heading(ui, "Item Already Exists");
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(dst.display().to_string()).strong(),
                );
                ui.label(
                    egui::RichText::new(format!("From: {}", src.display()))
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);
                ui.checkbox(apply_all, "Apply to all remaining conflicts");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::menu::primary_button(ui, "Replace").clicked() {
                            decision = Some(ConflictAction::Overwrite);
                        }
                        if crate::menu::ghost_button(ui, "Keep Both").clicked() {
                            decision = Some(ConflictAction::KeepBoth);
                        }
                        if crate::menu::ghost_button(ui, "Skip").clicked() {
                            decision = Some(ConflictAction::Skip);
                        }
                        if crate::menu::ghost_button(ui, "Cancel").clicked() {
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
        Dialog::BulkRename(br) => {
            bulk_rename_dialog(app, br, ctx, &mut close);
        }
        Dialog::Properties { path } => {
            let p = path.clone();
            card("Properties").resizable(true).min_size([340.0, 0.0]).show(ctx, |ui| {
                heading(ui, "Properties");
                ui.add_space(6.0);
                properties_body(ui, &p);
                let (_p, cancel) = footer(ui, "Close", true);
                if _p || cancel {
                    close = true;
                }
            });
        }
        Dialog::About => {
            card("About Flux Files").default_width(300.0).show(ctx, |ui| {
                heading(ui, "Flux Files");
                ui.label(
                    egui::RichText::new("A fast native file explorer")
                        .weak()
                        .small(),
                );
                ui.add_space(4.0);
                ui.weak("Built with Rust + egui.");
                let (_p, cancel) = footer(ui, "Close", true);
                if _p || cancel {
                    close = true;
                }
            });
        }
    }
    // Escape dismisses any open dialog.
    if !close && !matches!(dialog, Dialog::None)
        && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
    if !close {
        app.dialog = dialog;
    }
}

fn properties_body(ui: &mut egui::Ui, path: &std::path::Path) {
    let md = std::fs::metadata(path);
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
        if let Ok(md) = &md {
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
            ui.label(if md.permissions().readonly() { "Yes" } else { "No" });
            ui.end_row();
        }
    });
}

const PREVIEW_CAP: usize = 2_000;

/// Toggle chip — a small pill that lights up accent when on.
fn chip(ui: &mut egui::Ui, label: &str, on: bool) -> egui::Response {
    let accent = ui.visuals().selection.stroke.color;
    let mut b = egui::Button::new(if on {
        egui::RichText::new(label).color(egui::Color32::WHITE).size(13.0)
    } else {
        egui::RichText::new(label).size(13.0)
    })
    .corner_radius(egui::CornerRadius::same(14))
    .min_size(egui::vec2(0.0, 28.0));
    if on {
        b = b.fill(accent).stroke(egui::Stroke::NONE);
    }
    ui.add(b)
}

fn bulk_rename_dialog(app: &mut FluxApp, br: &mut BulkRenameState, ctx: &egui::Context, close: &mut bool) {
    if br.dirty && br.plan.is_empty() {
        br.recompute();
    }
    let n_ready = br.plan.iter().filter(|a| a.status.is_ready()).count();
    let n_skip = br.plan.len() - n_ready;
    let mut apply = false;
    card("Bulk Rename")
        .resizable(true)
        .default_size([440.0, 470.0])
        .min_size([380.0, 300.0])
        .show(ctx, |ui| {
            heading(ui, &format!("Rename {} items", br.plan.len()));
            ui.add_space(8.0);

            // Core flow: find → replace → preview → rename.
            ui.label(egui::RichText::new("Find").weak().small());
            let find_r = ui.add(
                egui::TextEdit::singleline(&mut br.find).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 8)),
            );
            if find_r.changed() {
                br.dirty = true;
                br.last_edit = std::time::Instant::now();
            }
            ui.add_space(6.0);
            ui.label(egui::RichText::new("Replace with").weak().small());
            if ui
                .add(egui::TextEdit::singleline(&mut br.replace).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 8)))
                .changed()
            {
                br.dirty = true;
                br.last_edit = std::time::Instant::now();
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if chip(ui, "Regex", br.regex).clicked() {
                    br.regex = !br.regex;
                    br.dirty = true;
                }
                if chip(ui, "Match case", br.case_sensitive).clicked() {
                    br.case_sensitive = !br.case_sensitive;
                    br.dirty = true;
                }
                if chip(ui, "Number", br.numbering).clicked() {
                    br.numbering = !br.numbering;
                    br.dirty = true;
                }
            });
            if br.numbering {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Start").weak().small());
                    if ui.add(egui::DragValue::new(&mut br.num_start)).changed() {
                        br.dirty = true;
                    }
                    ui.label(egui::RichText::new("Pad").weak().small());
                    if ui.add(egui::DragValue::new(&mut br.num_pad).range(0..=8)).changed() {
                        br.dirty = true;
                    }
                    if chip(ui, "Number first", br.num_prefix).clicked() {
                        br.num_prefix = !br.num_prefix;
                        br.dirty = true;
                    }
                });
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Case").weak().small());
                for (i, l) in ["Keep", "UPPER", "lower", "Title"].iter().enumerate() {
                    if chip(ui, l, br.case_mode == i).clicked() {
                        br.case_mode = i;
                        br.dirty = true;
                    }
                }
            });

            egui::CollapsingHeader::new("Advanced: template")
                .default_open(false)
                .show(ui, |ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut br.template)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(10, 8))
                                .hint_text("{name}{ext}"),
                        )
                        .changed()
                    {
                        br.dirty = true;
                        br.last_edit = std::time::Instant::now();
                    }
                    ui.weak("{name} {ext} {n} {n:000} {parent}");
                });
            ui.add_space(4.0);

            // Live preview — the confirmation of what will happen.
            ui.label(egui::RichText::new(format!("Preview — {n_ready} ready · {n_skip} skipped"))
                .weak()
                .small());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::ScrollArea::both()
                    .id_salt("bulk_preview")
                    .max_height(180.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Grid::new("bulk_grid").striped(true).show(ui, |ui| {
                            for a in br.plan.iter().take(PREVIEW_CAP) {
                                let src_n = a
                                    .src
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                let dst_n = a
                                    .dst
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                let col = match &a.status {
                                    RenameStatus::Ready => ui.visuals().text_color(),
                                    RenameStatus::Unchanged => ui.visuals().weak_text_color(),
                                    _ => ui.visuals().error_fg_color,
                                };
                                ui.label(egui::RichText::new(src_n).color(col).size(12.5));
                                let (ar, _) = ui.allocate_exact_size(
                                    egui::Vec2::splat(12.0),
                                    egui::Sense::hover(),
                                );
                                crate::glyphs::paint_glyph(
                                    ui.painter(),
                                    ar,
                                    crate::glyphs::Glyph::Forward,
                                    ui.visuals().weak_text_color(),
                                );
                                ui.label(egui::RichText::new(dst_n).color(col).size(12.5));
                                if !a.status.is_ready() && a.status != RenameStatus::Unchanged {
                                    ui.label(
                                        egui::RichText::new(format!("{:?}", a.status))
                                            .small()
                                            .color(ui.visuals().error_fg_color),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                        if br.plan.len() > PREVIEW_CAP {
                            ui.weak(format!("…and {} more", br.plan.len() - PREVIEW_CAP));
                        }
                    });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if n_ready > 0 {
                        if crate::menu::primary_button(ui, &format!("Rename {n_ready} items")).clicked() {
                            apply = true;
                        }
                    } else {
                        ui.add_enabled(false, egui::Button::new(format!("Rename {n_ready} items")));
                    }
                    ui.add_space(4.0);
                    if crate::menu::ghost_button(ui, "Cancel").clicked() {
                        *close = true;
                    }
                });
            });
        });
    let esc_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if apply {
        let actions = std::mem::take(&mut br.plan);
        app.ops.submit(
            OpKind::BatchRename { actions },
            flux_operations::ConflictPolicy::Ask,
        );
        *close = true;
    }
    if esc_pressed {
        *close = true;
    }
    let _ = RenameRule::ChangeCase;
}

/// Context menu for a file row. `items`: paths the action applies to.
pub fn row_context_menu(app: &mut FluxApp, ui: &mut egui::Ui, items: &[PathBuf]) {
    use crate::format::MOD_KEY as CMD;
    use crate::menu as m;
    m::begin(ui, 224.0);
    let single = items.len() == 1;
    let first = items.first().cloned();
    let single_dir = single && first.as_ref().is_some_and(|p| p.is_dir());
    let clipboard_ready = !app.clipboard.items.is_empty();
    if m::item(ui, "Open").clicked() {
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
        if m::item(ui, "Open in New Tab").clicked() {
            ui.close();
            app.new_tab_at(p.clone());
        }
        if m::item(ui, "Open Terminal Here").clicked() {
            ui.close();
            let _ = flux_platform::open_in_terminal(&p);
        }
    }
    if single && m::item(ui, "Reveal in File Manager").clicked() {
        ui.close();
        if let Some(p) = first.clone() {
            let _ = flux_platform::reveal_path(&p);
        }
    }
    m::sep(ui);
    if m::item_shortcut(ui, "Cut", &format!("{CMD}+X")).clicked() {
        ui.close();
        app.clipboard.items = items.to_vec();
        app.clipboard.cut = true;
    }
    if m::item_shortcut(ui, "Copy", &format!("{CMD}+C")).clicked() {
        ui.close();
        app.clipboard.items = items.to_vec();
        app.clipboard.cut = false;
    }
    if single_dir
        && clipboard_ready
        && m::item_shortcut(ui, "Paste Into Folder", &format!("{CMD}+V")).clicked()
    {
        ui.close();
        app.submit_copy(first.clone().unwrap());
    }
    if m::item_shortcut(ui, "Duplicate", &format!("{CMD}+D")).clicked() {
        ui.close();
        app.ops.submit(
            flux_operations::OpKind::Duplicate {
                items: items.to_vec(),
            },
            flux_operations::ConflictPolicy::SkipAll,
        );
    }
    if m::item(ui, "Compress to .zip").clicked() {
        ui.close();
        app.compress_items(items.to_vec());
    }
    m::sep(ui);
    let rename_sc = if single { "F2" } else { "" };
    if m::item_shortcut(ui, if single { "Rename…" } else { "Rename… (batch)" }, rename_sc).clicked() {
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
            app.dialog = Dialog::BulkRename(BulkRenameState::new(items.to_vec()));
        }
    }
    if single_dir
        && !app.settings.bookmarks.contains(first.as_ref().unwrap())
        && m::item(ui, "Add to Sidebar").clicked()
    {
        ui.close();
        app.pin_to_sidebar(first.clone().unwrap());
    }
    m::sep(ui);
    if m::item_shortcut(ui, "Move to Trash", "Del").clicked() {
        ui.close();
        app.submit_trash(items.to_vec());
    }
    if m::item_shortcut(ui, "Delete Permanently…", "Shift+Del").clicked() {
        ui.close();
        app.dialog = Dialog::ConfirmPermanentDelete {
            items: items.to_vec(),
        };
    }
    m::sep(ui);
    if m::item_shortcut(ui, "Copy Path", &format!("Shift+{CMD}+C")).clicked() {
        ui.close();
        app.pending_copy_text = Some(
            items
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if single && m::item(ui, "Copy Name").clicked() {
        ui.close();
        if let Some(p) = first.clone() {
            app.pending_copy_text =
                Some(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        }
    }
    if single
        && let Some(p) = first.clone()
            && m::item(ui, "Properties").clicked() {
                ui.close();
                app.dialog = Dialog::Properties { path: p };
            }
}

/// Context menu for empty space in the file list — actions on the *current*
/// directory itself (Explorer's background menu equivalent).
pub fn bg_context_menu(app: &mut FluxApp, ui: &mut egui::Ui) {
    use crate::format::MOD_KEY as CMD;
    use crate::menu as m;
    m::begin(ui, 200.0);
    if m::item_shortcut(ui, "New Folder…", &format!("Shift+{CMD}+N")).clicked() {
        ui.close();
        app.dialog = Dialog::NewFolder { input: String::new() };
    }
    if m::item(ui, "New File…").clicked() {
        ui.close();
        app.dialog = Dialog::NewFile { input: String::new() };
    }
    let has_clip = !app.clipboard.items.is_empty();
    if m::item_sc_enabled(ui, "Paste", &format!("{CMD}+V"), has_clip).clicked() {
        ui.close();
        let dest = app.current_path();
        app.submit_copy(dest);
    }
    m::sep(ui);
    // Sort submenu — same affordance as column headers.
    let cur = app.current_tab().snap.read().unwrap().sort;
    m::submenu(ui, "Sort By", |ui| {
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
    let cur_density = app.density;
    m::submenu(ui, "Row Density", |ui| {
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
    if m::item_shortcut(ui, "Refresh", "F5").clicked() {
        ui.close();
        app.refresh();
    }
    if m::item(ui, "Open Terminal Here").clicked() {
        ui.close();
        let p = app.current_path();
        let _ = flux_platform::open_in_terminal(&p);
    }
    m::sep(ui);
    if m::item(ui, "Properties").clicked() {
        ui.close();
        app.dialog = Dialog::Properties {
            path: app.current_path(),
        };
    }
}
