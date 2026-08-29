//! How the app keeps every pane equal to the active layout
//! ([`crate::layouts`]): the indicator fan-out, the per-market drawing
//! swap, and the layout operations the strip, the menu, the keyboard and
//! the control plane all call.
//!
//! A child of `app` rather than a sibling so it can reach the app's own
//! fields: this *is* app logic, split off only so the file that holds it can
//! be read in one sitting.
//!
//! **Indicators: the layout is edited, then mirrored.** An edit on one pane
//! — add, remove, hide, retune, restyle — is written into the active
//! layout's entry *from the edit itself* (the kind added, the index removed,
//! the values the trader committed) and then applied to every other pane on
//! the same frame, by *layout index*: the n-th indicator of a pane is the
//! n-th of every pane, which is what makes "the same set everywhere" hold
//! without a second identity scheme. Nothing here reads a view back to learn
//! what the layout holds: a view is the worker's answer, which lands later
//! and may be a preview the trader will discard, and a layout written from
//! it drifted exactly then. The file follows, debounced.
//!
//! Operator-attached scripts (the annotate tier) stay on the pane they were
//! attached to and out of the layout: they are an agent's overlay, removed
//! by the agent, and were never part of what a trader keeps.
//!
//! **Drawings: put away and brought out.** Each pane holds the drawings of
//! one [`DrawingKey`] at a time. When its tab moves to another market, or
//! the window switches layout, what it holds is serialised under the old
//! key and the new key's set is adopted — with the ids it had, so a strategy
//! armed on a region and an annotation an agent placed still name their
//! object, and anchored by market time, so a level comes back on the bar it
//! was drawn on. A pane's revision counter is compared each frame; when it
//! moved, the set is written into the layout and the debounced save follows.
//!
//! Per-frame cost: one integer compare and two short string compares per
//! pane, and a flag test per pane for seeding. Nothing here allocates on a
//! quiet frame.

use std::time::Instant;

use crate::indicator_style::StyleOverride;
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind, SavedPlotStyle};
use crate::layouts::{self, DrawingKey, LayoutBook, LayoutError, LayoutId, Loaded, SavedDrawing};
use crate::pane::{ChartPane, DrawingDrag, PaneIndex, PaneSide};

use super::{DEFAULT_EMA_LEN, INDICATOR_STATE_SAVE_DEBOUNCE, QuantickApp, TabSlot};

/// How long after the last layout edit the file is written. Drawing drags
/// and rename keystrokes come in bursts; one write per burst, off the frame
/// path.
const LAYOUTS_SAVE_DEBOUNCE: std::time::Duration = INDICATOR_STATE_SAVE_DEBOUNCE;

/// The feed half of a drawing key while a tab plays a recording.
///
/// A recording is its own market: its prices are the recorded day's, not
/// the live venue's, so a level drawn on it belongs to the recording's
/// symbol under this name and never lands on the live chart of the same
/// symbol, nor on another tab streaming it.
const REPLAY_FEED_KEY: &str = "replay";

impl QuantickApp {
    // ------------------------------------------------------------------
    // Boot and file
    // ------------------------------------------------------------------

