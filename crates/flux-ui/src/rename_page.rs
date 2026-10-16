//! Bulk Rename as a workspace, not a cramped dialog: a rules column on
//! the left and a full-height before/after preview on the right, with a
//! pinned footer carrying counts and the commit button.
//!
//! The plan is computed on the `flux-rename-plan` worker thread (see
//! `BulkRenameState::recompute_async`); this file only renders state.

use std::path::Path;

use flux_operations::RenameStatus;

use crate::app::{FluxApp, RenameFilter, Screen};
use crate::glyphs::{self, Glyph};
use crate::theme::Palette;
use crate::widgets::{self, sp};

const RULES_W: f32 = 292.0;
const ROW_H: f32 = 26.0;
/// Rows beyond this stay fully readable — names truncate at the column
/// split instead of scrolling horizontally.
const NARROW_W: f32 = 760.0;

/// The workspace — called from the central panel when
/// `app.screen == Screen::BulkRename`.
pub fn page(app: &mut FluxApp, ui: &mut egui::Ui) {
    let pal = Palette::of(&ui.style().visuals);

    header(app, ui, &pal);

    if ui.available_width() < NARROW_W {
        // Compact stacking: rules collapse into a disclosure header so the
        // preview keeps as much height as possible.
        let open_id = ui.id().with("rules_open");
        let open = ui.data_mut(|d| d.get_temp(open_id)).unwrap_or(false);
        let header_resp = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (ir, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
            // Disclosure triangle: right-pointing when closed, down when open.
            let c = ir.center();
            let r = ir.width() * 0.36;
            let pts = if open {
                vec![
                    c + egui::vec2(-r, -r * 0.6),
                    c + egui::vec2(r, -r * 0.6),
                    c + egui::vec2(0.0, r * 0.8),
                ]
            } else {
                vec![
                    c + egui::vec2(-r * 0.6, -r),
                    c + egui::vec2(-r * 0.6, r),
                    c + egui::vec2(r * 0.8, 0.0),
                ]
            };
            ui.painter().add(egui::Shape::convex_polygon(
                pts,
                pal.text_secondary,
                egui::Stroke::NONE,
            ));
            ui.label(
                egui::RichText::new("Rules")
                    .size(14.0)
                    .color(pal.text_primary),
            );
        });
        let resp = ui.interact(
            header_resp.response.rect,
            ui.id().with("rules_hdr"),
            egui::Sense::click(),
        );
        if resp.clicked() {
            ui.data_mut(|d| d.insert_temp(open_id, !open));
        }
        if open {
            egui::ScrollArea::vertical()
                .id_salt("rules_narrow")
                .max_height(260.0)
                .show(ui, |ui| rules(app, ui, &pal));
        }
        widgets::row_divider(ui, &pal);
        preview_and_footer(app, ui, &pal);
        return;
    }

    // Wide layout: split the remaining region manually. `ui.horizontal`
    // children only get `interact_size.y` of height until they grow, so a
    // plain horizontal would leave both columns vertically compressed.
    let avail = ui.available_rect_before_wrap();
    ui.allocate_rect(avail, egui::Sense::hover());
    let rules_w = RULES_W.min(avail.width() * 0.45);
    let sep_x = avail.left() + rules_w + 5.0;
    let rules_rect = egui::Rect::from_min_size(avail.min, egui::vec2(rules_w, avail.height()));
    let pv_rect = egui::Rect::from_min_max(egui::pos2(sep_x + 5.0, avail.top()), avail.max);

    let mut rules_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rules_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    rules_ui.add_space(sp::XS);
    egui::ScrollArea::vertical()
        .id_salt("rules")
        .auto_shrink([false, false])
        .show(&mut rules_ui, |ui| rules(app, ui, &pal));

    ui.painter().vline(
        sep_x,
        avail.top()..=avail.bottom(),
        egui::Stroke::new(1.0, pal.border_soft),
    );

    let mut pv_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(pv_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    preview_and_footer(app, &mut pv_ui, &pal);
}

