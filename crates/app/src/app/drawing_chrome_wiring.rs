//! The window side of the drawing chrome (§8): inspector, manager, notes.
//!
//! [`crate::surfaces::drawing_chrome`] draws; this file assembles what it
//! draws from and applies what it returns. [`drawing_env`] and
//! [`DrawingRead`] are the seam — free items rather than methods, so a
//! caller can hand the surface a disjoint borrow of the window.
//!
//! Editing a drawing *on the canvas* is [`super::drawing_input`]; this is
//! the chrome around it.

use std::time::Instant;

use eframe::egui;

use crate::drawings::{self, DeleteOutcome, DrawingAuthor};
use crate::pane::{DRAWING_ANCHOR_RADIUS_PX, PaneSide};
use crate::tab::Tab;
use crate::toolrail::{Tool, ToolRail};

use super::{DEMO_VISIBLE_SLOTS, QuantickApp};

/// The slice the drawing chrome reads, assembled from the pieces of the
/// application it is allowed to see.
///
/// A free function rather than a method for the reason
/// [`super::frame::indicator_preview_area`] is one: every caller has already
/// split `QuantickApp` into disjoint borrows to draw a surface through `&mut`,
/// and a method would want the whole of `self` back. That the compiler insists
/// on the split is the port working.
///
/// `manager_rows` is handed in rather than gathered here. Only one of the two
/// call sites draws the list, and building a row per object for the site that
/// does not would be a per-frame allocation for a window nobody is looking at.
fn drawing_env<'a>(
    tab: &'a Tab,
    toolrail: &ToolRail,
    presets: &'a drawings::presets::PresetStore,
    read: DrawingRead<'a>,
) -> crate::surfaces::DrawingEnv<'a> {
    let side = tab.drawing_side();
    let pane = tab.pane(side);
    let selected = pane.drawings.selected().and_then(|index| {
        pane.drawings
            .items()
            .get(index)
            .map(|drawing| crate::surfaces::drawing_chrome::SelectedDrawing { index, drawing })
    });
    crate::surfaces::DrawingEnv {
        selected,
        chart_area: pane.last_chart_area,
        focused_chart_area: tab.focused_pane().last_chart_area,
        lane_divider_x: pane.last_lane_divider_x,
        auto_range: pane.last_auto_range,
        selected_bbox: read.selected_bbox,
        selected_band: read.selected_band,
        tab: tab.id,
        side,
        drawing_tool_armed: matches!(toolrail.tool(), Tool::Drawing(_)),
        toolbox_dock: toolrail.dock(),
        authored_objects: read.authored_objects,
        manager_rows: read.manager_rows,
        presets,
    }
}

/// The parts of [`drawing_env`] that cost something to work out, gathered by
/// the caller so each pass pays only for what it draws.
///
/// Three fields and three prices. Projecting the selection's painted bounds
/// walks its anchors through the price scale; naming its band formats a
/// string; counting an assistant's objects walks every pane of every tab. All
/// three are per-frame while a selection is on screen, which is why the pass
/// that only runs the capture hooks gathers none of them and says so.
#[derive(Default)]
struct DrawingRead<'a> {
    selected_bbox: Option<egui::Rect>,
    selected_band: Option<String>,
    authored_objects: usize,
    manager_rows: &'a [crate::surfaces::drawing_chrome::ManagerRow],
}

