//! Placement rules: how a new object is drafted anchor by anchor, and how an
//! object already on the chart is re-anchored, translated or shifted.
//!
//! The collection these write into, and the undo history they record through,
//! live in [`super::collection`].

use super::{
    ChartPoint, Drawing, DrawingBand, DrawingScope, DrawingTool, Drawings, Duplicated, NewDrawing,
};
// Named only by the test-side placement shorthands below.
#[cfg(test)]
use super::DrawingStyle;

impl Drawings {
    /// [`Self::place_with`] with the tool's stock look — the test-side
    /// shorthand; the app always goes through `place_with` to honour the
    /// trader's saved defaults.
    #[cfg(test)]
    pub fn place(&mut self, tool: DrawingTool, point: ChartPoint) -> bool {
        self.place_on(tool, &DrawingBand::Price, point)
    }

    /// [`Self::place`] on a named band.
    #[cfg(test)]
    pub fn place_on(&mut self, tool: DrawingTool, band: &DrawingBand, point: ChartPoint) -> bool {
        self.place_with(tool, band, point, |tool| NewDrawing {
            style: DrawingStyle::default(),
            payload: tool.default_payload(),
        })
    }

    /// [`Self::place`] with a caller-chosen look for a new draft — how the
    /// app applies the trader's saved defaults to newly created objects only.
    /// Objects that already exist are never touched by a default changing.
    pub fn place_with(
        &mut self,
        tool: DrawingTool,
        band: &DrawingBand,
        point: ChartPoint,
        new_drawing: impl FnOnce(DrawingTool) -> NewDrawing,
    ) -> bool {
        if self.draft.as_ref().is_none_or(|draft| draft.tool != tool) {
            let fresh = new_drawing(tool);
            let id = self.alloc_id();
            self.draft = Some(Drawing {
                id,
                author: None,
                name: None,
                tool,
                points: Vec::with_capacity(tool.required_points()),
                // The band the *first* anchor landed in owns the whole
                // object: an in-flight draft that changed axis halfway
                // through would have anchors in two value spaces.
                band: tool.band_for(band),
                style: fresh.style,
                locked: false,
                hidden: false,
                scope: DrawingScope::default(),
                foreign_market: false,
                off_series: false,
                payload: fresh.payload,
            });
        }
        let draft = self.draft.as_mut().expect("draft was installed above");
        draft.points.push(point);
        // A freehand tool declares no anchor count: its draft is finished by
        // the release, through `finish_draft`, never by arithmetic here.
        if tool.required_points() > 0 && draft.points.len() == tool.required_points() {
            let before = self.snapshot();
            self.items
                .push(self.draft.take().expect("draft has points"));
            self.selected = Some(self.items.len() - 1);
            // Drawing while hide-all is engaged releases it (audit M8): the
            // act of placing a new object is the strongest possible request
            // to see drawings, and a mark that vanishes on the click that
            // finished it reads as a broken tool. One undo entry with the
            // placement — undoing the object restores the hidden state too.
            self.all_hidden = false;
            self.record(before);
            true
        } else {
            false
        }
    }

    pub fn cancel_draft(&mut self) {
        self.draft = None;
    }

    /// Finish a freehand draft: what the release does for a tool whose
    /// anchor count is whatever the hand gave.
    ///
    /// A stroke of fewer than two points is a click that missed, not a
    /// drawing — it is dropped rather than stored as an invisible object the
    /// trader can neither see nor select to delete.
    pub fn finish_draft(&mut self) -> bool {
        let Some(draft) = self.draft.take() else {
            return false;
        };
        if draft.points.len() < 2 {
            return false;
        }
        let before = self.snapshot();
        self.items.push(draft);
        self.selected = Some(self.items.len() - 1);
        // Same rule as a clicked placement: drawing releases hide-all, and
        // it does so inside the one undo entry the gesture records.
        self.all_hidden = false;
        self.record(before);
        true
    }

    /// Backspace during placement: drop the last placed anchor; dropping the
    /// only one cancels the draft.
    pub fn remove_last_draft_anchor(&mut self) {
        if let Some(draft) = &mut self.draft {
            draft.points.pop();
            if draft.points.is_empty() {
                self.draft = None;
            }
        }
    }