fn header(app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette) {
    ui.add_space(sp::MD);
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(34.0, 30.0), egui::Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(7), pal.surface_hover);
        }
        glyphs::paint_glyph(
            ui.painter(),
            rect.shrink(6.0),
            Glyph::Back,
            pal.text_primary,
        );
        if resp.on_hover_text("Back to files (Esc)").clicked() {
            app.close_screen();
        }
        ui.add_space(sp::SM);
        ui.label(
            egui::RichText::new("Bulk Rename")
                .color(pal.text_primary)
                .size(22.0)
                .strong(),
        );
        if let Screen::BulkRename(br) = &app.screen {
            ui.add_space(sp::XS);
            widgets::status_chip(ui, pal, "info", &format!("{} items", br.sources.len()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Screen::BulkRename(br) = &mut app.screen {
                let w = ui.available_width().clamp(110.0, 200.0);
                let r = widgets::text_field(ui, &mut br.query, "Filter preview…", w);
                let _ = r;
            }
        });
    });
    ui.add_space(sp::SM);
}

// ── Rules column ──

fn rules(app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette) {
    let Screen::BulkRename(br) = &mut app.screen else {
        return;
    };
    let field_w = ui.available_width();

    ui.label(
        egui::RichText::new("Find & replace")
            .size(15.0)
            .strong()
            .color(pal.text_primary),
    );
    ui.add_space(sp::SM);
    ui.label(
        egui::RichText::new("Find")
            .size(12.0)
            .color(pal.text_secondary),
    );
    if widgets::text_field(ui, &mut br.find, "Text to match", field_w).changed() {
        br.mark_dirty();
    }
    ui.add_space(sp::XS);
    ui.label(
        egui::RichText::new("Replace with")
            .size(12.0)
            .color(pal.text_secondary),
    );
    if widgets::text_field(ui, &mut br.replace, "Replacement", field_w).changed() {
        br.mark_dirty();
    }
    ui.add_space(sp::SM);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Regex")
                .size(12.5)
                .color(pal.text_secondary),
        );
        ui.add_space(4.0);
        let mut v = br.regex;
        if widgets::switch(ui, &mut v).changed() {
            br.regex = v;
            br.mark_dirty();
        }
        ui.add_space(sp::LG);
        ui.label(
            egui::RichText::new("Match case")
                .size(12.5)
                .color(pal.text_secondary),
        );
        ui.add_space(4.0);
        let mut v = br.case_sensitive;
        if widgets::switch(ui, &mut v).changed() {
            br.case_sensitive = v;
            br.mark_dirty();
        }
    });

    widgets::section_heading(ui, pal, "Case", None);
    if let Some(v) = widgets::segmented(
        ui,
        pal,
        &[(0usize, "Keep"), (1, "ABC"), (2, "abc"), (3, "Title")],
        br.case_mode,
    ) {
        br.case_mode = v;
        br.mark_dirty();
    }

    widgets::section_heading(
        ui,
        pal,
        "Numbering",
        Some("Appended or prepended sequence, after other rules."),
    );
    ui.horizontal(|ui| {
        let mut v = br.numbering;
        if widgets::switch(ui, &mut v).changed() {
            br.numbering = v;
            br.mark_dirty();
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Add a counter")
                .size(12.5)
                .color(pal.text_secondary),
        );
    });
    if br.numbering {
        ui.add_space(sp::SM);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Start")
                    .size(12.5)
                    .color(pal.text_secondary),
            );
            if ui
                .add(
                    egui::DragValue::new(&mut br.num_start)
                        .speed(1)
                        .range(0..=999_999),
                )
                .changed()
            {
                br.mark_dirty();
            }
            ui.add_space(sp::MD);
            ui.label(
                egui::RichText::new("Digits")
                    .size(12.5)
                    .color(pal.text_secondary),
            );
            if ui
                .add(
                    egui::DragValue::new(&mut br.num_pad)
                        .speed(0.1)
                        .range(1..=8),
                )
                .changed()
            {
                br.mark_dirty();
            }
        });
        ui.add_space(sp::XS);
        if let Some(v) = widgets::segmented(
            ui,
            pal,
            &[(false, "Suffix"), (true, "Prefix")],
            br.num_prefix,
        ) {
            br.num_prefix = v;
            br.mark_dirty();
        }
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(if br.num_prefix {
                "e.g. 001_photo.jpg"
            } else {
                "e.g. photo_001.jpg"
            })
            .size(11.0)
            .color(pal.text_muted),
        );
    }

    widgets::section_heading(ui, pal, "Extension", None);
    ui.horizontal(|ui| {
        let mut on = br.set_ext.is_some();
        if widgets::switch(ui, &mut on).changed() {
            br.set_ext = on.then(String::new);
            br.mark_dirty();
        }
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Change extension")
                .size(12.5)
                .color(pal.text_secondary),
        );
    });
    if br.set_ext.is_some() {
        ui.add_space(sp::XS);
        let mut ext = br.set_ext.clone().unwrap_or_default();
        let r = widgets::text_field(ui, &mut ext, "new extension (empty removes)", field_w * 0.7);
        if r.changed() {
            br.set_ext = Some(ext);
            br.mark_dirty();
        }
    }

    widgets::section_heading(
        ui,
        pal,
        "Template",
        Some("Tokens: {name} {ext} {n} {n:000} {parent}. Overrides other rules when set."),
    );
    if widgets::text_field(ui, &mut br.template, "e.g. {name}-{n:000}", field_w).changed() {
        br.mark_dirty();
    }
    ui.add_space(sp::LG);
}

