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
    /// Entry index the pointer rests on — drives Space-preview's
    /// hover-first target resolution.
    pub hovered: Option<EntryIdx>,
    /// Min/max entry index of the rows actually rendered this frame —
    /// fed to `MetaEngine::set_priority` so background statting covers
    /// the viewport first. A superset range is fine: the engine still
    /// fills sequentially for anything outside it.
    pub visible_range: Option<(EntryIdx, EntryIdx)>,
}

/// 16-px-ish painted glyphs: folder silhouette, document with folded corner,
/// symlink arrow. Vector painting avoids icon-font coverage issues entirely.
pub fn paint_entry_icon(p: &egui::Painter, rect: egui::Rect, kind: EntryKind, pal: &Palette) {
    let id = match kind {
        EntryKind::Dir => flux_core::file_type::IconId::Folder,
        EntryKind::Symlink => flux_core::file_type::IconId::Symlink,
        EntryKind::Other => flux_core::file_type::IconId::Special,
        EntryKind::File => flux_core::file_type::IconId::Unknown,
    };
    crate::file_icons::paint(p, rect, id, pal);
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
            flux_settings::ViewMode::Grid => icon_grid(app, ui, &pal, 120.0),
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
                    egui::Label::new(egui::RichText::new(label).size(12.5).strong().color(col))
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
    builder.header(32.0, header).body(|body| {
        // Read widths BEFORE rows() consumes the body — reflects the
        // resize state already applied for this frame.
        let w = body.widths();
        if w.len() == 4 {
            live_widths = Some([w[0], w[1], w[2], w[3]]);
        }
        body.rows(row_h, view_len, |mut row| {
            let row_idx = row.index();
            let (idx, name, kind, size, mtime, abs_path, hidden) = {
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
                match thumb.as_ref() {
                    Some(handle) => {
                        let tex = handle.id();
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
                    None => {
                        let mut painter = ui.painter().clone();
                        painter.multiply_opacity(ghost);
                        icons.paint(&painter, icon_rect, kind, &abs_path, &pal);
                    }
                }
                let text_pos = egui::pos2(rect.left() + row_h * 1.05, rect.center().y);
                crate::widgets::elided_text(
                    ui.painter(),
                    text_pos,
                    &name,
                    egui::FontId::proportional(crate::theme::metrics::BODY),
                    pal.text_primary.gamma_multiply(ghost),
                    (rect.right() - text_pos.x - 8.0).max(0.0),
                );
            });
            // Metadata is secondary information — quieter color.
            let meta_col = pal.text_secondary.gamma_multiply(ghost);
            row.col(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format_size(size))
                            .size(12.5)
                            .color(meta_col),
                    );
                });
            });
            row.col(|ui| {
                ui.label(egui::RichText::new(format_mtime(mtime)).color(meta_col));
            });
            row.col(|ui| {
                ui.label(
                    egui::RichText::new(icons.file_type(kind, &abs_path).info().label)
                        .color(meta_col),
                );
            });

            let resp = row.response();
            if resp.hovered() {
                action.hovered = Some(idx);
            }
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
            let (idx, name, kind, abs_path, hidden) = {
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
                let mut painter = ui.painter().clone();
                painter.multiply_opacity(ghost);
                icons.paint(&painter, icon_rect, kind, &abs_path, pal);
                ui.painter().text(
                    egui::pos2(rect.left() + row_h * 1.05, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &name,
                    egui::FontId::default(),
                    pal.text_primary.gamma_multiply(ghost),
                );
            });
            let resp = row.response();
            if resp.hovered() {
                action.hovered = Some(idx);
            }
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

/// Grid column math, kept pure for tests. `avail_w` is the central
/// region's width (already reduced by the preview panel); `bar_w` is
/// what a solid scrollbar reserves. `ui.horizontal` inserts `gap`
/// between cells, so the pitch is `cell_w + gap` — sizing on `cell_w`
/// alone pushes the last column behind the preview panel. Returns the
/// (possibly shrunk, ultra-narrow case) cell width and column count;
/// the row then fits `avail_w` exactly.
fn grid_cell_cols(avail_w: f32, cell_w: f32, gap: f32, bar_w: f32) -> (f32, usize) {
    let usable = (avail_w - bar_w).max(1.0);
    let cell_w = cell_w.min(usable);
    let cols = (((usable + gap) / (cell_w + gap)).floor() as usize).max(1);
    (cell_w, cols)
}

