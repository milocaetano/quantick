//! Projecting and painting this pane's own drawings: the bands they live in,
//! where an anchor lands on screen, the colour honesty paints them in, and the
//! two passes `draw_chart` runs over them.
//!
//! The band carve is the invariant everything here rests on: an object is
//! painted, hit and dragged on the band whose axis its value belongs to, and
//! the band's scale is the one its own curve was just drawn with (see
//! [`crate::bands`]). A pure move out of `pane.rs`; the free functions
//! `paint_placement_hint`, `snap_bar_to_tape` and `magnet_price_of` stay at
//! module scope in [`super`], where callers on both sides of the cut reach them.

use eframe::egui;
use smallvec::SmallVec;

use crate::bands::{self, Band, BandLabel, Bands};
use crate::chart::PriceScale;
use crate::drawings::{ChartPoint, DrawContext, Drawing, DrawingBand, DrawingStyle};
use crate::indicators::PaneSizing;
use crate::plot_area::PlotAreas;

use super::{ChartPane, DrawPass, paint_placement_hint};

impl ChartPane {
    pub(super) fn drawing_screen_point(
        &self,
        point: ChartPoint,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> egui::Pos2 {
        egui::pos2(
            self.viewport
                .x_at_bar_position(point.bar, history_right, total),
            scale.y(point.price),
        )
    }

    /// The colour an object is really painted in, honesty fade included.
    ///
    /// A mark whose anchors are outside the loaded series, or that was drawn
    /// on the instrument this tab used to show, is painted faded rather than
    /// passed off as a level on this market. Its axis tag wears the same fade
    /// for the same reason and through the same call: a full-strength chip on
    /// the gutter would be the axis making a claim the stroke beside it is
    /// explicitly not making.
    pub(super) fn painted_color(drawing: &Drawing) -> egui::Color32 {
        if drawing.off_series || drawing.foreign_market {
            drawing.style.color.gamma_multiply(Self::CLAMPED_OPACITY)
        } else {
            drawing.style.color
        }
    }

    /// Opacity a drawing is painted at on a pane whose series does not reach
    /// its anchors — a mirrored mark clamped to an edge, or one of this
    /// pane's own that outlived the bars it was drawn on. Faded, not hidden
    /// and not silently snapped: the mark is real, its position on *this*
    /// chart is not exact.
    pub(super) const CLAMPED_OPACITY: f32 = 0.45;

    /// Carve this pane into its bands, into a buffer the caller owns.
    ///
    /// The geometry is per-frame by nature — rects and scales move with pan
    /// and zoom — but the container is not: the caller hands the same buffer
    /// back every frame, so after the first one this allocates nothing.
    ///
    /// See [`crate::bands`] for the invariant it upholds.
    pub(super) fn carve_bands(&self, areas: &PlotAreas, out: &mut Bands) {
        let history = self.drawing_area(areas.chart);
        bands::carve(
            out,
            &bands::PriceBand {
                rect: history,
                range: self
                    .frame
                    .auto_range
                    .map(|auto| self.price_view.resolve(auto)),
                top: areas.chart.top(),
                bottom: areas.chart.bottom(),
                inverted: self.price_view.is_inverted(),
            },
            &self.indicators,
            areas,
            &self.price_band_label,
        );
    }

    /// The bands of this frame, in a fresh buffer — the input pass, which has
    /// no buffer of its own to lend.
    pub(super) fn bands(&self, areas: &PlotAreas) -> Bands {
        let mut out = Bands::new();
        self.carve_bands(areas, &mut out);
        out
    }

    /// What the chrome says about the band an object lives on.
    ///
    /// Answered from the indicator list rather than from the last frame's
    /// bands, so it is the same answer before the first paint and while the
    /// pane is hidden behind an eye — hidden is not removed.
    #[must_use]
    pub fn band_label(&self, drawing: &Drawing) -> BandLabel {
        match &drawing.band {
            DrawingBand::Price => BandLabel::Price,
            DrawingBand::AllBands => BandLabel::AllBands,
            DrawingBand::Indicator(key) => self
                .indicators
                .all()
                .iter()
                .filter(|view| !view.descriptor.overlay)
                .find(|view| &self.indicators.pane_key(view) == key)
                .map_or_else(
                    || BandLabel::Parked(std::sync::Arc::clone(&key.kind)),
                    |view| {
                        // A band drawing can stop painting for four different
                        // reasons, and only one of them is "the indicator is
                        // gone". A mark that vanishes unexplained is what
                        // teaches a trader to stop trusting the tool, so each
                        // reason says its own name.
                        match () {
                            () if view.error.is_some() => BandLabel::Unpainted(
                                view.label_shared(),
                                "this indicator is in error, so its pane and everything drawn on                                  it are not being painted",
                            ),
                            () if view.hidden => BandLabel::Unpainted(
                                view.label_shared(),
                                "this indicator is hidden - show it again and the object comes                                  back with it",
                            ),
                            () if view.sizing == PaneSizing::Collapsed => BandLabel::Unpainted(
                                view.label_shared(),
                                "this pane is collapsed - open it with the chevron and the object                                  is there",
                            ),
                            () => BandLabel::Indicator(view.label_shared()),
                        }
                    },
                ),
        }
    }

    /// The band key of every indicator pane, with one value its own series
    /// actually holds at `slot`.
    ///
    /// The seam the `QUANTICK_DRAWINGS_DEMO=bands` hook seeds through: an
    /// object placed at a sampled value lands on the curve it annotates, so
    /// a screenshot proves the projection rather than a number someone made
    /// up. A pane with nothing computed at that slot contributes nothing.
    #[must_use]
    pub fn indicator_band_samples(&self, slot: usize) -> Vec<(DrawingBand, f64)> {
        self.indicators
            .visible_panes()
            .filter_map(|view| {
                let value = view
                    .columns
                    .iter()
                    .find_map(|column| column.get(slot).copied().filter(|v| v.is_finite()))?;
                Some((
                    DrawingBand::Indicator(self.indicators.pane_key(view)),
                    value,
                ))
            })
            .collect()
    }

    /// How much of the selected object's own axis one pixel is worth.
    ///
    /// The keyboard nudge reads this instead of the candles' scale: a level
    /// on a CVD band moved by a quantity of *price* is a wrong number
    /// delivered through the one gesture that exists for precision, and every
    /// press would record it as its own undo entry. `None` when nothing is
    /// selected, when the object is parked, or before the first frame.
    #[must_use]
    pub fn selected_value_per_px(&self) -> Option<f64> {
        let drawing = self.drawings.items().get(self.drawings.selected()?)?;
        let band = bands::band_of(&self.frame.bands, drawing)?;
        let scale = band.scale?;
        let (lo, hi) = scale.range();
        let per_px = (hi - lo) / f64::from(band.rect.height().max(1.0));
        // Signed for the *screen* gesture: an upward step raises the value on
        // an upright band and lowers it on an inverted one, so the arrows
        // keep moving the object the way the key points.
        Some(if scale.is_inverted() { -per_px } else { per_px })
    }

    /// The band drawings live in: the candles minus the live lane.
    ///
    /// The lane is the tape's own reserved strip at the right edge — a live
    /// region, not a place for annotations, and a horizontal line running
    /// across it was drawing over the flow. Placement already refused to put
    /// an anchor there; paint, geometry and hit-test agree with it now, which
    /// also keeps a line's painted end and its grabbable end the same pixel.
    pub(crate) fn drawing_area(&self, chart: egui::Rect) -> egui::Rect {
        let right = self
            .frame
            .lane_divider_x
            .unwrap_or(chart.right())
            .clamp(chart.left(), chart.right());
        egui::Rect::from_min_max(chart.min, egui::pos2(right, chart.bottom()))
    }

    /// Paint the completed drawing objects. This runs once per frame and is
    /// O(number of drawings); it never touches the per-trade ingestion path.
    /// Paint one band's drawings, clipped to that band.
    ///
    /// The clip is not cosmetic: a CVD line crossing into the candles reads
    /// as a price level, and the two rects are adjacent.
    pub(super) fn draw_drawings(
        &self,
        painter: &egui::Painter,
        band: &Band,
        band_index: usize,
        history_right: f32,
        total: usize,
        pass: DrawPass,
    ) {
        let Some(scale) = band.scale.as_ref() else {
            return;
        };
        let chart_rect = band.rect;
        let clipped = painter.with_clip_rect(chart_rect);
        for (index, drawing) in self.drawings.items().iter().enumerate() {
            if !self.drawings.is_visible(index) || !bands::drawing_in_band(drawing, band) {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            let selected = self.drawings.selected() == Some(index);
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
                    color: Self::painted_color(drawing),
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
                content_editing: self.gestures.content_editing == Some(index),
            };
            // A locked object shows no resize handles: its geometry is not
            // editable, so the affordance would lie.
            if pass == DrawPass::UnderCandles {
                // The body only. Everything below this line — the caret, the
                // badges, the rubber band — is chrome about the object, and
                // chrome under the price is chrome nobody can read.
                drawing
                    .tool
                    .paint_under(&clipped, chart_rect, style, &points, &ctxt);
                continue;
            }
            drawing.tool.paint(
                &clipped,
                chart_rect,
                style,
                &points,
                &ctxt,
                selected && !drawing.locked && primary_band,
            );
            bands::paint_off_band_caret(&clipped, chart_rect, &points, drawing);
        }
        if pass == DrawPass::UnderCandles {
            return;
        }
        // Badges paint outside the visibility gate above: a hidden drawing
        // hides its geometry, never the fact that a bot rides it — an
        // invisible armed instance is the one state this surface must not
        // allow. O(armed instances), zero when none.
        for instance in &self.strategies.anchors.instances {
            let Some(index) = self.drawings.index_of(instance.drawing) else {
                continue;
            };
            let drawing = &self.drawings.items()[index];
            if !bands::drawing_in_band(drawing, band)
                || (drawing.band == DrawingBand::AllBands && band_index != 0)
            {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            self.paint_strategy_badge(&clipped, instance, drawing, &points);
        }

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
                && let Some(hover) = self.gestures.hover
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
            };
            // Both halves, in order. A tool whose body lives in the
            // background pass would otherwise preview as an empty outline
            // while it is being dragged out — the profile's own histogram is
            // already folded for a draft, so there is data to show.
            // Over the candles rather than under them: a preview is a thing
            // in flight, and burying it would hide the gesture.
            draft
                .tool
                .paint_under(&clipped, chart_rect, draft.style, &points, &ctxt);
            draft
                .tool
                .paint(&clipped, chart_rect, draft.style, &points, &ctxt, false);
            // What the next click will do, printed where the eye already is.
            // The rail's `n/N` badge says the same thing on the far side of
            // the screen, which is why a trader who drags a three-anchor tool
            // and lets go reads the waiting object as frozen.
            if let Some(cursor) = points.last().copied() {
                paint_placement_hint(&clipped, chart_rect, cursor, draft.tool, draft.points.len());
            }
        }
    }
}