// ── Preview + footer ──

fn preview_and_footer(app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette) {
    let Screen::BulkRename(br) = &mut app.screen else {
        return;
    };

    let w = ui.available_width().max(0.0);
    let counts = counts(&br.plan);
    let rows = filtered(&br.plan, br.filter, &br.query);
    let stale = br.plan_stale;

    // Filter bar — segmented + live counts.
    ui.horizontal(|ui| {
        let opts = [
            (RenameFilter::All, "All"),
            (RenameFilter::Changed, "Changed"),
            (RenameFilter::Unchanged, "Unchanged"),
            (RenameFilter::Issues, "Issues"),
        ];
        if let Some(f) = widgets::segmented(ui, pal, &opts, br.filter) {
            br.filter = f;
        }
        ui.add_space(sp::MD);
        if stale {
            ui.add(egui::Spinner::new().size(14.0));
            ui.label(
                egui::RichText::new("Updating…")
                    .size(12.0)
                    .color(pal.text_muted),
            );
        }
        if w >= 380.0 {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} shown", rows.len()))
                        .size(12.0)
                        .color(pal.text_muted),
                );
            });
        }
    });
    ui.add_space(sp::SM);

    // Column headers — fixed proportions shared with the rows so
    // before/after always line up, never scroll horizontally.
    let header = |ui: &mut egui::Ui| {
        ui.horizontal(|ui| {
            let w = ui.available_width().max(0.0);
            let status_w = 84.0;
            let name_w = ((w - status_w - 30.0) / 2.0).max(40.0);
            ui.allocate_ui(egui::vec2(name_w, 18.0), |ui| {
                ui.label(
                    egui::RichText::new("ORIGINAL")
                        .size(10.5)
                        .strong()
                        .color(pal.text_muted),
                );
            });
            ui.allocate_exact_size(egui::vec2(30.0, 18.0), egui::Sense::hover());
            ui.allocate_ui(egui::vec2(name_w, 18.0), |ui| {
                ui.label(
                    egui::RichText::new("NEW NAME")
                        .size(10.5)
                        .strong()
                        .color(pal.text_muted),
                );
            });
            ui.allocate_ui(egui::vec2(status_w, 18.0), |ui| {
                ui.label(
                    egui::RichText::new("STATUS")
                        .size(10.5)
                        .strong()
                        .color(pal.text_muted),
                );
            });
        });
    };
    header(ui);
    ui.add_space(2.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.center().y,
        egui::Stroke::new(1.0, pal.border_soft),
    );

    // `rows`/`stale` were computed above, before the filter bar could
    // mutate `br.filter` — consistent snapshot for header + list.
    let n = rows.len();
    // Subtract the footer height so the list never slides under it.
    let footer_h = 52.0;
    let list_h = (ui.available_height() - footer_h).max(80.0);
    ui.allocate_ui(egui::vec2(w, list_h), |ui| {
        if n == 0 {
            let (title, hint) = if br.plan.is_empty() {
                (
                    "Nothing to rename",
                    Some("Close this page and select some files first."),
                )
            } else if br.filter == RenameFilter::Issues {
                ("No issues", Some("Every planned rename is clean."))
            } else if br.filter == RenameFilter::Changed && counts.changed == 0 {
                (
                    "No changes yet",
                    Some("Adjust the rules on the left — the preview updates live."),
                )
            } else {
                ("No matches", Some("Try clearing the filter or search."))
            };
            widgets::empty_state(
                ui,
                pal,
                |p, r, c| glyphs::paint_glyph(p, r, Glyph::Pencil, c),
                title,
                hint,
            );
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("rename_rows")
            .auto_shrink([false, true])
            .show_rows(ui, ROW_H, n, |ui, range| {
                for i in range {
                    let a = &br.plan[rows[i]];
                    row(ui, pal, a, stale);
                }
            });
    });

    // ── Footer: counts left, actions right ──
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, footer_h), egui::Sense::hover());
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.top(),
        egui::Stroke::new(1.0, pal.border_soft),
    );
    let mut fui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(0.0, 10.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    fui.horizontal(|ui| {
        let issues = counts.issues;
        if w >= 460.0 {
            if issues > 0 {
                widgets::status_chip(
                    ui,
                    pal,
                    "err",
                    &format!("{issues} issues — those items are skipped"),
                );
            } else {
                widgets::status_chip(ui, pal, "ok", &format!("{} ready", counts.changed));
            }
            ui.add_space(sp::SM);
            ui.label(
                egui::RichText::new(format!("{} unchanged", counts.unchanged))
                    .size(12.0)
                    .color(pal.text_muted),
            );
        } else {
            // Narrow: keep just the essential count.
            let (col, txt) = if issues > 0 {
                (pal.danger, format!("{issues} issues"))
            } else {
                (pal.success, format!("{} ready", counts.changed))
            };
            ui.label(egui::RichText::new(txt).size(12.0).color(col));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if w >= 340.0 {
                format!(
                    "Rename {} item{}",
                    counts.changed,
                    if counts.changed == 1 { "" } else { "s" }
                )
            } else {
                "Rename".to_string()
            };
            let btn = ui.add_enabled(
                counts.changed > 0 && !stale,
                egui::Button::new(
                    egui::RichText::new(label)
                        .color(egui::Color32::WHITE)
                        .size(13.5),
                )
                .fill(pal.accent)
                .corner_radius(egui::CornerRadius::same(7))
                .min_size(egui::vec2(0.0, 30.0)),
            );
            if btn.clicked() {
                if let Screen::BulkRename(br) = &mut app.screen {
                    let actions = std::mem::take(&mut br.plan);
                    app.ops.submit(
                        flux_operations::OpKind::BatchRename { actions },
                        flux_operations::ConflictPolicy::Ask,
                    );
                }
                app.close_screen();
            }
            ui.add_space(sp::SM);
            if widgets::ghost_button(ui, "Cancel").clicked() {
                app.close_screen();
            }
        });
    });
}

