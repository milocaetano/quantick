//! Borrowed drawing coordinates and picks, shared by input and read-only consumers.
//!
//! The composed series borrows its prefix and engine state. Neither view owns
//! a drawing store, a gesture, or the pane that supplies these disjoint fields.
use super::{
    CANDLE_MAGNET_REACH_PX, DRAWING_ANCHOR_RADIUS_PX, DRAWING_SELECT_RADIUS_PX, MAGNET_REACH_PX,
    MAGNET_REACH_UNLIMITED_PX, magnet_price_of, snap_bar_to_tape,
};
use crate::bands::{self, Band};
use crate::chart::PriceScale;
use crate::drawings::{self, ChartPoint, DrawContext, DrawingBand};
use crate::indicators::IndicatorViews;
use crate::viewport::Viewport;
use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;
use smallvec::SmallVec;

pub(crate) use quantick_chart::pane_series::PaneSeriesRead;

/// `anchor`, read back off the screen at `screen`, with each coordinate a
/// tool copied from a source restored to that source's exact value: a pixel
/// read back into a price is ulps off it, which left a magnet-snapped corner
/// a hair above its high and crept the untouched corner on every drag frame.
/// Matched by value because a tool answers positions only: an equal `f32`
/// was copied from its source, or names the same pixel, which reads back
/// to that source's value anyway.
fn exact_coordinates(
    mut anchor: ChartPoint,
    screen: egui::Pos2,
    sources: &[(egui::Pos2, ChartPoint)],
) -> ChartPoint {
    if let Some((_, source)) = sources.iter().find(|(at, _)| at.x == screen.x) {
        anchor.bar = source.bar;
        anchor.time_ms = source.time_ms;
    }
    if let Some((_, source)) = sources.iter().find(|(at, _)| at.y == screen.y) {
        anchor.price = source.price;
    }
    anchor
}

pub(crate) struct DrawingProjection<'a> {
    pub(super) series: PaneSeriesRead<'a>,
    pub(super) viewport: &'a Viewport,
    pub(super) indicators: &'a IndicatorViews,
}