/// Grid/Gallery: fixed-size cells (icon on top, name below) laid out
/// left-to-right, top-to-bottom. Virtualized by `ScrollArea::show_rows`
/// over cell-rows — a 100k-entry dir still lays out only the viewport.
fn icon_grid(app: &mut FluxApp, ui: &mut egui::Ui, pal: &Palette, cell_w: f32) -> RowAction {
    let mut action = RowAction::default();
    let gallery = cell_w > 120.0;
    let Some(snap) = app.current_snap() else {
        return action;
    };
    let view_len = snap.read().map(|s| s.view_len()).unwrap_or(0);
    let (cell_w, cols) = grid_cell_cols(
        ui.available_width(),
        cell_w,
        ui.spacing().item_spacing.x,
        ui.spacing().scroll.allocated_width(),
    );
    let cell_h = if gallery { cell_w + 20.0 } else { 112.0 };
    app.grid_cols = cols;
    let n_rows = view_len.div_ceil(cols);

    let scroll_to = app
        .current_dir_mut()
        .and_then(|d| d.scroll_to_row.take())
        .map(|r| r / cols);
    let thumbs = if gallery {
        app.gallery_thumbs
            .get_or_insert_with(|| {
                let cache = std::rc::Rc::new(crate::thumbs::ThumbCache::new_gallery());
                cache.set_ctx(ui.ctx().clone());
                cache
            })
            .clone()
    } else {
        app.thumbs.clone()
    };
    let sel = app.current_dir().map(|d| &d.sel);
    let egui_ctx = ui.ctx().clone();
    let icons = app.icons.clone();
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
                        let (idx, name, kind, abs_path, hidden) = {
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

                        // Icon region: centered in the upper part of the
                        // cell. Capped so a shrunken ultra-narrow cell
                        // never bleeds into its neighbors.
                        let icon_side = if gallery
                            && kind == EntryKind::File
                            && crate::thumbs::is_thumb_ext(&abs_path)
                        {
                            cell_w * 0.88
                        } else {
                            52.0_f32.min(cell_w * 0.7)
                        };
                        let icon_rect = egui::Rect::from_center_size(
                            egui::pos2(rect.center().x, rect.top() + (cell_h - 30.0) * 0.5),
                            Vec2::splat(icon_side),
                        );
                        let thumb =
                            if kind == EntryKind::File && crate::thumbs::is_thumb_ext(&abs_path) {
                                thumbs.texture(&abs_path)
                            } else {
                                None
                            };
                        match thumb.as_ref() {
                            Some(handle) => {
                                let tex = handle.id();
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
                            None => {
                                let mut painter = ui.painter().clone();
                                painter.multiply_opacity(ghost);
                                icons.paint(&painter, icon_rect, kind, &abs_path, pal);
                            }
                        }
                        // Name line: centered, clipped to the cell.
                        let name_rect = egui::Rect::from_min_max(
                            egui::pos2(rect.left() + 4.0, rect.bottom() - 18.0),
                            egui::pos2(rect.right() - 4.0, rect.bottom() - 2.0),
                        );
                        let p = ui.painter().with_clip_rect(name_rect);
                        let color = pal.text_primary.gamma_multiply(ghost);
                        let mut job = egui::text::LayoutJob::simple(
                            name.clone(),
                            egui::FontId::proportional(13.5),
                            color,
                            name_rect.width().max(0.0),
                        );
                        job.wrap.max_rows = 1;
                        job.wrap.break_anywhere = true;
                        let galley = p.layout_job(job);
                        p.galley(name_rect.center() - galley.size() * 0.5, galley, color);

                        if resp.hovered() {
                            action.hovered = Some(idx);
                        }
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
                        resp.on_hover_text(&name);
                    }
                });
            }
        });
    if vis_min <= vis_max {
        action.visible_range = Some((vis_min, vis_max));
    }
    action
}

#[cfg(test)]
mod tests {
    use super::grid_cell_cols;

    /// Row width for a given column count — must never exceed `usable`.
    fn row_w(cols: usize, cell_w: f32, gap: f32) -> f32 {
        cols as f32 * (cell_w + gap) - gap
    }

    #[test]
    fn grid_cols_never_overflow_available_width() {
        // Sweep widths around the preview-panel toggle (±280 px) and
        // confirm the last column always ends inside the viewport.
        for w in (60..=1600).step_by(7) {
            let w = w as f32;
            for &(cell, gap, bar) in &[(96.0, 8.0, 0.0), (200.0, 8.0, 14.0)] {
                let (cw, cols) = grid_cell_cols(w, cell, gap, bar);
                assert!(
                    row_w(cols, cw, gap) <= (w - bar) + f32::EPSILON,
                    "w={w} cell={cw} cols={cols} overflows (row={})",
                    row_w(cols, cw, gap)
                );
                assert!(cols >= 1);
            }
        }
    }

    #[test]
    fn grid_cols_reflows_when_panel_opens() {
        // 1000 px central region; a 280 px preview panel leaves 720.
        // Old math: floor(720/96)=7 cols → 7*96+6*8=720+... overflow.
        let (cw_wide, cols_wide) = grid_cell_cols(1000.0, 96.0, 8.0, 0.0);
        let (cw_narrow, cols_narrow) = grid_cell_cols(720.0, 96.0, 8.0, 0.0);
        assert!(cols_wide > cols_narrow, "opening panel must reduce cols");
        assert_eq!(cw_wide, 96.0);
        assert_eq!(cw_narrow, 96.0);
        // Every item stays reachable: same item count, more rows.
        let items = 40usize;
        assert!(items.div_ceil(cols_narrow) >= items.div_ceil(cols_wide));
    }

    #[test]
    fn grid_cols_gap_is_counted() {
        // Exactly 4 cells of 96 px fit in 400 px only if the gap were
        // free — with an 8 px gap the honest count is 3.
        let (cw, cols) = grid_cell_cols(400.0, 96.0, 8.0, 0.0);
        assert_eq!(cols, 3, "gap must reduce the column count");
        assert!(row_w(cols, cw, 8.0) <= 400.0);
    }

    #[test]
    fn grid_cols_shrinks_cell_on_narrow_layout() {
        // Viewport narrower than one gallery cell → single shrunken
        // column instead of a clipped cell.
        let (cw, cols) = grid_cell_cols(150.0, 200.0, 8.0, 0.0);
        assert_eq!(cols, 1);
        assert_eq!(cw, 150.0);
    }

    #[test]
    fn grid_cols_reserves_scrollbar_width() {
        let (cw_a, cols_a) = grid_cell_cols(720.0, 96.0, 8.0, 0.0);
        let (cw_b, cols_b) = grid_cell_cols(720.0, 96.0, 8.0, 14.0);
        assert_eq!(cw_a, cw_b);
        assert!(cols_b <= cols_a);
        assert!(row_w(cols_b, cw_b, 8.0) <= 720.0 - 14.0 + f32::EPSILON);
    }
}