impl QuantickApp {
    /// One delete command for every trigger (inspector button, keyboard,
    /// manager). A locked object raises the confirmation next to the trigger
    /// instead of deleting; a landed delete raises the Undo toast.
    pub(super) fn request_delete_selected(&mut self, now: Instant) {
        // Read the name before the object is gone. "Drawing deleted" makes
        // the undo useless on a crowded chart: the trader has to know *what*
        // they lost to know whether they want it back — and the context bar
        // deletes on a bare glyph, so the toast is what pays for that.
        let doomed = self.drawing_pane().drawings.selected().and_then(|index| {
            let drawing = self.drawing_pane().drawings.items().get(index)?;
            // The trader's own name when one was given; the tool name
            // otherwise — a positional index would be noise on an object
            // that no longer has a position.
            let label = drawing
                .name
                .clone()
                .unwrap_or_else(|| drawing.tool.name().to_owned());
            Some((drawing.id, label))
        });
        match self.drawing_pane_mut().drawings.delete_selected(false) {
            DeleteOutcome::Deleted => {
                self.surfaces.drawing_chrome.set_delete_confirm(false);
                // The instance dies with its drawing, immediately — not on
                // the next closed bar, which a quiet tape may never bring.
                if let Some((id, _)) = &doomed {
                    self.drawing_pane_mut().remove_strategy_for_drawing(*id);
                }
                let name = doomed.map(|(_, label)| label);
                let message = name.map_or_else(
                    || "Drawing deleted.".to_owned(),
                    |name| format!("{name} deleted."),
                );
                self.surfaces.toast.note_with_undo(message, now);
            }
            DeleteOutcome::NeedsConfirmation => {
                self.surfaces.drawing_chrome.set_delete_confirm(true);
            }
            DeleteOutcome::NothingSelected => {}
        }
    }

