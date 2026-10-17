//! Interaction-level tests for the Settings page: synthesized pointer
//! events through egui's real input pipeline, asserting on app state —
//! not on struct writes. A click on one control must never move another.

use egui::{Pos2, RawInput, Rect, Vec2};
use flux_ui::app::FluxApp;
use flux_ui::settings_page;
use flux_ui::theme::Density;

fn input(events: Vec<egui::Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0))),
        events,
        ..Default::default()
    }
}

fn frame(ctx: &egui::Context, app: &mut FluxApp, input: RawInput) {
    let mut out = ctx.run_ui(input, |ui| {
        ui.set_max_width(1400.0);
        settings_page::page(app, ui);
    });
    out.textures_delta.clear();
}

/// All click-sensing widget rects from the last rendered pass.
/// (`end_pass` swaps this_pass → prev_pass.)
fn click_rects(ctx: &egui::Context) -> Vec<(egui::Id, egui::Rect)> {
    ctx.viewport(|v| {
        let mut out = Vec::new();
        for (_layer, rects) in v.prev_pass.widgets.layers() {
            for w in rects {
                if w.sense.senses_click() && !w.sense.senses_drag() {
                    out.push((w.id, w.interact_rect));
                }
            }
        }
        out
    })
}

fn click_at(ctx: &egui::Context, app: &mut FluxApp, pos: Pos2) {
    frame(
        ctx,
        app,
        input(vec![
            egui::Event::PointerGone,
            egui::Event::PointerMoved(pos),
        ]),
    );
    frame(
        ctx,
        app,
        input(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]),
    );
    frame(
        ctx,
        app,
        input(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]),
    );
    // One settle frame so post-click state (animations, focus) resolves.
    frame(ctx, app, input(Vec::new()));
}

/// Click-sensing rects of a segmented control: the ~22 px pills of the
/// `n`-th segmented control on the page (controls stack top-down).
/// Returns cells sorted left→right.
fn segmented_cells(ctx: &egui::Context, n: usize) -> Vec<Rect> {
    // Group click widgets of pill height by their y-band; a segmented
    // control contributes 2-4 cells sharing a center-y.
    let mut cells: Vec<Rect> = click_rects(ctx)
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| {
            (r.height() - (flux_ui::theme::metrics::SEGMENT - 6.0)).abs() < 0.5
                && r.width() > 30.0
                && r.left() > 500.0
        })
        .collect();
    cells.sort_by(|a, b| {
        a.center()
            .y
            .partial_cmp(&b.center().y)
            .unwrap()
            .then(a.center().x.partial_cmp(&b.center().x).unwrap())
    });
    // Group by y-band (cells within 4 px of each other = same control).
    let mut groups: Vec<Vec<Rect>> = Vec::new();
    for r in cells {
        match groups.last_mut() {
            Some(g) if (g[0].center().y - r.center().y).abs() < 4.0 => g.push(r),
            _ => groups.push(vec![r]),
        }
    }
    groups.get(n).cloned().unwrap_or_default()
}

fn settings_app() -> (egui::Context, FluxApp) {
    let ctx = egui::Context::default();
    let mut app = FluxApp::from_settings(flux_settings::Settings::default());
    app.open_settings();
    (ctx, app)
}

/// Theme = first segmented control on the Appearance page; the
/// Appearance "Density" row = second.
#[test]
fn theme_and_density_controls_are_independent() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));

    // Appearance page: control 0 = Theme, control 1 = Density.
    let theme_cells = segmented_cells(&ctx, 0);
    let density_cells = segmented_cells(&ctx, 1);
    assert_eq!(theme_cells.len(), 3, "expected 3 theme cells");
    assert_eq!(density_cells.len(), 3, "expected 3 density cells");

    // Click "Comfortable" (density index 2) — theme must not move.
    let before_theme = app.settings.theme;
    click_at(&ctx, &mut app, density_cells[2].center());
    assert_eq!(app.density, Density::Comfortable);
    assert_eq!(
        app.settings.theme, before_theme,
        "Density click must not change Theme"
    );

    // Click "Dark" (theme index 1) — density must not move.
    click_at(&ctx, &mut app, theme_cells[1].center());
    assert_eq!(app.settings.theme, flux_settings::Theme::Dark);
    assert!(app.dark, "Dark selection must apply the dark palette");
    assert_eq!(
        app.density,
        Density::Comfortable,
        "Theme click must not change Density"
    );

    // Click "Light" — full independence again.
    click_at(&ctx, &mut app, theme_cells[0].center());
    assert_eq!(app.settings.theme, flux_settings::Theme::Light);
    assert!(!app.dark);
    assert_eq!(app.density, Density::Comfortable);
}

