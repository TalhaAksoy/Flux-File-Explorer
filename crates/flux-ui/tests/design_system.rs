//! Contrast and layout invariants for both native themes.
use egui::{Pos2, RawInput, Rect, Vec2};
use flux_ui::{
    app::FluxApp,
    rename_page,
    theme::{self, Palette},
};

fn luminance(c: egui::Color32) -> f32 {
    let linear = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

#[test]
fn ordinary_text_meets_aa_on_its_surfaces() {
    for p in [Palette::dark(), Palette::light()] {
        for fg in [p.text_primary, p.text_secondary, p.text_muted] {
            for bg in [
                p.bg_window,
                p.bg_sidebar,
                p.bg_content,
                p.bg_elevated,
                p.field_inset,
            ] {
                let (a, b) = (luminance(fg), luminance(bg));
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(ratio >= 4.5, "{fg:?} on {bg:?}: {ratio}");
            }
        }
    }
}

fn rename_buttons(width: f32, height: f32, count: usize) -> Vec<Rect> {
    let ctx = egui::Context::default();
    theme::apply_theme(&ctx, true);
    let mut app = FluxApp::from_settings(flux_settings::Settings::default());
    app.open_bulk_rename(
        (0..count)
            .map(|i| std::env::temp_dir().join(format!("flux-layout-{i}.txt")))
            .collect(),
    );
    if let Some(br) = app.current_tab_mut().bulk_rename_mut() {
        br.recompute();
    }
    for _ in 0..3 {
        let mut out = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height))),
                ..Default::default()
            },
            |ui| rename_page::page(&mut app, ui),
        );
        out.textures_delta.clear();
    }
    ctx.viewport(|v| {
        v.prev_pass
            .widgets
            .layers()
            .flat_map(|(_, rects)| rects.iter())
            .filter(|w| {
                w.sense.senses_click()
                    && (w.rect.height() - theme::metrics::CONTROL).abs() < 0.5
                    && w.rect.center().y > height * 0.6
                    && w.rect.left() > width - 320.0
                    && !w.sense.senses_drag()
            })
            .map(|w| w.rect)
            .collect()
    })
}

#[test]
fn appearance_segments_fit_narrow_and_wide_workspaces() {
    for (width, height) in [
        (560.0, 560.0),
        (720.0, 560.0),
        (1024.0, 768.0),
        (1440.0, 900.0),
    ] {
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, true);
        let mut app = FluxApp::from_settings(flux_settings::Settings::default());
        app.open_settings();
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height));
        for _ in 0..3 {
            let mut out = ctx.run_ui(
                RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                },
                |ui| flux_ui::settings_page::page(&mut app, ui),
            );
            out.textures_delta.clear();
        }
        let segments: Vec<_> = ctx.viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, rects)| rects.iter())
                .filter(|w| {
                    w.sense.senses_click()
                        && (w.rect.height() - (theme::metrics::SEGMENT - 6.0)).abs() < 0.5
                })
                .map(|w| (w.id, w.rect))
                .collect()
        });
        assert_eq!(
            segments.len(),
            9,
            "missing theme/density/icon_source segments at {width}"
        );
        for (id, r) in &segments {
            assert!(
                viewport.contains_rect(*r),
                "offscreen segment {id:?}: {r:?} at {width}"
            );
            assert_eq!(segments.iter().filter(|(other, _)| other == id).count(), 1);
        }
    }
}

#[test]
fn rename_footer_stays_at_bottom_with_few_or_many_rows() {
    for (w, h) in [
        (680.0, 560.0),
        (1024.0, 768.0),
        (1440.0, 900.0),
        (2560.0, 1440.0),
    ] {
        for count in [1, 12, 1000] {
            let buttons = rename_buttons(w, h, count);
            assert!(buttons.len() >= 2, "missing footer at {w}×{h}: {buttons:?}");
            for r in buttons {
                assert!(r.bottom() <= h && r.right() <= w, "offscreen footer: {r:?}");
                assert!(r.bottom() > h - 70.0, "footer floated up: {r:?}");
            }
        }
    }
}