impl DrawingProjection<'_> {
    /// Convert a chart pixel into an overlay anchor. The x coordinate is a
    /// fractional bar slot, so drawings follow pan/zoom instead of being stuck
    /// to one screen pixel.
    /// The x half is shared by every band — the panes ride the candles' time
    /// axis — and only the y half asks which band it is being read against.
    pub(super) fn drawing_point_at(
        &self,
        pos: egui::Pos2,
        history_right: f32,
        total: usize,
        magnet: bool,
        snap: drawings::AnchorSnap,
        band: &Band,
    ) -> Option<ChartPoint> {
        let scale = band.scale.as_ref()?;
        if total == 0 || band.rect.height() <= 1.0 {
            return None;
        }
        let bar = self.viewport.bar_at_x(pos.x, history_right, total);
        // A candle-magnet anchor cannot land where no candle is: the bar
        // clamps to the tape before the snap reads it.
        let bar = if snap == drawings::AnchorSnap::NearestOhlc {
            snap_bar_to_tape(bar, total)
        } else {
            bar
        };
        let value = match snap {
            // A mark's own rule beats the magnet toggle in both directions:
            // it snaps with the magnet off, and it snaps to *its* extreme
            // rather than to whichever of the four OHLC prices is nearest.
            drawings::AnchorSnap::BarLow => self.bar_extreme(band, bar, false),
            drawings::AnchorSnap::BarHigh => self.bar_extreme(band, bar, true),
            drawings::AnchorSnap::NearestOhlc => self.candle_nearest_ohlc(band, bar, pos.y, scale),
            drawings::AnchorSnap::Pointer => magnet
                .then(|| self.magnet_value(band, bar, pos.y, scale))
                .flatten(),
        }
        .unwrap_or_else(|| scale.price_at(pos.y));
        Some(ChartPoint::at_time(
            bar,
            value,
            self.series.anchor_time(bar),
        ))
    }

    /// The high or low of the bar `bar` falls on, on the price band only.
    ///
    /// An indicator band has no candle, so a mark dropped there keeps the
    /// pointer's own value: inventing a high for a CVD pane would be the
    /// data-honesty failure this repo refuses, and refusing the click
    /// outright would read as a bug.
    fn bar_extreme(&self, band: &Band, bar: f32, high: bool) -> Option<f64> {
        if !matches!(band.key, DrawingBand::Price) {
            return None;
        }
        let slot = Viewport::slot_of(bar)?;
        // The forming bar counts. Marking the bar that is running *is* the
        // live use of this tool — marking a closed one is review — and
        // `closed_bar` stops one slot short of it, which would drop the mark
        // back onto the pointer's own price: exactly the failure the snap
        // exists to prevent, in the only moment it is used under pressure.
        //
        // The extreme is read at the instant of the click. A low that
        // deepens afterwards leaves the mark where the bar was when it was
        // marked, which is what the mark is a record of.
        let candle = self.series.candle_at_slot(slot)?;
        if high { candle.high } else { candle.low }.to_f64()
    }

    /// The magnet, applied to the bar the pointer is over, on the band it
    /// is over.
    ///
    /// Only that bar is considered: snapping to a neighbour would move the
    /// anchor sideways, and the trader chose the bar by pointing at it. On an
    /// indicator band the candidates are that pane's own plotted values plus
    /// zero — without them a "CVD zero line" is drawn by eye while the pane's
    /// own zero rule sits right there. Never across bands: a price would be a
    /// meaningless place to snap a CVD level to.
    fn magnet_value(
        &self,
        band: &Band,
        bar: f32,
        pointer_y: f32,
        scale: &PriceScale,
    ) -> Option<f64> {
        let row = Viewport::slot_of(bar)?;
        match &band.key {
            // `candle_at_slot`, not `closed_bar`: the forming bar is a slot
            // like any other and pointing at the live candle is when a magnet
            // is used under pressure. Its two siblings — `bar_extreme` and
            // `candle_nearest_ohlc` — already read it that way, and the odd
            // one out silently returned "nothing to snap to" on the bar the
            // trader was actually on.
            DrawingBand::Price => magnet_price_of(
                self.series.candle_at_slot(row)?,
                pointer_y,
                scale,
                CANDLE_MAGNET_REACH_PX,
            ),
            // A time-only object has no value to snap.
            DrawingBand::AllBands => None,
            DrawingBand::Indicator(_) => {
                let view = self.indicators.visible_panes().find(|view| {
                    DrawingBand::Indicator(crate::bands::pane_key(view)) == band.key
                })?;
                bands::magnet_value_of(view, row, pointer_y, scale, MAGNET_REACH_PX)
            }
        }
    }

    /// The magnet on a body drag: the edge of `points` nearest the pointer —
    /// an anchor's level, or a straight line's value at the pointer's bar —
    /// onto the print under the pointer, and the whole object shifted by the
    /// same amount so it keeps its shape. The snapped anchor takes the print
    /// exactly; nothing in reach leaves `points` as they are.
    pub(super) fn snap_body(
        &self,
        points: &mut [ChartPoint],
        body: drawings::BodySnap,
        band: &Band,
        pointer: egui::Pos2,
        (history_right, total): (f32, usize),
    ) {
        let (Some(scale), false) = (band.scale.as_ref(), body == drawings::BodySnap::Free) else {
            return;
        };
        // The candle's own bar, so a line meets the print at the candle.
        let Some(slot) = Viewport::slot_of(self.viewport.bar_at_x(pointer.x, history_right, total))
        else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        let bar = slot as f32;
        let line = match points {
            [a, b] if body == drawings::BodySnap::Line && a.bar != b.bar => Some(
                a.price + (b.price - a.price) * f64::from(bar - a.bar) / f64::from(b.bar - a.bar),
            ),
            _ => None,
        };
        let off = |price: f64| (scale.y(price) - pointer.y).abs();
        let edge = points
            .iter()
            .enumerate()
            .filter(|_| body == drawings::BodySnap::Levels || line.is_none())
            .map(|(index, point)| (Some(index), point.price))
            .chain(line.map(|price| (None, price)))
            .min_by(|left, right| off(left.1).total_cmp(&off(right.1)));
        let Some((snapped, price)) = edge else {
            return;
        };
        let Some(print) = self.magnet_value(band, bar, scale.y(price), scale) else {
            return;
        };
        for (index, point) in points.iter_mut().enumerate() {
            point.price = if snapped == Some(index) {
                print
            } else {
                point.price + (print - price)
            };
        }
    }

    /// The unconditional candle magnet: the nearest of the bar's OHLC with
    /// no reach limit, the forming bar included — [`AnchorSnap::NearestOhlc`]'s
    /// value rule. Price band only; a band with no candles answers `None`
    /// and the caller keeps the pointer's own value.
    fn candle_nearest_ohlc(
        &self,
        band: &Band,
        bar: f32,
        pointer_y: f32,
        scale: &PriceScale,
    ) -> Option<f64> {
        if !matches!(band.key, DrawingBand::Price) {
            return None;
        }
        let slot = Viewport::slot_of(bar)?;
        let candle = self.series.candle_at_slot(slot)?;
        magnet_price_of(candle, pointer_y, scale, MAGNET_REACH_UNLIMITED_PX)
    }

    pub fn projected_drawing_points(
        &self,
        drawing: &drawings::Drawing,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
    ) -> SmallVec<[egui::Pos2; 4]> {
        drawing
            .points
            .iter()
            .map(|point| self.drawing_screen_point(*point, history_right, total, scale))
            .collect()
    }

    /// The topmost object of `band` under the pointer. Objects of the other
    /// bands are not candidates at all — see [`bands::drawing_in_band`].
    pub(super) fn drawing_at(
        &self,
        drawings: &drawings::Drawings,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        let scale = band.scale.as_ref()?;
        drawings
            .items()
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, drawing)| {
                drawings.is_visible(*index) && bands::drawing_in_band(drawing, band)
            })
            .find_map(|(index, drawing)| {
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                // Locked selections paint no editable affordances.
                let selected = drawings.selected_id() == Some(drawing.id) && !drawing.locked;
                let ctxt = self.draw_context(drawing, scale, band, selected);
                drawing
                    .tool
                    .hit_test(band.rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
                    .then_some(index)
            })
    }

    /// Alt+click: deterministic z-order cycling through every visible object
    /// under the pointer. From the current selection, the next hit beneath
    /// it wins; past the bottom it wraps back to the top.
    pub(super) fn drawing_below_selection(
        &self,
        drawings: &drawings::Drawings,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        let scale = band.scale.as_ref()?;
        let hits: Vec<usize> = (0..drawings.items().len())
            .rev()
            .filter(|&index| drawings.is_visible(index))
            .filter(|&index| bands::drawing_in_band(&drawings.items()[index], band))
            .filter(|&index| {
                let drawing = &drawings.items()[index];
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let selected = drawings.selected_id() == Some(drawing.id) && !drawing.locked;
                let ctxt = self.draw_context(drawing, scale, band, selected);
                drawing
                    .tool
                    .hit_test(band.rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
            })
            .collect();
        match drawings
            .selected()
            .and_then(|current| hits.iter().position(|&index| index == current))
        {
            Some(at) => Some(hits[(at + 1) % hits.len()]),
            None => hits.first().copied(),
        }
    }

    /// Which handle of one object the pointer is on. The tool answers what
    /// its handles are, so the ring the trader sees is the ring they grab —
    /// a channel's width handle sits at the centre of a rail, not on the
    /// corner anchor that happens to define it.
    pub(super) fn drawing_handle_in(
        &self,
        drawings: &drawings::Drawings,
        drawing_index: usize,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        if !drawings.is_visible(drawing_index) {
            return None;
        }
        let scale = band.scale.as_ref()?;
        let drawing = drawings
            .items()
            .get(drawing_index)
            .filter(|drawing| bands::drawing_in_band(drawing, band))?;
        let projected = self.projected_drawing_points(drawing, history_right, total, scale);
        let selected = drawings.selected_id() == Some(drawing.id) && !drawing.locked;
        let ctxt = self.draw_context(drawing, scale, band, selected);
        drawing
            .tool
            .hit_handle(band.rect, &projected, pos, DRAWING_ANCHOR_RADIUS_PX, &ctxt)
    }

    /// What a pointer at `pos` is on: a drawing's handle first, then its
    /// body. One function, so the press and the click that follows it can
    /// never answer differently — grabbing a handle *is* clicking the object,
    /// and the handle radius is the wider of the two.
    pub(super) fn drawing_pick_at(
        &self,
        drawings: &drawings::Drawings,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        self.drawing_handle_at(drawings, pos, band, history_right, total)
            .map(|(drawing_index, _)| drawing_index)
            .or_else(|| self.drawing_at(drawings, pos, band, history_right, total))
    }

    /// Apply one frame of a handle drag, with the pointer already resolved to
    /// the chart point the trader is on (magnet included).
    ///
    /// A tool that owns its handles answers with every anchor's new screen
    /// position and the host projects them back; the anchors it *derived* are
    /// exact by construction and are never snapped a second time — the magnet
    /// belongs to the point under the pointer, not to a rail computed from it.
    /// Everything else is the plain "handle `handle` is anchor `handle`" move.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn drag_drawing_handle(
        &self,
        drawings: &mut drawings::Drawings,
        drawing_index: usize,
        handle: usize,
        target: ChartPoint,
        band: &Band,
        history_right: f32,
        total: usize,
        constrain: drawings::Constrain,
    ) {
        let moved = band.scale.as_ref().and_then(|scale| {
            let drawing = drawings.items().get(drawing_index)?;
            let projected = self.projected_drawing_points(drawing, history_right, total, scale);
            let ctxt = self.draw_context(drawing, scale, band, true);
            let to = self.drawing_screen_point(target, history_right, total, scale);
            let moved = drawing
                .tool
                .drag_handle(band.rect, &projected, handle, to, &ctxt, constrain)?;
            // The target (magnet included) first, then the anchors as they were.
            let sources: SmallVec<[(egui::Pos2, ChartPoint); 5]> = std::iter::once((to, target))
                .chain(projected.into_iter().zip(drawing.points.iter().copied()))
                .collect();
            Some((moved, sources))
        });
        let Some((moved, sources)) = moved else {
            drawings.move_anchor(drawing_index, handle, target);
            return;
        };
        let anchors: Option<SmallVec<[ChartPoint; 4]>> = moved
            .iter()
            // Derived anchors are exact by construction — neither the magnet
            // nor a tool's own snap rule applies to them a second time.
            .map(|point| {
                self.drawing_point_at(
                    *point,
                    history_right,
                    total,
                    false,
                    drawings::AnchorSnap::Pointer,
                    band,
                )
                .map(|anchor| exact_coordinates(anchor, *point, &sources))
            })
            .collect();
        if let Some(anchors) = anchors {
            drawings.set_points(drawing_index, &anchors);
        }
    }

    /// The context a tool reads `drawing` through on `band`, for hit-tests
    /// and gestures. `selected` is the caller's: the hit-tests pass a
    /// selection that is not locked, a handle drag passes `true`.
    fn draw_context<'a>(
        &self,
        drawing: &'a drawings::Drawing,
        scale: &'a PriceScale,
        band: &'a Band,
        selected: bool,
    ) -> DrawContext<'a> {
        DrawContext {
            payload: drawing.payload.as_ref(),
            anchors: &drawing.points,
            scale,
            px_per_bar: self.viewport.px_per_bar(),
            unit: band.unit(),
            primary_band: true,
            style: drawing.style,
            selected,
            halo: false,
            content_editing: false,
        }
    }

    /// What a double click at `pos` would do to drawing `drawing_index`, as
    /// its tool announces it on hover. Same projection and context as the
    /// hit-test, so the hint sits exactly where the click lands.
    pub(super) fn drawing_double_click_hint(
        &self,
        drawings: &drawings::Drawings,
        drawing_index: usize,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<drawings::DoubleClickHint> {
        let scale = band.scale.as_ref()?;
        let drawing = drawings
            .items()
            .get(drawing_index)
            .filter(|drawing| !drawing.locked && bands::drawing_in_band(drawing, band))?;
        let projected = self.projected_drawing_points(drawing, history_right, total, scale);
        let ctxt = self.draw_context(
            drawing,
            scale,
            band,
            drawings.selected_id() == Some(drawing.id),
        );
        drawing
            .tool
            .double_click_hint(band.rect, &projected, pos, &ctxt)
    }

    /// The drawing a double click at `pos` belongs to, with its payload as
    /// the click leaves it: the one the press `picked` when it picked one,
    /// else the topmost visible object whose tool would change there and
    /// whose *drawn* anchors enclose `pos` - an outline-only rectangle's
    /// interior takes no part in the click's hit-test, but its double click
    /// still lands. The fallback reads the drawn extent, never the painted
    /// one: a band run to the chart edges would otherwise swallow the
    /// chart's own double click across its whole width. Locked objects, and
    /// those `held` names (any strategy-bound region), never take one.
    ///
    /// The change is worked out on a copy, so the caller applies it whole
    /// or not at all; hover asks the same question for its hint.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn double_click_target(
        &self,
        drawings: &drawings::Drawings,
        picked: Option<usize>,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
        held: &dyn Fn(drawings::DrawingId) -> bool,
    ) -> Option<(usize, Box<dyn drawings::DrawingPayload>)> {
        let scale = band.scale.as_ref()?;
        let trial = |index: usize, fallback: bool| {
            let drawing = drawings.items().get(index)?;
            if drawing.locked
                || held(drawing.id)
                || !drawings.is_visible(index)
                || !bands::drawing_in_band(drawing, band)
            {
                return None;
            }
            let projected = self.projected_drawing_points(drawing, history_right, total, scale);
            if fallback && !egui::Rect::from_points(&projected).contains(pos) {
                return None;
            }
            let mut payload = drawing.payload.clone_box();
            drawing
                .tool
                .double_click(band.rect, &projected, pos, payload.as_mut())
                .then_some((index, payload))
        };
        match picked {
            Some(index) => trial(index, false),
            None => (0..drawings.items().len())
                .rev()
                .find_map(|index| trial(index, true)),
        }
    }

    pub(super) fn drawing_handle_at(
        &self,
        drawings: &drawings::Drawings,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<(usize, usize)> {
        let selected = drawings.selected();
        if let Some(drawing_index) = selected
            && let Some(handle) =
                self.drawing_handle_in(drawings, drawing_index, pos, band, history_right, total)
        {
            return Some((drawing_index, handle));
        }
        (0..drawings.items().len())
            .rev()
            .filter(|drawing_index| Some(*drawing_index) != selected)
            .find_map(|drawing_index| {
                self.drawing_handle_in(drawings, drawing_index, pos, band, history_right, total)
                    .map(|handle| (drawing_index, handle))
            })
    }

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

    /// Rewrite the market instants behind the selected object's anchors after
    /// a move that changed their bar positions (drag or keyboard nudge).
    ///
    /// Without this the mark moves on this chart and its shared twin stays
    /// where it was: market time is what the other panes read, so a move that
    /// does not update it has moved only half the object.
    pub fn retime_selected(&self, drawings: &mut drawings::Drawings) {
        let Some(index) = drawings.selected() else {
            return;
        };
        let Some(drawing) = drawings.items().get(index) else {
            return;
        };
        // Collected first so the immutable borrow of the store ends before
        // the write; every shipped tool has at most four anchors.
        let times: SmallVec<[Option<i64>; 4]> = drawing
            .points
            .iter()
            .map(|point| self.series.anchor_time(point.bar))
            .collect();
        drawings.set_times(index, &times);
    }
}

impl DrawingProjection<'_> {
    pub(crate) fn price_axis_levels(
        &self,
        drawings: &drawings::Drawings,
        chart_rect: egui::Rect,
        history_right: f32,
        total: usize,
        scale: &PriceScale,
        out: &mut Vec<super::PriceAxisLevel>,
    ) {
        out.clear();
        for (index, drawing) in drawings.items().iter().enumerate() {
            if !drawings.is_visible(index) || matches!(drawing.band, DrawingBand::Indicator(_)) {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            for y in drawing.tool.axis_levels(chart_rect, &points) {
                out.push(super::PriceAxisLevel {
                    id: drawing.id,
                    y,
                    price: scale.price_at(y),
                    color: drawings::painted_color(drawing),
                });
            }
        }
    }
}