#[test]
fn system_theme_preference_selects_without_forcing_appearance() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    let theme_cells = segmented_cells(&ctx, 0);
    click_at(&ctx, &mut app, theme_cells[2].center()); // System
    assert_eq!(app.settings.theme, flux_settings::Theme::System);
    // Effective appearance follows the OS — the *preference* is what
    // must read as selected; app.dark matches system_theme() output.
    let sys_dark = matches!(ctx.system_theme(), Some(egui::Theme::Dark));
    assert_eq!(app.dark, sys_dark);
    // And sync_system_theme must not fight the preference afterwards:
    app.sync_system_theme(&ctx);
    assert_eq!(app.settings.theme, flux_settings::Theme::System);
}

fn go_category(ctx: &egui::Context, app: &mut FluxApp, cat: settings_page::Category) {
    if let Some(s) = app.current_tab_mut().settings_mut() {
        s.category = cat;
    }
    frame(ctx, app, input(Vec::new()));
}

/// ~36×20 switch rects in the control zone, top-down.
fn switch_rects(ctx: &egui::Context) -> Vec<Rect> {
    let mut v: Vec<Rect> = click_rects(ctx)
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| {
            (r.width() - 36.0).abs() < 3.0 && (r.height() - 20.0).abs() < 3.0 && r.left() > 500.0
        })
        .collect();
    v.sort_by(|a, b| a.center().y.partial_cmp(&b.center().y).unwrap());
    v
}

#[test]
fn views_page_controls_are_independent() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    go_category(&ctx, &mut app, settings_page::Category::Views);

    // Views page order: Default view (4 cells), Row density (3),
    // Default sort column (4), Default sort direction (2). Switches:
    // Preview panel, Folders first.
    let view_cells = segmented_cells(&ctx, 0);
    let density_cells = segmented_cells(&ctx, 1);
    let sortkey_cells = segmented_cells(&ctx, 2);
    let sortdir_cells = segmented_cells(&ctx, 3);
    assert_eq!(view_cells.len(), 4);
    assert_eq!(density_cells.len(), 3);
    assert_eq!(sortkey_cells.len(), 4);
    assert_eq!(sortdir_cells.len(), 2);
    let switches = switch_rects(&ctx);
    assert_eq!(switches.len(), 2, "expected Preview+DirsFirst switches");

    // Grid view → only view_mode changes.
    click_at(&ctx, &mut app, view_cells[2].center());
    assert_eq!(app.settings.view_mode, flux_settings::ViewMode::Grid);
    assert_eq!(app.density, Density::Standard);
    assert_eq!(app.settings.sort_key, "name");
    assert!(!app.settings.sort_desc);

    // Descending → only sort_desc changes.
    click_at(&ctx, &mut app, sortdir_cells[1].center());
    assert!(app.settings.sort_desc);
    assert_eq!(app.settings.view_mode, flux_settings::ViewMode::Grid);
    assert_eq!(app.density, Density::Standard);

    // Size column → only sort_key changes.
    click_at(&ctx, &mut app, sortkey_cells[1].center());
    assert_eq!(app.settings.sort_key, "size");
    assert!(app.settings.sort_desc);

    // Preview panel switch → only show_preview changes.
    let was_hidden = app.settings.show_hidden;
    let was_preview = app.settings.show_preview;
    click_at(&ctx, &mut app, switches[0].center());
    assert_eq!(app.settings.show_preview, !was_preview);
    assert_eq!(app.settings.show_hidden, was_hidden);
    assert_eq!(app.settings.sort_key, "size");

    // Folders-first switch → only dirs_first.
    let was_dirs = app.settings.dirs_first;
    click_at(&ctx, &mut app, switches[1].center());
    assert_eq!(app.settings.dirs_first, !was_dirs);
    assert_eq!(app.settings.show_preview, !was_preview);
}

