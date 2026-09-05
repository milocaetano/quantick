//! Placing, picking and dragging a drawing: the arithmetic between a pointer
//! and an anchor.
//!
//! Everything here answers one of three questions — where in the chart is this
//! pixel (`drawing_point_at`, `anchor_time`, the magnet), what object is under
//! it (`drawing_at`, `drawing_pick_at`, `drawing_handle_at`), and what does the
//! next click do to the object being placed (`handle_drawing_placement` and the
//! placement helpers). They travel together because they share a coordinate
//! system: a fractional bar slot on the x axis and the band's own scale on the
//! y, so a drawing follows pan and zoom instead of sticking to a screen pixel.
//!
//! [`super::ChartPane::interact_shared`] stays in the parent deliberately. It
//! reads like a gesture, but it is cross-pane shared-mark work — its answers
//! leave in market time and price so neither pane learns the other's bar space
//! — and its only caller is `handle_navigation`, which does not move.

use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;
use smallvec::SmallVec;

use crate::bands::{self, Band};
use crate::chart::PriceScale;
use crate::drawings::{self, ChartPoint, DrawContext, DrawingBand};
use crate::plot_area::PlotAreas;
use crate::state::BarKind;
use crate::toolrail::Tool;
use crate::viewport::Viewport;

use super::{
    ChartPane, DRAWING_DRAG_COMPLETES_PX, DRAWING_SELECT_RADIUS_PX, FREEHAND_MAX_POINTS,
    FREEHAND_MIN_STEP_PX, MAGNET_REACH_PX, MAGNET_REACH_UNLIMITED_PX, PaneChrome, anchor_hit,
    magnet_price_of, snap_bar_to_tape,
};

