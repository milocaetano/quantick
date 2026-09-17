//! Drawing input: egui keyboard intent and chrome responses share the same
//! drawing-store mutations, undo, clipboard and confirmation state. Rendering
//! and geometry assembly remain separate; neither input path owns a pane.
use super::{DrawingAccess, DrawingController, DrawingEffects};
use crate::toolrail::{Tool, ToolRail};
use eframe::egui;
use std::time::Instant;
struct DrawingKeys {
    escape: bool,
    delete: bool,
    backspace: bool,
    undo: bool,
    redo: bool,
    lock: bool,
    hide: bool,
    duplicate: bool,
    copy: bool,
    paste: bool,
    nudge_bars: f32,
    nudge_px: f32,
}

impl DrawingKeys {
    fn read(ctx: &egui::Context) -> Self {
        ctx.input(|input| {
            let command = input.modifiers.command;
            let shift = input.modifiers.shift;
            let alt = input.modifiers.alt;
            // Shift turns a nudge into ten steps (UX spec).
            let step = if shift { 10.0 } else { 1.0 };
            let horizontal = f32::from(input.key_pressed(egui::Key::ArrowRight))
                - f32::from(input.key_pressed(egui::Key::ArrowLeft));
            let vertical = f32::from(input.key_pressed(egui::Key::ArrowUp))
                - f32::from(input.key_pressed(egui::Key::ArrowDown));
            Self {
                escape: input.key_pressed(egui::Key::Escape),
                delete: input.key_pressed(egui::Key::Delete),
                backspace: input.key_pressed(egui::Key::Backspace),
                undo: command && !shift && input.key_pressed(egui::Key::Z),
                redo: (command && input.key_pressed(egui::Key::Y))
                    || (command && shift && input.key_pressed(egui::Key::Z)),
                lock: alt && input.key_pressed(egui::Key::L),
                hide: alt && input.key_pressed(egui::Key::H),
                duplicate: command && input.key_pressed(egui::Key::D),
                copy: input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy))
                    || (command && input.key_pressed(egui::Key::C)),
                // The chord is read on *release*, not on press, because
                // `egui-winit` turns a paste shortcut into `Event::Paste`
                // — and swallows the pressed `Key` — only while the OS
                // clipboard returns non-empty text. With an image or an
                // empty clipboard the frame carries neither signal, and the
                // release is the one thing the platform pushes either way.
                // A held modifier also tells the two apart: a platform
                // `Event::Paste` under the chord is that same gesture's
                // press frame, so honouring it too would paste twice.
                // Shift+Insert is the same gesture with another key, and a
                // bare `Event::Paste` still pastes for whoever sends one
                // without the chord.
                paste: if command {
                    input.key_released(egui::Key::V)
                } else if shift {
                    input.key_released(egui::Key::Insert)
                } else {
                    input
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::Paste(_)))
                },
                nudge_bars: horizontal * step,
                nudge_px: vertical * step,
            }
        })
    }
}

impl DrawingController {
    pub(crate) fn handle_drawing_keys(
        &mut self,
        host: &mut DrawingAccess<'_>,
        tools: &mut ToolRail,
        alerts: &mut dyn crate::audio::AlertSink,
        ctx: &egui::Context,
        now: Instant,
    ) -> DrawingEffects {
        let mut output = DrawingEffects::default();
        let effects = &mut output;
        if ctx.memory(|memory| memory.focused().is_some()) {
            return output;
        }
        let keys = DrawingKeys::read(ctx);
        if keys.escape {
            self.cancel_one_layer(host, tools);
        }
        self.apply_edit_keys(&keys, host, effects, now);
        self.apply_clipboard_keys(&keys, host, effects, alerts);
        output
    }

