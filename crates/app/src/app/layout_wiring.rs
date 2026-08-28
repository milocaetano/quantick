//! How the app keeps every pane equal to the active layout
//! ([`crate::layouts`]): the indicator fan-out, the per-market drawing
//! swap, and the layout operations the strip, the menu, the keyboard and
//! the control plane all call.
//!
//! A child of `app` rather than a sibling so it can reach the app's own
//! fields: this *is* app logic, split off only so the file that holds it can
//! be read in one sitting.
//!
//! **Indicators: mirror now, reconcile on settle.** An edit on one pane —
//! add, remove, hide, retune, restyle — is mirrored onto every other pane on
//! the same frame, by *layout index*: the n-th indicator of a pane is the
//! n-th of every pane, which is what makes "the same set everywhere" hold
//! without a second identity scheme. A second later, once the workers have
//! answered and the views exist, the edited pane's set is snapshotted into
//! the active layout, every pane is checked against it, and the file is
//! written. The mirror is what the trader sees; the reconciliation is what
//! makes a dropped command or a slow script converge rather than drift.
//!
//! Operator-attached scripts (the annotate tier) stay on the pane they were
//! attached to and out of the layout: they are an agent's overlay, removed
//! by the agent, and were never part of what a trader keeps.
//!
//! **Drawings: put away and brought out.** Each pane holds the drawings of
//! one [`DrawingKey`] at a time. When its tab moves to another market, or
//! the window switches layout, what it holds is serialised under the old
//! key and the new key's set is adopted — anchored by market time, so a
//! level comes back on the bar it was drawn on. A pane's revision counter is
//! compared each frame; when it moved, the set is written into the layout
//! and the debounced save follows.
//!
//! Per-frame cost: one integer compare and two short string compares per
//! pane, and a flag test per pane for seeding. Nothing here allocates on a
//! quiet frame.

use std::time::Instant;

use crate::indicator_style::StyleOverride;
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind, SavedPlotStyle};
use crate::layouts::{self, DrawingKey, LayoutBook, LayoutError, LayoutId, Loaded, SavedDrawing};
use crate::pane::{ChartPane, PaneSide};

use super::{DEFAULT_EMA_LEN, INDICATOR_STATE_SAVE_DEBOUNCE, QuantickApp, TabSlot};

/// How long after the last layout edit the file is written. Drawing drags
/// and rename keystrokes come in bursts; one write per burst, off the frame
/// path.
const LAYOUTS_SAVE_DEBOUNCE: std::time::Duration = INDICATOR_STATE_SAVE_DEBOUNCE;

impl QuantickApp {
    // ------------------------------------------------------------------
    // Boot and file
    // ------------------------------------------------------------------

    /// Read the layouts file, or migrate the indicator set a cockpit kept
    /// before layouts existed, or start with one empty layout.
    pub(super) fn load_layouts(
        path: &std::path::Path,
        legacy_indicators: &std::path::Path,
    ) -> LayoutBook {
        match layouts::load(path) {
            Loaded::Book(book) => book,
            Loaded::Refused(_) => LayoutBook::default(),
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
                LayoutBook::starter(legacy)
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
            self.layouts_dirty = false;
            self.last_layout_change = None;
            layouts::save(&self.layouts_path, &self.layouts);
        }
    }

    /// Write the file now, whatever the debounce says — the way out on exit.
    pub(super) fn flush_layouts(&mut self) {
        self.persist_changed_drawings();
        if self.layouts_dirty {
            self.layouts_dirty = false;
            self.last_layout_change = None;
            layouts::save(&self.layouts_path, &self.layouts);
        }
    }

    // ------------------------------------------------------------------
    // Layout operations
    // ------------------------------------------------------------------