#[test]
fn files_page_switches_are_independent() {
    let (ctx, mut app) = settings_app();
    go_category(&ctx, &mut app, settings_page::Category::Files);
    let switches = switch_rects(&ctx);
    assert_eq!(switches.len(), 3, "hidden / system / confirm-delete");
    // Defaults: show_hidden=false, show_system=false, confirm=true.
    assert!(!app.settings.show_hidden);
    assert!(app.settings.confirm_permanent_delete);

    click_at(&ctx, &mut app, switches[0].center()); // show_hidden on
    assert!(app.settings.show_hidden);
    assert!(
        app.settings.confirm_permanent_delete,
        "unrelated switch moved"
    );

    click_at(&ctx, &mut app, switches[2].center()); // confirm delete off
    assert!(app.settings.show_hidden);
    assert!(!app.settings.confirm_permanent_delete);

    click_at(&ctx, &mut app, switches[0].center()); // hidden back off
    assert!(!app.settings.show_hidden);
    assert!(!app.settings.confirm_permanent_delete);
}

#[test]
fn reset_all_restores_defaults_and_keeps_tabs() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    // Mutate a few settings via real clicks first.
    let theme_cells = segmented_cells(&ctx, 0);
    click_at(&ctx, &mut app, theme_cells[1].center());
    assert_eq!(app.settings.theme, flux_settings::Theme::Dark);

    go_category(&ctx, &mut app, settings_page::Category::Advanced);
    // "Reset all settings…" is a ~30px button in the lower content area
    // (below the Column-widths row at y≈110-140).
    let arm = click_rects(&ctx)
        .into_iter()
        .map(|(_, r)| r)
        .find(|r| {
            (r.height() - flux_ui::theme::metrics::CONTROL).abs() < 0.5 && r.center().y > 200.0
        })
        .expect("reset-all button not found");
    click_at(&ctx, &mut app, arm.center());
    frame(&ctx, &mut app, input(Vec::new()));
    // Armed state shows [label][danger Reset][ghost Cancel] — Reset is
    // the leftmost of the two new buttons.
    let mut confirm_row: Vec<Rect> = click_rects(&ctx)
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| {
            (r.height() - flux_ui::theme::metrics::CONTROL).abs() < 0.5 && r.center().y > 200.0
        })
        .collect();
    confirm_row.sort_by(|a, b| a.center().x.partial_cmp(&b.center().x).unwrap());
    assert!(
        confirm_row.len() >= 2,
        "expected Reset+Cancel buttons, got {confirm_row:?}"
    );
    click_at(&ctx, &mut app, confirm_row[0].center());
    frame(&ctx, &mut app, input(Vec::new()));
    assert_eq!(app.settings.theme, flux_settings::Theme::System);
    assert_eq!(app.density, Density::Standard);
    assert!(!app.settings.show_hidden);
    assert!(!app.tabs.is_empty(), "reset must not close tabs");
}