    fn cancel_one_layer(&mut self, host: &mut DrawingAccess<'_>, tools: &mut ToolRail) {
        use quantick_chart_interaction::drawing_commands::{EscapeLayer, consume_escape};
        consume_escape(|layer| match layer {
            EscapeLayer::Rail => tools.drag_active(),
            EscapeLayer::Paper => host.cancel_paper(),
            EscapeLayer::Confirmation => {
                let pending = self.chrome.delete_confirm();
                if pending {
                    self.chrome.set_delete_confirm(false);
                }
                pending
            }
            EscapeLayer::QuickRange => self.chrome.quick_range.dismiss(),
            EscapeLayer::InlineText => {
                let editing = self.chrome.inline_text_editing().is_some();
                if editing {
                    host.commit_inline(&mut self.chrome);
                }
                editing
            }
            EscapeLayer::Draft => {
                let drafting = host.drawings().draft().is_some();
                if drafting {
                    host.drawings_mut().cancel_draft();
                    tools.arm(Tool::Pointer);
                }
                drafting
            }
            EscapeLayer::Selection => {
                let selected = host.drawings().selected().is_some();
                if selected {
                    host.drawings_mut().select(None);
                }
                selected
            }
            EscapeLayer::Pointer => {
                tools.arm(Tool::Pointer);
                true
            }
        });
    }
    fn apply_edit_keys(
        &mut self,
        keys: &DrawingKeys,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        now: Instant,
    ) {
        if host.drawings().draft().is_some() {
            // During placement the delete keys belong to the draft workflow:
            // Backspace steps back one anchor.
            if keys.backspace {
                host.drawings_mut().remove_last_draft_anchor();
            }
        } else if keys.delete || keys.backspace {
            self.request_delete_selected(host, effects, now);
        }
        if keys.undo {
            host.undo_drawings();
            // Undoing a rectangle's placement takes its drawing away from
            // any armed instance without passing through the removal
            // funnel; the sweep keeps a resting bot order from outliving
            // its badge (redo below, and delete-all, share the risk).
        }
        if keys.redo {
            host.redo_drawings();
        }
        if keys.lock
            && let Some(index) = host.drawings().selected()
        {
            let locked = host.drawings().items()[index].locked;
            host.drawings_mut().set_selected_locked(!locked);
        }
        if keys.hide
            && let Some(index) = host.drawings().selected()
        {
            let hidden = host.drawings().items()[index].hidden;
            host.drawings_mut().set_selected_hidden(!hidden);
        }
    }
    fn apply_clipboard_keys(
        &mut self,
        keys: &DrawingKeys,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        alerts: &mut dyn crate::audio::AlertSink,
    ) {
        if keys.duplicate {
            self.duplicate_selected_drawing(host, effects, alerts);
        }
        if keys.copy
            && let Some(drawing) = host
                .drawings()
                .selected()
                .and_then(|index| host.drawings().items().get(index))
        {
            self.chrome.copy_drawing(drawing);
            effects.note("Drawing copied. Press Ctrl+V to paste.", Instant::now());
        }
        if keys.paste
            && let Some((drawing, count)) = self.chrome.next_drawing_paste()
        {
            host.paste(&drawing, DUPLICATE_OFFSET_BARS * count as f32);
        }
        if (keys.nudge_bars != 0.0 || keys.nudge_px != 0.0) && host.drawings().selected().is_some()
        {
            // Arrows write the same honest chart coordinates a drag does:
            // one bar per horizontal step, one pixel's worth of *that
            // object's own axis* per vertical step. Each press lands as one
            // undo entry. Reading the candles' scale for an object on an
            // indicator band would nudge a CVD level by a quantity of price —
            // a wrong number arriving through the one gesture that exists for
            // precision. Asked of the pane that owns the mark, which for a
            // shared one is not always the focused pane.
            let price_per_px = host.selected_value_per_px().unwrap_or(0.0);
            host.drawings_mut().begin_gesture();
            host.drawings_mut()
                .translate_selected(keys.nudge_bars, f64::from(keys.nudge_px) * price_per_px);
            // Same rule as the drag: the instants behind the anchors move
            // with them, or the object's shared twin stays behind.
            host.retime_selected();
            host.drawings_mut().commit_gesture();
        }
    }
}