    /// Record one edit gesture as the single undo entry it earned, on the pane
    /// it started on.
    ///
    /// That pane's tab may have been closed under the gesture, in which case
    /// the object it described is gone with it.
    pub(super) fn record_drawing_edit(
        &mut self,
        tab_id: u64,
        side: PaneSide,
        index: usize,
        before: drawings::Drawing,
    ) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.pane_mut(side).drawings.record_edit_of(index, before);
        }
    }

    /// Put the caret in a note, on the chart — the one call that opens the
    /// editor, whether a placement, a double click or a script asked for it.
    ///
    /// The surface decides whether the caret is allowed: an object that holds
    /// no words, or a locked one, refuses it, because an editor that opened and
    /// then dropped every keystroke would be worse than none. The store command
    /// and the per-pane stand-down are the host's, so they happen here.
    pub fn begin_inline_text_edit(&mut self, index: usize) -> bool {
        let Self {
            tabs,
            active_tab,
            surfaces,
            ..
        } = self;
        let tab = &tabs[*active_tab];
        let side = tab.drawing_side();
        let Some(drawing) = tab.pane(side).drawings.items().get(index) else {
            return false;
        };
        if !surfaces
            .drawing_chrome
            .begin_inline_text_edit(tab.id, side, index, drawing)
        {
            return false;
        }
        self.drawing_pane_mut().drawings.select(Some(index));
        self.sync_content_editing();
        true
    }

    /// Close the editor, keeping whatever was typed and recording it as the one
    /// edit it was — on the pane the note actually lives on, which is not
    /// necessarily the one in front when it closes.
    pub(super) fn end_inline_text_edit(&mut self) {
        if let Some(edit) = self.surfaces.drawing_chrome.end_inline_text_edit() {
            self.record_drawing_edit(edit.tab, edit.side, edit.index, edit.before);
        }
        self.sync_content_editing();
    }

    /// Tell every pane whether one of its objects is having its content typed
    /// somewhere else on screen, so exactly one object anywhere stands down.
    ///
    /// Every pane, not just the one in front: the flag is what suppresses the
    /// object's own painting, and a pane left holding a stale index would keep
    /// a note invisible for the rest of the session with no way back.
    pub(super) fn sync_content_editing(&mut self) {
        let editing = self.surfaces.drawing_chrome.content_editing_target();
        for tab in &mut self.tabs {
            let target = editing
                .filter(|(id, _, _)| *id == tab.id)
                .map(|(_, side, index)| (side, index));
            tab.set_content_editing(target);
        }
    }

    /// Which note is being typed on the chart right now — what a second
    /// operator reads to know the keyboard belongs to an object.
    #[must_use]
    pub fn inline_text_editing(&self) -> Option<usize> {
        self.surfaces.drawing_chrome.inline_text_editing()
    }

    /// The rows the object manager lists.
    ///
    /// A row's facts come from the drawing, the pane's band registry and the
    /// tab's layout, and assembling them here is what keeps the manager from
    /// needing all three. Built only while the window is open, like the market
    /// dialog's list of open markets: a dozen short strings once a frame, on a
    /// window that is shut the rest of the session.
    fn drawing_manager_rows(&self) -> Vec<crate::surfaces::drawing_chrome::ManagerRow> {
        let pane = self.drawing_pane();
        let selected = pane.drawings.selected();
        let focused = self.focused_pane();
        pane.drawings
            .items()
            .iter()
            .enumerate()
            .map(
                |(index, drawing)| crate::surfaces::drawing_chrome::ManagerRow {
                    name: drawing.display_label(index),
                    selected: selected == Some(index),
                    locked: drawing.locked,
                    hidden: drawing.hidden,
                    shared: drawing.scope == drawings::DrawingScope::AllCharts,
                    off_series: drawing.off_series,
                    foreign_market: drawing.foreign_market,
                    author: drawing.author.as_ref().map(DrawingAuthor::label),
                    band: focused.band_label(drawing),
                },
            )
            .collect()
    }

    /// Where the selected object is painted, in screen points. The chrome
    /// cannot work this out for itself: it needs the viewport and the price
    /// scale the host owns.
    ///
    /// Two separate answers rather than one pair, because they cost different
    /// things and not every pass wants both — this one walks the object's
    /// anchors through the price scale. Nothing is projected while nothing is
    /// selected, which is every frame of an ordinary session.
    fn selected_drawing_bbox(&self) -> Option<egui::Rect> {
        let pane = self.drawing_pane();
        let index = pane.drawings.selected()?;
        let chart = pane.last_chart_area?;
        self.drawing_bbox_on_screen(chart, index)
    }

    /// What the band the selected object lives on is called, for the
    /// inspector's title. `None` on the price band, where a suffix on every
    /// object would be noise. Formats a string, so it is asked for only by a
    /// pass that shows the title.
    fn selected_drawing_band(&self) -> Option<String> {
        let pane = self.drawing_pane();
        let index = pane.drawings.selected()?;
        self.focused_pane()
            .band_label(pane.drawings.items().get(index)?)
            .chip()
    }

    /// The docked inspector.
    ///
    /// Its own call site because a `SidePanel` has to be declared *before* the
    /// central canvas — the canvas pays its width, and a panel declared after
    /// it would overlay the chart instead of docking beside it.
    pub(super) fn draw_pinned_inspector(&mut self, ctx: &egui::Context, now: Instant) {
        if !self.surfaces.drawing_chrome.inspector_pinned() {
            return;
        }
        // No painted bounds: a docked panel has no placement rule to keep
        // clear of the object, so the projection the floating one needs is not
        // gathered here. The band still is — it is in the title.
        let read = DrawingRead {
            selected_band: self.selected_drawing_band(),
            ..DrawingRead::default()
        };
        let ask = self.draw_chrome_pass(ctx, read, false);
        self.apply_drawing_chrome(ask, now);
    }

    /// The four floating pieces, registered after the canvas so they stay in
    /// front of the chart they are anchored to.
    pub(super) fn draw_drawing_chrome(&mut self, ctx: &egui::Context, now: Instant) {
        let manager_open = self.surfaces.drawing_chrome.manager_open();
        let rows = if manager_open {
            self.drawing_manager_rows()
        } else {
            Vec::new()
        };
        // The band name goes in the inspector's title and nowhere else, so
        // it is formatted only when one of the two inspector hosts is on
        // screen. A selection alone raises the context bar, which never shows
        // it — and `band_label` scans the pane's indicator views and `chip`
        // allocates, every frame, for a value nothing would read.
        let inspector_showing = self.surfaces.drawing_chrome.inspector_open()
            || self.surfaces.drawing_chrome.inspector_pinned();
        let read = DrawingRead {
            selected_bbox: self.selected_drawing_bbox(),
            selected_band: inspector_showing
                .then(|| self.selected_drawing_band())
                .flatten(),
            // Counted only for the window that offers to take them back, and
            // over every tab: an object an assistant placed on another chart
            // still belongs in that count.
            authored_objects: if manager_open {
                Self::authored_object_count(&self.tabs)
            } else {
                0
            },
            manager_rows: &rows,
        };
        let ask = self.draw_chrome_pass(ctx, read, true);
        self.apply_drawing_chrome(ask, now);
    }

    /// One split for both call sites. `floating` picks which of the surface's
    /// two entry points runs.
    fn draw_chrome_pass(
        &mut self,
        ctx: &egui::Context,
        read: DrawingRead<'_>,
        floating: bool,
    ) -> crate::surfaces::drawing_chrome::DrawingChromeAsk {
        // Split into disjoint borrows, like the surface registry above: the
        // chrome is drawn through `&mut` while what it reads is borrowed from
        // the rest of the application.
        let Self {
            surfaces,
            tabs,
            active_tab,
            toolrail,
            drawing_presets,
            ..
        } = self;
        let env = drawing_env(&tabs[*active_tab], toolrail, drawing_presets, read);
        if floating {
            surfaces.drawing_chrome.draw_floating(ctx, &env)
        } else {
            surfaces.drawing_chrome.draw_pinned_panel(ctx, &env)
        }
    }

    /// The `QUANTICK_TEXT_NOTE` hook's other half: place a note in the middle
    /// of the window and open its editor, through the same two calls a click
    /// makes.
    ///
    /// Here rather than in the surface because every line of it is the host's:
    /// where the visible window is, what the tape last closed at, and the saved
    /// defaults a fresh object opens with.
    pub(super) fn place_text_note(&mut self) -> bool {
        let Some(tool) = drawings::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.holds_text())
        else {
            return false;
        };
        let point = {
            let pane = self.drawing_pane();
            let slots = pane.slots();
            if pane.last_chart_area.is_none() || slots == 0 {
                // No laid-out pane yet, and nothing to place against. The ask
                // stands and the next frame tries again.
                return false;
            }
            let close = pane
                .closed_bar(slots.saturating_sub(1))
                .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
                .unwrap_or(1.0);
            let centre = pane
                .last_auto_range
                .filter(|(lo, hi)| hi > lo)
                .map_or(close, |(lo, hi)| (lo + hi) / 2.0);
            let visible = DEMO_VISIBLE_SLOTS.min(slots);
            let slot = (slots - visible / 2).min(slots.saturating_sub(1));
            drawings::ChartPoint::at_time(slot as f32 + 0.5, centre, pane.slot_open_time(slot))
        };
        // Through the same door the click path uses, saved defaults and all —
        // and on the same pane every drawing surface reads, so the index the
        // editor opens on is the object this just placed.
        let fresh = drawings::new_drawing_from_defaults(&self.drawing_presets, tool);
        let placed = self.drawing_pane_mut().drawings.place_with(
            tool,
            &drawings::DrawingBand::Price,
            point,
            |_| fresh,
        );
        if placed && let Some(index) = self.drawing_pane().drawings.selected() {
            self.begin_inline_text_edit(index);
        }
        placed
    }

    /// The selected object's screen bounding box, expanded by the anchor
    /// radius — the rectangle the inspector must not cover. Projected on the
    /// focused pane, which is where the selection lives.
    pub(super) fn drawing_bbox_on_screen(
        &self,
        chart: egui::Rect,
        index: usize,
    ) -> Option<egui::Rect> {
        let total = self.drawing_pane().slots();
        let auto = self.drawing_pane().last_auto_range?;
        let scale = self.drawing_pane().price_view.scale(
            auto,
            self.drawing_pane().last_chart_top,
            self.drawing_pane().last_chart_top + self.drawing_pane().last_chart_height,
        );
        let history_right = self
            .drawing_pane()
            .last_lane_divider_x
            .unwrap_or(chart.right());
        let drawing = self.drawing_pane().drawings.items().get(index)?;
        let points =
            self.drawing_pane()
                .projected_drawing_points(drawing, history_right, total, &scale);
        let first = points.first()?;
        let mut bbox = egui::Rect::from_min_max(*first, *first);
        for point in &points {
            bbox.extend_with(*point);
        }
        // What the tool paints, which is not always where its anchors are: a
        // fixed-range profile anchors at one price and covers the axis. Every
        // popup that keeps clear of an object reads this rectangle, so asking
        // the anchors alone is what let a panel land in the middle of a
        // profile while believing it had walked around it.
        let bbox = drawing.tool.painted_bounds(bbox, chart);
        Some(bbox.expand(DRAWING_ANCHOR_RADIUS_PX))
    }
}