fn row(ui: &mut egui::Ui, pal: &Palette, a: &flux_operations::RenameAction, stale: bool) {
    let w = ui.available_width().max(0.0);
    let status_w = 84.0;
    let name_w = ((w - status_w - 30.0) / 2.0).max(40.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, ROW_H), egui::Sense::hover());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(5), pal.surface_hover);
    }
    let dim = if stale { 0.55 } else { 1.0 };
    let old_name = a
        .src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let new_name = a
        .dst
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cy = rect.center().y;
    // Original — always readable start; truncated tail keeps extension-ish.
    let mut lrect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 6.0, rect.top()),
        egui::pos2(rect.left() + name_w, rect.bottom()),
    );
    ui.painter().text(
        egui::pos2(lrect.left(), cy),
        egui::Align2::LEFT_CENTER,
        truncated(ui, &old_name, name_w - 12.0),
        egui::FontId::monospace(12.5),
        pal.text_secondary.gamma_multiply(dim),
    );
    // Arrow between the columns.
    glyphs::paint_glyph(
        ui.painter(),
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + name_w + 15.0, cy),
            egui::Vec2::splat(11.0),
        ),
        Glyph::Forward,
        pal.text_muted.gamma_multiply(dim),
    );
    lrect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + name_w + 30.0, rect.top()),
        egui::pos2(rect.left() + name_w + 30.0 + name_w, rect.bottom()),
    );
    let new_col = match &a.status {
        RenameStatus::Ready => pal.success,
        RenameStatus::Unchanged => pal.text_muted,
        _ => pal.danger,
    };
    ui.painter().text(
        egui::pos2(lrect.left(), cy),
        egui::Align2::LEFT_CENTER,
        truncated(ui, &new_name, name_w - 12.0),
        egui::FontId::monospace(12.5),
        new_col.gamma_multiply(dim),
    );
    // Status chip on the right.
    let (label, col) = match &a.status {
        RenameStatus::Ready => ("Ready", pal.success),
        RenameStatus::Unchanged => ("Same", pal.text_muted),
        RenameStatus::Conflict => ("Exists", pal.warn),
        RenameStatus::DuplicateTarget => ("Dup.", pal.warn),
        RenameStatus::Invalid(_) => ("Invalid", pal.danger),
    };
    ui.painter().text(
        egui::pos2(rect.right() - 6.0, cy),
        egui::Align2::RIGHT_CENTER,
        label,
        egui::FontId::proportional(11.0),
        col.gamma_multiply(dim),
    );
    if let RenameStatus::Invalid(msg) = &a.status {
        resp.on_hover_text(format!("{old_name}: {msg}"));
    }
}