impl ChartPane {
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
        Some(ChartPoint::at_time(bar, value, self.anchor_time(bar)))
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
        let candle = self.candle_at_slot(slot)?;
        if high { candle.high } else { candle.low }.to_f64()
    }

    /// The candle behind a slot, the forming bar included — the one lookup
    /// every candle-reading snap shares.
    pub(super) fn candle_at_slot(&self, slot: usize) -> Option<&quantick_engine::Bar> {
        self.closed_bar(slot)
            .or_else(|| (slot == self.closed_slots()).then(|| self.state.partial())?)
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
            DrawingBand::Price => {
                magnet_price_of(self.candle_at_slot(row)?, pointer_y, scale, MAGNET_REACH_PX)
            }
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
        let candle = self.candle_at_slot(slot)?;
        magnet_price_of(candle, pointer_y, scale, MAGNET_REACH_UNLIMITED_PX)
    }

    /// The market time behind a fractional bar slot, for anchors that may have
    /// to be re-expressed on another pane (§D7 of the drawing-tools design).
    ///
    /// Only a slot that actually holds a bar has an instant behind it: the
    /// empty space past the newest bar is future the tape has not written, and
    /// naming a time there would be an invention. `None` is the honest answer
    /// there, and it is what keeps such an anchor out of a shared drawing.
    pub(super) fn anchor_time(&self, bar: f32) -> Option<i64> {
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
        if self.kind != BarKind::Time || self.time_interval_ms <= 0 {
            return None;
        }
        let last = slots.checked_sub(1)?;
        let ahead = i64::try_from(slot - last).ok()?;
        self.slot_open_time(last)?
            .checked_add(ahead.checked_mul(self.time_interval_ms)?)
    }

    /// Placement consumes clicks while a drawing tool is armed, preventing a
    /// mark from also panning the chart. A completed object returns to Pointer,
    /// matching the one-shot TradingView interaction.
    pub(super) fn handle_drawing_placement(
        &mut self,
        ui: &egui::Ui,
        areas: &PlotAreas,
        bands: &[Band],
        chrome: &mut PaneChrome<'_>,
    ) -> bool {
        let magnet = chrome.toolrail.magnet();
        // Shift, read once for the whole pass: the preview, the press and the
        // release must agree about it as strictly as they agree about where
        // the pointer is. A parked hand supplies it for a run with nobody at
        // the keyboard, the same way it supplies the pointer.
        let constrain = if ui.input(|input| input.modifiers.shift) {
            drawings::Constrain::Level
        } else {
            self.gestures
                .parked_hand
                .map_or(drawings::Constrain::Free, |hand| hand.constrain)
        };
        let Some(tool) = chrome.toolrail.tool().drawing_tool() else {
            self.drawings.cancel_draft();
            self.gestures.hover = None;
            self.gestures.band_hint = None;
            self.gestures.press_position = None;
            self.gestures.press_started_empty = false;
            return false;
        };
        let history_right = self.frame.lane_divider_x.unwrap_or(areas.chart.right());
        // Every band at once: the panes are drawing surfaces now, so hovering
        // one has to read as one rather than as dead space beneath the chart.
        let surface = bands
            .iter()
            .fold(bands[0].rect, |union, band| union.union(band.rect));
        let response = ui.interact(
            surface,
            self.interaction_id("drawing_placement"),
            egui::Sense::click_and_drag(),
        );
        self.hover_pos = response.hover_pos();
        // Floating chrome is opaque to the pointer here too: a press on the
        // inspector must not drop an anchor on the canvas underneath it. The
        // Pointer path has always honoured this; placement reads the raw
        // pointer, so it has to ask the same question itself.
        let over_chrome = |ui: &egui::Ui, position: egui::Pos2| {
            ui.ctx()
                .layer_id_at(position)
                .is_some_and(|layer| layer != ui.layer_id())
        };
        let hovered = response
            .hover_pos()
            .filter(|position| !over_chrome(ui, *position))
            .and_then(|position| bands::band_at(bands, position));
        // The accent hairline the draw pass puts on the band about to receive
        // the anchor — the split view's own "your next command lands here".
        let over_pane_chrome = response
            .hover_pos()
            .is_some_and(|position| Self::pane_chrome_hit(areas, position));
        self.gestures.band_hint = hovered
            .filter(|band| band.drawable() && !over_pane_chrome)
            .map(|band| band.rect);
        // The raw pointer, never the widget's hover.
        //
        // "Is this widget the top interactable" is a different question from
        // the one placement asks, which is "is the pointer inside the drawing
        // surface, and is floating chrome on top of it". The widget's answer
        // already had to be patched once, because a dragged widget is not
        // "hovered" (egui) and the rubber band blanked for exactly the frames
        // the trader was shaping the object; the patch read the raw pointer,
        // but only while a press was down.
        //
        // So the preview and the click were reading two different sources for
        // the same fact. That is the shape of the bug this change is about,
        // and it is also why the preview could not be tested at all: under a
        // headless context the widget reports no hover ever, so the preview
        // painted the bare anchors and no test could see the shape. The press
        // path's two questions are the honest ones and they are asked here
        // now, so preview and commit cannot disagree about where the pointer
        // is — in any host.
        let preview_pos = ui
            .input(|input| input.pointer.latest_pos())
            .filter(|position| surface.contains(*position))
            .or_else(|| self.gestures.parked_hand.map(|hand| hand.position));
        self.gestures.hover = preview_pos
            .filter(|position| !over_chrome(ui, *position))
            .and_then(|position| {
                let (_, point) = self.shaped_placement(
                    tool,
                    areas,
                    bands,
                    position,
                    history_right,
                    magnet,
                    constrain,
                )?;
                Some(point)
            });
        if (response.hovered() || response.dragged()) && !over_pane_chrome {
            ui.ctx().set_cursor_icon(match hovered {
                Some(band) if band.drawable() => egui::CursorIcon::Crosshair,
                // A refusing band announces itself before the press, never by
                // swallowing the click that follows it.
                _ => egui::CursorIcon::NotAllowed,
            });
        }
        if let Some(refusal) = hovered.and_then(|band| band.refusal) {
            response.clone().on_hover_text(refusal);
        }

        let pressed_position = ui.input(|input| {
            input
                .pointer
                .primary_pressed()
                .then(|| input.pointer.interact_pos())
                .flatten()
        });
        if let Some(position) = pressed_position
            .filter(|position| surface.contains(*position) && !over_chrome(ui, *position))
            && let Some((band, point)) = self.shaped_placement(
                tool,
                areas,
                bands,
                position,
                history_right,
                magnet,
                constrain,
            )
        {
            let band = band.key.clone();
            self.gestures.press_started_empty = self.drawings.draft_len() == 0;
            self.gestures.press_position = Some(position);
            // The first anchor of a stroke also seeds its decimation, or the
            // very next frame records a second point on the same pixel and a
            // stationary click becomes a two-point "drawing".
            if tool.freehand() {
                self.gestures.freehand_last_position = Some(position);
            }
            self.place_drawing_point(tool, &band, point, chrome);
        }

        let released_position = ui.input(|input| {
            input
                .pointer
                .primary_released()
                .then(|| input.pointer.latest_pos())
                .flatten()
        });
        // A held drag, not N clicks: the press above laid the first anchor,
        // every frame the pointer stays down feeds the path, and the release
        // is what finishes the object.
        if tool.freehand() {
            if self.drawings.draft_len() > 0
                && ui.input(|input| input.pointer.primary_down())
                && let Some(position) = ui.input(|input| input.pointer.latest_pos())
                && let Some((band, position)) = self.placement_target(areas, bands, position)
                // The draft belongs to the band its first anchor landed in.
                // A hand that strays 15 px into the CVD pane mid-stroke
                // would otherwise write a CVD value into an object living on
                // the price axis — and the stroke, having no handles, could
                // only be deleted and redrawn. Points outside the draft's
                // own band are dropped; the stroke resumes when the hand
                // comes back.
                && self
                    .drawings
                    .draft()
                    .is_some_and(|draft| draft.band == tool.band_for(&band.key))
                && let Some(point) = self.drawing_point_at(
                    position,
                    history_right,
                    self.slots(),
                    magnet,
                    tool.anchor_snap(),
                    band,
                )
                // Decimate on the way in rather than simplifying afterwards.
                // A fast hand on a dense tape produces hundreds of points a
                // second, and every one of them costs a paint and a hit-test
                // on every later frame — for a shape whose whole value is
                // roughly where it is.
                && self
                    .gestures.freehand_last_position
                    .is_none_or(|last| last.distance(position) >= FREEHAND_MIN_STEP_PX)
                && self.drawings.draft_len() < FREEHAND_MAX_POINTS
            {
                self.gestures.freehand_last_position = Some(position);
                let band = band.key.clone();
                self.place_drawing_point(tool, &band, point, chrome);
            }
            if released_position.is_some() {
                self.gestures.freehand_last_position = None;
                if self.drawings.finish_draft() {
                    // Same one-shot rule the clicked tools follow.
                    if !chrome.toolrail.repeat() {
                        chrome.toolrail.arm(Tool::Pointer);
                    }
                    self.gestures.hover = None;
                }
                self.gestures.press_position = None;
                self.gestures.press_started_empty = false;
            }
            return true;
        }
        if tool.required_points() > 1
            && self.gestures.press_started_empty
            && let Some(start) = self.gestures.press_position
            && let Some(position) = released_position
            && surface.contains(position)
            && start.distance(position) >= DRAWING_DRAG_COMPLETES_PX
            && let Some((band, point)) = self.shaped_placement(
                tool,
                areas,
                bands,
                position,
                history_right,
                magnet,
                constrain,
            )
        {
            let band = band.key.clone();
            self.place_drawing_point(tool, &band, point, chrome);
        }
        if released_position.is_some() {
            self.gestures.press_position = None;
            self.gestures.press_started_empty = false;
        }
        true
    }

    /// Which band the next anchor belongs to, and where in it the pointer
    /// counts as being.
    ///
    /// A draft already down pins its band: an object with anchors in two
    /// value spaces would be a shape nobody can read. The pointer is then
    /// clamped into that band, so dragging a trend line up into the candles
    /// stretches it to the top of its own pane instead of writing a price
    /// into a CVD anchor. `None` where nothing may be placed.
    fn placement_target<'a>(
        &self,
        areas: &PlotAreas,
        bands: &'a [Band],
        position: egui::Pos2,
    ) -> Option<(&'a Band, egui::Pos2)> {
        if Self::pane_chrome_hit(areas, position) {
            return None;
        }
        let pinned = self
            .drawings
            .draft()
            .filter(|draft| draft.band != DrawingBand::AllBands)
            .and_then(|draft| bands.iter().find(|band| band.key == draft.band));
        let band = match pinned {
            Some(band) => band,
            None => bands::band_at(bands, position)?,
        };
        if !band.drawable() {
            return None;
        }
        let clamped = egui::pos2(
            position.x,
            position.y.clamp(band.rect.top(), band.rect.bottom()),
        );
        Some((band, clamped))
    }

    /// Where an anchor dropped at `position` really lands: the band that will
    /// own it, and the chart point it takes once the tool has had its say
    /// about an anchor it is still shaping
    /// ([`drawings::DrawingTool::pending_anchor`]).
    ///
    /// The preview, the press and the release all come through here. They
    /// used to compute their point apart from one another, and that is how a
    /// channel could be previewed as a corridor and then born as a line: the
    /// draft preview completed the geometry with the hovered anchor while the
    /// click that committed it read the raw pointer. One door, so the object
    /// a click creates is the one that was under the cursor when it was
    /// clicked.
    ///
    /// The shaped point is deliberately *not* re-clamped into the band. A
    /// tool floors a collapsed shape by pixels, so the anchor can end a hair
    /// outside the band it was aimed at — clamping it back would hand the
    /// degenerate case straight back to the trader, which is the whole thing
    /// being fixed.
    #[allow(clippy::too_many_arguments)]
    fn shaped_placement<'a>(
        &self,
        tool: drawings::DrawingTool,
        areas: &PlotAreas,
        bands: &'a [Band],
        position: egui::Pos2,
        history_right: f32,
        magnet: bool,
        constrain: drawings::Constrain,
    ) -> Option<(&'a Band, ChartPoint)> {
        let (band, position) = self.placement_target(areas, bands, position)?;
        let total = self.slots();
        // A tool shapes in the space it paints in, so the anchors already
        // down are handed over projected. A draft belonging to another tool
        // is not this tool's draft — `place_with` will start a fresh one, so
        // there is nothing shaped yet.
        //
        // A freehand draft is skipped, and the reason is runtime rather than
        // taste. This runs up to three times a frame while a tool is armed
        // (hover, press, release), and a pencil stroke holds up to
        // `FREEHAND_MAX_POINTS` anchors against a `SmallVec` that keeps four
        // inline — so projecting one would allocate and walk the whole stroke
        // every frame, during exactly the gesture where the hand is moving
        // fastest, to hand it to a port that has no anchor to shape: a
        // freehand tool declares no anchor count, and its draft is finished by
        // the release, never by a click. Every other tool's draft is three
        // anchors at most, which stays inline and allocates nothing.
        let shaped = match (self.drawings.draft(), band.scale.as_ref()) {
            (Some(draft), Some(scale)) if draft.tool == tool && !tool.freehand() => {
                let placed = self.projected_drawing_points(draft, history_right, total, scale);
                tool.pending_anchor(&placed, position, constrain)
            }
            _ => position,
        };
        let point = self.drawing_point_at(
            shaped,
            history_right,
            total,
            magnet,
            tool.anchor_snap(),
            band,
        )?;
        Some((band, point))
    }

    pub(super) fn place_drawing_point(
        &mut self,
        tool: drawings::DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        chrome: &mut PaneChrome<'_>,
    ) {
        // A new object starts from whatever the trader told the app to
        // remember for this tool — assembled in one place, so the click path
        // and the scripted one open the same object. Existing objects are
        // never touched by that choice.
        let presets = chrome.presets;
        let completed = self.drawings.place_with(tool, band, point, |tool| {
            drawings::new_drawing_from_defaults(presets, tool)
        });
        if completed {
            // One-shot by default; the toolbox repeat pin keeps the tool
            // armed for the next object.
            if !chrome.toolrail.repeat() {
                chrome.toolrail.arm(Tool::Pointer);
            }
            // A tool whose content is words asks for the caret, not for a
            // panel — see `PaneChrome::begin_text_edit`.
            //
            // The object stands down here rather than waiting for the host to
            // notice next frame: the placement happens *inside* the canvas
            // pass, so a note placed by click would otherwise paint its grey
            // placeholder under the field that opens over it, for the one
            // frame between the two.
            if tool.holds_text() {
                self.gestures.content_editing = self.drawings.selected();
                *chrome.begin_text_edit = true;
            }
            self.gestures.hover = None;
        }
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
    /// bands are not candidates at all — see [`Self::drawing_in_band`].
    pub(super) fn drawing_at(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        let scale = band.scale.as_ref()?;
        self.drawings
            .items()
            .iter()
            .enumerate()
            .rev()
            .filter(|(index, drawing)| {
                self.drawings.is_visible(*index) && bands::drawing_in_band(drawing, band)
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
                    selected: self.drawings.selected() == Some(index),
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
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        let scale = band.scale.as_ref()?;
        let hits: Vec<usize> = (0..self.drawings.items().len())
            .rev()
            .filter(|&index| self.drawings.is_visible(index))
            .filter(|&index| bands::drawing_in_band(&self.drawings.items()[index], band))
            .filter(|&index| {
                let drawing = &self.drawings.items()[index];
                let projected = self.projected_drawing_points(drawing, history_right, total, scale);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &drawing.points,
                    scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: true,
                    style: drawing.style,
                    selected: self.drawings.selected() == Some(index),
                    halo: false,
                    content_editing: false,
                };
                drawing
                    .tool
                    .hit_test(band.rect, &projected, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
            })
            .collect();
        match self
            .drawings
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
        drawing_index: usize,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        if !self.drawings.is_visible(drawing_index) {
            return None;
        }
        let scale = band.scale.as_ref()?;
        let drawing = self
            .drawings
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
            selected: self.drawings.selected() == Some(drawing_index),
            halo: false,
            content_editing: false,
        };
        anchor_hit(&drawing.tool.handles(band.rect, &projected, &ctxt), pos)
    }

    /// What a pointer at `pos` is on: a drawing's handle first, then its
    /// body. One function, so the press and the click that follows it can
    /// never answer differently — grabbing a handle *is* clicking the object,
    /// and the handle radius is the wider of the two.
    pub(super) fn drawing_pick_at(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<usize> {
        self.drawing_handle_at(pos, band, history_right, total)
            .map(|(drawing_index, _)| drawing_index)
            .or_else(|| self.drawing_at(pos, band, history_right, total))
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
        &mut self,
        drawing_index: usize,
        handle: usize,
        target: ChartPoint,
        band: &Band,
        history_right: f32,
        total: usize,
        constrain: drawings::Constrain,
    ) {
        let moved = band.scale.as_ref().and_then(|scale| {
            let drawing = self.drawings.items().get(drawing_index)?;
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
            self.drawings.move_anchor(drawing_index, handle, target);
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
            self.drawings.set_points(drawing_index, &anchors);
        }
    }

    pub(super) fn drawing_handle_at(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<(usize, usize)> {
        let selected = self.drawings.selected();
        if let Some(drawing_index) = selected
            && let Some(handle) =
                self.drawing_handle_in(drawing_index, pos, band, history_right, total)
        {
            return Some((drawing_index, handle));
        }
        (0..self.drawings.items().len())
            .rev()
            .filter(|drawing_index| Some(*drawing_index) != selected)
            .find_map(|drawing_index| {
                self.drawing_handle_in(drawing_index, pos, band, history_right, total)
                    .map(|handle| (drawing_index, handle))
            })
    }
}