use crate::drawings::{self, DeleteOutcome};
pub(crate) const DUPLICATE_OFFSET_BARS: f32 = 2.0;
impl DrawingController {
    pub(crate) fn request_delete_selected(
        &mut self,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        now: Instant,
    ) {
        // Read the name before the object is gone. "Drawing deleted" makes
        // the undo useless on a crowded chart: the trader has to know *what*
        // they lost to know whether they want it back — and the context bar
        // deletes on a bare glyph, so the toast is what pays for that.
        let doomed = host.drawings().selected().and_then(|index| {
            let drawing = host.drawings().items().get(index)?;
            // The trader's own name when one was given; the tool name
            // otherwise — a positional index would be noise on an object
            // that no longer has a position.
            let label = drawing
                .name
                .clone()
                .unwrap_or_else(|| drawing.tool.name().to_owned());
            Some((drawing.id, label))
        });
        match host.drawings_mut().delete_selected(false) {
            DeleteOutcome::Deleted => {
                self.chrome.set_delete_confirm(false);
                // The instance dies with its drawing, immediately — not on
                // the next closed bar, which a quiet tape may never bring.
                if let Some((id, _)) = &doomed {
                    host.remove_strategy(*id);
                }
                let name = doomed.map(|(_, label)| label);
                let message = name.map_or_else(
                    || "Drawing deleted.".to_owned(),
                    |name| format!("{name} deleted."),
                );
                effects.note_with_undo(message, now);
            }
            DeleteOutcome::NeedsConfirmation => {
                self.chrome.set_delete_confirm(true);
            }
            DeleteOutcome::NothingSelected => {}
        }
    }
    pub(crate) fn apply_drawing_chrome(
        &mut self,
        ask: crate::surfaces::drawing_chrome::DrawingChromeAsk,
        host: &mut DrawingAccess<'_>,
        alerts: &mut dyn crate::audio::AlertSink,
        now: Instant,
    ) -> DrawingEffects {
        let mut output = DrawingEffects::default();
        let effects = &mut output;
        let mut ask = ask;
        self.apply_selection_edit(&mut ask, host, effects, now);
        self.apply_preferences_and_copy(&mut ask, host, effects, alerts, now);
        self.apply_clear(&mut ask, host, effects, now);
        self.apply_manager_rows(&mut ask, host, effects, now);
        self.apply_editor_completion(&mut ask, host, effects);
        output
    }
    fn apply_selection_edit(
        &mut self,
        ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        now: Instant,
    ) {
        if let Some(edited) = ask.edited.take() {
            // Through the selection, never `items_mut`: that hatch is
            // documented for derived-state refresh only, and a style or a
            // note's words take part in payload equality — writing them through
            // it would let an unrelated in-flight gesture swallow this edit.
            if let Some(drawing) = host.drawings_mut().selected_mut() {
                *drawing = *edited;
            }
        }
        if let Some(edit) = ask.commit_edit_gesture.take() {
            host.record(edit.tab, edit.side, edit.index, edit.before);
        }
        if ask.toggle_selected_hidden
            && let Some(index) = host.drawings().selected()
        {
            let hidden = host.drawings().items()[index].hidden;
            host.drawings_mut().set_selected_hidden(!hidden);
        }
        if ask.toggle_selected_locked
            && let Some(index) = host.drawings().selected()
        {
            let locked = host.drawings().items()[index].locked;
            host.drawings_mut().set_selected_locked(!locked);
        }
        if ask.request_delete {
            self.request_delete_selected(host, effects, now);
        }
        if ask.cancel_delete {
            // After the request, never before: a frame carrying both must end
            // with the prompt gone, and a delete on a locked object raises it.
            self.chrome.set_delete_confirm(false);
        }
        if ask.force_delete {
            let doomed = {
                let store = host.drawings();
                store
                    .selected()
                    .and_then(|index| store.items().get(index))
                    .map(|drawing| drawing.id)
            };
            if host.drawings_mut().delete_selected(true) == DeleteOutcome::Deleted {
                if let Some(id) = doomed {
                    host.remove_strategy(id);
                }
                effects.note_with_undo("Drawing deleted.", now);
            }
        }
    }
    fn apply_preferences_and_copy(
        &mut self,
        ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        alerts: &mut dyn crate::audio::AlertSink,
        now: Instant,
    ) {
        if ask.duplicate {
            self.duplicate_selected_drawing(host, effects, alerts);
        }
        if let Some(saved) = ask.saved_default {
            // Nothing to undo: this changed a preference, not the chart.
            effects.note(saved.message(), now);
        }
        for write in ask.presets.drain(..) {
            write.apply_to(&mut self.presets);
        }
        if ask.sweep_authored {
            let removed = host.remove_authored();
            if removed > 0 {
                effects.note_with_undo(format!("{removed} object(s) placed for you removed."), now);
            }
        }
    }
    fn apply_clear(
        &mut self,
        ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        now: Instant,
    ) {
        if ask.delete_all {
            let deleted = host.delete_all_drawings();
            // Every armed instance just lost its drawing at once; sweep them
            // now so no resting bot order outlives its badge.
            if deleted > 0 {
                effects.note_with_undo("All drawings deleted.", now);
            }
        }
    }
    fn apply_manager_rows(
        &mut self,
        ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        now: Instant,
    ) {
        if let Some(index) = ask
            .manager_select
            .filter(|index| *index < host.drawings().items().len())
        {
            host.drawings_mut().select(Some(index));
            host.center_on_drawing(index);
        }
        // Through `get`, never `[]`: the rows were snapshotted before any of
        // the four pieces drew, and a destructive ask applied above — delete
        // all, the assistant sweep, a confirmed delete — can have shortened
        // the list under an index that was valid when the row was clicked. The
        // store's own setters are already bounds-safe; these two reads were
        // the only raw ones left.
        if let Some(hidden) = ask
            .manager_toggle_hidden
            .and_then(|index| Some((index, host.drawings().items().get(index)?.hidden)))
        {
            host.drawings_mut().set_hidden_at(hidden.0, !hidden.1);
        }
        if let Some(locked) = ask
            .manager_toggle_locked
            .and_then(|index| Some((index, host.drawings().items().get(index)?.locked)))
        {
            host.drawings_mut().set_locked_at(locked.0, !locked.1);
        }
        if let Some(index) = ask.manager_bring_to_front {
            host.drawings_mut().bring_to_front(index);
        }
        if let Some(index) = ask.manager_delete {
            // The exact same command path as the inspector button and the
            // keyboard: select, then request. Locked rows raise the same
            // confirmation in the inspector.
            host.drawings_mut().select(Some(index));
            self.request_delete_selected(host, effects, now);
        }
    }
    fn apply_editor_completion(
        &mut self,
        ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
    ) {
        if ask.show_all {
            host.drawings_mut().set_all_hidden(false);
        }
        if ask.unlock_all {
            host.drawings_mut().set_all_locked(false);
        }
        if ask.place_text_note && self.place_text_note(host) {
            self.chrome.note_text_note_placed();
        }
        if let Some(edit) = ask.record_inline_edit.take() {
            host.record(edit.tab, edit.side, edit.index, edit.before);
        }
        if ask.content_editing_changed {
            host.sync_inline(&self.chrome);
        }
        if self.chrome.take_inspector_position_dirty() {
            effects.inspector_moved = true;
        }
    }

