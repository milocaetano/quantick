//! Drawing placement and adapters to the shared borrowed projection.
//!
//! These entry points answer one of three questions — where in the chart is this
//! pixel (`drawing_point_at`, `anchor_time`, the magnet), what object is under
//! it (`drawing_at`, `drawing_pick_at`, `drawing_handle_at`), and what does the
//! next click do to the object being placed (`handle_drawing_placement` and the
//! placement helpers). They travel together because they share a coordinate
//! system: a fractional bar slot on the x axis and the band's own scale on the
//! y, so a drawing follows pan and zoom instead of sticking to a screen pixel.
//!
use eframe::egui;
use smallvec::SmallVec;

use crate::bands::{self, Band};
use crate::chart::PriceScale;
use crate::drawings::{self, ChartPoint, DrawingBand};
use crate::plot_area::PlotAreas;
use crate::toolrail::Tool;

use super::{
    ChartPane, DRAWING_DRAG_COMPLETES_PX, FREEHAND_MAX_POINTS, FREEHAND_MIN_STEP_PX, PaneChrome,
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
        self.drawing_projection()
            .drawing_point_at(pos, history_right, total, magnet, snap, band)
    }

    /// The candle behind a slot, the forming bar included — the one lookup
    /// every candle-reading snap shares.
    pub(super) fn candle_at_slot(&self, slot: usize) -> Option<&quantick_engine::Bar> {
        self.series_read().candle_at_slot(slot)
    }

    /// The market time behind a fractional bar slot, for anchors that may have
    /// to be re-expressed on another pane (§D7 of the drawing-tools design).
    ///
    /// Only a slot that actually holds a bar has an instant behind it: the
    /// empty space past the newest bar is future the tape has not written, and
    /// naming a time there would be an invention. `None` is the honest answer
    /// there, and it is what keeps such an anchor out of a shared drawing.
    pub(crate) fn anchor_time(&self, bar: f32) -> Option<i64> {
        self.series_read().anchor_time(bar)
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
        self.drawing_projection()
            .projected_drawing_points(drawing, history_right, total, scale)
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
        self.drawing_projection()
            .drawing_at(&self.drawings, pos, band, history_right, total)
    }

    pub(super) fn drawing_handle_at(
        &self,
        pos: egui::Pos2,
        band: &Band,
        history_right: f32,
        total: usize,
    ) -> Option<(usize, usize)> {
        self.drawing_projection()
            .drawing_handle_at(&self.drawings, pos, band, history_right, total)
    }
}
