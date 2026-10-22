//! Borrowed drawing coordinates and picks, shared by input and read-only consumers.
//!
//! The composed series borrows its prefix and engine state. Neither view owns
//! a drawing store, a gesture, or the pane that supplies these disjoint fields.
use quantick_chart::indicator_views::IndicatorViews;
use quantick_chart_interaction::pane::CanvasHit;
use quantick_chart_interaction::viewport::Viewport;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use smallvec::SmallVec;

use super::band::{self, Band};
use super::{
    CANDLE_MAGNET_REACH_PX, DRAWING_ANCHOR_RADIUS_PX, DRAWING_SELECT_RADIUS_PX, MAGNET_REACH_PX,
    MAGNET_REACH_UNLIMITED_PX, Mark, Marks,
};
use crate::chart::{self, PriceScale};
use crate::{
    AnchorSnap, BodySnap, ChartPoint, Constrain, DrawContext, DrawingBand, DrawingId,
    DrawingPayload, PointerHint, Px, PxRect, UNMEASURED,
};

pub use quantick_chart::pane_series::PaneSeriesRead;

/// What a press at a pixel takes under the Pointer tool. Press, hover
/// cursor and the control plane's pick all read this one answer, so the
/// cursor always names what the press does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressTarget {
    /// Handle `handle` of drawing `index`: the press reshapes it.
    Handle { index: usize, handle: usize },
    /// The body of drawing `index`, and what its tool says the pointer
    /// does there.
    Body {
        index: usize,
        hint: Option<PointerHint>,
    },
}

impl PressTarget {
    /// The drawing the press lands on.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Handle { index, .. } | Self::Body { index, .. } => index,
        }
    }
}

/// Which click a release completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickKind {
    Single,
    Double,
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod projection_tests;

/// Clamp a fractional bar coordinate onto the bars that exist:
/// `0 ..= total - 1`. The candle magnet's time half — a snap that reads a
/// candle must stand on one.
#[must_use]
pub fn snap_bar_to_tape(bar: f32, total: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    bar.clamp(0.0, total.saturating_sub(1) as f32)
}

/// The open / high / low / close of `candle` nearest to the pointer on
/// screen, when the pointer is on the candle or within `reach_px` of it.
///
/// This is the difference between a line that *looks* drawn off the swing
/// high and one that is (`docs/ux/drawing-tools-2026-08.md` §D6). Reach is
/// measured to the candle's span, not to each print, or a pointer inside a
/// tall body missed. Off the candle and out of reach returns `None` and the
/// free price is used — a magnet that always snaps is a magnet you cannot
/// draw a diagonal with.
#[must_use]
pub fn magnet_price_of(
    candle: &quantick_engine::Bar,
    pointer_y: f32,
    scale: &PriceScale,
    reach_px: f32,
) -> Option<f64> {
    let (high, low) = (
        scale.y(candle.high.to_f64()?),
        scale.y(candle.low.to_f64()?),
    );
    if pointer_y < high.min(low) - reach_px || pointer_y > high.max(low) + reach_px {
        return None;
    }
    [candle.open, candle.high, candle.low, candle.close]
        .into_iter()
        .filter_map(|price| {
            let price = price.to_f64()?;
            Some(((scale.y(price) - pointer_y).abs(), price))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, price)| price)
}

/// The prices [`CANDLE_MAGNET_REACH_PX`] around `pointer_y` reach, as one
/// `[lo, hi]` interval, so a candle that cannot snap is skipped before any of
/// its prints is converted. A pixel wider than the reach: rounding may only
/// let a candle through to the exact test, never drop one. `None` when the
/// bounds are no decimal, and every candle is tested.
fn reach_interval(scale: &PriceScale, pointer_y: f32) -> Option<(Decimal, Decimal)> {
    let edge = CANDLE_MAGNET_REACH_PX + 1.0;
    let (a, b) = (
        scale.price_at(pointer_y - edge),
        scale.price_at(pointer_y + edge),
    );
    Some((Decimal::from_f64(a.min(b))?, Decimal::from_f64(a.max(b))?))
}

