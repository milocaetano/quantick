//! Drawing geometry consumes only the drawings owner, viewport and gesture readback.
//!
//! The tools paint headless [`MarkScene`]s; [`paint_scene`] is the one place
//! a scene becomes egui shapes, and [`TextFonts`] is how a tool sizes a label
//! with the window's fonts.
use super::super::{DrawPass, paint_placement_hint};
use super::{Contribution, Package};
use crate::{
    bands::{self, Band},
    chart::PriceScale,
    drawings::{
        ChartPoint, DrawContext, DrawingBand, DrawingStyle, Drawings, MarkScene, pos, px_rect,
        to_color32,
    },
    viewport::Viewport,
};
use eframe::egui;
use quantick_drawings::{Align, Align2, FontFamily, FontSpec, MarkShape, Px, TextMetrics};
use smallvec::SmallVec;

/// How big a label sets, measured with the window's own fonts — the same
/// layout [`paint_scene`] will paint it with.
pub(in crate::pane) struct TextFonts<'a>(pub &'a egui::Context);

impl TextMetrics for TextFonts<'_> {
    fn size(&self, text: &str, font: FontSpec) -> Px {
        let size = self
            .0
            .fonts(|fonts| {
                fonts.layout_no_wrap(text.to_owned(), font_id(font), egui::Color32::PLACEHOLDER)
            })
            .size();
        Px::new(size.x, size.y)
    }
}

fn font_id(font: FontSpec) -> egui::FontId {
    match font.family {
        FontFamily::Proportional => egui::FontId::proportional(font.size_px),
        FontFamily::Monospace => egui::FontId::monospace(font.size_px),
    }
}

fn align(align: Align) -> egui::Align {
    match align {
        Align::Min => egui::Align::Min,
        Align::Center => egui::Align::Center,
        Align::Max => egui::Align::Max,
    }
}

fn stroke(stroke: quantick_drawings::Stroke) -> egui::Stroke {
    egui::Stroke::new(stroke.width, to_color32(stroke.color))
}