    /// Duplicate the selected object as one undo entry: the copy lands
    /// `offset_bars` to the right, unlocked, and becomes the selection.
    ///
    /// Returns the pair so the layer above can carry whatever rides the
    /// drawing without living in it. `#[must_use]`: dropping the answer is
    /// how a copied band silently loses its bot.
    #[must_use]
    pub fn duplicate_selected(&mut self, offset_bars: f32) -> Option<Duplicated> {
        let index = self.selected.filter(|&index| index < self.items.len())?;
        let before = self.snapshot();
        let mut copy = self.items[index].clone();
        // A copy is a new object: its own identity, and never the
        // original's name — two drawings answering to "congestão 108k"
        // would make every reference ambiguous.
        copy.id = self.alloc_id();
        copy.name = None;
        for point in &mut copy.points {
            point.bar += offset_bars;
            // The offset moves the copy in *bar* space, and the instant an
            // anchor remembers is what survives a re-cut: `reanchor` puts
            // every timestamped anchor back where its own market moment
            // landed. Carrying the source's instants would therefore snap
            // the copy onto the original at the next bar-spec change,
            // replay seek, reconnect or symbol switch — two rectangles at
            // one place, and since this branch hangs a bot on the copy, two
            // bots there too. The copy is a mark the trader placed just
            // now, at no market moment of its own; that is what an anchor
            // dropped past the newest bar already carries, and it is the
            // honest reading here.
            point.time_ms = None;
        }
        copy.locked = false;
        let duplicated = Duplicated {
            source: self.items[index].id,
            copy: copy.id,
        };
        self.items.push(copy);
        self.selected = Some(self.items.len() - 1);
        self.record(before);
        Some(duplicated)
    }