    /// Read the layouts file, or migrate the indicator set a cockpit kept
    /// before layouts existed, or start with one empty layout.
    ///
    /// The flag says whether the file may be written back: a file this build
    /// could not read *and* could not set aside is the trader's only copy,
    /// and the empty book the session opens on must never be saved over it.
    pub(super) fn load_layouts(
        path: &std::path::Path,
        legacy_indicators: &std::path::Path,
    ) -> (LayoutBook, bool) {
        match layouts::load(path) {
            Loaded::Book(book) => (book, false),
            Loaded::Refused { set_aside, .. } => (LayoutBook::default(), !set_aside),
            Loaded::Missing => {
                let legacy = crate::indicators::state_file::load(legacy_indicators);
                if !legacy.is_empty() {
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "LAYOUTS_MIGRATED",
                        indicators = legacy.len(),
                        action = "indicator_state_moved_into_layout_1",
                        "the indicator set became Layout 1"
                    );
                }
                (LayoutBook::starter(legacy), false)
            }
        }
    }

    /// The book, for the strip, the menu and the control plane to read.
    pub(crate) fn layouts(&self) -> &LayoutBook {
        &self.layouts
    }

    fn mark_layouts_dirty(&mut self) {
        self.layouts_dirty = true;
        self.last_layout_change = Some(Instant::now());
    }

    fn save_layouts_now(&mut self) {
        self.layouts_dirty = false;
        self.last_layout_change = None;
        if self.layouts_save_blocked {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUTS_SAVE_BLOCKED",
                path = %self.layouts_path.display(),
                action = "file_left_untouched",
                "the layouts file could not be read at launch and was not set aside; this session's layouts are not written over it"
            );
            return;
        }
        layouts::save(&self.layouts_path, &self.layouts);
    }

    /// Everything the frame owes the layouts: seed panes that just appeared,
    /// follow tabs that changed market, notice drawings that changed, and
    /// write the file once the last change has settled.
    pub(super) fn maintain_layouts(&mut self) {
        self.seed_new_panes();
        self.follow_market_changes();
        self.persist_changed_drawings();
        let settled = self
            .last_layout_change
            .is_some_and(|changed| changed.elapsed() >= LAYOUTS_SAVE_DEBOUNCE);
        if self.layouts_dirty && settled {
            self.save_layouts_now();
        }
    }

    /// Write the file now, whatever the debounce says — the way out on exit,
    /// and the moment before a bundle export reads it.
    pub(super) fn flush_layouts(&mut self) {
        self.persist_changed_drawings();
        if self.layouts_dirty {
            self.save_layouts_now();
        }
    }

    // ------------------------------------------------------------------
    // Layout operations
    // ------------------------------------------------------------------

    /// Whether the panes can be swapped under the trader right now.
    ///
    /// A strategy armed on a region names that drawing; putting the drawing
    /// away would orphan the instance and drop it with no reason given. A
    /// gesture in flight — a drag, a half-placed object — addresses the
    /// store by index, and a swap under it would land on another layout's
    /// object. Both are the trader's to finish first, and the refusal says so.
    fn layout_swap_refusal(&self) -> Option<LayoutError> {
        for tab in &self.tabs {
            for (pane, _) in tab.panes() {
                if !pane.strategies.is_empty() {
                    return Some(LayoutError::StrategyArmed);
                }
                if pane.drawings.in_gesture()
                    || pane.drawings.draft().is_some()
                    || !matches!(pane.drawing_drag, DrawingDrag::None)
                {
                    return Some(LayoutError::GestureInFlight);
                }
            }
        }
        None
    }

    /// Make `id` the active layout on every pane of every tab.
    pub(crate) fn switch_layout(&mut self, id: LayoutId) -> Result<bool, LayoutError> {
        if self.layouts.get(id).is_none() {
            return Err(LayoutError::Unknown);
        }
        if self.layouts.active_id() == id {
            return Ok(false);
        }
        if let Some(refusal) = self.layout_swap_refusal() {
            return Err(refusal);
        }
        // Whatever was being typed into a note belongs to the layout going
        // out, and is committed to it before the stores are swapped.
        self.end_inline_text_edit();
        self.inspector_edit_baseline = None;
        // Everything the panes hold belongs to the layout going out.
        self.persist_changed_drawings();
        let targets = self.layout_pane_targets();
        for (tab, side) in &targets {
            self.put_away_drawings(*tab, *side);
            self.remove_layout_indicators_at(*tab, *side);
        }
        let from = self.layouts.active_id();
        self.layouts.switch(id)?;
        let set = self.layouts.active().indicators.clone();
        for (tab, side) in &targets {
            self.materialize_indicators_at(*tab, *side, &set);
            self.bring_out_drawings(*tab, *side);
        }
        self.mark_layouts_dirty();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUT_SWITCHED",
            from = from.0,
            to = id.0,
            name = %self.layouts.active().name,
            panes = targets.len(),
            action = "panes_rematerialized",
            "the active layout changed"
        );
        Ok(true)
    }

    /// Switch to the layout at strip position `index`.
    pub(crate) fn switch_layout_index(&mut self, index: usize) -> Result<bool, LayoutError> {
        let id = self.layouts.at(index).ok_or(LayoutError::Unknown)?.id;
        self.switch_layout(id)
    }

    /// Add a layout and switch to it — a new tab opens where it was made,
    /// which is what a `+` on a strip means everywhere else.
    ///
    /// The switch is checked before the layout is made, so a refusal leaves
    /// the strip as it was rather than with a tab nobody asked to keep.
    pub(crate) fn create_layout(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError> {
        if let Some(refusal) = self.layout_swap_refusal() {
            return Err(refusal);
        }
        let id = self.layouts.create(name)?;
        self.mark_layouts_dirty();
        self.switch_layout(id)?;
        Ok(id)
    }

    pub(crate) fn rename_layout(&mut self, id: LayoutId, name: &str) -> Result<bool, LayoutError> {
        let changed = self.layouts.rename(id, name)?;
        if changed {
            self.mark_layouts_dirty();
        }
        Ok(changed)
    }

    /// Delete a layout. Deleting the active one switches to its neighbour
    /// first, so the panes never show a layout that no longer exists.
    pub(crate) fn delete_layout(&mut self, id: LayoutId) -> Result<(), LayoutError> {
        if self.layouts.get(id).is_none() {
            return Err(LayoutError::Unknown);
        }
        if self.layouts.layouts().len() == 1 {
            return Err(LayoutError::Last);
        }
        if self.layouts.active_id() == id {
            let index = self.layouts.index_of(id).unwrap_or(0);
            let neighbour = self
                .layouts
                .at(index.saturating_sub(1))
                .filter(|layout| layout.id != id)
                .or_else(|| self.layouts.at(index + 1))
                .map(|layout| layout.id)
                .ok_or(LayoutError::Last)?;
            self.switch_layout(neighbour)?;
        }
        self.layouts.delete(id)?;
        self.mark_layouts_dirty();
        Ok(())
    }

    /// Move a context chart within a tab's stack, and move everything keyed
    /// by its position with it.
    ///
    /// The slot bookkeeping (`slot_kinds`, the operator's slots, the pending
    /// hides and styles) and each pane's drawing key name a context chart by
    /// its slot. A move that left them behind would have the charts swap
    /// their drawing sets on the next switch and the layout address the
    /// wrong pane's slots. The one door for the View menu and the control
    /// plane, so neither can forget the second half.
    pub(crate) fn move_context_pane_at(
        &mut self,
        tab_id: u64,
        from: PaneIndex,
        to: PaneIndex,
    ) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return false;
        };
        if !tab.move_context_pane(from, to) {
            return false;
        }
        // Where each old address now sits: the moved pane at `to`, the ones
        // between shifted one step towards `from`.
        let rekey = |index: PaneIndex| -> PaneIndex {
            if index == from {
                to
            } else if from < to && index > from && index <= to {
                index - 1
            } else if to < from && index >= to && index < from {
                index + 1
            } else {
                index
            }
        };
        let reside = |side: PaneSide| PaneSide::from_index(rekey(side.index()));
        for (owner, _) in &mut self.slot_kinds {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        for (owner, _) in &mut self.pending_styles {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        for owner in &mut self.pending_hidden {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        for (owner, ..) in &mut self.script_files {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        let operator: Vec<TabSlot> = self
            .operator_slots
            .iter()
            .copied()
            .map(|mut owner| {
                if owner.tab == tab_id {
                    owner.side = reside(owner.side);
                }
                owner
            })
            .collect();
        self.operator_slots = operator.into_iter().collect();
        if self.indicator_settings_target.tab == tab_id {
            self.indicator_settings_target.side = reside(self.indicator_settings_target.side);
        }
        // The drawings travel with the pane; their key follows its address.
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            for (pane, side) in tab.panes_with_sides_mut() {
                if let Some(key) = pane.drawings_key.as_mut() {
                    key.pane = side.index();
                }
            }
        }
        // Keys changed under stored sets: write them where they now belong.
        self.mark_layouts_dirty();
        true
    }

    // ------------------------------------------------------------------
    // Indicators
    // ------------------------------------------------------------------

    /// Every (tab, pane) that carries the layout, flow first per tab.
    fn layout_pane_targets(&self) -> Vec<(u64, PaneSide)> {
        self.tabs
            .iter()
            .flat_map(|tab| tab.sides().map(move |side| (tab.id, side)))
            .collect()
    }

    fn pane_mut_at(&mut self, tab: u64, side: PaneSide) -> Option<&mut ChartPane> {
        self.tabs
            .iter_mut()
            .find(|candidate| candidate.id == tab)
            .map(|candidate| candidate.pane_mut(side))
    }

    fn pane_at(&self, tab: u64, side: PaneSide) -> Option<&ChartPane> {
        self.tabs
            .iter()
            .find(|candidate| candidate.id == tab)
            .map(|candidate| candidate.pane(side))
    }

    /// The layout's slots on one pane, in layout order: every slot the
    /// bookkeeping knows for the pane, minus the operator's overlays.
    fn layout_slots_at(&self, tab: u64, side: PaneSide) -> Vec<SlotId> {
        self.slot_kinds
            .iter()
            .map(|(owner, _)| *owner)
            .filter(|owner| owner.tab == tab && owner.side == side)
            .filter(|owner| !self.operator_slots.contains(owner))
            .map(|owner| owner.slot)
            .collect()
    }

    /// Where a slot sits in the layout, or `None` for a slot the layout does
    /// not carry — an operator's, or one a validation hook added.
    fn layout_index_of(&self, target: TabSlot) -> Option<usize> {
        self.layout_slots_at(target.tab, target.side)
            .iter()
            .position(|slot| *slot == target.slot)
    }

    /// Add one indicator to one pane, with no fan-out and no dirty mark —
    /// the primitive both the mirror and the materialisation are built on.
    pub(super) fn add_indicator_at(
        &mut self,
        tab: u64,
        side: PaneSide,
        kind: &SavedKind,
    ) -> Option<SlotId> {
        let source = match kind {
            SavedKind::NativeCvd => IndicatorSource::NativeCvd,
            SavedKind::NativeEma => IndicatorSource::NativeEma {
                len: DEFAULT_EMA_LEN,
                source: quantick_indicators::SourceId::Close,
            },
            SavedKind::Script { name } => {
                let index = self
                    .script_library
                    .entries()
                    .iter()
                    .position(|candidate| candidate.name == *name);
                let Some(index) = index else {
                    tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "INDICATOR_STATE_SCRIPT_MISSING",
                        script = %name,
                        action = "entry_skipped",
                        "the layout references a script the library no longer has"
                    );
                    return None;
                };
                let file_info = self.script_library.file_info(index);
                let slot = match self.script_library.read(index) {
                    Some(Ok(text)) => {
                        let pane = self.pane_mut_at(tab, side)?;
                        pane.add_indicator(IndicatorSource::Script {
                            name: name.clone(),
                            text,
                        })
                    }
                    Some(Err(message)) => {
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "INDICATOR_SCRIPT_UNREADABLE",
                            script = %name,
                            error = %message,
                            action = "error_slot_shown",
                            "cannot read an indicator script"
                        );
                        // The same error slot the menu's own add builds, so a
                        // script the trader fixes and reloads keeps its place.
                        let pane = self.pane_mut_at(tab, side)?;
                        let slot = pane.indicators.allocate_slot(format!("script.{name}"));
                        pane.indicators.apply(IndicatorEvent::Rebuilt {
                            slot,
                            descriptor: quantick_indicators::IndicatorDescriptor {
                                title: name.clone(),
                                short_title: None,
                                overlay: false,
                                plots: Vec::new(),
                                fills: Vec::new(),
                                inputs: Vec::new(),
                            },
                            columns: Vec::new(),
                            bar_paint: Vec::new(),
                            rows: 0,
                            inputs: Vec::new(),
                            stale: None,
                        });
                        pane.indicators.apply(IndicatorEvent::Error {
                            slot,
                            error: quantick_indicators::EvalError {
                                bar_index: 0,
                                message,
                            },
                        });
                        slot
                    }
                    None => return None,
                };
                let owner = TabSlot { tab, side, slot };
                self.slot_kinds.push((owner, kind.clone()));
                if let Some((_, mtime)) = file_info {
                    self.script_files.push((owner, index, mtime));
                }
                return Some(slot);
            }
        };
        let slot = self.pane_mut_at(tab, side)?.add_indicator(source);
        self.slot_kinds
            .push((TabSlot { tab, side, slot }, kind.clone()));
        Some(slot)
    }

    /// Take every layout slot off one pane, view and worker alike.
    fn remove_layout_indicators_at(&mut self, tab: u64, side: PaneSide) {
        for slot in self.layout_slots_at(tab, side) {
            self.remove_indicator_silently(TabSlot { tab, side, slot });
        }
    }

    /// Remove one slot with no fan-out and no dirty mark.
    fn remove_indicator_silently(&mut self, target: TabSlot) {
        if let Some(pane) = self.pane_mut_at(target.tab, target.side) {
            pane.indicators.remove(target.slot);
            pane.indicator_worker
                .send(IndicatorCommand::Remove(target.slot));
        }
        self.slot_kinds.retain(|(owner, _)| *owner != target);
        self.operator_slots.remove(&target);
        self.script_files.retain(|(owner, ..)| *owner != target);
        self.pending_hidden.retain(|owner| *owner != target);
        self.pending_styles.retain(|(owner, _)| *owner != target);
    }

    /// Put a whole saved set on one pane: add, bind inputs, queue the hide
    /// and the style until the worker's first `Rebuilt` gives them a view.
    pub(super) fn materialize_indicators_at(
        &mut self,
        tab: u64,
        side: PaneSide,
        set: &[SavedIndicator],
    ) {
        for entry in set {
            let Some(slot) = self.add_indicator_at(tab, side, &entry.kind) else {
                continue;
            };
            let owner = TabSlot { tab, side, slot };
            self.apply_saved_entry(owner, entry);
        }
    }

    /// Bind one saved entry's inputs, hide flag and style onto a slot.
    fn apply_saved_entry(&mut self, owner: TabSlot, entry: &SavedIndicator) {
        let values: Vec<_> = entry
            .inputs
            .iter()
            .filter_map(SavedInput::to_value)
            .collect();
        if !values.is_empty() && values.len() == entry.inputs.len() {
            if let Some(pane) = self.pane_mut_at(owner.tab, owner.side) {
                pane.indicator_worker.send(IndicatorCommand::SetInputs {
                    slot: owner.slot,
                    values,
                });
            }
        } else if !entry.inputs.is_empty() {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_STATE_INPUTS_DROPPED",
                kind = ?entry.kind,
                saved = entry.inputs.len(),
                readable = values.len(),
                action = "declared_defaults_used",
                "saved indicator inputs could not be read; using the declared defaults"
            );
        }
        if entry.hidden {
            self.pending_hidden.push(owner);
        }
        if !entry.plot_styles.is_empty() {
            self.pending_styles.push((
                owner,
                StyleOverride::from_plots(
                    entry
                        .plot_styles
                        .iter()
                        .copied()
                        .map(SavedPlotStyle::to_override)
                        .collect(),
                ),
            ));
        }
    }

    /// Apply queued hide flags and styles to the views that now exist.
    ///
    /// A queued hide means "hide this once it is born": views are born
    /// visible, so applying it is a toggle. Whoever queues one dequeues it
    /// again on the matching unhide ([`Self::mirror_hidden`]).
    pub(super) fn apply_pending_indicator_state(&mut self) {
        if !self.pending_hidden.is_empty() {
            let pending = std::mem::take(&mut self.pending_hidden);
            for owner in pending {
                let applied = self
                    .pane_mut_at(owner.tab, owner.side)
                    .filter(|pane| {
                        pane.indicators
                            .all()
                            .iter()
                            .any(|view| view.slot == owner.slot)
                    })
                    .map(|pane| pane.indicators.toggle_hidden(owner.slot))
                    .is_some();
                if !applied
                    && self
                        .slot_kinds
                        .iter()
                        .any(|(candidate, _)| *candidate == owner)
                {
                    self.pending_hidden.push(owner);
                }
            }
        }
        if !self.pending_styles.is_empty() {
            let pending = std::mem::take(&mut self.pending_styles);
            for (owner, style) in pending {
                let known = self
                    .slot_kinds
                    .iter()
                    .any(|(candidate, _)| *candidate == owner);
                match self
                    .pane_mut_at(owner.tab, owner.side)
                    .and_then(|pane| pane.indicators.view_mut(owner.slot))
                {
                    Some(view) => view.style = style,
                    None if known => self.pending_styles.push((owner, style)),
                    None => {}
                }
            }
        }
    }

    /// The active layout's entry at a layout index, for an edit to write.
    fn layout_entry_mut(&mut self, index: usize) -> Option<&mut SavedIndicator> {
        self.layouts.active_mut().indicators.get_mut(index)
    }

    /// Mirror an add: the layout gains the entry, and the same kind goes on
    /// every other pane, so the new indicator is on every chart the frame it
    /// was asked for. Inputs start empty — "the declared defaults" — until
    /// the trader commits some.
    pub(super) fn mirror_add(&mut self, origin: TabSlot, kind: &SavedKind) {
        let Some(index) = self.layout_index_of(origin) else {
            return;
        };
        let entry = SavedIndicator {
            kind: kind.clone(),
            hidden: false,
            inputs: Vec::new(),
            plot_styles: Vec::new(),
        };
        let entries = &mut self.layouts.active_mut().indicators;
        if index <= entries.len() {
            entries.insert(index, entry);
        } else {
            entries.push(entry);
        }
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            self.add_indicator_at(tab, side, kind);
        }
        self.mark_layouts_dirty();
    }

    /// Mirror a removal by layout index: the entry goes, and the slot at that
    /// position on every other pane with it.
    pub(super) fn mirror_remove(&mut self, origin: TabSlot) {
        let Some(index) = self.layout_index_of(origin) else {
            return;
        };
        let entries = &mut self.layouts.active_mut().indicators;
        if index < entries.len() {
            entries.remove(index);
        }
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            if let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() {
                self.remove_indicator_silently(TabSlot { tab, side, slot });
            }
        }
        self.mark_layouts_dirty();
    }

    /// Mirror an eye toggle: the entry records it, and the same position on
    /// every other pane follows — now, or once its view is born.
    pub(super) fn mirror_hidden(&mut self, origin: TabSlot) {
        let Some(index) = self.layout_index_of(origin) else {
            return;
        };
        let Some(hidden) = self
            .pane_at(origin.tab, origin.side)
            .and_then(|pane| {
                pane.indicators
                    .all()
                    .iter()
                    .find(|view| view.slot == origin.slot)
            })
            .map(|view| view.hidden)
        else {
            return;
        };
        if let Some(entry) = self.layout_entry_mut(index) {
            entry.hidden = hidden;
        }
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() else {
                continue;
            };
            let owner = TabSlot { tab, side, slot };
            let Some(pane) = self.pane_mut_at(tab, side) else {
                continue;
            };
            let live = pane
                .indicators
                .all()
                .iter()
                .find(|view| view.slot == slot)
                .map(|view| view.hidden);
            match live {
                Some(live) if live != hidden => pane.indicators.toggle_hidden(slot),
                Some(_) => {}
                None => {
                    // Unborn: the queue says what its first frame should
                    // be. An unhide takes a queued hide back out, or the
                    // view would be born and hidden a moment later.
                    self.pending_hidden.retain(|candidate| *candidate != owner);
                    if hidden {
                        self.pending_hidden.push(owner);
                    }
                }
            }
        }
        self.mark_layouts_dirty();
    }

    /// Mirror committed inputs: the entry records the values the trader
    /// applied, and the same position on every other pane is sent them.
    ///
    /// Committed, never previewed: the settings dialog's live preview goes
    /// to the origin's worker alone and never reaches here, so a slider
    /// mid-drag can never land in the file or on another chart.
    pub(super) fn mirror_inputs(
        &mut self,
        origin: TabSlot,
        values: &[quantick_indicators::InputValue],
    ) {
        let Some(index) = self.layout_index_of(origin) else {
            return;
        };
        if let Some(entry) = self.layout_entry_mut(index) {
            entry.inputs = values.iter().map(SavedInput::from_value).collect();
        }
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            if let Some(slot) = self.layout_slots_at(tab, side).get(index).copied()
                && let Some(pane) = self.pane_mut_at(tab, side)
            {
                pane.indicator_worker.send(IndicatorCommand::SetInputs {
                    slot,
                    values: values.to_vec(),
                });
            }
        }
        self.mark_layouts_dirty();
    }

    /// Mirror the origin's style layer: the entry records it, and the same
    /// position everywhere wears it.
    pub(super) fn mirror_style(&mut self, origin: TabSlot) {
        let Some(index) = self.layout_index_of(origin) else {
            return;
        };
        let Some(style) = self
            .pane_at(origin.tab, origin.side)
            .and_then(|pane| {
                pane.indicators
                    .all()
                    .iter()
                    .find(|view| view.slot == origin.slot)
            })
            .map(|view| view.style.clone())
        else {
            return;
        };
        if let Some(entry) = self.layout_entry_mut(index) {
            entry.plot_styles = style
                .plots()
                .iter()
                .copied()
                .map(SavedPlotStyle::from_override)
                .collect();
        }
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() else {
                continue;
            };
            let owner = TabSlot { tab, side, slot };
            match self
                .pane_mut_at(tab, side)
                .and_then(|pane| pane.indicators.view_mut(slot))
            {
                Some(view) => view.style = style.clone(),
                None => {
                    self.pending_styles
                        .retain(|(candidate, _)| *candidate != owner);
                    self.pending_styles.push((owner, style.clone()));
                }
            }
        }
        self.mark_layouts_dirty();
    }

    /// An indicator edit happened on a pane the layout carries. The mirror
    /// that made it has already written the layout; this starts the save
    /// clock for edits that reach the layout by no other path.
    pub(super) fn note_indicator_edit_at(&mut self, _tab: u64, _side: PaneSide) {
        self.mark_layouts_dirty();
    }

    /// Panes that appeared since last frame — a tab opened, a split built —
    /// take the active layout: its indicators, and their market's drawings.
    pub(super) fn seed_new_panes(&mut self) {
        let unseeded: Vec<(u64, PaneSide)> = self
            .tabs
            .iter()
            .flat_map(|tab| {
                tab.panes()
                    .filter(|(pane, _)| !pane.layout_seeded)
                    .map(move |(_, side)| (tab.id, side))
            })
            .collect();
        if unseeded.is_empty() {
            return;
        }
        let set = self.layouts.active().indicators.clone();
        for (tab, side) in unseeded {
            if let Some(pane) = self.pane_mut_at(tab, side) {
                pane.layout_seeded = true;
            }
            self.materialize_indicators_at(tab, side, &set);
            self.bring_out_drawings(tab, side);
        }
    }

    // ------------------------------------------------------------------
    // Drawings
    // ------------------------------------------------------------------

    /// The market a tab's panes are showing, as a drawing key names it.
    ///
    /// `tab.active` is the feed thread's market and does not move for a
    /// recording; the recording's symbol does, under its own feed name.
    fn market_of(tab: &crate::tab::Tab) -> (&str, &str) {
        if tab.replay.is_some() {
            (REPLAY_FEED_KEY, tab.symbol.as_str())
        } else {
            (tab.active.0.as_str(), tab.active.1.as_str())
        }
    }

    fn drawing_key(&self, tab: u64, side: PaneSide) -> Option<DrawingKey> {
        let tab = self.tabs.iter().find(|candidate| candidate.id == tab)?;
        let (feed, symbol) = Self::market_of(tab);
        Some(DrawingKey {
            feed: feed.to_owned(),
            symbol: symbol.to_owned(),
            pane: side.index(),
        })
    }

    /// Serialise what a pane holds into the active layout under the key the
    /// pane says it holds, and empty the pane.
    fn put_away_drawings(&mut self, tab: u64, side: PaneSide) {
        let Some(pane) = self.pane_mut_at(tab, side) else {
            return;
        };
        let Some(key) = pane.drawings_key.take() else {
            // Never loaded: nothing of the layout's is on it. What a hook or
            // a test placed before seeding is dropped with the key.
            pane.drawings.take_all();
            return;
        };
        let items: Vec<SavedDrawing> = pane
            .drawings
            .take_all()
            .iter()
            .map(SavedDrawing::from_drawing)
            .collect();
        pane.drawings_saved_revision = pane.drawings.revision();
        self.layouts.active_mut().set_drawings(&key, items);
    }

    /// Adopt the active layout's drawings for the pane's current market.
    fn bring_out_drawings(&mut self, tab: u64, side: PaneSide) {
        let Some(key) = self.drawing_key(tab, side) else {
            return;
        };
        let saved = self.layouts.active().drawings(&key).unwrap_or(&[]);
        let mut items: Vec<crate::drawings::Drawing> = Vec::with_capacity(saved.len());
        for entry in saved {
            match entry.to_drawing(crate::drawings::DrawingId(entry.id.unwrap_or(0))) {
                Some(drawing) => items.push(drawing),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUT_DRAWING_TOOL_UNKNOWN",
                    tool = %entry.tool,
                    feed = %key.feed,
                    symbol = %key.symbol,
                    pane = key.pane,
                    action = "kept_in_file_not_drawn",
                    "a saved drawing uses a tool this build does not have"
                ),
            }
        }
        let Some(pane) = self.pane_mut_at(tab, side) else {
            return;
        };
        pane.drawings.adopt(items);
        // The saved bar offsets are the old series'; market time is what
        // puts each anchor back on its bar — once there are bars. At launch
        // the pane is seeded before its first print, and an anchor asked
        // against an empty series would be marked off it for the session.
        if pane.slots() == 0 {
            pane.defer_reanchor();
        } else {
            let slots = pane.slots();
            pane.reanchor_drawings(slots);
        }
        pane.drawings_key = Some(key);
        pane.drawings_saved_revision = pane.drawings.revision();
    }

    /// A tab whose market moved out from under a pane: the drawings go to
    /// the market they were drawn on and the new market's come out.
    fn follow_market_changes(&mut self) {
        let moved: Vec<(u64, PaneSide)> = self
            .tabs
            .iter()
            .flat_map(|tab| {
                let (feed, symbol) = Self::market_of(tab);
                tab.panes()
                    .filter(move |(pane, _)| {
                        pane.layout_seeded
                            && pane
                                .drawings_key
                                .as_ref()
                                .is_some_and(|key| key.feed != feed || key.symbol != symbol)
                    })
                    .map(move |(_, side)| (tab.id, side))
            })
            .collect();
        for (tab, side) in moved {
            self.put_away_drawings(tab, side);
            self.bring_out_drawings(tab, side);
            self.mark_layouts_dirty();
        }
    }

    /// Any pane whose drawings changed since they were last written has its
    /// set copied into the active layout.
    pub(super) fn persist_changed_drawings(&mut self) {
        let changed: Vec<(u64, PaneSide)> = self
            .tabs
            .iter()
            .flat_map(|tab| {
                tab.panes()
                    .filter(|(pane, _)| {
                        pane.drawings_key.is_some()
                            && pane.drawings.revision() != pane.drawings_saved_revision
                            && !pane.drawings.in_gesture()
                    })
                    .map(move |(_, side)| (tab.id, side))
            })
            .collect();
        for (tab, side) in changed {
            let Some(pane) = self.pane_mut_at(tab, side) else {
                continue;
            };
            let Some(key) = pane.drawings_key.clone() else {
                continue;
            };
            let items: Vec<SavedDrawing> = pane
                .drawings
                .items()
                .iter()
                .map(SavedDrawing::from_drawing)
                .collect();
            pane.drawings_saved_revision = pane.drawings.revision();
            self.layouts.active_mut().set_drawings(&key, items);
            self.mark_layouts_dirty();
            // Two tabs on one market show one set of drawings: the other
            // pane holding this key is brought to what was just written,
            // rather than keeping a copy that drifts until a switch. Ids
            // travel, so a strategy or an annotation on the twin still
            // names its object. A twin mid-gesture is left alone: its own
            // change is written on the frame the gesture ends.
            let twins: Vec<(u64, PaneSide)> = self
                .tabs
                .iter()
                .flat_map(|other| {
                    other
                        .panes()
                        .filter(|(pane, _)| {
                            pane.drawings_key.as_ref() == Some(&key) && !pane.drawings.in_gesture()
                        })
                        .map(move |(_, other_side)| (other.id, other_side))
                })
                .filter(|target| *target != (tab, side))
                .collect();
            for (twin_tab, twin_side) in twins {
                if let Some(twin) = self.pane_mut_at(twin_tab, twin_side) {
                    twin.drawings.take_all();
                }
                self.bring_out_drawings(twin_tab, twin_side);
            }
        }
    }
}