/// `anchor`, read back off the screen at `screen`, with each coordinate a
/// tool copied from a source restored to that source's exact value: a pixel
/// read back into a price is ulps off it, which left a magnet-snapped corner
/// a hair above its high and crept the untouched corner on every drag frame.
/// Matched by value because a tool answers positions only: an equal `f32`
/// was copied from its source, or names the same pixel, which reads back
/// to that source's value anyway.
fn exact_coordinates(
    mut anchor: ChartPoint,
    screen: Px,
    sources: &[(Px, ChartPoint)],
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

pub struct DrawingProjection<'a> {
    pub series: PaneSeriesRead<'a>,
    pub viewport: &'a Viewport,
    pub indicators: &'a IndicatorViews,
}

impl DrawingProjection<'_> {
    /// Convert a chart pixel into an overlay anchor. The x coordinate is a
    /// fractional bar slot, so drawings follow pan/zoom instead of being stuck
    /// to one screen pixel.
    /// The x half is shared by every band — the panes ride the candles' time
    /// axis — and only the y half asks which band it is being read against.
    #[must_use]
    pub fn drawing_point_at(
        &self,
        pos: Px,
        history_right: f32,
        total: usize,
        magnet: bool,
        snap: AnchorSnap,
        band: &Band,
    ) -> Option<ChartPoint> {
        let scale = band.scale.as_ref()?;
        if total == 0 || band.rect.height() <= 1.0 {
            return None;
        }
        let bar = self.viewport.bar_at_x(pos.x, history_right, total);
        // A candle-magnet anchor cannot land where no candle is: the bar
        // clamps to the tape before the snap reads it.
        let bar = if snap == AnchorSnap::NearestOhlc {
            snap_bar_to_tape(bar, total)
        } else {
            bar
        };
        let value = match snap {
            // A mark's own rule beats the magnet toggle in both directions:
            // it snaps with the magnet off, and it snaps to *its* extreme
            // rather than to whichever of the four OHLC prices is nearest.
            AnchorSnap::BarLow => self.bar_extreme(band, bar, false),
            AnchorSnap::BarHigh => self.bar_extreme(band, bar, true),
            AnchorSnap::NearestOhlc => self.candle_nearest_ohlc(band, bar, pos.y, scale),
            AnchorSnap::Pointer => magnet
                .then(|| self.magnet_value(band, bar, pos.y, scale))
                .flatten(),
            AnchorSnap::VisibleLevels => magnet
                .then(|| self.visible_level_value(band, bar, pos.y, scale, (history_right, total)))
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
                let view = self
                    .indicators
                    .visible_panes()
                    .find(|view| DrawingBand::Indicator(band::pane_key(view)) == band.key)?;
                band::magnet_value_of(view, row, pointer_y, scale, MAGNET_REACH_PX)
            }
        }
    }

    /// The magnet for a level ([`AnchorSnap::VisibleLevels`]): the open,
    /// high, low or close of any candle in view — the forming one included —
    /// nearest `pointer_y` on screen within [`CANDLE_MAGNET_REACH_PX`]. The
    /// candle under the pointer keeps its own rule ([`Self::magnet_value`]):
    /// a pointer inside a tall body reaches that candle's nearest print even
    /// past the reach.
    ///
    /// A level is drawn off a swing wherever it sits on screen, so the
    /// candidates are not tied to the pointer's bar; only the value moves,
    /// never the bar. A candle is in view when its centre is inside the band
    /// (`left ..= history_right`): one mostly scrolled off an edge is no
    /// candidate, as a level snapping to a print the trader cannot see reads
    /// as a bug. An indicator band keeps its own magnet unchanged.
    fn visible_level_value(
        &self,
        band: &Band,
        bar: f32,
        pointer_y: f32,
        scale: &PriceScale,
        (history_right, total): (f32, usize),
    ) -> Option<f64> {
        if !matches!(band.key, DrawingBand::Price) {
            return self.magnet_value(band, bar, pointer_y, scale);
        }
        let pointer_slot = Viewport::slot_of(bar);
        let left = band.rect.left();
        let (start, end) = self
            .viewport
            .visible_range((history_right - left).max(0.0), total);
        let in_view = |slot: usize| {
            (left..=history_right).contains(&self.viewport.x_center(slot, history_right, total))
        };
        // The pointer's own candle, when the scan below does not reach it.
        let under = pointer_slot
            .filter(|slot| !((start..end).contains(slot) && in_view(*slot)))
            .and_then(|_| self.magnet_value(band, bar, pointer_y, scale));
        let reach = reach_interval(scale, pointer_y);
        let off = |price: f64| (scale.y(price) - pointer_y).abs();
        (start..end)
            .filter(|slot| in_view(*slot))
            .filter_map(|slot| Some((slot, self.series.candle_at_slot(slot)?)))
            .filter(|(_, candle)| {
                reach.is_none_or(|(lo, hi)| candle.high >= lo && candle.low <= hi)
            })
            .filter_map(|(slot, candle)| {
                if pointer_slot == Some(slot) {
                    return magnet_price_of(candle, pointer_y, scale, CANDLE_MAGNET_REACH_PX);
                }
                [candle.open, candle.high, candle.low, candle.close]
                    .into_iter()
                    .filter_map(|price| price.to_f64())
                    .filter(|price| off(*price) <= CANDLE_MAGNET_REACH_PX)
                    .min_by(|left, right| off(*left).total_cmp(&off(*right)))
            })
            .chain(under)
            .min_by(|left, right| off(*left).total_cmp(&off(*right)))
    }

    /// The magnet on a body drag: the edge of `points` nearest the pointer —
    /// an anchor's level, or a straight line's value at the pointer's bar —
    /// onto the print under the pointer (for levels, any visible print — see
    /// [`Self::visible_level_value`] — whatever bar the pointer is on), and
    /// the whole object shifted by the
    /// same amount so it keeps its shape. The snapped anchor takes the print
    /// exactly; nothing in reach leaves `points` as they are.
    pub fn snap_body(
        &self,
        points: &mut [ChartPoint],
        body: BodySnap,
        band: &Band,
        pointer: Px,
        (history_right, total): (f32, usize),
    ) {
        let (Some(scale), false) = (band.scale.as_ref(), body == BodySnap::Free) else {
            return;
        };
        // The candle's own bar, so a line meets the print at the candle. A
        // level reads every candle in view, so a pointer on no bar at all —
        // left of the first — still snaps it; a line needs the bar.
        let pointer_bar = self.viewport.bar_at_x(pointer.x, history_right, total);
        #[allow(clippy::cast_precision_loss)]
        let bar = match Viewport::slot_of(pointer_bar) {
            Some(slot) => slot as f32,
            None if body == BodySnap::Levels => pointer_bar,
            None => return,
        };
        let line = match points {
            [a, b] if body == BodySnap::Line && a.bar != b.bar => Some(
                a.price + (b.price - a.price) * f64::from(bar - a.bar) / f64::from(b.bar - a.bar),
            ),
            _ => None,
        };
        let off = |price: f64| (scale.y(price) - pointer.y).abs();
        let edge = points
            .iter()
            .enumerate()
            .filter(|_| body == BodySnap::Levels || line.is_none())
            .map(|(index, point)| (Some(index), point.price))
            .chain(line.map(|price| (None, price)))
            .min_by(|left, right| off(left.1).total_cmp(&off(right.1)));
        let Some((snapped, price)) = edge else {
            return;
        };
        let print = if body == BodySnap::Levels {
            self.visible_level_value(band, bar, scale.y(price), scale, (history_right, total))
        } else {
            self.magnet_value(band, bar, scale.y(price), scale)
        };
        let Some(print) = print else {
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

    #[must_use]
    pub fn projected_drawing_points(
        &self,
        drawing: &Mark,
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

    /// What a double click at `pos` lands on in the candles' price `band`,
    /// left of `history_right`, where the candles end:
    /// a candle's body or wick opens the chart settings, open canvas returns
    /// to live at the overview spacing. A band with no price scale yet has no
    /// candle to hit, so it is open canvas: the gesture is never dead. `None`
    /// on a drawing or its handle, and in any other band — each keeps its own
    /// meaning for the gesture.
    #[must_use]
    pub fn canvas_hit(
        &self,
        drawings: &Marks,
        pos: Px,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<CanvasHit> {
        let right = history_right;
        if !matches!(band.key, DrawingBand::Price)
            || self
                .drawing_handle_at(drawings, pos, band, right, total)
                .is_some()
            || self.drawing_at(drawings, pos, band, right, total).is_some()
        {
            return None;
        }
        let on_candle = band.scale.as_ref().is_some_and(|scale| {
            let wick_y = |slot| {
                let bar = self.series.candle_at_slot(slot)?;
                Some((
                    scale.y(chart::to_f64(bar.high)),
                    scale.y(chart::to_f64(bar.low)),
                ))
            };
            self.viewport
                .on_candle([pos.x, pos.y], right, total, wick_y)
        });
        Some(if on_candle {
            CanvasHit::Candle
        } else {
            CanvasHit::Empty
        })
    }

    /// The topmost object of `band` under the pointer. Objects of the other
    /// bands are not candidates at all — see [`band::drawing_in_band`].
    #[must_use]
    pub fn drawing_at(
        &self,
        drawings: &Marks,
        pos: Px,
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
                drawings.is_visible(*index) && band::drawing_in_band(drawing, band)
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
    #[must_use]
    pub fn drawing_below_selection(
        &self,
        drawings: &Marks,
        pos: Px,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        let scale = band.scale.as_ref()?;
        let hits: Vec<usize> = (0..drawings.items().len())
            .rev()
            .filter(|&index| drawings.is_visible(index))
            .filter(|&index| band::drawing_in_band(&drawings.items()[index], band))
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
    #[must_use]
    pub fn drawing_handle_in(
        &self,
        drawings: &Marks,
        drawing_index: usize,
        pos: Px,
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
            .filter(|drawing| band::drawing_in_band(drawing, band))?;
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
    #[must_use]
    pub fn drawing_pick_at(
        &self,
        drawings: &Marks,
        pos: Px,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        self.press_target(drawings, pos, band, history_right, total)
            .map(PressTarget::index)
    }

    /// What a press at `pos` takes: the selected object's handles first -
    /// its point handles, then a side line its tool names - then any other
    /// object's point handle, then the topmost body under the pointer, whose
    /// side line the press grabs. A side line belongs to the selected object
    /// or the topmost one only, never to one beneath another's body. Each
    /// tool is asked for its hint once.
    #[must_use]
    pub fn press_target(
        &self,
        drawings: &Marks,
        pos: Px,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<PressTarget> {
        let hint_of =
            |index| self.drawing_pointer_hint(drawings, index, pos, band, history_right, total);
        let side_line = |index, hint| match hint {
            Some(PointerHint::ResizeSide { handle }) => Some(PressTarget::Handle { index, handle }),
            _ => None,
        };
        let point_handle = |index| {
            self.drawing_handle_in(drawings, index, pos, band, history_right, total)
                .map(|handle| PressTarget::Handle { index, handle })
        };
        let selected = drawings.selected();
        let mut selected_hint = None;
        if let Some(index) = selected {
            if let Some(target) = point_handle(index) {
                return Some(target);
            }
            selected_hint = hint_of(index);
            if let Some(target) = side_line(index, selected_hint) {
                return Some(target);
            }
        }
        let others = (0..drawings.items().len()).rev();
        if let Some(target) = others
            .filter(|index| Some(*index) != selected)
            .find_map(point_handle)
        {
            return Some(target);
        }
        let index = self.drawing_at(drawings, pos, band, history_right, total)?;
        let hint = if Some(index) == selected {
            selected_hint
        } else {
            hint_of(index)
        };
        Some(side_line(index, hint).unwrap_or(PressTarget::Body { index, hint }))
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
    pub fn drag_drawing_handle(
        &self,
        drawings: &mut Marks,
        drawing_index: usize,
        handle: usize,
        target: ChartPoint,
        band: &Band,
        history_right: f32,
        total: usize,
        constrain: Constrain,
    ) {
        let moved = band.scale.as_ref().and_then(|scale| {
            let drawing = drawings.items().get(drawing_index)?;
            let projected = self.projected_drawing_points(drawing, history_right, total, scale);
            let ctxt = self.draw_context(drawing, scale, band, true);
            let to = self.drawing_screen_point(target, history_right, total, scale);
            let moved: SmallVec<[Px; 6]> = drawing
                .tool
                .drag_handle(band.rect, &projected, handle, to, &ctxt, constrain)?
                .into_iter()
                .collect();
            // The target (magnet included) first, then the anchors as they were.
            let sources: SmallVec<[(Px, ChartPoint); 5]> = std::iter::once((to, target))
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
                    AnchorSnap::Pointer,
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
        drawing: &'a Mark,
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
            // Hit-tests and gestures paint nothing, so set no label.
            text: &UNMEASURED,
        }
    }

    /// What the pointer at `pos` would do to drawing `drawing_index`, as
    /// its tool announces it on hover. Same projection and context as the
    /// hit-test, so the hint sits exactly where the click lands.
    #[must_use]
    pub fn drawing_pointer_hint(
        &self,
        drawings: &Marks,
        drawing_index: usize,
        pos: Px,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<PointerHint> {
        let scale = band.scale.as_ref()?;
        let drawing = drawings
            .items()
            .get(drawing_index)
            .filter(|drawing| !drawing.locked && band::drawing_in_band(drawing, band))
            .filter(|_| drawings.is_visible(drawing_index))?;
        let projected = self.projected_drawing_points(drawing, history_right, total, scale);
        let ctxt = self.draw_context(
            drawing,
            scale,
            band,
            drawings.selected_id() == Some(drawing.id),
        );
        drawing.tool.pointer_hint(band.rect, &projected, pos, &ctxt)
    }

    /// The drawing a click of `kind` at `pos` belongs to, with its payload as the click leaves it: the one the
    /// press `picked` when it picked one, else the topmost visible object
    /// whose tool would change there and whose *drawn* anchors enclose
    /// `pos` - an outline-only rectangle's interior takes no part in the
    /// click's hit-test, but its band's click and its double click still
    /// land. The fallback reads the drawn extent, never the painted
    /// one: a band run to the chart edges would otherwise swallow the
    /// chart's own double click across its whole width. Locked objects, and
    /// those `held` names (any strategy-bound region), never take one.
    ///
    /// The change is worked out on a copy, so the caller applies it whole
    /// or not at all; hover reads [`Self::missed_click_hint`] instead.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn click_target(
        &self,
        drawings: &Marks,
        picked: Option<usize>,
        pos: Px,
        kind: ClickKind,
        band: &Band,
        history_right: f32,
        total: usize,
        held: &dyn Fn(DrawingId) -> bool,
    ) -> Option<(usize, Box<dyn DrawingPayload>)> {
        let trial = |index: usize, fallback: bool| {
            let (drawing, projected) =
                self.click_candidate(drawings, index, band, (history_right, total), held)?;
            if fallback && !PxRect::from_points(&projected).contains(pos) {
                return None;
            }
            let mut payload = drawing.payload.clone_box();
            let (tool, rect) = (drawing.tool, band.rect);
            let changed = match kind {
                ClickKind::Single => tool.click(rect, &projected, pos, payload.as_mut()),
                ClickKind::Double => tool.double_click(rect, &projected, pos, payload.as_mut()),
            };
            changed.then_some((index, payload))
        };
        match picked {
            Some(index) => trial(index, false),
            None => (0..drawings.items().len())
                .rev()
                .find_map(|index| trial(index, true)),
        }
    }

    /// The hint a click at `pos` that missed every hit-test still answers
    /// to: the topmost object whose drawn anchors enclose `pos` and whose
    /// tool says one click acts there - the drawing [`Self::click_target`]'s
    /// fallback hands that click. Read off the tool's hint, never by trying
    /// the click, so hover copies nothing.
    #[must_use]
    pub fn missed_click_hint(
        &self,
        drawings: &Marks,
        pos: Px,
        band: &Band,
        view: (f32, usize),
        held: &dyn Fn(DrawingId) -> bool,
    ) -> Option<PointerHint> {
        let scale = band.scale.as_ref()?;
        (0..drawings.items().len()).rev().find_map(|index| {
            let (drawing, projected) = self.click_candidate(drawings, index, band, view, held)?;
            if !PxRect::from_points(&projected).contains(pos) {
                return None;
            }
            let selected = drawings.selected_id() == Some(drawing.id);
            let ctxt = self.draw_context(drawing, scale, band, selected);
            let hint = drawing.tool.pointer_hint(band.rect, &projected, pos, &ctxt);
            hint.filter(|hint| *hint == PointerHint::ExtendSide)
        })
    }

    /// Drawing `index` with its anchors on screen, when a click may change
    /// it: unlocked, not `held`, visible and on `band`.
    fn click_candidate<'d>(
        &self,
        drawings: &'d Marks,
        index: usize,
        band: &Band,
        (history_right, total): (f32, usize),
        held: &dyn Fn(DrawingId) -> bool,
    ) -> Option<(&'d Mark, SmallVec<[Px; 4]>)> {
        let scale = band.scale.as_ref()?;
        let drawing = drawings.items().get(index)?;
        let refuses = drawing.locked
            || held(drawing.id)
            || !drawings.is_visible(index)
            || !band::drawing_in_band(drawing, band);
        (!refuses).then(|| {
            let projected = self.projected_drawing_points(drawing, history_right, total, scale);
            (drawing, projected)
        })
    }

    /// The handle a press at `pos` grabs, as [`Self::press_target`] answers.
    #[must_use]
    pub fn drawing_handle_at(
        &self,
        drawings: &Marks,
        pos: Px,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<(usize, usize)> {
        match self.press_target(drawings, pos, band, history_right, total)? {
            PressTarget::Handle { index, handle } => Some((index, handle)),
            PressTarget::Body { .. } => None,
        }
    }

    /// The point handle - a drawing's own ring, never a side line - a press
    /// at `pos` grabs, the selected object's first: what a held modifier
    /// yields the canvas to.
    #[must_use]
    pub fn drawing_point_handle_at(
        &self,
        drawings: &Marks,
        pos: Px,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<(usize, usize)> {
        let selected = drawings.selected();
        selected
            .into_iter()
            .chain(
                (0..drawings.items().len())
                    .rev()
                    .filter(|i| Some(*i) != selected),
            )
            .find_map(|index| {
                self.drawing_handle_in(drawings, index, pos, band, history_right, total)
                    .map(|handle| (index, handle))
            })
    }

    #[must_use]
    pub fn drawing_screen_point(
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

    /// Each visible price-band drawing with the axis heights its tool
    /// declares on `chart`, handed to `out` one height at a time.
    pub fn axis_levels(
        &self,
        drawings: &Marks,
        chart: PxRect,
        (history_right, total): (f32, usize),
        scale: &PriceScale,
        out: &mut dyn FnMut(&Mark, f32),
    ) {
        for (index, drawing) in drawings.items().iter().enumerate() {
            if !drawings.is_visible(index) || matches!(drawing.band, DrawingBand::Indicator(_)) {
                continue;
            }
            let points = self.projected_drawing_points(drawing, history_right, total, scale);
            for y in drawing.tool.axis_levels(chart, &points) {
                out(drawing, y);
            }
        }
    }

    /// Rewrite the market instants behind the selected object's anchors after
    /// a move that changed their bar positions (drag or keyboard nudge).
    ///
    /// Without this the mark moves on this chart and its shared twin stays
    /// where it was: market time is what the other panes read, so a move that
    /// does not update it has moved only half the object.
    pub fn retime_selected(&self, drawings: &mut Marks) {
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