    /// Re-express every anchor against a series that was cut again — a
    /// timeframe or bar-kind switch, a replay seek, a reconnect, a symbol
    /// change.
    ///
    /// A bar index means nothing across two cuts of the tape; the market
    /// instant each anchor captured at placement does, and it is the same
    /// coordinate that already carries a drawing between the panes of a tab
    /// (`docs/ux/drawing-tools-2026-08.md` §D7). So the object is not
    /// discarded and not left pointing at a stale index: it is asked where
    /// its own timestamps landed.
    ///
    /// `slot_of` answers where a market instant sits on the new series, or
    /// `None` when the series does not reach it — before its first bar, or on
    /// an instrument that never traded at that moment. Those anchors clamp to
    /// the nearest edge and the object is flagged [`Drawing::off_series`], so
    /// it fades and says so rather than pretending to sit on data.
    ///
    /// An anchor with no timestamp at all is one dropped past the newest bar,
    /// where the tape has written nothing (see `ChartPoint::time_ms`). It has
    /// no instant to look up, so it keeps its distance past the end of the
    /// series instead — which is exactly where the trader put it.
    ///
    /// Rate: once per re-cut, never per frame or per trade, over a handful of
    /// objects. The undo stacks travel with the live items for the same
    /// reason [`Self::shift_bars`] moves them — undoing later must not
    /// resurrect coordinates from a series that no longer exists.
    pub fn reanchor(
        &mut self,
        old_slots: usize,
        new_slots: usize,
        slot_of: impl Fn(i64) -> Option<f32>,
    ) {
        #[allow(clippy::cast_precision_loss)]
        let past_end = new_slots as f32 - old_slots as f32;
        let reanchor_all = |items: &mut [Drawing]| {
            for drawing in items {
                let mut off_series = false;
                for point in &mut drawing.points {
                    let Some(time) = point.time_ms else {
                        point.bar += past_end;
                        continue;
                    };
                    match slot_of(time) {
                        Some(slot) => point.bar = slot,
                        None => {
                            off_series = true;
                            point.bar = 0.0;
                        }
                    }
                }
                drawing.off_series = off_series;
            }
        };
        reanchor_all(&mut self.items);
        if let Some(draft) = self.draft.as_mut() {
            reanchor_all(std::slice::from_mut(draft));
        }
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            reanchor_all(&mut entry.items);
        }
        if let Some(baseline) = &mut self.gesture_baseline {
            reanchor_all(&mut baseline.items);
        }
    }

    /// Mark every object as belonging to a market this tab no longer shows.
    ///
    /// Called on the one transition that changes the instrument under the
    /// marks. Everything present at that moment was drawn on the old market;
    /// anything placed afterwards is on the new one and starts clean.
    ///
    /// The undo stacks travel with the live items, for the same reason
    /// [`Self::reanchor`] moves them: undoing back to a state from before the
    /// switch must not restore a mark that claims to be a level on this
    /// instrument.
    pub fn mark_market_changed(&mut self) {
        let mark = |items: &mut [Drawing]| {
            for drawing in items {
                drawing.foreign_market = true;
            }
        };
        mark(&mut self.items);
        if let Some(draft) = self.draft.as_mut() {
            mark(std::slice::from_mut(draft));
        }
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            mark(&mut entry.items);
        }
        if let Some(baseline) = &mut self.gesture_baseline {
            mark(&mut baseline.items);
        }
    }

    /// Rewrite the market instants behind one object's anchors, after a move
    /// that changed their bar positions.
    ///
    /// [`Self::translate_selected`] and the keyboard nudge shift bar indices
    /// directly; the timestamp behind each anchor is what every *other* pane
    /// reads, so leaving it stale would drag a mark on one chart and leave
    /// its shared twin standing where it used to be. Only the pane knows how
    /// to name the instant under a slot, so it hands the answers back here.
    pub fn set_times(&mut self, index: usize, times: &[Option<i64>]) {
        let Some(drawing) = self.items.get_mut(index) else {
            return;
        };
        for (point, time) in drawing.points.iter_mut().zip(times) {
            point.time_ms = *time;
        }
    }

    // There is deliberately no `clear`. A re-cut of the bars used to wipe the
    // store, on the reasoning that a bar index cannot survive one; the anchors
    // carry market time, so they are re-expressed instead
    // ([`Self::reanchor`]). The only way a drawing leaves is the trader
    // removing it — [`Self::delete_selected`] or [`Self::delete_all`], both of
    // which are undoable.

    pub fn shift_bars(&mut self, delta: isize) {
        if delta == 0 {
            return;
        }
        let delta = delta as f32;
        let shift = |items: &mut Vec<Drawing>| {
            for drawing in items {
                for point in &mut drawing.points {
                    point.bar += delta;
                }
            }
        };
        shift(&mut self.items);
        if let Some(draft) = &mut self.draft {
            for point in &mut draft.points {
                point.bar += delta;
            }
        }
        // History snapshots hold the same bar-index coordinates, so a prepend
        // shifts them too — undoing later must not re-anchor objects to bars
        // that moved underneath them.
        for entry in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            shift(&mut entry.items);
        }
        if let Some(baseline) = &mut self.gesture_baseline {
            shift(&mut baseline.items);
        }
    }

    /// Rigid translation of the selected object. Locked geometry stays put.
    pub fn translate_selected(&mut self, delta_bar: f32, delta_price: f64) {
        let Some(drawing) = self.selected_mut() else {
            return;
        };
        if drawing.locked {
            return;
        }
        for point in &mut drawing.points {
            point.bar += delta_bar;
            point.price += delta_price;
        }
    }

    /// Move one anchor of one object. Locked geometry stays put.
    pub fn move_anchor(
        &mut self,
        drawing_index: usize,
        point_index: usize,
        point: ChartPoint,
    ) -> bool {
        let Some(drawing) = self.items.get_mut(drawing_index) else {
            return false;
        };
        if drawing.locked {
            return false;
        }
        let Some(anchor) = drawing.points.get_mut(point_index) else {
            return false;
        };
        *anchor = point;
        true
    }

    /// Replace every anchor of one object at once — what a tool-owned handle
    /// drag produces, because a handle that moves a rail moves the anchors
    /// that define it together. Locked geometry stays put, and an anchor
    /// count that does not match is refused rather than reshaping the object
    /// into something the tool cannot paint.
    pub fn set_points(&mut self, drawing_index: usize, points: &[ChartPoint]) -> bool {
        let Some(drawing) = self.items.get_mut(drawing_index) else {
            return false;
        };
        if drawing.locked || drawing.points.len() != points.len() {
            return false;
        }
        drawing.points.clear();
        drawing.points.extend_from_slice(points);
        true
    }
}
