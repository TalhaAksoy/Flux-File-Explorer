//! Original Flux file artwork. Normalized 24-unit vector geometry, no
//! external logos, icon font, SVG parser, embedded bitmap or network access.
//! Small and large icons use the same optical box; theme colors are chosen
//! here, independently of document contents and native thumbnail textures.
use crate::theme::Palette;
use egui::{Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use flux_core::file_type::{Category, IconId as I};

struct Canvas<'a> {
    p: &'a Painter,
    origin: Pos2,
    scale: f32,
    ink: Color32,
    neutral: Color32,
}
impl Canvas<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x, y) * self.scale
    }
    fn stroke(&self, width: f32) -> Stroke {
        Stroke::new((width * self.scale).max(0.65), self.ink)
    }
    fn line(&self, a: (f32, f32), b: (f32, f32)) {
        self.p
            .line_segment([self.pt(a.0, a.1), self.pt(b.0, b.1)], self.stroke(1.65));
    }
    fn poly(&self, points: &[(f32, f32)], fill: Color32) {
        debug_assert!(
            convex(points),
            "icon polygon must be decomposed into convex pieces: {points:?}"
        );
        self.p.add(egui::Shape::convex_polygon(
            points.iter().map(|&(x, y)| self.pt(x, y)).collect(),
            fill,
            Stroke::NONE,
        ));
    }
    fn outline(&self, points: &[(f32, f32)]) {
        self.p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| self.pt(x, y)).collect(),
            self.stroke(1.65),
        ));
    }
    fn circle(&self, x: f32, y: f32, r: f32, filled: bool) {
        if filled {
            self.p
                .circle_filled(self.pt(x, y), r * self.scale, self.ink);
        } else {
            self.p
                .circle_stroke(self.pt(x, y), r * self.scale, self.stroke(1.65));
        }
    }
    fn rect(&self, x: f32, y: f32, w: f32, h: f32, filled: bool) {
        let r = Rect::from_min_size(self.pt(x, y), Vec2::new(w, h) * self.scale);
        if filled {
            self.p.rect_filled(r, 1.5 * self.scale, self.ink);
        } else {
            self.p
                .rect_stroke(r, 1.5 * self.scale, self.stroke(1.5), StrokeKind::Inside);
        }
    }
    fn text(&self, text: &str, x: f32, y: f32, size: f32, color: Color32) {
        self.p.text(
            self.pt(x, y),
            egui::Align2::CENTER_CENTER,
            text,
            FontId::proportional(size * self.scale),
            color,
        );
    }
    fn arc(&self, x: f32, y: f32, r: f32, start: f32, end: f32) {
        let points: Vec<_> = (0..=16)
            .map(|i| {
                let a = start + (end - start) * i as f32 / 16.0;
                self.pt(x + r * a.cos(), y + r * a.sin())
            })
            .collect();
        self.p.add(egui::Shape::line(points, self.stroke(1.8)));
    }
    fn orbit(&self, angle: f32) {
        let points: Vec<_> = (0..=24)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 24.0;
                let (x, y) = (9.0 * a.cos(), 3.6 * a.sin());
                self.pt(
                    12.0 + x * angle.cos() - y * angle.sin(),
                    11.0 + x * angle.sin() + y * angle.cos(),
                )
            })
            .collect();
        self.p.add(egui::Shape::line(points, self.stroke(1.25)));
    }
    fn ellipse(&self, x: f32, y: f32, rx: f32, ry: f32) {
        let points = (0..=24)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 24.0;
                self.pt(x + rx * a.cos(), y + ry * a.sin())
            })
            .collect();
        self.p.add(egui::Shape::line(points, self.stroke(1.5)));
    }
    fn gear(&self) {
        self.circle(12.0, 12.0, 6.6, false);
        self.circle(12.0, 12.0, 2.4, false);
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::TAU / 8.0;
            self.line(
                (12.0 + 6.5 * a.cos(), 12.0 + 6.5 * a.sin()),
                (12.0 + 9.0 * a.cos(), 12.0 + 9.0 * a.sin()),
            );
        }
    }
    fn page(&self) {
        self.poly(
            &[
                (5.0, 2.5),
                (14.5, 2.5),
                (20.0, 8.0),
                (20.0, 21.5),
                (5.0, 21.5),
            ],
            self.ink.gamma_multiply(0.15),
        );
        self.outline(&[
            (14.0, 3.0),
            (5.0, 3.0),
            (5.0, 21.0),
            (20.0, 21.0),
            (20.0, 8.0),
            (14.0, 3.0),
            (14.0, 8.0),
            (20.0, 8.0),
        ]);
    }
    fn document(&self, mode: u8) {
        self.rect(3.0, 3.0, 18.0, 18.0, false);
        self.rect(3.0, 3.0, 4.0, 18.0, true);
        match mode {
            1 => {
                for y in [9.0, 13.0, 17.0] {
                    self.line((10.0, y), (18.0, y));
                }
                self.line((14.0, 6.0), (14.0, 19.0));
            }
            2 => {
                self.circle(14.0, 11.0, 4.0, false);
                self.line((14.0, 7.0), (14.0, 11.0));
                self.line((14.0, 11.0), (18.0, 11.0));
                self.line((10.0, 18.0), (18.0, 18.0));
            }
            _ => {
                for (y, x) in [(8.0, 18.0), (12.0, 18.0), (16.0, 16.0)] {
                    self.line((10.0, y), (x, y));
                }
            }
        }
    }
}

