//! The virtualized file table. Only visible rows are laid out — egui_extras'
//! TableBuilder takes total row count + row height and invokes the closure
//! for the visible range plus a small overscan.

use egui::{Align, Color32, Layout, Sense, Vec2};
use egui_extras::{Column, TableBuilder};
use flux_core::entry::EntryKind;
use flux_core::{EntryIdx, SortKey, SortSpec};

use crate::app::FluxApp;
use crate::format::{format_mtime, format_size};
use crate::theme::Palette;

/// Mutations the table requests; applied by the app after rendering so the
/// row closures only ever need shared borrows.
#[derive(Default)]
pub struct RowAction {
    pub open: Option<EntryIdx>,
    pub click: Option<usize>,
    pub click_shift: bool,
    pub click_cmd: bool,
    pub context: Option<(usize, egui::Pos2)>,
    pub sort: Option<SortSpec>,
    pub retry: bool,
    /// Middle-click on a directory row → open it in a new tab.
    pub open_new_tab: Option<EntryIdx>,
    /// A row drag crossed egui's threshold — start a file drag.
    pub drag_start: Option<usize>,
    /// While dragging: the hovered directory row (drop target).
    pub drop_row: Option<usize>,
    /// Min/max entry index of the rows actually rendered this frame —
    /// fed to `MetaEngine::set_priority` so background statting covers
    /// the viewport first. A superset range is fine: the engine still
    /// fills sequentially for anything outside it.
    pub visible_range: Option<(EntryIdx, EntryIdx)>,
}

/// 16-px-ish painted glyphs: folder silhouette, document with folded corner,
/// symlink arrow. Vector painting avoids icon-font coverage issues entirely.
pub fn paint_entry_icon(p: &egui::Painter, rect: egui::Rect, kind: EntryKind, pal: &Palette) {
    match kind {
        EntryKind::Dir => {
            // Two-tone folder: darker back + tab, lighter front body —
            // keeps the silhouette legible down to ~12px.
            let (w, h) = (rect.width(), rect.height());
            let dark = pal.dir_icon.gamma_multiply(0.62);
            let light = pal.dir_icon;
            let tab = egui::Rect::from_min_size(rect.min, Vec2::new(w * 0.44, h * 0.34));
            p.rect_filled(tab, 1.5, dark);
            let back = egui::Rect::from_min_max(
                egui::pos2(rect.left() + w * 0.4, rect.top() + h * 0.12),
                egui::pos2(rect.right(), rect.top() + h * 0.42),
            );
            p.rect_filled(back, 1.5, dark);
            let body =
                egui::Rect::from_min_max(egui::pos2(rect.left(), rect.top() + h * 0.32), rect.max);
            p.rect_filled(body, 2.0, light);
            // Sheen line across the front edge.
            p.line_segment(
                [
                    body.left_top() + Vec2::new(1.0, 0.5),
                    body.right_top() + Vec2::new(-1.0, 0.5),
                ],
                (0.8, Color32::WHITE.gamma_multiply(0.35)),
            );
        }
        EntryKind::Symlink => {
            let body = rect.shrink(1.0);
            p.rect_stroke(body, 1.5, (1.0, pal.link_icon), egui::StrokeKind::Inside);
            let c = body.center();
            p.line_segment(
                [c - Vec2::new(3.5, 0.0), c + Vec2::new(2.5, 0.0)],
                (1.4, pal.link_icon),
            );
            p.line_segment(
                [c + Vec2::new(0.5, -2.5), c + Vec2::new(2.5, 0.0)],
                (1.4, pal.link_icon),
            );
            p.line_segment(
                [c + Vec2::new(0.5, 2.5), c + Vec2::new(2.5, 0.0)],
                (1.4, pal.link_icon),
            );
        }
        _ => {
            let body = rect.shrink(1.0);
            p.rect_stroke(body, 1.5, (1.0, pal.file_icon), egui::StrokeKind::Inside);
            let w = body.width() * 0.3;
            let h = body.height() * 0.3;
            let corner = vec![
                egui::pos2(body.right() - w, body.top()),
                egui::pos2(body.right(), body.top() + h),
                egui::pos2(body.right() - w, body.top() + h),
            ];
            p.add(egui::Shape::convex_polygon(
                corner,
                Color32::TRANSPARENT,
                (1.0, pal.file_icon),
            ));
        }
    }
}