/// Single-line ellipsis truncate at pixel width — cheap version using
/// egui's galley for a byte-boundary-safe cut.
fn truncated(ui: &egui::Ui, s: &str, max_w: f32) -> String {
    let fid = egui::FontId::monospace(12.5);
    let full = ui
        .painter()
        .layout_no_wrap(s.to_string(), fid.clone(), egui::Color32::WHITE);
    if full.size().x <= max_w {
        return s.to_string();
    }
    // Binary-search the longest prefix that fits with "…".
    let mut lo = 0;
    let mut hi = s.len();
    while lo + 1 < hi {
        let mut mid = (lo + hi) / 2;
        while mid > lo && !s.is_char_boundary(mid) {
            mid -= 1;
        }
        if mid <= lo {
            break;
        }
        let t = format!("{}…", &s[..mid]);
        if ui
            .painter()
            .layout_no_wrap(t, fid.clone(), egui::Color32::WHITE)
            .size()
            .x
            <= max_w
        {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    format!("{}…", &s[..lo])
}

struct Counts {
    changed: usize,
    unchanged: usize,
    issues: usize,
}

fn counts(plan: &[flux_operations::RenameAction]) -> Counts {
    let mut c = Counts {
        changed: 0,
        unchanged: 0,
        issues: 0,
    };
    for a in plan {
        match a.status {
            RenameStatus::Ready => c.changed += 1,
            RenameStatus::Unchanged => c.unchanged += 1,
            _ => c.issues += 1,
        }
    }
    c
}

/// Indices into `plan` matching the active filter + filename query.
fn filtered(
    plan: &[flux_operations::RenameAction],
    filter: RenameFilter,
    query: &str,
) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    plan.iter()
        .enumerate()
        .filter(|(_, a)| {
            let keep = match filter {
                RenameFilter::All => true,
                RenameFilter::Changed => a.status.is_ready(),
                RenameFilter::Unchanged => a.status == RenameStatus::Unchanged,
                RenameFilter::Issues => !a.status.is_ready() && a.status != RenameStatus::Unchanged,
            };
            if !keep {
                return false;
            }
            if q.is_empty() {
                return true;
            }
            let hay = |p: &Path| {
                p.file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default()
            };
            hay(&a.src).contains(&q) || hay(&a.dst).contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}
