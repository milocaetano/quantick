//! Borrowed drawing coordinates and picks, shared by input and read-only consumers.
//!
//! The composed series borrows its prefix and engine state. Neither view owns
//! a drawing store, a gesture, or the pane that supplies these disjoint fields.
use super::{
    DRAWING_ANCHOR_RADIUS_PX, DRAWING_SELECT_RADIUS_PX, MAGNET_REACH_PX, MAGNET_REACH_UNLIMITED_PX,
    magnet_price_of, snap_bar_to_tape,
};
use crate::bands::{self, Band};
use crate::chart::PriceScale;
use crate::drawings::{self, ChartPoint, DrawContext, DrawingBand};
use crate::indicators::IndicatorViews;
use crate::state::{ChartState, SpecSelector};
use crate::viewport::Viewport;
use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;
use smallvec::SmallVec;

pub(crate) struct PaneSeriesRead<'a> {
    pub(super) history_prefix: &'a [quantick_engine::Bar],
    pub(super) state: &'a ChartState,
    pub(super) spec: &'a SpecSelector,
}

impl<'a> PaneSeriesRead<'a> {
    /// How many bar slots the chart draws: the venue prefix, the closed bars
    /// the engine cut from trades, and the forming one after them.
    pub fn slots(&self) -> usize {
        self.closed_slots() + usize::from(self.state.partial().is_some())
    }

    /// Slots holding a *closed* bar — everything before the forming one.
    pub fn closed_slots(&self) -> usize {
        self.history_prefix.len() + self.state.bars().len()
    }

    /// The slot the trade-derived series starts at: the seam between venue
    /// candles and bars this app built from prints.
    pub fn seam_slot(&self) -> usize {
        self.history_prefix.len()
    }