/// Largest centered rect inside `outer` that keeps `img`'s aspect ratio.
fn fit_rect(outer: egui::Rect, img: egui::Vec2) -> egui::Rect {
    if img.x <= 0.0 || img.y <= 0.0 {
        return outer;
    }
    let scale = (outer.width() / img.x).min(outer.height() / img.y);
    egui::Rect::from_center_size(outer.center(), img * scale)
}

fn row_icon_rect(row_rect: egui::Rect, row_h: f32) -> egui::Rect {
    let size = row_h * 0.62;
    egui::Rect::from_center_size(
        egui::pos2(row_rect.left() + row_h * 0.55, row_rect.center().y),
        Vec2::splat(size),
    )
}

pub fn file_table(app: &mut FluxApp, ui: &mut egui::Ui) -> RowAction {
    let mut action = RowAction::default();
    let pal = Palette::of(&ui.style().visuals);
    let row_h = app.density.row_height();
    let widths = app.settings.column_widths;
    // Only called for directory tabs — workspace tabs render elsewhere.
    let Some(snap) = app.current_snap() else {
        return action;
    };

    let (view_len, loading, state_label, sort_spec, show_hidden) = {
        let s = snap.read().unwrap();
        let label = match &s.state {
            flux_core::LoadState::Loading { received } => Some(format!(
                "Loading… {}",
                crate::format::format_count(*received)
            )),
            flux_core::LoadState::Failed(m) => Some(format!("Cannot read folder: {m}")),
            flux_core::LoadState::Ready { .. } if s.view_len() == 0 => {
                if s.filter.is_some() {
                    Some("No items match the filter".into())
                } else {
                    Some("This folder is empty".into())
                }
            }
            _ => None,
        };
        (
            s.view_len(),
            s.state.is_loading(),
            label,
            s.sort,
            s.show_hidden,
        )
    };
    let _ = show_hidden;

    // Empty / failed state occupies the area (except while streaming, where
    // partial rows still show).
    if let Some(msg) = &state_label
        && (!loading || view_len == 0)
    {
        let failed = msg.starts_with("Cannot read");
        if loading {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(26.0));
                ui.add_space(8.0);
                ui.label(egui::RichText::new(msg).weak());
            });
        } else {
            crate::widgets::empty_state(
                ui,
                &pal,
                |p, r, c| {
                    crate::glyphs::paint_glyph(
                        p,
                        r,
                        if failed {
                            crate::glyphs::Glyph::Info
                        } else {
                            crate::glyphs::Glyph::Folder
                        },
                        c,
                    )
                },
                msg,
                if failed {
                    Some("Check the path and permissions.")
                } else if msg.starts_with("No items") {
                    Some("Clear the filter to see everything.")
                } else {
                    None
                },
            );
            if failed {
                ui.vertical_centered(|ui| {
                    if crate::widgets::secondary_button(ui, "Retry").clicked() {
                        action.retry = true;
                    }
                });
            }
        }
        return action;
    }

    if !matches!(app.settings.view_mode, flux_settings::ViewMode::List) {
        return match app.settings.view_mode {
            flux_settings::ViewMode::Compact => compact_list(app, ui, &pal),
            flux_settings::ViewMode::Grid => icon_grid(app, ui, &pal, 96.0),
            flux_settings::ViewMode::Gallery => icon_grid(app, ui, &pal, 200.0),
            flux_settings::ViewMode::List => unreachable!(),
        };
    }

    let (focus_row, scroll_to) = {
        let Some(d) = app.current_dir_mut() else {
            return action;
        };
        (d.sel.focus, d.scroll_to_row.take())
    };
    // Borrow the selection once; row closure does O(1) lookups, no clones.
    let Some(dtab) = app.current_dir() else {
        return action;
    };
    let sel = &dtab.sel;
    let egui_ctx = ui.ctx().clone();
    let icons = app.icons.clone();
    let thumbs = app.thumbs.clone();
    let drag_active = app.dragging.is_some();

    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .resizable(true)
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(
            Column::initial(widths[0])
                .at_least(120.0)
                .resizable(true)
                .clip(true),
        )
        .column(Column::initial(widths[1]).at_least(50.0).resizable(true))
        .column(Column::initial(widths[2]).at_least(80.0).resizable(true))
        .column(Column::remainder().at_least(60.0).clip(true))
        .min_scrolled_height(0.0)
        .sense(Sense::click_and_drag());
    if let Some(r) = scroll_to {
        builder = builder.scroll_to_row(r, Some(Align::Center));
    }

    let header = |mut header: egui_extras::TableRow<'_, '_>| {
        for (label, key) in [
            ("Name", SortKey::Name),
            ("Size", SortKey::Size),
            ("Modified", SortKey::Modified),
            ("Type", SortKey::Kind),
        ] {
            header.col(|ui| {
                let sorted = sort_spec.key == key;
                // Quiet header: secondary text that brightens on hover,
                // sorted column in primary + a painted triangle.
                let col = if sorted {
                    pal.text_primary
                } else {
                    pal.text_secondary
                };
                let resp = ui.add(
                    egui::Label::new(egui::RichText::new(label).size(12.0).color(col))
                        .sense(egui::Sense::click()),
                );
                if sorted {
                    let r = egui::Rect::from_center_size(
                        egui::pos2(resp.rect.right() + 7.0, resp.rect.center().y),
                        egui::Vec2::splat(7.0),
                    );
                    crate::glyphs::paint_triangle(
                        ui.painter(),
                        r,
                        sort_spec.dir == flux_core::SortDir::Asc,
                        pal.text_primary,
                    );
                }
                if resp.clicked() {
                    action.sort = Some(SortSpec::for_header_click(&sort_spec, key));
                }
            });
        }
    };

    let mut vis_min = EntryIdx::MAX;
    let mut vis_max = 0;
    let mut live_widths: Option<[f32; 4]> = None;
    builder.header(22.0, header).body(|body| {
        // Read widths BEFORE rows() consumes the body — reflects the
        // resize state already applied for this frame.
        let w = body.widths();
        if w.len() == 4 {
            live_widths = Some([w[0], w[1], w[2], w[3]]);
        }
        body.rows(row_h, view_len, |mut row| {
            let row_idx = row.index();
            let (idx, name, kind, size, mtime, ext, icon_key, abs_path, hidden) = {
                let s = match snap.read() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let Some((idx, e)) = s.row(row_idx) else {
                    return;
                };
                (
                    idx,
                    e.display_name().into_owned(),
                    e.kind,
                    e.size,
                    e.mtime,
                    e.extension().map(|x| x.to_string_lossy().into_owned()),
                    crate::icon_cache::IconKey::for_entry(e, &s.path.join(&e.name)),
                    s.path.join(&e.name),
                    e.hidden,
                )
            };

            vis_min = vis_min.min(idx);
            vis_max = vis_max.max(idx);
            let selected = sel.contains(idx);
            row.set_selected(selected);

            // Explorer ghosts revealed hidden items — dim icon + text.
            let ghost = if hidden { 0.55 } else { 1.0 };
            row.col(|ui| {
                let rect = ui.available_rect_before_wrap();
                let icon_rect = row_icon_rect(rect, row_h);
                // Image files get a decoded thumbnail; everything else the
                // kind-keyed OS icon, falling back to painted glyphs.
                let thumb = if kind == EntryKind::File && crate::thumbs::is_thumb_ext(&abs_path) {
                    thumbs.texture(&abs_path)
                } else {
                    None
                };
                let tex = thumb
                    .as_ref()
                    .map(|t| t.id())
                    .or_else(|| icons.texture(&icon_key, &abs_path));
                match tex {
                    Some(tex) => {
                        // Thumbs keep aspect; kind icons are already square.
                        let r = match &thumb {
                            Some(h) => fit_rect(icon_rect, h.size_vec2()),
                            None => icon_rect,
                        };
                        ui.painter().image(
                            tex,
                            r,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE.gamma_multiply(ghost),
                        );
                    }
                    None => paint_entry_icon(ui.painter(), icon_rect, kind, &pal),
                }
                let text_pos = egui::pos2(rect.left() + row_h * 1.05, rect.center().y);
                ui.painter().text(
                    text_pos,
                    egui::Align2::LEFT_CENTER,
                    &name,
                    egui::FontId::default(),
                    pal.text_primary.gamma_multiply(ghost),
                );
            });
            // Metadata is secondary information — quieter color.
            let meta_col = pal.text_secondary.gamma_multiply(ghost);
            row.col(|ui| {
                ui.label(egui::RichText::new(format_size(size)).color(meta_col));
            });
            row.col(|ui| {
                ui.label(egui::RichText::new(format_mtime(mtime)).color(meta_col));
            });
            row.col(|ui| {
                ui.label(
                    egui::RichText::new(match kind {
                        EntryKind::Dir => "Folder",
                        EntryKind::Symlink => "Link",
                        _ => flux_platform::type_label(ext.as_deref()),
                    })
                    .color(meta_col),
                );
            });

            let resp = row.response();
            if resp.drag_started() {
                action.drag_start = Some(row_idx);
            }
            // `contains_pointer`: `hovered` is only the dragged widget while
            // a button is down — drop zones need the positional test.
            if drag_active && kind == EntryKind::Dir && resp.contains_pointer() {
                action.drop_row = Some(row_idx);
                // Bright accent glow marking the drop target row.
                crate::glyphs::glow_drop_target(
                    &egui_ctx.debug_painter(),
                    resp.rect,
                    egui_ctx
                        .style_of(egui_ctx.theme())
                        .visuals
                        .selection
                        .stroke
                        .color,
                );
            }
            if resp.clicked() {
                let mods = egui_ctx.input(|i| i.modifiers);
                action.click = Some(row_idx);
                action.click_shift = mods.shift;
                action.click_cmd = mods.command;
            }
            if resp.double_clicked() {
                action.open = Some(idx);
            }
            if resp.middle_clicked() && kind == EntryKind::Dir {
                action.open_new_tab = Some(idx);
            }
            if resp.secondary_clicked() {
                action.context = Some((row_idx, resp.interact_pointer_pos().unwrap_or_default()));
            }
        });
    });

    if vis_min <= vis_max {
        action.visible_range = Some((vis_min, vis_max));
    }
    // Persist user-resized column widths — debounced: mark dirty here,
    // the app writes the settings file once resizing has been quiet.
    if let Some(w) = live_widths
        && w != app.settings.column_widths
    {
        app.settings.column_widths = w;
        app.settings_dirty_at = Some(std::time::Instant::now());
    }
    let _ = focus_row;
    action
}

