//! Modal dialogs and the row context menu. Rendered each frame on top of
//! the main UI; they mutate app state directly (dialogs are UI state).

use std::path::PathBuf;

use flux_operations::conflict::{ConflictAction, ConflictDecision};
use flux_operations::{OpKind, RenameRule, RenameStatus};

use crate::app::{BulkRenameState, Dialog, FluxApp};
use crate::format::format_size;

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
            let mut cancel = false;
            egui::Window::new("Rename")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Rename {}", src_c.display()));
                    let resp = ui.text_edit_singleline(input);
                    resp.request_focus();
                    ui.horizontal(|ui| {
                        if ui.button("Rename").clicked()
                            || (resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            do_rename = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
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
            if cancel {
                close = true;
            }
        }
        Dialog::NewFolder { input } => {
            let mut ok = false;
            egui::Window::new("New Folder").collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Folder name");
                    let r = ui.text_edit_singleline(input);
                    r.request_focus();
                    ui.horizontal(|ui| {
                        ok = ui.button("Create").clicked()
                            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
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
            egui::Window::new("New File").collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("File name");
                    let r = ui.text_edit_singleline(input);
                    r.request_focus();
                    ui.horizontal(|ui| {
                        ok = ui.button("Create").clicked()
                            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
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
            egui::Window::new("Delete Permanently").collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Permanently delete {n} item(s)? This cannot be undone."
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            confirm = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
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
            egui::Window::new("Trash unavailable").collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Trash is unavailable for {n} item(s). Permanently delete them instead?"
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Delete permanently").clicked() {
                            yes = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
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
            egui::Window::new("Item already exists")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("The destination already contains an item with this name:");
                    ui.label(egui::RichText::new(dst.display().to_string()).strong());
                    ui.label(egui::RichText::new(format!("Source: {}", src.display())).small().weak());
                    ui.checkbox(apply_all, "Apply to all remaining conflicts");
                    ui.horizontal(|ui| {
                        if ui.button("Replace").clicked() {
                            decision = Some(ConflictAction::Overwrite);
                        }
                        if ui.button("Keep Both").clicked() {
                            decision = Some(ConflictAction::KeepBoth);
                        }
                        if ui.button("Skip").clicked() {
                            decision = Some(ConflictAction::Skip);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(ConflictAction::CancelOp);
                        }
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
            egui::Window::new("Properties").collapsible(false).resizable(true)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    properties_body(ui, &p);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
        }
        Dialog::About => {
            egui::Window::new("About Flux Files").collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Flux Files — a fast native file explorer");
                    ui.label("Built with Rust + egui.");
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
        }
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

fn bulk_rename_dialog(app: &mut FluxApp, br: &mut BulkRenameState, ctx: &egui::Context, close: &mut bool) {
    if br.dirty && br.plan.is_empty() {
        br.recompute();
    }
    let n_ready = br.plan.iter().filter(|a| a.status.is_ready()).count();
    let n_skip = br.plan.len() - n_ready;
    let mut apply = false;
    egui::Window::new("Bulk Rename")
        .collapsible(false)
        .resizable(true)
        .default_size([560.0, 420.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.columns(2, |cols| {
                cols[0].set_min_width(200.0);
                cols[0].vertical(|ui| {
                    ui.label("Find");
                    if ui.text_edit_singleline(&mut br.find).changed() {
                        br.dirty = true;
                        br.last_edit = std::time::Instant::now();
                    }
                    ui.label("Replace with");
                    if ui.text_edit_singleline(&mut br.replace).changed() {
                        br.dirty = true;
                        br.last_edit = std::time::Instant::now();
                    }
                    if ui.checkbox(&mut br.regex, "Regex").changed() {
                        br.dirty = true;
                    }
                    if ui.checkbox(&mut br.case_sensitive, "Match case").changed() {
                        br.dirty = true;
                    }
                    ui.separator();
                    egui::ComboBox::from_label("Case")
                        .selected_text(match br.case_mode {
                            1 => "UPPERCASE",
                            2 => "lowercase",
                            3 => "Title Case",
                            _ => "Keep case",
                        })
                        .show_ui(ui, |ui| {
                            for (i, l) in ["Keep case", "UPPERCASE", "lowercase", "Title Case"]
                                .iter()
                                .enumerate()
                            {
                                if ui.selectable_label(br.case_mode == i, *l).clicked() {
                                    br.case_mode = i;
                                    br.dirty = true;
                                }
                            }
                        });
                    if ui.checkbox(&mut br.numbering, "Numbering").changed() {
                        br.dirty = true;
                    }
                    if br.numbering {
                        ui.horizontal(|ui| {
                            ui.label("Start");
                            if ui.add(egui::DragValue::new(&mut br.num_start)).changed() {
                                br.dirty = true;
                            }
                            ui.label("Pad");
                            if ui.add(egui::DragValue::new(&mut br.num_pad).range(0..=8)).changed() {
                                br.dirty = true;
                            }
                        });
                        if ui.checkbox(&mut br.num_prefix, "Number as prefix").changed() {
                            br.dirty = true;
                        }
                    }
                    ui.separator();
                    ui.label("Template (overrides above)");
                    if ui.text_edit_singleline(&mut br.template).changed() {
                        br.dirty = true;
                        br.last_edit = std::time::Instant::now();
                    }
                    ui.weak("Tokens: {name} {ext} {n} {n:000} {parent}");
                });
                cols[1].vertical(|ui| {
                    ui.label(format!(
                        "{} items · {} ready · {} skipped",
                        br.plan.len(),
                        n_ready,
                        n_skip
                    ));
                    egui::ScrollArea::both().id_salt("bulk_preview").show(ui, |ui| {
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
                                ui.label(egui::RichText::new(src_n).color(col));
                                ui.label("→");
                                ui.label(egui::RichText::new(dst_n).color(col));
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
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(n_ready > 0, egui::Button::new(format!("Rename {n_ready} items")))
                    .clicked()
                {
                    apply = true;
                }
                if ui.button("Cancel").clicked() {
                    *close = true;
                }
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
    let single = items.len() == 1;
    let first = items.first().cloned();
    if ui.button("Open").clicked() {
        ui.close();
        if let Some(p) = first.clone() {
            if p.is_dir() {
                app.navigate(p);
            } else {
                let _ = flux_platform::open_path(&p);
            }
        }
    }
    if let Some(p) = first.clone()
        && p.is_dir() && ui.button("Open in New Tab").clicked() {
            ui.close();
            app.new_tab_at(p);
        }
    ui.separator();
    if ui.button("Copy").clicked() {
        ui.close();
        app.clipboard.items = items.to_vec();
        app.clipboard.cut = false;
    }
    if ui.button("Cut").clicked() {
        ui.close();
        app.clipboard.items = items.to_vec();
        app.clipboard.cut = true;
    }
    if ui.button("Duplicate").clicked() {
        ui.close();
        app.ops.submit(
            flux_operations::OpKind::Duplicate {
                items: items.to_vec(),
            },
            flux_operations::ConflictPolicy::SkipAll,
        );
    }
    ui.separator();
    if ui.button("Rename…").clicked() {
        ui.close();
        if items.len() == 1 {
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
    if ui.button("Move to Trash").clicked() {
        ui.close();
        app.submit_trash(items.to_vec());
    }
    ui.separator();
    if ui.button("Copy Path").clicked() {
        ui.close();
        app.pending_copy_text = Some(
            items
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    if single
        && let Some(p) = first.clone()
            && ui.button("Properties").clicked() {
                ui.close();
                app.dialog = Dialog::Properties { path: p };
            }
}