fn convex(points: &[(f32, f32)]) -> bool {
    let mut sign = 0.0_f32;
    for i in 0..points.len() {
        let (a, b, c) = (
            points[i],
            points[(i + 1) % points.len()],
            points[(i + 2) % points.len()],
        );
        let cross = (b.0 - a.0) * (c.1 - b.1) - (b.1 - a.1) * (c.0 - b.0);
        if cross.abs() > 0.0001 {
            if sign != 0.0 && sign * cross < 0.0 {
                return false;
            }
            sign = cross.signum();
        }
    }
    sign != 0.0
}

fn color(id: I, dark: bool) -> Color32 {
    let pair = match id {
        I::Python | I::PythonConfig => ((100, 166, 234), (47, 106, 166)),
        I::JavaScript | I::Bun | I::Nim => ((235, 195, 70), (151, 107, 0)),
        I::Odt | I::Ods | I::Odp => ((97, 184, 180), (28, 121, 119)),
        I::Pages | I::Notebook => ((239, 165, 93), (175, 94, 25)),
        I::Rust | I::Cargo | I::RustConfig | I::Swift | I::Rss | I::Powerpoint => {
            ((239, 148, 91), (183, 79, 28))
        }
        I::ReactJs | I::ReactTs | I::Go | I::Dart | I::Tailwind => ((90, 197, 218), (0, 120, 147)),
        I::Ruby | I::Java | I::Git | I::Pdf | I::Rpm | I::Scala | I::Angular => {
            ((238, 115, 121), (186, 48, 59))
        }
        I::Vue | I::Excel | I::Numbers | I::Node | I::Npm | I::Shell | I::Ansible => {
            ((89, 190, 140), (27, 125, 78))
        }
        I::Elixir | I::Csharp | I::Haskell | I::Fsharp | I::Graphql | I::Terraform | I::Kotlin => {
            ((190, 150, 236), (126, 74, 172))
        }
        I::Svelte | I::Html | I::Astro | I::Zig => ((245, 152, 108), (184, 80, 34)),
        I::Css | I::Scss | I::Sass | I::Less | I::Php | I::Prettier | I::Figma => {
            ((220, 146, 204), (158, 72, 141))
        }
        _ => match id.info().category {
            Category::Audio | Category::Feed => ((210, 164, 236), (142, 87, 173)),
            Category::Security => ((222, 186, 100), (143, 105, 22)),
            Category::Archive => ((207, 172, 122), (138, 95, 39)),
            Category::Image => ((113, 193, 161), (34, 128, 101)),
            Category::Video => ((167, 160, 235), (111, 91, 177)),
            Category::Config => ((161, 179, 197), (85, 106, 128)),
            _ => ((111, 174, 242), (38, 110, 182)),
        },
    };
    let (r, g, b) = if dark { pair.0 } else { pair.1 };
    Color32::from_rgb(r, g, b)
}

