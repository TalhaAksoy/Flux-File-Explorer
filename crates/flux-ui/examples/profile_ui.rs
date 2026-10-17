//! CPU-only egui layout/paint-submission probe, not GPU frame latency.
//! Run: cargo run --release -p flux-ui --example profile_ui -- <directory>
use std::time::{Duration, Instant};

use egui::{Pos2, RawInput, Rect, Vec2};
use flux_settings::{Settings, ViewMode};
use flux_ui::{app::FluxApp, settings_page, theme, view};

fn main() {
    let path = std::env::args_os().nth(1).expect("directory argument");
    let ctx = egui::Context::default();
    theme::apply_theme(&ctx, true);
    let mut app = FluxApp::from_settings(Settings::default());
    app.navigate(path.into());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        app.pump(&ctx);
        let ready = app.current_dir().is_some_and(|d| {
            let s = d.snap.read().unwrap();
            !s.state.is_loading() && s.view_len() > 0
        });
        if ready {
            break;
        }
        assert!(Instant::now() < deadline, "directory did not load");
        std::thread::sleep(Duration::from_millis(2));
    }
    // Let the worker's final sorted view arrive before sampling.
    std::thread::sleep(Duration::from_millis(500));
    app.pump(&ctx);
    for (name, mode) in [
        ("details", ViewMode::List),
        ("compact", ViewMode::Compact),
        ("grid", ViewMode::Grid),
        ("gallery", ViewMode::Gallery),
        ("settings", ViewMode::List),
    ] {
        app.settings.view_mode = mode;
        if name == "settings" {
            app.open_settings();
        }
        let mut samples = Vec::with_capacity(300);
        let mut max_shapes = 0;
        for i in 0..360 {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 720.0))),
                time: Some(i as f64 / 60.0),
                events: vec![
                    egui::Event::PointerMoved(Pos2::new(640.0, 360.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: Vec2::new(0.0, -24.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            };
            let start = Instant::now();
            let mut out = ctx.run_ui(input, |ui| {
                if name == "settings" {
                    settings_page::page(&mut app, ui);
                } else {
                    view::file_table(&mut app, ui);
                }
            });
            if i >= 60 {
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
                max_shapes = max_shapes.max(out.shapes.len());
            }
            out.textures_delta.clear();
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{name}: median={:.3}ms p95={:.3}ms p99={:.3}ms max={:.3}ms max_shapes={max_shapes}",
            samples[150], samples[285], samples[297], samples[299]
        );
    }
}