    /// Make `id` the active layout on every pane of every tab.
    pub(crate) fn switch_layout(&mut self, id: LayoutId) -> Result<bool, LayoutError> {
        if self.layouts.get(id).is_none() {
            return Err(LayoutError::Unknown);
        }
        if self.layouts.active_id() == id {
            return Ok(false);
        }
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
        // The reconciliation would only re-derive what was just placed.
        self.indicator_state_dirty = false;
        self.last_indicator_change = None;
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
    pub(crate) fn create_layout(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError> {
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

    /// The saved form of one pane's layout set, as its views stand now.
    ///
    /// A slot whose build failed keeps the inputs the layout already holds
    /// for it: rewriting them from the error's empty view would erase the
    /// trader's parameters before they had a chance to fix the script.
    fn snapshot_indicators_at(&self, tab: u64, side: PaneSide) -> Vec<SavedIndicator> {
        let Some(pane) = self.pane_at(tab, side) else {
            return Vec::new();
        };
        let previous = &self.layouts.active().indicators;
        self.layout_slots_at(tab, side)
            .into_iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let owner = TabSlot { tab, side, slot };
                let kind = self
                    .slot_kinds
                    .iter()
                    .find(|(candidate, _)| *candidate == owner)
                    .map(|(_, kind)| kind.clone())?;
                let view = pane.indicators.all().iter().find(|view| view.slot == slot);
                let Some(view) = view else {
                    // No view yet: the worker has not answered the add. Keep
                    // what the layout says about this position, if anything.
                    return previous.get(index).cloned().or(Some(SavedIndicator {
                        kind,
                        hidden: false,
                        inputs: Vec::new(),
                        plot_styles: Vec::new(),
                    }));
                };
                let inputs = if view.error.is_some() {
                    previous
                        .get(index)
                        .filter(|entry| entry.kind == kind)
                        .map_or_else(Vec::new, |entry| entry.inputs.clone())
                } else {
                    view.input_values
                        .iter()
                        .map(SavedInput::from_value)
                        .collect()
                };
                Some(SavedIndicator {
                    kind,
                    hidden: view.hidden,
                    inputs,
                    plot_styles: view
                        .style
                        .plots()
                        .iter()
                        .copied()
                        .map(SavedPlotStyle::from_override)
                        .collect(),
                })
            })
            .collect()
    }

    /// Bring one pane's set to `set`: in place when the kinds line up,
    /// rebuilt from scratch when they do not.
    fn sync_indicators_at(&mut self, tab: u64, side: PaneSide, set: &[SavedIndicator]) {
        let slots = self.layout_slots_at(tab, side);
        let kinds: Vec<SavedKind> = slots
            .iter()
            .filter_map(|slot| {
                let owner = TabSlot {
                    tab,
                    side,
                    slot: *slot,
                };
                self.slot_kinds
                    .iter()
                    .find(|(candidate, _)| *candidate == owner)
                    .map(|(_, kind)| kind.clone())
            })
            .collect();
        let same_shape = kinds.len() == set.len()
            && kinds
                .iter()
                .zip(set)
                .all(|(kind, entry)| *kind == entry.kind);
        if !same_shape {
            self.remove_layout_indicators_at(tab, side);
            self.materialize_indicators_at(tab, side, set);
            return;
        }
        for (slot, entry) in slots.into_iter().zip(set) {
            let owner = TabSlot { tab, side, slot };
            let Some(view) = self
                .pane_at(tab, side)
                .and_then(|pane| pane.indicators.all().iter().find(|view| view.slot == slot))
            else {
                continue;
            };
            let live_inputs: Vec<SavedInput> = view
                .input_values
                .iter()
                .map(SavedInput::from_value)
                .collect();
            let live_hidden = view.hidden;
            let live_styles: Vec<SavedPlotStyle> = view
                .style
                .plots()
                .iter()
                .copied()
                .map(SavedPlotStyle::from_override)
                .collect();
            let errored = view.error.is_some();
            if !errored && live_inputs != entry.inputs {
                let values: Vec<_> = entry
                    .inputs
                    .iter()
                    .filter_map(SavedInput::to_value)
                    .collect();
                if values.len() == entry.inputs.len()
                    && let Some(pane) = self.pane_mut_at(tab, side)
                {
                    pane.indicator_worker
                        .send(IndicatorCommand::SetInputs { slot, values });
                }
            }
            if live_hidden != entry.hidden
                && let Some(pane) = self.pane_mut_at(tab, side)
            {
                pane.indicators.toggle_hidden(slot);
            }
            if live_styles != entry.plot_styles
                && let Some(view) = self
                    .pane_mut_at(tab, side)
                    .and_then(|pane| pane.indicators.view_mut(slot))
            {
                view.style = StyleOverride::from_plots(
                    entry
                        .plot_styles
                        .iter()
                        .copied()
                        .map(SavedPlotStyle::to_override)
                        .collect(),
                );
            }
            let _ = owner;
        }
    }

    /// The settled half of an indicator edit: the edited pane's set becomes
    /// the layout's, every other pane is brought to it, and the file follows.
    pub(super) fn reconcile_indicators(&mut self) {
        let Some((tab, side)) = self.indicator_edit_origin.take() else {
            return;
        };
        if self.pane_at(tab, side).is_none() {
            return;
        }
        let set = self.snapshot_indicators_at(tab, side);
        if self.layouts.active().indicators != set {
            self.layouts.active_mut().indicators = set.clone();
            self.mark_layouts_dirty();
        }
        for (other_tab, other_side) in self.layout_pane_targets() {
            if (other_tab, other_side) == (tab, side) {
                continue;
            }
            self.sync_indicators_at(other_tab, other_side, &set);
        }
    }

    /// Mirror an add: the same kind goes on every other pane, so the new
    /// indicator is on every chart the frame it was asked for.
    pub(super) fn mirror_add(&mut self, origin: TabSlot, kind: &SavedKind) {
        if self.layout_index_of(origin).is_none() {
            return;
        }
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            self.add_indicator_at(tab, side, kind);
        }
    }