/// Paint original artwork directly at the requested size. Bundled icons
/// share identities, colors and egui's glyph atlas, not per-path textures.
pub fn paint(p: &Painter, rect: Rect, id: I, pal: &Palette) {
    let edge = rect.width().min(rect.height()).max(0.0);
    if edge < 1.0 || !p.is_visible() {
        return;
    }
    let dark = pal.bg_content.r() < 128;
    let ink = color(id, dark);
    let c = Canvas {
        p,
        origin: rect.center() - Vec2::splat(edge * 0.5),
        scale: edge / 24.0,
        ink,
        neutral: pal.text_primary,
    };
    let white = if dark {
        Color32::from_rgb(28, 32, 38)
    } else {
        Color32::WHITE
    };
    match id {
        I::Folder => {
            let back = pal.dir_icon.gamma_multiply(0.65);
            c.p.rect_filled(
                Rect::from_min_max(c.pt(2.0, 8.0), c.pt(22.0, 21.0)),
                1.5 * c.scale,
                back,
            );
            c.poly(&[(2.0, 5.0), (10.0, 5.0), (13.0, 9.0), (2.0, 9.0)], back);
            c.p.rect_filled(
                Rect::from_min_max(c.pt(2.0, 9.0), c.pt(22.0, 21.0)),
                2.0 * c.scale,
                pal.dir_icon,
            );
            c.p.line_segment(
                [c.pt(3.0, 10.0), c.pt(21.0, 10.0)],
                (0.8 * c.scale, Color32::WHITE.gamma_multiply(0.35)),
            );
        }
        I::FolderLink => {
            paint(p, rect, I::Folder, pal);
            c.p.circle_filled(c.pt(5.0, 19.0), 4.5 * c.scale, pal.bg_content);
            c.outline(&[(1.0, 22.0), (1.0, 17.0), (9.0, 17.0)]);
            c.outline(&[(6.0, 14.0), (9.0, 17.0), (6.0, 20.0)]);
        }
        I::Socket => {
            c.circle(12.0, 7.0, 4.0, false);
            c.line((12.0, 11.0), (12.0, 20.0));
            c.line((4.0, 20.0), (20.0, 20.0));
            for x in [4.0, 12.0, 20.0] {
                c.circle(x, 21.0, 1.7, true);
            }
        }
        I::Pipe => {
            c.outline(&[(3.0, 2.0), (3.0, 14.0), (15.0, 14.0), (15.0, 22.0)]);
            c.outline(&[(9.0, 2.0), (9.0, 8.0), (21.0, 8.0), (21.0, 22.0)]);
            c.line((1.0, 2.0), (11.0, 2.0));
            c.line((13.0, 22.0), (23.0, 22.0));
        }
        I::Device => {
            c.rect(6.0, 6.0, 12.0, 12.0, false);
            c.rect(9.0, 9.0, 6.0, 6.0, true);
            for i in [8.0, 12.0, 16.0] {
                c.line((2.0, i), (6.0, i));
                c.line((18.0, i), (22.0, i));
                c.line((i, 2.0), (i, 6.0));
                c.line((i, 18.0), (i, 22.0));
            }
        }
        I::Symlink => {
            c.page();
            c.outline(&[(2.0, 19.0), (2.0, 15.0), (10.0, 15.0)]);
            c.outline(&[(7.0, 12.0), (10.0, 15.0), (7.0, 18.0)]);
        }
        I::Unknown => {
            c.page();
            c.line((9.0, 13.0), (16.0, 13.0));
            c.line((9.0, 17.0), (14.0, 17.0));
        }
        I::Special => {
            c.gear();
        }
        I::JavaScript | I::TypeScript => {
            c.rect(3.0, 3.0, 18.0, 18.0, true);
            c.text(
                if id == I::JavaScript { "JS" } else { "TS" },
                13.0,
                15.0,
                10.0,
                white,
            );
        }
        I::ReactJs | I::ReactTs => {
            for angle in [0.0, std::f32::consts::PI / 3.0, -std::f32::consts::PI / 3.0] {
                c.orbit(angle);
            }
            c.circle(12.0, 11.0, 1.9, true);
            let badge = color(
                if id == I::ReactTs {
                    I::TypeScript
                } else {
                    I::JavaScript
                },
                dark,
            );
            c.p.rect_filled(
                Rect::from_min_max(c.pt(14.0, 16.0), c.pt(23.0, 23.0)),
                1.0 * c.scale,
                badge,
            );
            c.text(
                if id == I::ReactTs { "TS" } else { "JS" },
                18.5,
                19.5,
                5.5,
                white,
            );
        }
        I::Python | I::PythonConfig => {
            c.rect(5.0, 3.0, 11.0, 9.0, true);
            c.rect(3.0, 7.0, 6.0, 9.0, true);
            let other = Canvas {
                ink: if dark {
                    Color32::from_rgb(235, 198, 91)
                } else {
                    Color32::from_rgb(165, 117, 14)
                },
                ..c
            };
            other.rect(8.0, 12.0, 11.0, 9.0, true);
            other.rect(15.0, 8.0, 6.0, 9.0, true);
            c.p.circle_filled(c.pt(9.0, 6.0), 0.9 * c.scale, white);
            c.p.circle_filled(c.pt(15.0, 18.0), 0.9 * c.scale, white);
        }
        I::Rust | I::RustConfig => {
            c.gear();
            c.p.circle_filled(c.pt(12.0, 12.0), 5.0 * c.scale, pal.bg_content);
            c.text("R", 12.0, 12.5, 11.0, ink);
        }
        I::C | I::Cpp | I::Csharp | I::ObjectiveC => {
            c.poly(
                &[
                    (12.0, 2.0),
                    (21.0, 7.0),
                    (21.0, 17.0),
                    (12.0, 22.0),
                    (3.0, 17.0),
                    (3.0, 7.0),
                ],
                ink.gamma_multiply(0.18),
            );
            c.text("C", 11.0, 12.0, 17.0, ink);
            if id != I::C {
                c.text(
                    match id {
                        I::Cpp => "++",
                        I::Csharp => "#",
                        _ => "m",
                    },
                    19.0,
                    18.0,
                    7.0,
                    c.neutral,
                );
            }
        }
        I::Java => {
            c.outline(&[(5.0, 11.0), (6.0, 19.0), (16.0, 19.0), (17.0, 11.0)]);
            c.arc(17.0, 14.0, 3.0, -1.5, 1.5);
            c.line((4.0, 22.0), (19.0, 22.0));
            c.outline(&[(10.0, 2.0), (8.0, 5.0), (12.0, 7.0), (10.0, 9.0)]);
        }
        I::Kotlin => {
            c.rect(3.0, 3.0, 4.0, 18.0, true);
            c.poly(&[(7.0, 11.0), (16.0, 3.0), (22.0, 3.0), (11.0, 14.0)], ink);
            c.poly(&[(7.0, 12.0), (12.0, 9.0), (22.0, 21.0), (15.0, 21.0)], ink);
        }
        I::Swift => {
            c.poly(&[(2.0, 16.0), (18.0, 12.0), (22.0, 21.0)], ink);
            c.poly(&[(3.0, 5.0), (18.0, 12.0), (13.0, 17.0)], ink);
            c.poly(&[(10.0, 3.0), (20.0, 14.0), (15.0, 17.0)], ink);
            c.poly(&[(2.0, 16.0), (10.0, 16.0), (8.0, 20.0)], ink);
        }
        I::Go | I::GoMod => {
            c.line((2.0, 7.0), (8.0, 7.0));
            c.line((1.0, 12.0), (6.0, 12.0));
            c.line((3.0, 17.0), (8.0, 17.0));
            c.text("GO", 15.0, 12.0, 11.0, ink);
        }
        I::Dart | I::Pub => {
            c.poly(
                &[
                    (3.0, 10.0),
                    (11.0, 2.0),
                    (21.0, 12.0),
                    (21.0, 21.0),
                    (12.0, 21.0),
                ],
                ink,
            );
            c.line((5.0, 11.0), (18.0, 18.0));
        }
        I::Ruby => {
            c.poly(
                &[
                    (6.0, 4.0),
                    (18.0, 4.0),
                    (22.0, 10.0),
                    (12.0, 22.0),
                    (2.0, 10.0),
                ],
                ink,
            );
            c.p.line_segment([c.pt(2.0, 10.0), c.pt(22.0, 10.0)], (0.8 * c.scale, white));
            c.p.line_segment([c.pt(8.0, 10.0), c.pt(12.0, 21.0)], (0.8 * c.scale, white));
        }
        I::Lua => {
            c.circle(11.0, 13.0, 8.0, true);
            c.p.circle_filled(c.pt(14.0, 10.0), 2.6 * c.scale, white);
            c.circle(20.0, 3.0, 2.0, true);
        }
        I::Scala => {
            for y in [4.0, 10.0, 16.0] {
                c.poly(
                    &[(4.0, y + 2.0), (20.0, y), (20.0, y + 4.0), (4.0, y + 6.0)],
                    ink,
                );
            }
        }
        I::Elixir => {
            c.poly(
                &[
                    (12.0, 2.0),
                    (18.0, 10.0),
                    (20.0, 17.0),
                    (16.0, 22.0),
                    (8.0, 22.0),
                    (4.0, 17.0),
                    (7.0, 9.0),
                ],
                ink,
            );
            c.p.line_segment([c.pt(9.0, 18.0), c.pt(13.0, 7.0)], (1.0 * c.scale, white));
        }
        I::Haskell | I::Ocaml | I::Fsharp => {
            c.outline(&[(3.0, 5.0), (9.0, 12.0), (3.0, 19.0)]);
            c.outline(&[(8.0, 3.0), (18.0, 21.0)]);
            c.line((13.0, 12.0), (8.0, 21.0));
            c.line((17.0, 10.0), (22.0, 10.0));
            c.line((18.0, 14.0), (23.0, 14.0));
            if id != I::Haskell {
                c.text(
                    if id == I::Ocaml { "ML" } else { "F" },
                    18.0,
                    4.0,
                    5.5,
                    c.neutral,
                );
            }
        }
        I::Julia => {
            c.circle(7.0, 7.0, 3.0, true);
            c.circle(17.0, 7.0, 3.0, true);
            c.circle(12.0, 17.0, 3.5, true);
        }
        I::Zig => {
            c.rect(3.0, 3.0, 18.0, 4.0, true);
            c.poly(&[(15.0, 6.0), (21.0, 6.0), (8.0, 20.0), (2.0, 20.0)], ink);
            c.rect(2.0, 17.0, 19.0, 4.0, true);
        }
        I::Nim => {
            c.rect(6.0, 14.0, 12.0, 7.0, true);
            c.poly(&[(3.0, 7.0), (10.0, 15.0), (6.0, 18.0)], ink);
            c.poly(&[(12.0, 3.0), (17.0, 15.0), (7.0, 15.0)], ink);
            c.poly(&[(21.0, 7.0), (18.0, 18.0), (14.0, 15.0)], ink);
        }
        I::Crystal | I::Solidity => {
            c.poly(
                &[
                    (12.0, 2.0),
                    (21.0, 9.0),
                    (17.0, 20.0),
                    (7.0, 22.0),
                    (3.0, 10.0),
                ],
                ink,
            );
            c.p.line_segment([c.pt(12.0, 2.0), c.pt(7.0, 22.0)], (1.0 * c.scale, white));
            c.p.line_segment([c.pt(3.0, 10.0), c.pt(21.0, 9.0)], (1.0 * c.scale, white));
            if id == I::Solidity {
                c.text("S", 16.0, 16.0, 7.0, white);
            }
        }
        I::Vue | I::V => {
            c.poly(&[(2.0, 4.0), (7.0, 4.0), (14.0, 18.0), (12.0, 22.0)], ink);
            c.poly(&[(17.0, 4.0), (22.0, 4.0), (12.0, 22.0), (9.0, 17.0)], ink);
            if id == I::Vue {
                c.poly(
                    &[(7.0, 4.0), (10.0, 4.0), (13.0, 10.0), (12.0, 13.0)],
                    pal.text_secondary,
                );
                c.poly(
                    &[(14.0, 4.0), (17.0, 4.0), (12.0, 13.0), (11.0, 10.0)],
                    pal.text_secondary,
                );
            }
        }
        I::Svelte => {
            c.outline(&[
                (19.0, 5.0),
                (14.0, 3.0),
                (5.0, 9.0),
                (5.0, 14.0),
                (15.0, 17.0),
                (19.0, 13.0),
                (9.0, 10.0),
                (5.0, 17.0),
                (10.0, 21.0),
                (19.0, 16.0),
            ]);
        }
        I::Astro => {
            c.poly(&[(4.0, 19.0), (10.0, 3.0), (13.0, 3.0), (9.0, 17.0)], ink);
            c.poly(&[(12.0, 3.0), (15.0, 3.0), (21.0, 19.0), (16.0, 17.0)], ink);
            c.circle(12.0, 21.0, 1.6, true);
        }
        I::Html | I::Xml | I::Json | I::Json5 | I::Jsonc | I::JsonLines | I::Assembly => {
            if matches!(id, I::Json | I::Json5 | I::Jsonc | I::JsonLines) {
                c.outline(&[
                    (8.0, 3.0),
                    (5.0, 3.0),
                    (5.0, 9.0),
                    (2.0, 12.0),
                    (5.0, 15.0),
                    (5.0, 21.0),
                    (8.0, 21.0),
                ]);
                c.outline(&[
                    (16.0, 3.0),
                    (19.0, 3.0),
                    (19.0, 9.0),
                    (22.0, 12.0),
                    (19.0, 15.0),
                    (19.0, 21.0),
                    (16.0, 21.0),
                ]);
                c.circle(12.0, 9.0, 1.0, true);
                c.circle(12.0, 15.0, 1.0, true);
            } else {
                c.outline(&[(8.0, 6.0), (2.0, 12.0), (8.0, 18.0)]);
                c.outline(&[(16.0, 6.0), (22.0, 12.0), (16.0, 18.0)]);
                c.line((14.0, 4.0), (10.0, 20.0));
            }
        }
        I::Css | I::Scss | I::Sass | I::Less => {
            c.poly(
                &[
                    (3.0, 3.0),
                    (21.0, 3.0),
                    (19.0, 19.0),
                    (12.0, 22.0),
                    (5.0, 19.0),
                ],
                ink,
            );
            c.text(
                match id {
                    I::Css => "#",
                    I::Less => "L",
                    _ => "S",
                },
                12.0,
                12.0,
                14.0,
                white,
            );
        }
        I::Graphql | I::Rdf => {
            let pts = [
                (12.0, 3.0),
                (20.0, 8.0),
                (20.0, 17.0),
                (12.0, 22.0),
                (4.0, 17.0),
                (4.0, 8.0),
            ];
            for i in 0..6 {
                c.line(pts[i], pts[(i + 1) % 6]);
                c.circle(pts[i].0, pts[i].1, 1.7, true);
            }
            c.line(pts[0], pts[2]);
            c.line(pts[0], pts[4]);
            c.line(pts[2], pts[4]);
        }
        I::Rss | I::Atom | I::JsonFeed => {
            c.circle(5.0, 19.0, 2.0, true);
            c.arc(5.0, 19.0, 7.0, -std::f32::consts::FRAC_PI_2, 0.0);
            c.arc(5.0, 19.0, 14.0, -std::f32::consts::FRAC_PI_2, 0.0);
            if id == I::Atom {
                c.circle(19.0, 5.0, 2.5, false);
            }
            if id == I::JsonFeed {
                c.text("{}", 19.0, 5.0, 6.0, c.neutral);
            }
        }
        I::Opml => {
            c.line((5.0, 4.0), (5.0, 20.0));
            for y in [5.0, 12.0, 19.0] {
                c.line((5.0, y), (13.0, y));
                c.rect(14.0, y - 2.0, 6.0, 4.0, false);
            }
        }
        I::Word | I::Excel | I::Powerpoint => {
            c.document(match id {
                I::Excel => 1,
                I::Powerpoint => 2,
                _ => 0,
            });
            c.rect(1.0, 9.0, 10.0, 12.0, true);
            c.text(
                match id {
                    I::Word => "W",
                    I::Excel => "X",
                    _ => "P",
                },
                6.0,
                15.0,
                8.0,
                white,
            );
        }
        I::Odt | I::Ods | I::Odp => {
            c.document(match id {
                I::Ods => 1,
                I::Odp => 2,
                _ => 0,
            });
            c.circle(5.0, 5.0, 3.0, false);
        }
        I::Pages => {
            c.document(0);
            c.line((9.0, 20.0), (20.0, 7.0));
            c.line((12.0, 20.0), (21.0, 9.0));
        }
        I::Numbers => {
            c.rect(2.0, 3.0, 20.0, 19.0, false);
            for (x, y, h) in [(5.0, 13.0, 6.0), (10.0, 9.0, 10.0), (15.0, 6.0, 13.0)] {
                c.rect(x, y, 3.0, h, true);
            }
        }
        I::Keynote => {
            c.rect(3.0, 3.0, 18.0, 13.0, false);
            c.line((12.0, 16.0), (12.0, 22.0));
            c.line((7.0, 22.0), (17.0, 22.0));
            c.poly(&[(10.0, 6.0), (16.0, 10.0), (10.0, 14.0)], ink);
        }
        I::Csv | I::Tsv => c.document(1),
        I::Rtf => {
            c.page();
            c.text("RTF", 12.0, 15.0, 6.0, ink);
        }
        I::Pdf => {
            c.page();
            c.outline(&[
                (8.0, 18.0),
                (13.0, 8.0),
                (14.0, 14.0),
                (18.0, 17.0),
                (9.0, 16.0),
            ]);
        }
        I::Markdown | I::Mdx | I::Readme => {
            c.rect(2.0, 5.0, 20.0, 15.0, false);
            c.outline(&[
                (5.0, 17.0),
                (5.0, 9.0),
                (8.0, 13.0),
                (11.0, 9.0),
                (11.0, 17.0),
            ]);
            c.line((17.0, 8.0), (17.0, 16.0));
            c.outline(&[(14.0, 13.0), (17.0, 16.0), (20.0, 13.0)]);
            if id == I::Readme {
                c.circle(4.0, 3.0, 1.5, true);
            }
            if id == I::Mdx {
                c.text("X", 20.0, 3.0, 6.0, c.neutral);
            }
        }
        I::License => {
            c.line((12.0, 3.0), (12.0, 21.0));
            c.line((4.0, 7.0), (20.0, 7.0));
            c.outline(&[(2.0, 14.0), (6.0, 7.0), (10.0, 14.0), (2.0, 14.0)]);
            c.outline(&[(14.0, 14.0), (18.0, 7.0), (22.0, 14.0), (14.0, 14.0)]);
            c.line((7.0, 21.0), (17.0, 21.0));
        }
        I::Git => {
            c.poly(&[(12.0, 1.0), (23.0, 12.0), (12.0, 23.0), (1.0, 12.0)], ink);
            let b = Canvas { ink: white, ..c };
            b.line((8.0, 6.0), (8.0, 17.0));
            b.line((8.0, 8.0), (16.0, 15.0));
            b.circle(8.0, 7.0, 1.6, true);
            b.circle(8.0, 17.0, 1.6, true);
            b.circle(16.0, 15.0, 1.6, true);
        }
        I::Docker => {
            for (x, y) in [(3.0, 9.0), (9.0, 9.0), (15.0, 9.0), (9.0, 3.0), (15.0, 3.0)] {
                c.rect(x, y, 5.0, 5.0, false);
            }
            c.poly(&[(2.0, 16.0), (22.0, 16.0), (19.0, 22.0), (5.0, 22.0)], ink);
        }
        I::Cargo
        | I::Node
        | I::Npm
        | I::Yarn
        | I::Pnpm
        | I::Bun
        | I::Deb
        | I::Rpm
        | I::Pkg
        | I::Msi
        | I::Msix
        | I::Flatpak => {
            c.outline(&[
                (3.0, 7.0),
                (12.0, 2.0),
                (21.0, 7.0),
                (21.0, 18.0),
                (12.0, 23.0),
                (3.0, 18.0),
                (3.0, 7.0),
                (12.0, 12.0),
                (21.0, 7.0),
            ]);
            c.line((12.0, 12.0), (12.0, 23.0));
            c.text(
                match id {
                    I::Cargo => "R",
                    I::Node | I::Npm => "N",
                    I::Yarn => "Y",
                    I::Pnpm => "p",
                    I::Bun => "b",
                    I::Deb => "d",
                    I::Rpm => "r",
                    I::Flatpak => "F",
                    _ => "+",
                },
                12.0,
                7.0,
                7.0,
                ink,
            );
        }
        I::Make | I::Cmake | I::Meson | I::Just | I::Task | I::Bazel | I::Buck => {
            c.poly(&[(4.0, 2.0), (17.0, 2.0), (20.0, 5.0), (7.0, 5.0)], ink);
            c.poly(&[(17.0, 2.0), (22.0, 7.0), (19.0, 11.0), (14.0, 6.0)], ink);
            c.poly(&[(7.0, 5.0), (14.0, 5.0), (10.0, 10.0)], ink);
            c.line((11.0, 8.0), (4.0, 21.0));
            c.line((13.0, 9.0), (6.0, 22.0));
            c.text(
                match id {
                    I::Cmake => "C",
                    I::Meson => "M",
                    I::Just => "J",
                    I::Bazel => "B",
                    I::Buck => "K",
                    _ => "",
                },
                19.0,
                19.0,
                7.0,
                c.neutral,
            );
        }
        I::Font => {
            c.line((3.0, 21.0), (11.0, 3.0));
            c.line((11.0, 3.0), (19.0, 21.0));
            c.line((6.0, 15.0), (16.0, 15.0));
            c.line((1.0, 21.0), (7.0, 21.0));
            c.line((16.0, 21.0), (22.0, 21.0));
        }
        I::Certificate | I::SecurityPolicy => {
            c.poly(
                &[
                    (12.0, 2.0),
                    (21.0, 6.0),
                    (19.0, 17.0),
                    (12.0, 23.0),
                    (5.0, 17.0),
                    (3.0, 6.0),
                ],
                ink.gamma_multiply(0.2),
            );
            c.outline(&[
                (12.0, 2.0),
                (21.0, 6.0),
                (19.0, 17.0),
                (12.0, 23.0),
                (5.0, 17.0),
                (3.0, 6.0),
                (12.0, 2.0),
            ]);
            c.outline(&[(7.0, 12.0), (11.0, 16.0), (17.0, 8.0)]);
        }
        I::Key | I::PublicKey => {
            c.circle(8.0, 8.0, 5.0, false);
            c.line((11.0, 12.0), (21.0, 22.0));
            c.line((16.0, 17.0), (19.0, 14.0));
            c.line((19.0, 20.0), (22.0, 17.0));
            if id == I::PublicKey {
                c.circle(8.0, 8.0, 1.4, true);
            }
        }
        I::Encrypted => {
            c.rect(4.0, 10.0, 16.0, 12.0, false);
            c.arc(12.0, 10.0, 5.0, std::f32::consts::PI, std::f32::consts::TAU);
            c.circle(12.0, 16.0, 1.5, true);
            c.line((12.0, 17.0), (12.0, 19.0));
        }
        I::Signature => {
            c.page();
            c.outline(&[
                (7.0, 18.0),
                (12.0, 13.0),
                (11.0, 19.0),
                (17.0, 14.0),
                (16.0, 18.0),
                (21.0, 17.0),
            ]);
        }
        I::Sql
        | I::Sqlite
        | I::Database
        | I::Duckdb
        | I::Access
        | I::ColumnData
        | I::Scientific => {
            c.ellipse(12.0, 6.0, 8.0, 3.0);
            c.line((4.0, 6.0), (4.0, 18.0));
            c.line((20.0, 6.0), (20.0, 18.0));
            c.ellipse(12.0, 18.0, 8.0, 3.0);
            c.line((5.0, 14.0), (19.0, 14.0));
            c.text(
                match id {
                    I::Sql => "SQL",
                    I::Sqlite => "S",
                    I::Duckdb => "D",
                    I::ColumnData => "|||",
                    I::Scientific => "f",
                    _ => "",
                },
                12.0,
                12.0,
                6.0,
                c.neutral,
            );
        }
        I::Shell | I::Batch | I::Powershell => {
            c.rect(2.0, 4.0, 20.0, 17.0, false);
            c.outline(&[(6.0, 9.0), (10.0, 13.0), (6.0, 17.0)]);
            c.line((13.0, 17.0), (18.0, 17.0));
            if id == I::Powershell {
                c.line((15.0, 7.0), (11.0, 18.0));
            }
        }
        I::Ebook | I::Djvu => {
            c.outline(&[
                (12.0, 5.0),
                (4.0, 3.0),
                (2.0, 4.0),
                (2.0, 20.0),
                (12.0, 22.0),
                (22.0, 20.0),
                (22.0, 4.0),
                (20.0, 3.0),
                (12.0, 5.0),
                (12.0, 22.0),
            ]);
            for y in [9.0, 13.0, 17.0] {
                c.line((5.0, y), (9.0, y + 1.0));
                c.line((15.0, y + 1.0), (19.0, y));
            }
        }
        I::Php => {
            c.ellipse(12.0, 12.0, 10.0, 7.0);
            c.text("php", 12.0, 12.0, 9.0, ink);
        }
        I::R => {
            c.ellipse(12.0, 11.0, 10.0, 6.0);
            c.text("R", 13.0, 14.0, 15.0, ink);
        }
        I::Erlang => {
            c.outline(&[(6.0, 3.0), (3.0, 3.0), (3.0, 21.0), (6.0, 21.0)]);
            c.outline(&[(18.0, 3.0), (21.0, 3.0), (21.0, 21.0), (18.0, 21.0)]);
            c.text("E", 12.0, 12.0, 15.0, ink);
        }
        I::Protobuf => {
            c.rect(2.0, 2.0, 7.0, 6.0, false);
            c.rect(15.0, 9.0, 7.0, 6.0, false);
            c.rect(2.0, 17.0, 7.0, 6.0, false);
            c.line((9.0, 5.0), (15.0, 12.0));
            c.line((9.0, 20.0), (15.0, 12.0));
        }
        I::Latex => {
            c.text("T", 7.0, 10.0, 17.0, ink);
            c.text("E", 13.0, 16.0, 12.0, ink);
            c.text("X", 19.0, 11.0, 14.0, ink);
        }
        I::Photoshop | I::Indesign | I::Figma => {
            c.rect(3.0, 3.0, 15.0, 15.0, false);
            c.rect(7.0, 8.0, 15.0, 14.0, false);
            c.text(
                match id {
                    I::Photoshop => "Ps",
                    I::Indesign => "Id",
                    _ => "F",
                },
                13.0,
                14.0,
                9.0,
                ink,
            );
        }
        I::Illustrator | I::Krita | I::Gimp => {
            c.poly(
                &[
                    (13.0, 2.0),
                    (21.0, 10.0),
                    (12.0, 20.0),
                    (3.0, 21.0),
                    (4.0, 12.0),
                ],
                ink.gamma_multiply(0.2),
            );
            c.outline(&[
                (13.0, 2.0),
                (21.0, 10.0),
                (12.0, 20.0),
                (3.0, 21.0),
                (4.0, 12.0),
                (13.0, 2.0),
            ]);
            c.line((4.0, 20.0), (12.0, 12.0));
            c.circle(13.0, 11.0, 2.0, false);
        }
        I::Sketch => {
            c.outline(&[
                (5.0, 4.0),
                (19.0, 4.0),
                (23.0, 10.0),
                (12.0, 23.0),
                (1.0, 10.0),
                (5.0, 4.0),
            ]);
            c.line((1.0, 10.0), (23.0, 10.0));
            c.line((7.0, 10.0), (12.0, 23.0));
            c.line((17.0, 10.0), (12.0, 23.0));
        }
        I::Blender => {
            c.circle(14.0, 12.0, 7.0, false);
            c.circle(14.0, 12.0, 3.0, true);
            c.line((8.0, 8.0), (3.0, 3.0));
            c.line((7.0, 12.0), (1.0, 12.0));
            c.line((8.0, 16.0), (3.0, 21.0));
        }
        I::Svg => {
            c.rect(3.0, 3.0, 18.0, 18.0, false);
            c.line((5.0, 19.0), (19.0, 5.0));
            for (x, y) in [(5.0, 19.0), (19.0, 5.0)] {
                c.rect(x - 2.0, y - 2.0, 4.0, 4.0, true);
            }
        }
        _ => paint_family(&c, id, white),
    }
}

