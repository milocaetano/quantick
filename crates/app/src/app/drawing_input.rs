//! The drawing tools' keyboard and chrome input.
//!
//! Two halves of one gesture loop: [`super::QuantickApp::handle_drawing_keys`]
//! turns key presses over the canvas into edits of the focused pane's
//! objects, and `apply_drawing_chrome` turns the answers the drawing chrome
//! surfaces give back — the object manager's rows, the inspector's fields,
//! the context bar's buttons — into the same edits. Both run per frame and
//! both write through the pane rather than around it.

use std::time::Instant;

use eframe::egui;

use crate::drawings::DeleteOutcome;
use crate::toolrail::Tool;

use super::QuantickApp;

impl QuantickApp {
    /// Keyboard grammar for drawings. Any focused widget wins: while an input
    /// owns the keyboard, chart shortcuts stay suspended.
    pub(super) fn handle_drawing_keys(&mut self, ctx: &egui::Context, now: Instant) {
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }
        struct DrawingKeys {
            escape: bool,
            delete: bool,
            backspace: bool,
            undo: bool,
            redo: bool,
            lock: bool,
            hide: bool,
            duplicate: bool,
            nudge_bars: f32,
            nudge_px: f32,
        }
        let keys = ctx.input(|input| {
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            let alt = input.modifiers.alt;
            // Shift turns a nudge into ten steps (UX spec).
            let step = if shift { 10.0 } else { 1.0 };
            let horizontal = f32::from(input.key_pressed(egui::Key::ArrowRight))
                - f32::from(input.key_pressed(egui::Key::ArrowLeft));
            let vertical = f32::from(input.key_pressed(egui::Key::ArrowUp))
                - f32::from(input.key_pressed(egui::Key::ArrowDown));
            DrawingKeys {
                escape: input.key_pressed(egui::Key::Escape),
                delete: input.key_pressed(egui::Key::Delete),
                backspace: input.key_pressed(egui::Key::Backspace),
                undo: command && !shift && input.key_pressed(egui::Key::Z),
                redo: (command && input.key_pressed(egui::Key::Y))
                    || (command && shift && input.key_pressed(egui::Key::Z)),
                lock: alt && input.key_pressed(egui::Key::L),
                hide: alt && input.key_pressed(egui::Key::H),
                duplicate: command && input.key_pressed(egui::Key::D),
                nudge_bars: horizontal * step,
                nudge_px: vertical * step,
            }
        });
        // The escape stack: rail drag → paper interaction → pending
        // confirmation → draft → selection → Pointer, one layer per press.
        // Paper trading's armed placement / grabbed line reads Escape here,
        // in the single stack — what keeps one press from firing two
        // cancels at once.
        //
        // The context bar's parked position is deliberately *not* a layer of
        // this stack. It is a preference, not a gesture left half-finished:
        // a trader who moved the bar out of the way wants it to stay out of
        // the way, and Escape is a key they press many times an hour to drop
        // a selection. Spending it on the parked point would undo, several
        // times a session and without being asked, the very thing parking it
        // was for. The way back is the grip's double-click, which is aimed at
        // the bar and at nothing else — and, for an operator with no hand on
        // the mouse, `ContextBar::clear_manual`, which is what that
        // double-click calls rather than reimplements.
        if keys.escape {
            if self.toolrail.drag_active() {
                // The rail consumes this Esc to abort its dock drag.
            } else if self.active_tab_mut().paper.cancel_interaction() {
                // An armed order placement or a grabbed order line was
                // dropped; nothing else loses state on this press. Only the
                // active tab can have one in flight — a background tab has
                // no pointer over it to arm or grab with.
            } else if self.surfaces.drawing_chrome.delete_confirm() {
                self.surfaces.drawing_chrome.set_delete_confirm(false);
            } else if self.inline_text_editing().is_some() {
                // A note being typed is its own layer, and it has to be one:
                // egui clears widget focus at the top of the frame Escape
                // arrives on, so by the time this stack runs the editor no
                // longer looks focused — and without a layer of its own the
                // press fell straight through to "drop the selection",
                // taking away the context bar for the note just written.
                self.end_inline_text_edit();
            } else if self.drawing_pane().drawings.draft().is_some() {
                self.drawing_pane_mut().drawings.cancel_draft();
                self.toolrail.arm(Tool::Pointer);
            } else if self.drawing_pane().drawings.selected().is_some() {
                self.drawing_pane_mut().drawings.select(None);
            } else {
                self.toolrail.arm(Tool::Pointer);
            }
        }
        if self.drawing_pane().drawings.draft().is_some() {
            // During placement the delete keys belong to the draft workflow:
            // Backspace steps back one anchor.
            if keys.backspace {
                self.drawing_pane_mut().drawings.remove_last_draft_anchor();
            }
        } else if keys.delete || keys.backspace {
            self.request_delete_selected(now);
        }
        if keys.undo {
            let pane = self.drawing_pane_mut();
            pane.drawings.undo();
            // Undoing a rectangle's placement takes its drawing away from
            // any armed instance without passing through the removal
            // funnel; the sweep keeps a resting bot order from outliving
            // its badge (redo below, and delete-all, share the risk).
            pane.sweep_strategy_orphans();
        }
        if keys.redo {
            let pane = self.drawing_pane_mut();
            pane.drawings.redo();
            pane.sweep_strategy_orphans();
        }
        if keys.lock
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let locked = self.drawing_pane().drawings.items()[index].locked;
            self.drawing_pane_mut()
                .drawings
                .set_selected_locked(!locked);
        }
        if keys.hide
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let hidden = self.drawing_pane().drawings.items()[index].hidden;
            self.drawing_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if keys.duplicate {
            self.duplicate_selected_drawing();
        }
        if (keys.nudge_bars != 0.0 || keys.nudge_px != 0.0)
            && self.drawing_pane().drawings.selected().is_some()
        {
            // Arrows write the same honest chart coordinates a drag does:
            // one bar per horizontal step, one pixel's worth of *that
            // object's own axis* per vertical step. Each press lands as one
            // undo entry. Reading the candles' scale for an object on an
            // indicator band would nudge a CVD level by a quantity of price —
            // a wrong number arriving through the one gesture that exists for
            // precision. Asked of the pane that owns the mark, which for a
            // shared one is not always the focused pane.
            let price_per_px = self.drawing_pane().selected_value_per_px().unwrap_or(0.0);
            self.drawing_pane_mut().drawings.begin_gesture();
            self.drawing_pane_mut()
                .drawings
                .translate_selected(keys.nudge_bars, f64::from(keys.nudge_px) * price_per_px);
            // Same rule as the drag: the instants behind the anchors move
            // with them, or the object's shared twin stays behind.
            self.drawing_pane_mut().retime_selected();
            self.drawing_pane_mut().drawings.commit_gesture();
        }
    }

    /// Carry out what the drawing chrome asked for.
    ///
    /// One applier for all four pieces, in the order the trunk applied them
    /// when each drew itself: the edit lands before the gesture that coalesces
    /// it is committed, and both land before anything that can delete the
    /// object they describe.
    pub(super) fn apply_drawing_chrome(
        &mut self,
        ask: crate::surfaces::drawing_chrome::DrawingChromeAsk,
        now: Instant,
    ) {
        if let Some(edited) = ask.edited {
            // Through the selection, never `items_mut`: that hatch is
            // documented for derived-state refresh only, and a style or a
            // note's words take part in payload equality — writing them through
            // it would let an unrelated in-flight gesture swallow this edit.
            if let Some(drawing) = self.drawing_pane_mut().drawings.selected_mut() {
                *drawing = *edited;
            }
        }
        if let Some(edit) = ask.commit_edit_gesture {
            self.record_drawing_edit(edit.tab, edit.side, edit.index, edit.before);
        }
        if ask.toggle_selected_hidden
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let hidden = self.drawing_pane().drawings.items()[index].hidden;
            self.drawing_pane_mut()
                .drawings
                .set_selected_hidden(!hidden);
        }
        if ask.toggle_selected_locked
            && let Some(index) = self.drawing_pane().drawings.selected()
        {
            let locked = self.drawing_pane().drawings.items()[index].locked;
            self.drawing_pane_mut()
                .drawings
                .set_selected_locked(!locked);
        }
        if ask.request_delete {
            self.request_delete_selected(now);
        }
        if ask.cancel_delete {
            // After the request, never before: a frame carrying both must end
            // with the prompt gone, and a delete on a locked object raises it.
            self.surfaces.drawing_chrome.set_delete_confirm(false);
        }
        if ask.force_delete {
            let doomed = {
                let pane = self.drawing_pane();
                pane.drawings
                    .selected()
                    .and_then(|index| pane.drawings.items().get(index))
                    .map(|drawing| drawing.id)
            };
            if self.drawing_pane_mut().drawings.delete_selected(true) == DeleteOutcome::Deleted {
                if let Some(id) = doomed {
                    self.drawing_pane_mut().remove_strategy_for_drawing(id);
                }
                self.surfaces.toast.note_with_undo("Drawing deleted.", now);
            }
        }
        if ask.duplicate {
            self.duplicate_selected_drawing();
        }
        if let Some(saved) = ask.saved_default {
            // Nothing to undo: this changed a preference, not the chart.
            self.surfaces.toast.note(saved.message(), now);
        }
        for write in ask.presets {
            write.apply_to(&mut self.drawing_presets);
        }
        if ask.sweep_authored {
            let removed = self.remove_every_authored_object();
            if removed > 0 {
                self.surfaces
                    .toast
                    .note_with_undo(format!("{removed} object(s) placed for you removed."), now);
            }
        }
        if ask.delete_all {
            let pane = self.drawing_pane_mut();
            let deleted = pane.drawings.delete_all();
            // Every armed instance just lost its drawing at once; sweep them
            // now so no resting bot order outlives its badge.
            pane.sweep_strategy_orphans();
            if deleted > 0 {
                self.surfaces
                    .toast
                    .note_with_undo("All drawings deleted.", now);
            }
        }
        if let Some(index) = ask
            .manager_select
            .filter(|index| *index < self.drawing_pane().drawings.items().len())
        {
            self.drawing_pane_mut().drawings.select(Some(index));
            // Centre the viewport on the object's bar span.
            let slots = self.drawing_pane().slots();
            if let Some(chart) = self.drawing_pane().frame.chart_area {
                let points = &self.drawing_pane().drawings.items()[index].points;
                if !points.is_empty() {
                    let mid =
                        points.iter().map(|point| point.bar).sum::<f32>() / points.len() as f32;
                    self.drawing_pane_mut()
                        .viewport
                        .center_on_bar(mid, chart.width(), slots);
                }
            }
        }
        // Through `get`, never `[]`: the rows were snapshotted before any of
        // the four pieces drew, and a destructive ask applied above — delete
        // all, the assistant sweep, a confirmed delete — can have shortened
        // the list under an index that was valid when the row was clicked. The
        // store's own setters are already bounds-safe; these two reads were
        // the only raw ones left.
        if let Some(hidden) = ask.manager_toggle_hidden.and_then(|index| {
            Some((
                index,
                self.drawing_pane().drawings.items().get(index)?.hidden,
            ))
        }) {
            self.drawing_pane_mut()
                .drawings
                .set_hidden_at(hidden.0, !hidden.1);
        }
        if let Some(locked) = ask.manager_toggle_locked.and_then(|index| {
            Some((
                index,
                self.drawing_pane().drawings.items().get(index)?.locked,
            ))
        }) {
            self.drawing_pane_mut()
                .drawings
                .set_locked_at(locked.0, !locked.1);
        }
        if let Some(index) = ask.manager_bring_to_front {
            self.drawing_pane_mut().drawings.bring_to_front(index);
        }
        if let Some(index) = ask.manager_delete {
            // The exact same command path as the inspector button and the
            // keyboard: select, then request. Locked rows raise the same
            // confirmation in the inspector.
            self.drawing_pane_mut().drawings.select(Some(index));
            self.request_delete_selected(now);
        }
        if ask.show_all {
            self.drawing_pane_mut().drawings.set_all_hidden(false);
        }
        if ask.unlock_all {
            self.drawing_pane_mut().drawings.set_all_locked(false);
        }
        if ask.place_text_note && self.place_text_note() {
            self.surfaces.drawing_chrome.note_text_note_placed();
        }
        if let Some(edit) = ask.record_inline_edit {
            self.record_drawing_edit(edit.tab, edit.side, edit.index, edit.before);
        }
        if ask.content_editing_changed {
            self.sync_content_editing();
        }
        if self.surfaces.drawing_chrome.take_inspector_position_dirty() {
            self.chrome.inspector_position_dirty = true;
        }
    }
}
