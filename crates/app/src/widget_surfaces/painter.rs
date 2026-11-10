//! The one painter every widget pane shares: each `WidgetScene` item becomes
//! one egui shape at the body's origin. Layout, text and colour roles were
//! decided headlessly; this only resolves a role to the theme's colour.

use eframe::egui::{Align2, Painter, Pos2, Rect, Shape, Stroke, vec2};
use quantick_chart::widgets::scene::{ScenePrim, WidgetScene};

use crate::theme::Palette;

pub(super) fn paint(painter: &Painter, origin: Pos2, scene: &WidgetScene, palette: &Palette) {
    let at = |[x, y]: [f32; 2]| origin + vec2(x, y);
    for prim in &scene.prims {
        match prim {
            ScenePrim::Rect([x, y, w, h], token, opacity) => {
                let fill = palette.get(*token).gamma_multiply(*opacity);
                painter.rect_filled(Rect::from_min_size(at([*x, *y]), vec2(*w, *h)), 0.0, fill);
            }
            ScenePrim::Text(run) => {
                let anchor = [
                    Align2::LEFT_CENTER,
                    Align2::CENTER_CENTER,
                    Align2::RIGHT_CENTER,
                ];
                let anchor = anchor[run.align as usize];
                let (font, ink) = (
                    palette.mono_font(run.size.points()),
                    *palette.get(run.token),
                );
                painter.text(at(run.at), anchor, &run.text, font, ink);
            }
            ScenePrim::Line(points, token) => {
                let points = points.iter().map(|point| at(*point)).collect();
                painter.add(Shape::line(
                    points,
                    Stroke::new(1.25_f32, *palette.get(*token)),
                ));
            }
            ScenePrim::Circle(centre, radius, token) => {
                painter.circle_filled(at(*centre), *radius, *palette.get(*token));
            }
        }
    }
}
