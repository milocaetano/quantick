//! Drawing query adapters to the shared borrowed projection.
//! Placement transitions live on PaneGestures in placement_gestures.
use eframe::egui;
use smallvec::SmallVec;

use crate::bands::Band;
use crate::chart::PriceScale;
use crate::drawings::{self, ChartPoint};

use super::ChartPane;

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