    /// Mirror a removal by layout index onto every other pane.
    pub(super) fn mirror_remove(&mut self, origin: TabSlot) {
        let Some(index) = self.layout_index_of(origin) else {
            return;
        };
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            if let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() {
                self.remove_indicator_silently(TabSlot { tab, side, slot });
            }
        }
    }

    /// Mirror an eye toggle onto the same position of every other pane.
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
        for (tab, side) in self.layout_pane_targets() {
            if (tab, side) == (origin.tab, origin.side) {
                continue;
            }
            let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() else {
                continue;
            };
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
                    let owner = TabSlot { tab, side, slot };
                    if hidden && !self.pending_hidden.contains(&owner) {
                        self.pending_hidden.push(owner);
                    }
                }
            }
        }
    }

    /// Mirror committed inputs onto the same position of every other pane.
    pub(super) fn mirror_inputs(
        &mut self,
        origin: TabSlot,
        values: &[quantick_indicators::InputValue],
    ) {
        let Some(index) = self.layout_index_of(origin) else {
            return;
        };
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
    }

    /// Mirror the origin's style layer onto the same position everywhere.
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
    }

    /// Note which pane an edit happened on, for the reconciliation.
    pub(super) fn note_indicator_edit_at(&mut self, tab: u64, side: PaneSide) {
        self.indicator_edit_origin = Some((tab, side));
        self.indicator_state_dirty = true;
        self.last_indicator_change = Some(Instant::now());
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

    fn drawing_key(&self, tab: u64, side: PaneSide) -> Option<DrawingKey> {
        let tab = self.tabs.iter().find(|candidate| candidate.id == tab)?;
        Some(DrawingKey {
            feed: tab.active.0.clone(),
            symbol: tab.active.1.clone(),
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
        let items: Vec<crate::drawings::Drawing> = self
            .layouts
            .active()
            .drawings(&key)
            .map(|saved| {
                saved
                    .iter()
                    .filter_map(|entry| entry.to_drawing(crate::drawings::DrawingId(0)))
                    .collect()
            })
            .unwrap_or_default();
        let Some(pane) = self.pane_mut_at(tab, side) else {
            return;
        };
        pane.drawings.adopt(items);
        // The saved bar offsets are the old series'; market time is what
        // puts each anchor back on its bar.
        let slots = pane.slots();
        pane.reanchor_drawings(slots);
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
                tab.panes()
                    .filter(|(pane, _)| {
                        pane.layout_seeded
                            && pane.drawings_key.as_ref().is_some_and(|key| {
                                key.feed != tab.active.0 || key.symbol != tab.active.1
                            })
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
            // rather than keeping a copy that drifts until a switch.
            let twins: Vec<(u64, PaneSide)> = self
                .tabs
                .iter()
                .flat_map(|other| {
                    other
                        .panes()
                        .filter(|(pane, _)| pane.drawings_key.as_ref() == Some(&key))
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
    pub(super) fn reload_layouts(&mut self) {
        self.clear_indicators();
        for tab in &mut self.tabs {
            for pane in tab.panes_mut() {
                pane.drawings.take_all();
                pane.drawings_key = None;
                pane.layout_seeded = false;
            }
        }
        let legacy = crate::indicators::state_file::default_path();
        self.layouts = Self::load_layouts(&self.layouts_path, &legacy);
        self.layout_rename = None;
        self.seed_new_panes();
        // The screen is now the file's; nothing changed since.
        self.layouts_dirty = false;
        self.last_layout_change = None;
        self.indicator_state_dirty = false;
        self.last_indicator_change = None;
        self.indicator_edit_origin = None;
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
    /// it, and the two buttons. Escape cancels; nothing else on the window
    /// is touched while it is up.
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
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            decision = Some(false);
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