impl QuantickApp {
    // ------------------------------------------------------------------
    // Reload, rename, strip
    // ------------------------------------------------------------------

    /// Read the layouts file again and put it on every pane — after a
    /// workspace import replaced the file under the running app.
    ///
    /// `imported` names the stores the import wrote. A bundle from before
    /// layouts existed carries an `indicators` section and no `layouts`
    /// section; reading the cockpit's own layouts file back would show the
    /// cockpit's indicators under a toast saying the bundle's were restored,
    /// so that case migrates the imported set into "Layout 1" instead —
    /// the same migration a launch performs.
    pub(super) fn reload_layouts(&mut self, imported: &[&str]) {
        self.clear_indicators();
        for tab in &mut self.tabs {
            for pane in tab.panes_mut() {
                pane.drawings.take_all();
                pane.drawings_key = None;
                pane.layout_seeded = false;
            }
        }
        let legacy = crate::indicators::state_file::default_path();
        let migrate_imported_indicators =
            imported.contains(&"indicators") && !imported.contains(&"layouts");
        let (book, blocked) = if migrate_imported_indicators {
            let set = crate::indicators::state_file::load(&legacy);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUTS_MIGRATED",
                indicators = set.len(),
                action = "imported_indicator_state_moved_into_layout_1",
                "the imported bundle predates layouts; its indicator set became Layout 1"
            );
            (LayoutBook::starter(set), false)
        } else {
            Self::load_layouts(&self.layouts_path, &legacy)
        };
        self.layouts = book;
        self.layouts_save_blocked = blocked;
        self.layout_rename = None;
        self.layout_delete_confirm = None;
        self.seed_new_panes();
        // The screen is now the file's; nothing changed since — unless the
        // book was made from an imported indicator set, which the file does
        // not hold yet.
        self.layouts_dirty = migrate_imported_indicators;
        self.last_layout_change = migrate_imported_indicators.then(Instant::now);
    }

    /// Open the rename box on `id`, seeded with its current name.
    pub(crate) fn begin_layout_rename(&mut self, id: LayoutId) {
        if let Some(layout) = self.layouts.get(id) {
            self.layout_rename = Some((id, layout.name.clone()));
        }
    }

    /// Draw the strip and apply what it asked for.
    pub(super) fn draw_layout_strip(&mut self, ctx: &eframe::egui::Context) {
        let actions = {
            let can_add = self.layouts.layouts().len() < layouts::MAX_LAYOUTS;
            let can_delete = self.layouts.layouts().len() > 1;
            let Self {
                layouts,
                layout_rename,
                ..
            } = self;
            crate::layout_strip::draw(
                ctx,
                crate::layout_strip::StripModel {
                    layouts: layouts.layouts(),
                    active: layouts.active_id(),
                    rename: layout_rename,
                    can_add,
                    can_delete,
                },
            )
        };
        for action in actions {
            self.apply_strip_action(action);
        }
    }

    /// One door for the strip, the menu and the keyboard's rename box.
    pub(crate) fn apply_strip_action(&mut self, action: crate::layout_strip::StripAction) {
        use crate::layout_strip::StripAction;
        let outcome: Result<(), LayoutError> = match action {
            StripAction::Switch(id) => self.switch_layout(id).map(|_| ()),
            StripAction::Create => self.create_layout(None).map(|_| ()),
            StripAction::BeginRename(id) => {
                self.begin_layout_rename(id);
                Ok(())
            }
            StripAction::CommitRename(id, name) => {
                self.layout_rename = None;
                // An empty box is a cancelled rename, not an error to show.
                match layouts::clean_name(&name) {
                    Some(_) => self.rename_layout(id, &name).map(|_| ()),
                    None => Ok(()),
                }
            }
            StripAction::CancelRename => {
                self.layout_rename = None;
                Ok(())
            }
            // Deleting takes the layout's drawings with it, on disk as well
            // as on screen: the one strip action that asks first.
            StripAction::Delete(id) => {
                if self.layouts.get(id).is_some() {
                    self.layout_delete_confirm = Some(id);
                }
                Ok(())
            }
        };
        if let Err(error) = outcome {
            self.note_workspace(error.to_string());
        }
    }
}

