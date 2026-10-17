//! Execute the actual workspace action through egui pointer input, using
//! only disposable files. Rename engine semantics are not mocked.
use egui::{Pos2, RawInput, Rect, Vec2};
use flux_ui::{app::FluxApp, rename_page, theme};
use std::time::{Duration, Instant};

fn frame(ctx: &egui::Context, app: &mut FluxApp, events: Vec<egui::Event>) {
    let mut out = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0))),
            events,
            ..Default::default()
        },
        |ui| rename_page::page(app, ui),
    );
    out.textures_delta.clear();
}

#[test]
fn clicking_primary_rename_executes_the_visible_plan() {
    let dir = tempfile::tempdir().unwrap();
    let sources: Vec<_> = (0..3)
        .map(|i| dir.path().join(format!("photo_{i}.txt")))
        .collect();
    for path in &sources {
        std::fs::write(path, "preserved contents").unwrap();
    }
    let ctx = egui::Context::default();
    theme::apply_theme(&ctx, true);
    let mut app = FluxApp::from_settings(flux_settings::Settings::default());
    app.open_bulk_rename(sources);
    let planning_deadline = Instant::now() + Duration::from_secs(10);
    while app.current_tab().bulk_rename().unwrap().plan_stale {
        app.pump(&ctx);
        assert!(
            Instant::now() < planning_deadline,
            "rename preview never completed"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let destinations = {
        let br = app.current_tab().bulk_rename().unwrap();
        assert_eq!(br.plan.len(), 3);
        assert!(br.plan.iter().all(|a| a.status.is_ready()));
        br.plan.iter().map(|a| a.dst.clone()).collect::<Vec<_>>()
    };
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    let action = ctx
        .viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, widgets)| widgets.iter())
                .filter(|w| {
                    w.sense.senses_click()
                        && !w.sense.senses_drag()
                        && (w.rect.height() - theme::metrics::CONTROL).abs() < 0.5
                        && w.rect.left() > 1000.0
                        && w.rect.top() > 600.0
                })
                .map(|w| w.interact_rect)
                .max_by(|a, b| a.right().total_cmp(&b.right()))
        })
        .expect("primary rename button missing");
    frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(action.center())],
    );
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerButton {
                pos: action.center(),
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
    assert!(
        app.current_tab().bulk_rename().is_none(),
        "workspace did not close after execution"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !destinations.iter().all(|p| p.is_file()) {
        assert!(
            Instant::now() < deadline,
            "rename operation never completed"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    for path in destinations {
        assert_eq!(std::fs::read_to_string(path).unwrap(), "preserved contents");
    }
}