    /// The closed bar in `slot`, from whichever series owns it.
    pub fn closed_bar(&self, slot: usize) -> Option<&'a quantick_engine::Bar> {
        self.history_prefix
            .get(slot)
            .or_else(|| self.state.bars().get(slot - self.history_prefix.len()))
    }

    /// When the bar in `slot` opened, across both series and the forming bar.
    ///
    /// Past the prefix this is the engine's own answer, shifted into the
    /// composed slot space — there is one rule for what a slot means and the
    /// prefix only moves where it starts.
    pub fn slot_open_time(&self, slot: usize) -> Option<i64> {
        match self.history_prefix.get(slot) {
            Some(bar) => Some(bar.open_time),
            None => self.state.slot_open_time(slot - self.seam_slot()),
        }
    }

    /// The slot showing market time `ms`, across both series.
    ///
    /// The seam rule keeps `open_time` non-decreasing across the join, so the
    /// question splits cleanly: anything from the first engine bar onward is
    /// the engine's own answer shifted by the prefix, anything before it is a
    /// search of the prefix.
    pub fn slot_at_time(&self, ms: i64) -> Option<usize> {
        let seam = self.seam_slot();
        if seam == 0 {
            return self.state.slot_at_time(ms);
        }
        if self
            .state
            .bars()
            .first()
            .or_else(|| self.state.partial())
            .is_some_and(|bar| bar.open_time <= ms)
        {
            return self.state.slot_at_time(ms).map(|slot| slot + seam);
        }
        let after = self
            .history_prefix
            .partition_point(|bar| bar.open_time <= ms);
        Some(after.saturating_sub(1))
    }

    /// Where a market instant sits on this pane's series, as a fractional
    /// slot — the strict answer, behind re-anchoring and every edit arriving
    /// from another pane.
    ///
    /// Bar centres, not edges: an anchor is being asked which *bar* it belongs
    /// to, and the middle of that bar is where it reads as being on it. `None`
    /// means the series does not reach the instant at all.
    ///
    /// Deliberately not what `ChartPane::reproject` uses. That one is answering a
    /// different question — *where do I paint a mark whose instant may be off
    /// my series?* — so it clamps to the nearest edge and reports the clamp,
    /// which is what the fade is drawn from. This one is asked before the
    /// store is written, where a clamp would silently move the trader's mark
    /// onto data it has nothing to do with. Same lookup, opposite answer at
    /// the edges, on purpose.
    ///
    /// Also not [`super::ChartPane::covering_slot_at_time`], which refuses the future end
    /// as well: a drawing may point past the newest bar, a fill may not.
    pub(super) fn slot_of_time(&self, time: i64) -> Option<f32> {
        // Past the newest bar first: on a time chart that space has an exact
        // clock, and asking `slot_at_time` there would clamp a future anchor
        // onto the right edge instead of letting it run on.
        if let Some(future) = self.future_slot_at_time(time) {
            return Some(future + 0.5);
        }
        // Before the first bar this pane holds. `slot_at_time` answers slot 0
        // there, which is a clamp and not a location — taking it would put the
        // anchor on a bar it has nothing to do with and say nothing about it.
        // `None` is what the off-series fade and the refused drag both read.
        if self.slot_open_time(0).is_some_and(|first| time < first) {
            return None;
        }
        let slots = self.slots();
        let slot = self.slot_at_time(time)?.min(slots.checked_sub(1)?);
        #[allow(clippy::cast_precision_loss)]
        Some(slot as f32 + 0.5)
    }

    /// Where an instant later than this pane's newest bar falls, as a
    /// fractional slot past the end. `None` unless the pane's bars run on a
    /// fixed interval — see [`Self::anchor_time`] for why a tick chart has no
    /// answer here.
    pub(super) fn future_slot_at_time(&self, time: i64) -> Option<f32> {
        let interval = self.spec.spec().time_interval_ms()?;
        let last = self.slots().checked_sub(1)?;
        let last_open = self.slot_open_time(last)?;
        let ahead = time.checked_sub(last_open)?;
        if ahead < interval {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(last as f32 + ahead as f32 / interval as f32)
    }

    /// The market time behind a fractional bar slot, for anchors that may have
    /// to be re-expressed on another pane (§D7 of the drawing-tools design).
    ///
    /// Only a slot that actually holds a bar has an instant behind it: the
    /// empty space past the newest bar is future the tape has not written, and
    /// naming a time there would be an invention. `None` is the honest answer
    /// there, and it is what keeps such an anchor out of a shared drawing.
    pub(crate) fn anchor_time(&self, bar: f32) -> Option<i64> {
        let slot = Viewport::slot_of(bar)?;
        let slots = self.slots();
        if slot < slots {
            return self.slot_open_time(slot);
        }
        // Past the newest bar. Traders draw here constantly — a channel or a
        // trend line pointing into the empty space to the right of the tape
        // is the normal way to say "if this continues". Refusing the whole
        // gesture a time would block sharing exactly where it is most used.
        //
        // On a *time* chart that space has an exact clock: the bars are one
        // fixed interval apart, so the slot after the last one is the last
        // one plus that interval. Nothing is inferred.
        //
        // On a tick or volume chart it does not: the next bar happens when
        // enough trades happen, and no elapsed time can be named for it. That
        // stays `None` — an invented timestamp is worse than a control that
        // says why it is off.
        let interval = self.spec.spec().time_interval_ms()?;
        let last = slots.checked_sub(1)?;
        let ahead = i64::try_from(slot - last).ok()?;
        self.slot_open_time(last)?
            .checked_add(ahead.checked_mul(interval)?)
    }

    /// The candle behind a slot, the forming bar included — the one lookup
    /// every candle-reading snap shares.
    pub(super) fn candle_at_slot(&self, slot: usize) -> Option<&'a quantick_engine::Bar> {
        self.closed_bar(slot)
            .or_else(|| (slot == self.closed_slots()).then(|| self.state.partial())?)
    }
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
                MAGNET_REACH_PX,
            ),
            // A time-only object has no value to snap.
            DrawingBand::AllBands => None,
            DrawingBand::Indicator(_) => {
                let view = self.indicators.visible_panes().find(|view| {
                    DrawingBand::Indicator(self.indicators.pane_key(view)) == band.key
                })?;
                bands::magnet_value_of(view, row, pointer_y, scale, MAGNET_REACH_PX)
            }
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
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: true,
                    style: drawing.style,
                    // Locked selections paint no editable affordances.
                    selected: drawings.selected() == Some(index) && !drawing.locked,
                    halo: false,
                    content_editing: false,
                };
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
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: true,
                    style: drawing.style,
                    selected: drawings.selected() == Some(index) && !drawing.locked,
                    halo: false,
                    content_editing: false,
                };
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
        let ctxt = DrawContext {
            payload: drawing.payload.as_ref(),
            anchors: &drawing.points,
            scale,
            px_per_bar: self.viewport.px_per_bar(),
            unit: band.unit(),
            primary_band: true,
            style: drawing.style,
            selected: drawings.selected() == Some(drawing_index) && !drawing.locked,
            halo: false,
            content_editing: false,
        };
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
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &drawing.points,
                scale,
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band: true,
                style: drawing.style,
                selected: true,
                halo: false,
                content_editing: false,
            };
            let to = self.drawing_screen_point(target, history_right, total, scale);
            drawing
                .tool
                .drag_handle(band.rect, &projected, handle, to, &ctxt, constrain)
        });
        let Some(moved) = moved else {
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
            })
            .collect();
        if let Some(anchors) = anchors {
            drawings.set_points(drawing_index, &anchors);
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