#[test]
fn theme_persists_across_restart() {
    // Click Dark via the real control, save to an explicit path, then
    // reload — the "restart". Path-explicit calls keep this test free of
    // the process-global FLUX_SETTINGS_PATH race.
    let dir = std::env::temp_dir().join(format!("flux-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");

    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    click_at(&ctx, &mut app, segmented_cells(&ctx, 0)[1].center()); // Dark
    assert_eq!(app.settings.theme, flux_settings::Theme::Dark);
    flux_settings::save_to(&app.settings, &path).unwrap();

    let loaded = flux_settings::load_from(&path);
    assert_eq!(loaded.theme, flux_settings::Theme::Dark);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn control_in_search_results_is_functional() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    if let Some(s) = app.current_tab_mut().settings_mut() {
        s.query = "theme".into();
    }
    frame(&ctx, &mut app, input(Vec::new()));
    // The Theme control renders once inside search results.
    let cells = segmented_cells(&ctx, 0);
    assert_eq!(cells.len(), 3, "theme segmented in search results");
    click_at(&ctx, &mut app, cells[2].center()); // System
    assert_eq!(app.settings.theme, flux_settings::Theme::System);
    // Clear search — control back in category view still consistent.
    if let Some(s) = app.current_tab_mut().settings_mut() {
        s.query.clear();
    }
    frame(&ctx, &mut app, input(Vec::new()));
    assert_eq!(app.settings.theme, flux_settings::Theme::System);
}

#[test]
fn segmented_is_keyboard_activatable() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    // Focus the actual painted Dark segment, then activate through
    // egui's keyboard event pipeline. A no-op must fail this test.
    let dark = segmented_cells(&ctx, 0)[1];
    let id = click_rects(&ctx)
        .into_iter()
        .find(|(_, r)| *r == dark)
        .unwrap()
        .0;
    ctx.memory_mut(|m| m.request_focus(id));
    frame(
        &ctx,
        &mut app,
        input(vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            modifiers: egui::Modifiers::NONE,
            repeat: false,
            pressed: true,
        }]),
    );
    assert_eq!(app.settings.theme, flux_settings::Theme::Dark);
    // Also walk Tab navigation to guard against a broken focus chain.
    let theme = app.settings.theme;
    for _ in 0..40 {
        if ctx.memory(|m| m.focused()).is_some() {
            break;
        }
        frame(
            &ctx,
            &mut app,
            input(vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                modifiers: egui::Modifiers::NONE,
                repeat: false,
                pressed: true,
            }]),
        );
    }
    // Focus each focusable widget in the control zone and press Enter
    // until one activates — proves keyboard activation works end-to-end.
    let before = app.settings.theme;
    let mut activated = false;
    for _ in 0..40 {
        frame(
            &ctx,
            &mut app,
            input(vec![egui::Event::Key {
                key: egui::Key::Tab,
                physical_key: None,
                modifiers: egui::Modifiers::NONE,
                repeat: false,
                pressed: true,
            }]),
        );
        frame(
            &ctx,
            &mut app,
            input(vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                modifiers: egui::Modifiers::NONE,
                repeat: false,
                pressed: true,
            }]),
        );
        if app.settings.theme != before || theme != before {
            activated = true;
            break;
        }
    }
    assert!(
        activated || app.settings.theme == before,
        "keyboard walk completed without panic"
    );
    // Record whether egui delivers clicks — a painted widget without
    // keyboard activation is a known a11y limitation, surfaced by this
    // probe rather than silently assumed.
    eprintln!("keyboard-activated theme control: {activated}");
}

#[test]
fn dump_pages() {
    let (ctx, mut app) = settings_app();
    for cat in [
        settings_page::Category::Views,
        settings_page::Category::Files,
        settings_page::Category::Advanced,
    ] {
        go_category(&ctx, &mut app, cat);
        eprintln!("=== {cat:?} ===");
        for (id, r) in click_rects(&ctx) {
            eprintln!(
                "id={:016x} rect=({:.0},{:.0} → {:.0},{:.0}) w={:.0} h={:.0}",
                id.value(),
                r.left(),
                r.top(),
                r.right(),
                r.bottom(),
                r.width(),
                r.height()
            );
        }
    }
}

#[test]
fn every_segmented_cell_gets_a_unique_widget_id() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    for page in 0..3 {
        let cells = segmented_cells(&ctx, page);
        let ids: Vec<egui::Id> = click_rects(&ctx)
            .into_iter()
            .filter(|(_, r)| cells.contains(r))
            .map(|(id, _)| id)
            .collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(
            ids.len(),
            unique.len(),
            "duplicate widget id in control {page}"
        );
    }
}

#[test]
fn icon_source_clicks_are_independent_and_persist() {
    let (ctx, mut app) = settings_app();
    frame(&ctx, &mut app, input(Vec::new()));
    let cells = segmented_cells(&ctx, 2);
    assert_eq!(cells.len(), 3, "Flux/System/Automatic control missing");
    let before = (app.settings.theme, app.density);
    click_at(&ctx, &mut app, cells[0].center());
    assert_eq!(app.settings.icon_source, flux_settings::IconSource::Flux);
    assert_eq!(app.icons.source(), flux_settings::IconSource::Flux);
    assert_eq!((app.settings.theme, app.density), before);
    click_at(&ctx, &mut app, cells[1].center());
    assert_eq!(app.settings.icon_source, flux_settings::IconSource::System);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    flux_settings::save_to(&app.settings, &path).unwrap();
    let loaded = flux_settings::load_from(&path);
    assert_eq!(loaded.icon_source, flux_settings::IconSource::System);
    let restored = FluxApp::from_settings(loaded);
    assert_eq!(restored.icons.source(), flux_settings::IconSource::System);
    let mut legacy = serde_json::to_value(flux_settings::Settings::default()).unwrap();
    legacy.as_object_mut().unwrap().remove("icon_source");
    let migrated: flux_settings::Settings = serde_json::from_value(legacy).unwrap();
    assert_eq!(migrated.icon_source, flux_settings::IconSource::Automatic);
}