/// Apply a RowAction to the app's selection state; `open` handled by caller.
pub fn apply_click(app: &mut FluxApp, action: &RowAction) {
    if let Some(row) = action.click
        && let Some(tab) = app.current_dir_mut()
    {
        let s = tab.snap.read().unwrap();
        if action.click_shift {
            tab.sel.range_to(&s, row);
        } else if action.click_cmd {
            tab.sel.toggle_row(&s, row);
        } else {
            tab.sel.select_row(&s, row);
        }
    }
}

/// Compact mode: names-only dense rows, no column header — one row per
/// entry, same virtualization as the details table.
fn compact_list(app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette) -> RowAction {
    let mut action = RowAction::default();
    let row_h = app.density.row_height() * 0.82;
    let Some(snap) = app.current_snap() else {
        return action;
    };
    let view_len = snap.read().map(|s| s.view_len()).unwrap_or(0);
    let scroll_to = app.current_dir_mut().and_then(|d| d.scroll_to_row.take());
    let sel = app.current_dir().map(|d| &d.sel);
    let egui_ctx = ui.ctx().clone();
    let icons = app.icons.clone();
    let drag_active = app.dragging.is_some();

    let mut vis_min = EntryIdx::MAX;
    let mut vis_max = 0;
    let mut builder = TableBuilder::new(ui)
        .striped(true)
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::remainder().clip(true))
        .min_scrolled_height(0.0)
        .sense(Sense::click_and_drag());
    if let Some(r) = scroll_to {
        builder = builder.scroll_to_row(r, Some(Align::Center));
    }
    builder.body(|body| {
        body.rows(row_h, view_len, |mut row| {
            let row_idx = row.index();
            let (idx, name, kind, icon_key, abs_path, hidden) = {
                let s = match snap.read() {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let Some((idx, e)) = s.row(row_idx) else {
                    return;
                };
                (
                    idx,
                    e.display_name().into_owned(),
                    e.kind,
                    crate::icon_cache::IconKey::for_entry(e, &s.path.join(&e.name)),
                    s.path.join(&e.name),
                    e.hidden,
                )
            };
            vis_min = vis_min.min(idx);
            vis_max = vis_max.max(idx);
            row.set_selected(sel.is_some_and(|s| s.contains(idx)));
            let ghost = if hidden { 0.55 } else { 1.0 };
            row.col(|ui| {
                let rect = ui.available_rect_before_wrap();
                let icon_rect = row_icon_rect(rect, row_h);
                match icons.texture(&icon_key, &abs_path) {
                    Some(tex) => {
                        ui.painter().image(
                            tex,
                            icon_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE.gamma_multiply(ghost),
                        );
                    }
                    None => paint_entry_icon(ui.painter(), icon_rect, kind, pal),
                }
                ui.painter().text(
                    egui::pos2(rect.left() + row_h * 1.05, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &name,
                    egui::FontId::default(),
                    pal.text_primary.gamma_multiply(ghost),
                );
            });
            let resp = row.response();
            if resp.drag_started() {
                action.drag_start = Some(row_idx);
            }
            if drag_active && kind == EntryKind::Dir && resp.contains_pointer() {
                action.drop_row = Some(row_idx);
                crate::glyphs::glow_drop_target(
                    &egui_ctx.debug_painter(),
                    resp.rect,
                    egui_ctx
                        .style_of(egui_ctx.theme())
                        .visuals
                        .selection
                        .stroke
                        .color,
                );
            }
            if resp.clicked() {
                let mods = egui_ctx.input(|i| i.modifiers);
                action.click = Some(row_idx);
                action.click_shift = mods.shift;
                action.click_cmd = mods.command;
            }
            if resp.double_clicked() {
                action.open = Some(idx);
            }
            if resp.middle_clicked() && kind == EntryKind::Dir {
                action.open_new_tab = Some(idx);
            }
            if resp.secondary_clicked() {
                action.context = Some((row_idx, resp.interact_pointer_pos().unwrap_or_default()));
            }
        });
    });
    if vis_min <= vis_max {
        action.visible_range = Some((vis_min, vis_max));
    }
    action
}

/// Grid/Gallery: fixed-size cells (icon on top, name below) laid out
/// left-to-right, top-to-bottom. Virtualized by `ScrollArea::show_rows`
/// over cell-rows — a 100k-entry dir still lays out only the viewport.
fn icon_grid(app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette, cell_w: f32) -> RowAction {
    let mut action = RowAction::default();
    let gallery = cell_w > 120.0;
    let cell_h = cell_w + 20.0; // icon area + one name line
    let Some(snap) = app.current_snap() else {
        return action;
    };
    let view_len = snap.read().map(|s| s.view_len()).unwrap_or(0);
    let avail_w = ui.available_width().max(cell_w);
    let cols = ((avail_w / cell_w).floor() as usize).max(1);
    app.grid_cols = cols;
    let n_rows = view_len.div_ceil(cols);

    let scroll_to = app
        .current_dir_mut()
        .and_then(|d| d.scroll_to_row.take())
        .map(|r| r / cols);
    let sel = app.current_dir().map(|d| &d.sel);
    let egui_ctx = ui.ctx().clone();
    let icons = app.icons.clone();
    let thumbs = app.thumbs.clone();
    let drag_active = app.dragging.is_some();
    let visuals = egui_ctx.style_of(egui_ctx.theme()).visuals.clone();

    let mut vis_min = EntryIdx::MAX;
    let mut vis_max = 0;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, cell_h, n_rows, |ui, row_range| {
            if let Some(target) = scroll_to
                && row_range.contains(&target)
            {
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(
                        ui.cursor().min
                            + egui::vec2(0.0, (target - row_range.start) as f32 * cell_h),
                        egui::vec2(1.0, cell_h),
                    ),
                    Some(Align::Center),
                );
            }
            for r in row_range {
                ui.horizontal(|ui| {
                    for c in 0..cols {
                        let row_idx = r * cols + c;
                        if row_idx >= view_len {
                            break;
                        }
                        let (idx, name, kind, icon_key, abs_path, hidden) = {
                            let s = match snap.read() {
                                Ok(s) => s,
                                Err(_) => return,
                            };
                            let Some((idx, e)) = s.row(row_idx) else {
                                return;
                            };
                            (
                                idx,
                                e.display_name().into_owned(),
                                e.kind,
                                crate::icon_cache::IconKey::for_entry(e, &s.path.join(&e.name)),
                                s.path.join(&e.name),
                                e.hidden,
                            )
                        };
                        vis_min = vis_min.min(idx);
                        vis_max = vis_max.max(idx);

                        let (rect, resp) = ui.allocate_exact_size(
                            Vec2::new(cell_w, cell_h),
                            Sense::click_and_drag(),
                        );
                        let selected = sel.is_some_and(|s| s.contains(idx));
                        if selected || resp.hovered() {
                            let (fill, stroke) = if selected {
                                (visuals.selection.bg_fill, visuals.selection.stroke)
                            } else {
                                (visuals.widgets.hovered.bg_fill, egui::Stroke::NONE)
                            };
                            ui.painter().rect(
                                rect.shrink(1.0),
                                6.0,
                                fill,
                                stroke,
                                egui::StrokeKind::Inside,
                            );
                        }
                        let ghost = if hidden { 0.55 } else { 1.0 };

                        // Icon region: centered in the upper part of the cell.
                        let icon_side = if gallery { cell_w * 0.78 } else { 44.0 };
                        let icon_rect = egui::Rect::from_center_size(
                            egui::pos2(rect.center().x, rect.top() + icon_side * 0.6),
                            Vec2::splat(icon_side),
                        );
                        let thumb =
                            if kind == EntryKind::File && crate::thumbs::is_thumb_ext(&abs_path) {
                                thumbs.texture(&abs_path)
                            } else {
                                None
                            };
                        let tex = thumb
                            .as_ref()
                            .map(|t| t.id())
                            .or_else(|| icons.texture(&icon_key, &abs_path));
                        match tex {
                            Some(tex) => {
                                let r = match &thumb {
                                    Some(h) => fit_rect(icon_rect, h.size_vec2()),
                                    None => icon_rect,
                                };
                                ui.painter().image(
                                    tex,
                                    r,
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    Color32::WHITE.gamma_multiply(ghost),
                                );
                            }
                            None => paint_entry_icon(ui.painter(), icon_rect, kind, pal),
                        }
                        // Name line: centered, clipped to the cell.
                        let name_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.left() + 4.0, rect.bottom() - 18.0),
                            egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
                        );
                        let p = ui.painter().with_clip_rect(name_rect);
                        p.text(
                            egui::pos2(name_rect.center().x, name_rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            &name,
                            egui::FontId::proportional(if gallery { 13.0 } else { 11.5 }),
                            pal.text_primary.gamma_multiply(ghost),
                        );

                        if resp.drag_started() {
                            action.drag_start = Some(row_idx);
                        }
                        if drag_active && kind == EntryKind::Dir && resp.contains_pointer() {
                            action.drop_row = Some(row_idx);
                            crate::glyphs::glow_drop_target(
                                &egui_ctx.debug_painter(),
                                resp.rect,
                                visuals.selection.stroke.color,
                            );
                        }
                        if resp.clicked() {
                            let mods = egui_ctx.input(|i| i.modifiers);
                            action.click = Some(row_idx);
                            action.click_shift = mods.shift;
                            action.click_cmd = mods.command;
                        }
                        if resp.double_clicked() {
                            action.open = Some(idx);
                        }
                        if resp.middle_clicked() && kind == EntryKind::Dir {
                            action.open_new_tab = Some(idx);
                        }
                        if resp.secondary_clicked() {
                            action.context =
                                Some((row_idx, resp.interact_pointer_pos().unwrap_or_default()));
                        }
                    }
                });
            }
        });
    if vis_min <= vis_max {
        action.visible_range = Some((vis_min, vis_max));
    }
    action
}
