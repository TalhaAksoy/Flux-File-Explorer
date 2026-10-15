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
    let snap = app.current_snap();

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
        ui.add_space(50.0);
        ui.vertical_centered(|ui| {
            if loading {
                ui.add(egui::Spinner::new());
            }
            ui.label(egui::RichText::new(msg).weak());
            if msg.starts_with("Cannot read") && ui.button("Retry").clicked() {
                action.retry = true;
            }
        });
        return action;
    }

    let focus_row = app.current_tab().sel.focus;
    let scroll_to = app.current_tab_mut().scroll_to_row.take();
    // Borrow the selection once; row closure does O(1) lookups, no clones.
    let sel = &app.tabs[app.active].sel;
    let egui_ctx = ui.ctx().clone();
    let icons = app.icons.clone();
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

    builder.header(22.0, header).body(|body| {
        body.rows(row_h, view_len, |mut row| {
            let row_idx = row.index();
            let (idx, name, kind, size, mtime, ext, icon_key, abs_path) = {
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
                )
            };

            let selected = sel.contains(idx);
            row.set_selected(selected);

            row.col(|ui| {
                let rect = ui.available_rect_before_wrap();
                let icon_rect = row_icon_rect(rect, row_h);
                match icons.texture(&icon_key, &abs_path) {
                    Some(tex) => {
                        ui.painter().image(
                            tex,
                            icon_rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
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
                    pal.text_primary,
                );
            });
            // Metadata is secondary information — quieter color.
            row.col(|ui| {
                ui.label(egui::RichText::new(format_size(size)).color(pal.text_secondary));
            });
            row.col(|ui| {
                ui.label(egui::RichText::new(format_mtime(mtime)).color(pal.text_secondary));
            });
            row.col(|ui| {
                ui.label(
                    egui::RichText::new(match kind {
                        EntryKind::Dir => "Folder",
                        EntryKind::Symlink => "Link",
                        _ => flux_platform::type_label(ext.as_deref()),
                    })
                    .color(pal.text_secondary),
                );
            });

            let resp = row.response();
            if resp.drag_started() {
                action.drag_start = Some(row_idx);
            }
            if drag_active && kind == EntryKind::Dir && resp.hovered() {
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

    let _ = focus_row;
    action
}

/// Apply a RowAction to the app's selection state; `open` handled by caller.
pub fn apply_click(app: &mut FluxApp, action: &RowAction) {
    if let Some(row) = action.click {
        let tab = app.current_tab_mut();
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
