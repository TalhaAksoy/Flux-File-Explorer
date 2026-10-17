use egui::{Pos2, RawInput, Rect, Vec2};
use flux_core::{
    EntryKind,
    file_type::{CATALOG, IconId},
};
use flux_ui::{
    file_icons,
    icon_cache::{IconCache, IconKey},
    theme::Palette,
};

#[test]
fn every_identity_has_finite_bounded_geometry_in_both_themes_at_multiple_sizes() {
    let ctx = egui::Context::default();
    for palette in [Palette::dark(), Palette::light()] {
        for edge in [16.0, 24.0, 48.0, 128.0] {
            let rect = Rect::from_min_size(Pos2::new(30.0, 30.0), Vec2::splat(edge));
            for ty in CATALOG {
                let mut out = ctx.run_ui(
                    RawInput {
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(220.0))),
                        ..Default::default()
                    },
                    |ui| file_icons::paint(ui.painter(), rect, ty.icon, &palette),
                );
                assert!(!out.shapes.is_empty(), "missing artwork: {:?}", ty.icon);
                for shape in &out.shapes {
                    let bounds = shape.shape.visual_bounding_rect();
                    if bounds.is_positive() {
                        assert!(bounds.is_finite(), "nonfinite {:?}", ty.icon);
                        assert!(
                            rect.expand(edge * 0.12 + 1.0).contains_rect(bounds),
                            "{:?} overflow at {edge}: {bounds:?}",
                            ty.icon
                        );
                    }
                }
                out.textures_delta.clear();
            }
        }
    }
}

#[test]
fn ten_thousand_typescript_files_share_identity_without_per_path_textures() {
    let keys: std::collections::HashSet<_> = (0..10_000)
        .map(|i| {
            IconKey::for_entry_path(
                EntryKind::File,
                std::path::Path::new(&format!("module_{i}.ts")),
            )
        })
        .collect();
    assert_eq!(keys.len(), 1);
    assert!(keys.contains(&IconKey::Type(IconId::TypeScript)));
    let ctx = egui::Context::default();
    let cache = IconCache::new();
    cache.set_source(flux_settings::IconSource::Flux);
    let mut out = ctx.run_ui(RawInput::default(), |ui| {
        cache.paint(
            ui.painter(),
            Rect::from_min_size(Pos2::ZERO, Vec2::splat(32.0)),
            EntryKind::File,
            std::path::Path::new("test.rs"),
            &Palette::dark(),
        );
    });
    out.textures_delta.clear();
    assert_eq!(cache.stats().texture_bytes, 0);
    assert_eq!(cache.stats().requests, 0);
    assert_eq!(cache.stats().bundled_draws, 1);
}
