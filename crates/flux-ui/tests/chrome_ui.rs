//! Real pointer input through egui, including an overflowing tab strip.
use egui::{Pos2, RawInput, Rect, Vec2};
use flux_ui::{
    app::FluxApp,
    chrome::{self, ChromeAction},
    pane::Tab,
    theme,
};

fn render(
    ctx: &egui::Context,
    app: &mut FluxApp,
    width: f32,
    events: Vec<egui::Event>,
) -> ChromeAction {
    let mut action = ChromeAction::default();
    let mut out = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 400.0))),
            events,
            ..Default::default()
        },
        |ui| chrome::tab_bar(app, ui, &mut action),
    );
    out.textures_delta.clear();
    action
}

#[test]
fn overflowing_tabs_keep_the_new_tab_button_visible_and_clickable() {
    for width in [560.0, 1024.0, 1440.0] {
        let ctx = egui::Context::default();
        theme::apply_theme(&ctx, true);
        let mut app = FluxApp::from_settings(flux_settings::Settings::default());
        app.tabs = (0..40)
            .map(|i| Tab::directory(i, format!("/a/long/directory-name-{i}").into()))
            .collect();
        app.active = 39;
        for _ in 0..4 {
            render(&ctx, &mut app, width, vec![]);
        }
        let plus = ctx
            .viewport(|v| {
                v.prev_pass
                    .widgets
                    .layers()
                    .flat_map(|(_, widgets)| widgets.iter())
                    .filter(|w| {
                        w.sense.senses_click()
                            && (w.rect.width() - theme::metrics::TAB).abs() < 0.5
                            && (w.rect.height() - theme::metrics::TAB).abs() < 0.5
                    })
                    .map(|w| w.interact_rect)
                    .last()
            })
            .expect("new-tab control missing");
        assert!(
            plus.right() <= width && plus.width() >= 30.0,
            "new tab clipped: {plus:?}"
        );
        render(
            &ctx,
            &mut app,
            width,
            vec![egui::Event::PointerMoved(plus.center())],
        );
        render(
            &ctx,
            &mut app,
            width,
            vec![egui::Event::PointerButton {
                pos: plus.center(),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        let action = render(
            &ctx,
            &mut app,
            width,
            vec![egui::Event::PointerButton {
                pos: plus.center(),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(action.new_tab, "new-tab click lost at {width}");
        assert!(
            action.activate_tab.is_none(),
            "new-tab click activated an overlapping tab"
        );
    }
}