/// Paint `scene` through `painter`: each [`MarkShape`] becomes exactly the
/// egui shape the tool's own painter call made before the tools left `app`
/// (`drawings_scene_golden` holds the two to the same list).
pub(in crate::pane) fn paint_scene(painter: &egui::Painter, scene: &MarkScene) {
    for shape in &scene.shapes {
        match shape {
            MarkShape::Line {
                from,
                to,
                stroke: line,
            } => {
                painter.line_segment([pos(*from), pos(*to)], stroke(*line));
            }
            MarkShape::Path {
                points,
                closed,
                fill,
                stroke: outline,
            } => {
                painter.add(egui::Shape::Path(egui::epaint::PathShape {
                    points: points.iter().copied().map(pos).collect(),
                    closed: *closed,
                    fill: to_color32(*fill),
                    stroke: stroke(*outline).into(),
                }));
            }
            MarkShape::DashedLine {
                points,
                stroke: line,
                dash_px,
                gap_px,
            } => {
                let points: Vec<egui::Pos2> = points.iter().copied().map(pos).collect();
                painter.add(egui::Shape::dashed_line(
                    &points,
                    stroke(*line),
                    *dash_px,
                    *gap_px,
                ));
            }
            MarkShape::Rect {
                rect,
                rounding_px,
                fill,
                stroke: outline,
            } => {
                painter.add(egui::epaint::RectShape::new(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left, rect.top),
                        egui::pos2(rect.right, rect.bottom),
                    ),
                    egui::Rounding::same(*rounding_px),
                    to_color32(*fill),
                    stroke(*outline),
                ));
            }
            MarkShape::Circle {
                center,
                radius_px,
                fill,
                stroke: outline,
            } => {
                painter.add(egui::epaint::CircleShape {
                    center: pos(*center),
                    radius: *radius_px,
                    fill: to_color32(*fill),
                    stroke: stroke(*outline),
                });
            }
            MarkShape::Text {
                at,
                anchor: Align2([x, y]),
                text,
                font,
                color,
            } => {
                painter.text(
                    pos(*at),
                    egui::Align2([align(*x), align(*y)]),
                    text,
                    font_id(*font),
                    to_color32(*color),
                );
            }
        }
    }
}
pub(super) const PACKAGE: Package = Package {
    layers: &[quantick_layers::ChartLayer::Drawings],
    contributions: &[
        Contribution::Drawings(|view| view.objects()),
        Contribution::DrawingDraft(|view| view.draft()),
    ],
};
pub(in crate::pane) struct DrawingPass<'a> {
    pub painter: &'a egui::Painter,
    pub band: &'a Band,
    pub band_index: usize,
    pub drawings: &'a Drawings,
    pub viewport: &'a Viewport,
    pub history_right: f32,
    pub total: usize,
    pub pass: DrawPass,
    pub content_editing: Option<usize>,
    pub hover: Option<ChartPoint>,
}
impl DrawingPass<'_> {
    fn drawing_screen_point(
        &self,
        point: ChartPoint,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> Px {
        Px::new(
            self.viewport
                .x_at_bar_position(point.bar, history_right, total),
            scale.y(point.price),
        )
    }
    fn projected_drawing_points(
        &self,
        drawing: &crate::drawings::Drawing,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> SmallVec<[Px; 4]> {
        drawing
            .points
            .iter()
            .map(|point| self.drawing_screen_point(*point, history_right, total, scale))
            .collect()
    }
    fn objects(&mut self) {
        let DrawingPass {
            painter,
            band,
            band_index,
            history_right,
            total,
            pass,
            ..
        } = *self;
        let Some(scale) = band.scale.as_ref() else {
            return;
        };
        let chart_rect = band.rect;
        let clipped = painter.with_clip_rect(chart_rect);
        let fonts = TextFonts(painter.ctx());
        for (index, drawing) in self.drawings.items().iter().enumerate() {
            if !self.drawings.is_visible(index) || !bands::drawing_in_band(drawing, band) {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            let selected = self.drawings.selected_id() == Some(drawing.id);
            // An object that crosses every band draws its stroke in each and
            // its readout and handles in the first: three copies of
            // "17 bars 4m 21s" stacked down the screen is not three facts.
            let primary_band = drawing.band != DrawingBand::AllBands || band_index == 0;
            // A mark this chart's data does not back: its anchors are outside
            // the loaded series, or it was drawn on the instrument this tab
            // used to show. It survived the change, because only the trader
            // deletes a drawing, and it says what it is by fading rather than
            // by passing itself off as a level on this market. Same opacity
            // the mirrored marks use for the same reason.
            let style = if drawing.off_series || drawing.foreign_market {
                DrawingStyle {
                    color: crate::drawings::from_color32(crate::drawings::painted_color(drawing)),
                    fill_alpha: 0,
                    ..drawing.style
                }
            } else {
                drawing.style
            };
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &drawing.points,
                scale,
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band,
                style,
                selected,
                halo: false,
                content_editing: self.content_editing == Some(index),
                text: &fonts,
            };
            let mut scene = MarkScene::default();
            // A locked object shows no resize handles: its geometry is not
            // editable, so the affordance would lie.
            if pass == DrawPass::UnderCandles {
                // The body only. Everything below this line — the caret, the
                // badges, the rubber band — is chrome about the object, and
                // chrome under the price is chrome nobody can read.
                drawing
                    .tool
                    .paint_under(&mut scene, px_rect(chart_rect), style, &points, &ctxt);
                paint_scene(&clipped, &scene);
                continue;
            }
            drawing.tool.paint(
                &mut scene,
                px_rect(chart_rect),
                style,
                &points,
                &ctxt,
                selected && !drawing.locked && primary_band,
            );
            paint_scene(&clipped, &scene);
            let points: SmallVec<[egui::Pos2; 4]> = points.iter().copied().map(pos).collect();
            bands::paint_off_band_caret(&clipped, chart_rect, &points, drawing);
        }
    }
    fn draft(&mut self) {
        let DrawingPass {
            painter,
            band,
            band_index,
            history_right,
            total,
            ..
        } = *self;
        let Some(scale) = band.scale.as_ref() else {
            return;
        };
        let chart_rect = band.rect;
        let clipped = painter.with_clip_rect(chart_rect);
        let fonts = TextFonts(painter.ctx());
        // With hide-all engaged the finished object would be invisible, so
        // the rubber-band must not pretend otherwise (audit M8) — placement
        // itself releases hide-all when it commits, in `place_with`.
        if let Some(draft) = self
            .drawings
            .draft()
            .filter(|draft| bands::drawing_in_band(draft, band))
            .filter(|_| !self.drawings.all_hidden())
        {
            let mut points = self.projected_drawing_points(draft, history_right, total, scale);
            // The preview completes the geometry with the hovered anchor, in
            // both screen and chart space, so payload-driven tools can show
            // their real shape while placing.
            let mut anchors: SmallVec<[ChartPoint; 4]> = SmallVec::from_slice(&draft.points);
            if points.len() < draft.tool.required_points()
                && let Some(hover) = self.hover
            {
                points.push(self.drawing_screen_point(hover, history_right, total, scale));
                anchors.push(hover);
            }
            let ctxt = DrawContext {
                payload: draft.payload.as_ref(),
                anchors: &anchors,
                scale,
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band: draft.band != DrawingBand::AllBands || band_index == 0,
                style: draft.style,
                selected: false,
                halo: false,
                content_editing: false,
                text: &fonts,
            };
            // Both halves, in order. A tool whose body lives in the
            // background pass would otherwise preview as an empty outline
            // while it is being dragged out — the profile's own histogram is
            // already folded for a draft, so there is data to show.
            // Over the candles rather than under them: a preview is a thing
            // in flight, and burying it would hide the gesture.
            let mut scene = MarkScene::default();
            draft
                .tool
                .paint_under(&mut scene, px_rect(chart_rect), draft.style, &points, &ctxt);
            draft.tool.paint(
                &mut scene,
                px_rect(chart_rect),
                draft.style,
                &points,
                &ctxt,
                false,
            );
            paint_scene(&clipped, &scene);
            // What the next click will do, printed where the eye already is.
            // The rail's `n/N` badge says the same thing on the far side of
            // the screen, which is why a trader who drags a three-anchor tool
            // and lets go reads the waiting object as frozen.
            if let Some(cursor) = points.last().copied() {
                paint_placement_hint(
                    &clipped,
                    chart_rect,
                    pos(cursor),
                    draft.tool,
                    draft.points.len(),
                );
            }
        }
    }
}

impl DrawingPass<'_> {
    pub fn paint(
        &mut self,
        registry: &super::RenderRegistry,
        anchors: &crate::strategy_anchors::StrategyAnchors,
        projection: &super::super::drawing_projection::DrawingProjection<'_>,
        closed_slots: usize,
    ) {
        let DrawingPass {
            painter,
            band,
            band_index,
            drawings,
            history_right,
            total,
            pass,
            ..
        } = *self;
        registry.drawings(self);
        if pass == DrawPass::UnderCandles {
            return;
        }
        let Some(scale) = band.scale.as_ref() else {
            return;
        };
        let clipped = painter.with_clip_rect(band.rect);
        // Badges paint outside the visibility gate above: a hidden drawing
        // hides its geometry, never the fact that a bot rides it — an
        // invisible armed instance is the one state this surface must not
        // allow. O(armed instances), zero when none.
        for instance in &anchors.instances {
            let Some(index) = drawings.index_of(instance.drawing) else {
                continue;
            };
            let drawing = &drawings.items()[index];
            if !bands::drawing_in_band(drawing, band)
                || (drawing.band == DrawingBand::AllBands && band_index != 0)
            {
                continue;
            }
            let points = projection.projected_drawing_points(drawing, history_right, total, scale);
            super::super::strategies::paint_strategy_badge(
                &clipped,
                instance,
                drawing,
                &points,
                drawings.all_hidden(),
                closed_slots,
            );
        }

        registry.drawing_draft(self);
    }
}