impl QuantickApp {
    /// The confirmation a delete waits on: the layout's name, what goes with
    /// it, and the two buttons. Enter deletes and Escape cancels — both
    /// consumed here, so neither reaches the chart's own key handling —
    /// and nothing else on the window is touched while it is up.
    pub(super) fn draw_layout_delete_confirm(&mut self, ctx: &eframe::egui::Context) {
        use eframe::egui;
        let Some(id) = self.layout_delete_confirm else {
            return;
        };
        let Some(layout) = self.layouts.get(id) else {
            self.layout_delete_confirm = None;
            return;
        };
        let name = layout.name.clone();
        let drawings: usize = layout.drawing_count();
        let mut decision: Option<bool> = None;
        egui::Window::new("Delete layout")
            .id(egui::Id::new("layout_delete_confirm"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(format!("Delete \"{name}\"?"));
                ui.label(
                    egui::RichText::new(if drawings == 0 {
                        "Its indicator set goes with it. Nothing is drawn under it.".to_owned()
                    } else {
                        format!(
                            "Its indicator set and the {drawings} drawing(s) kept under it go with it. This cannot be undone."
                        )
                    })
                    .small()
                    .color(crate::theme::TEXT_SUPPORT),
                );
                ui.horizontal(|ui| {
                    if ui.button("Delete").clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            decision = Some(false);
        } else if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            decision = Some(true);
        }
        match decision {
            Some(true) => self.confirm_layout_delete(),
            Some(false) => self.layout_delete_confirm = None,
            None => {}
        }
    }

    /// The confirmed half of a delete: what the dialog's Delete button does.
    pub(crate) fn confirm_layout_delete(&mut self) {
        let Some(id) = self.layout_delete_confirm.take() else {
            return;
        };
        if let Err(error) = self.delete_layout(id) {
            self.note_workspace(error.to_string());
        }
    }
}