fn paint_family(c: &Canvas<'_>, id: I, white: Color32) {
    match id.info().category {
        Category::Image => {
            c.rect(2.0, 4.0, 20.0, 16.0, false);
            c.circle(17.0, 8.0, 2.0, true);
            c.poly(&[(4.0, 18.0), (9.0, 10.0), (15.0, 18.0)], c.ink);
            c.poly(&[(11.0, 18.0), (16.0, 12.0), (21.0, 18.0)], c.ink);
        }
        Category::Audio => {
            if id == I::Midi {
                c.rect(2.0, 5.0, 20.0, 15.0, false);
                for x in [7.0, 12.0, 17.0] {
                    c.line((x, 6.0), (x, 19.0));
                }
                for x in [6.0, 11.0, 16.0] {
                    c.rect(x, 5.0, 3.0, 8.0, true);
                }
            } else {
                c.circle(6.0, 18.0, 3.0, true);
                c.circle(17.0, 16.0, 3.0, true);
                c.line((9.0, 18.0), (9.0, 5.0));
                c.line((20.0, 16.0), (20.0, 3.0));
                c.line((9.0, 5.0), (20.0, 3.0));
            }
        }
        Category::Video => {
            c.rect(2.0, 4.0, 20.0, 16.0, false);
            c.poly(&[(9.0, 8.0), (17.0, 12.0), (9.0, 16.0)], c.ink);
            for y in [6.0, 11.0, 16.0] {
                c.rect(3.0, y, 2.0, 2.0, true);
                c.rect(19.0, y, 2.0, 2.0, true);
            }
        }
        Category::Archive => {
            c.rect(3.0, 4.0, 18.0, 18.0, false);
            c.line((3.0, 8.0), (21.0, 8.0));
            for y in [3.0, 7.0, 11.0, 15.0] {
                c.rect(
                    if y == 7.0 || y == 15.0 { 12.0 } else { 9.0 },
                    y,
                    3.0,
                    3.0,
                    true,
                );
            }
            c.rect(9.0, 18.0, 6.0, 3.0, false);
        }
        Category::Application => {
            c.rect(3.0, 3.0, 18.0, 18.0, false);
            c.line((3.0, 8.0), (21.0, 8.0));
            c.poly(&[(10.0, 11.0), (17.0, 15.0), (10.0, 19.0)], c.ink);
        }
        Category::Config | Category::Tool => match id {
            I::Next => {
                c.circle(12.0, 12.0, 9.0, false);
                c.text("N", 12.0, 12.0, 12.0, c.ink);
            }
            I::Vite => {
                c.poly(&[(3.0, 4.0), (21.0, 4.0), (10.0, 22.0)], c.ink);
                let bolt = Canvas { ink: white, ..*c };
                bolt.outline(&[(14.0, 2.0), (10.0, 12.0), (16.0, 10.0), (10.0, 21.0)]);
            }
            I::Kubernetes | I::Helm => {
                c.circle(12.0, 12.0, 8.0, false);
                for i in 0..6 {
                    let a = i as f32 * std::f32::consts::TAU / 6.0;
                    c.line((12.0, 12.0), (12.0 + 10.0 * a.cos(), 12.0 + 10.0 * a.sin()));
                }
                c.circle(12.0, 12.0, 2.0, true);
            }
            I::Terraform => {
                for (x, y) in [(3.0, 2.0), (12.0, 6.0), (12.0, 14.0)] {
                    c.poly(
                        &[
                            (x, y),
                            (x + 7.0, y + 3.0),
                            (x + 7.0, y + 10.0),
                            (x, y + 7.0),
                        ],
                        c.ink,
                    );
                }
            }
            I::Github | I::Gitlab | I::Circle | I::Travis | I::Azure | I::Jenkins => {
                c.line((5.0, 5.0), (19.0, 12.0));
                c.line((5.0, 19.0), (19.0, 12.0));
                for (x, y) in [(5.0, 5.0), (5.0, 19.0), (19.0, 12.0)] {
                    c.circle(x, y, 3.0, false);
                }
            }
            _ => {
                c.gear();
                let label = match id {
                    I::Yaml => "Y",
                    I::Toml => "T",
                    I::Env => "E",
                    I::Ini => "I",
                    I::Plist => "P",
                    I::Eslint => "E",
                    I::Prettier => "P",
                    I::Webpack => "W",
                    I::Gradle => "G",
                    I::Maven => "M",
                    I::Babel => "B",
                    I::Nix => "N",
                    _ => "",
                };
                if !label.is_empty() {
                    c.p.circle_filled(c.pt(12.0, 12.0), 5.0 * c.scale, white);
                    c.text(label, 12.0, 12.0, 9.0, c.ink);
                }
            }
        },
        Category::Language | Category::Web => {
            let label = match id {
                I::Php => "php",
                I::Perl => "PL",
                I::R => "R",
                I::Erlang => "E",
                I::Wasm => "WA",
                I::Matlab => "M",
                _ => "<>",
            };
            c.poly(
                &[
                    (12.0, 2.0),
                    (22.0, 7.0),
                    (22.0, 17.0),
                    (12.0, 22.0),
                    (2.0, 17.0),
                    (2.0, 7.0),
                ],
                c.ink.gamma_multiply(0.2),
            );
            c.text(
                label,
                12.0,
                12.0,
                if label.len() > 2 { 9.0 } else { 13.0 },
                c.ink,
            );
        }
        Category::Specialized => match id {
            I::Iso | I::Dmg | I::DiskImage | I::VmDisk => {
                c.circle(12.0, 12.0, 9.0, false);
                c.circle(12.0, 12.0, 3.0, false);
                c.line((14.0, 4.0), (19.0, 9.0));
            }
            I::Model3d | I::Cad => {
                c.outline(&[
                    (3.0, 7.0),
                    (12.0, 2.0),
                    (21.0, 7.0),
                    (21.0, 18.0),
                    (12.0, 23.0),
                    (3.0, 18.0),
                    (3.0, 7.0),
                    (12.0, 12.0),
                    (21.0, 7.0),
                ]);
                c.line((12.0, 12.0), (12.0, 23.0));
            }
            I::Subtitle => {
                c.rect(2.0, 5.0, 20.0, 15.0, false);
                c.text("CC", 12.0, 12.0, 10.0, c.ink);
            }
            I::Geo => {
                c.circle(12.0, 9.0, 6.0, false);
                c.poly(&[(7.0, 13.0), (17.0, 13.0), (12.0, 23.0)], c.ink);
                c.circle(12.0, 9.0, 2.0, true);
            }
            I::Torrent => {
                c.line((12.0, 2.0), (12.0, 17.0));
                c.outline(&[(6.0, 11.0), (12.0, 17.0), (18.0, 11.0)]);
                c.outline(&[(3.0, 17.0), (3.0, 22.0), (21.0, 22.0), (21.0, 17.0)]);
            }
            I::Backup => {
                c.arc(12.0, 12.0, 9.0, -2.0, 3.0);
                c.outline(&[(2.0, 8.0), (2.0, 14.0), (8.0, 14.0)]);
                c.line((12.0, 6.0), (12.0, 12.0));
                c.line((12.0, 12.0), (17.0, 15.0));
            }
            _ => {
                c.rect(3.0, 5.0, 18.0, 15.0, false);
                c.line((7.0, 12.0), (13.0, 12.0));
                c.line((10.0, 9.0), (10.0, 15.0));
                c.circle(17.0, 14.0, 1.5, true);
            }
        },
        Category::Repository | Category::Text | Category::Document | Category::Data => {
            c.page();
            for y in [11.0, 15.0, 18.0] {
                c.line((8.0, y), (17.0, y));
            }
            if id == I::Diff {
                c.line((3.0, 7.0), (9.0, 7.0));
                c.line((6.0, 4.0), (6.0, 10.0));
            }
            if id == I::Notebook {
                c.text("J", 12.0, 14.0, 10.0, c.ink);
            }
        }
        _ => c.page(),
    }
}