    pub(crate) fn duplicate_selected_drawing(
        &mut self,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        alerts: &mut dyn crate::audio::AlertSink,
    ) {
        let side = host.target().1;
        let Some(duplicated) = host
            .drawings_mut()
            .duplicate_selected(DUPLICATE_OFFSET_BARS)
        else {
            return;
        };
        self.carry_strategy_to_duplicate(host, effects, alerts, side, duplicated);
    }
    fn carry_strategy_to_duplicate(
        &mut self,
        host: &mut DrawingAccess<'_>,
        effects: &mut DrawingEffects,
        alerts: &mut dyn crate::audio::AlertSink,
        side: crate::pane::PaneSide,
        duplicated: drawings::Duplicated,
    ) {
        let Some((spec, label)) = host.watching_strategy(duplicated.source) else {
            return;
        };
        if let Err(reason) = host.arm_copy(side, duplicated.copy, &spec, label, alerts) {
            effects.note(
                format!("the copy carries no strategy: {reason}"),
                Instant::now(),
            );
        }
    }
}

pub(crate) struct RegisteredDrawingAction {
    pub(crate) capability: &'static str,
    pub(crate) version: u32,
    pub(crate) input: serde_json::Value,
    pub(crate) pending: PendingDrawingAction,
}

#[must_use]
pub(crate) struct PendingDrawingAction(u64);
impl DrawingController {
    pub(crate) fn begin_registered_action(
        &mut self,
        ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
    ) -> Option<RegisteredDrawingAction> {
        use crate::surfaces::drawing_chrome::QuickRangeActionUi as _;
        if ask.dismiss_quick_range {
            self.chrome.quick_range.dismiss();
        }
        let request = ask.place_quick_range.take()?;
        let operation = request.operation;
        let Some(input) = crate::control::quick_range_input(&operation, request.side) else {
            self.chrome.quick_range.completed(operation.id, false);
            tracing::warn!(target:"quantick::control",event_code="QUICK_RANGE_INPUT_INVALID","the quick-range drawing coordinates could not be serialized");
            return None;
        };
        let version = match operation.action {
            crate::surfaces::drawing_chrome::QuickRangeAction::Profile => {
                crate::control::PROFILE_CAPABILITY_VERSION
            }
            _ => crate::control::FIB_CAPABILITY_VERSION,
        };
        Some(RegisteredDrawingAction {
            capability: operation.action.capability_id(),
            version,
            input,
            pending: PendingDrawingAction(operation.id),
        })
    }
    pub(crate) fn finish_registered_action(
        &mut self,
        pending: PendingDrawingAction,
        succeeded: bool,
    ) -> bool {
        self.chrome.quick_range.completed(pending.0, succeeded)
    }
}
