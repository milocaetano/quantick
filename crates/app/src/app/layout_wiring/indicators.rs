//! The indicator half of the layout wiring: putting one indicator on one
//! pane, materialising a layout's saved set, and mirroring an add, a
//! removal, a hide, an input edit or a style edit onto every pane showing
//! the same layout.

use super::LayoutAdapter;
use super::TabSlot;
use crate::indicator_style::StyleOverride;
use crate::indicator_worker::{IndicatorCommand, IndicatorSource, SlotId};
use crate::indicators::IndicatorView;
use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind, SavedPlotStyle};
use crate::indicators::state_file::{SavedInputExt, SavedPlotStyleExt};
use crate::layouts::LayoutId;
use crate::pane::PaneSide;
use quantick_indicator_session::unloaded_script_events;
use quantick_workspace::indicator_mirror::{self as mirror, LayoutEdit, MirrorAction, SavedInputs};

mod guide;
mod native;
pub(crate) use guide::set_indicator_mouse_vertical_line;

/// One mirrored action, addressed by tab id and side.
type Mirrored = MirrorAction<(u64, PaneSide), SlotId>;

/// Read the same durable values materialization uses, never a live preview.
pub(in crate::app) fn saved_inputs(
    tabs: &super::super::arrangement_host::ArrangementHost,
    indicators: &super::super::indicator_manager::IndicatorState,
    store: &crate::workspace_store::LayoutStore,
    owner: TabSlot,
) -> Option<Vec<quantick_indicators::InputValue>> {
    let pane = tabs.by_id(owner.tab)?.pane_at(owner.side.index())?;
    if tabs.owner() != owner.workspace || pane.id != owner.pane {
        return None;
    }
    let index = indicators
        .registry
        .layout_slots_at(owner.tab, owner.side)
        .iter()
        .position(|slot| *slot == owner.slot)?;
    let layout = store.session().resolve_layout(pane.layout_id());
    let entry = store.book().get(layout)?.indicators.get(index)?;
    Some(mirror::saved_inputs(&entry.inputs, SavedInput::to_value).into_values())
}

impl LayoutAdapter<'_> {
    /// Apply an owner-validated edit through the same layout and worker ports.
    pub(in crate::app) fn apply_indicator_edit(
        &mut self,
        edit: super::super::indicator_manager::IndicatorEdit,
    ) {
        use super::super::indicator_manager::IndicatorEdit;
        let target = match edit {
            IndicatorEdit::Attached(attached) => {
                if let Some(kind) = attached.layout_entry {
                    self.mirror_add(attached.target, &kind);
                }
                attached.target
            }
            IndicatorEdit::Hidden(target) => {
                self.mirror_hidden(target);
                target
            }
            IndicatorEdit::Remove(target) => {
                // Read its layout position before either mirror or origin cleanup.
                self.mirror_remove(target);
                let Some(tab) = self.tabs.by_id_mut(target.tab) else {
                    return;
                };
                self.indicators
                    .remove(Some(tab.pane_mut(target.side)), target);
                target
            }
            IndicatorEdit::Inputs(target, values) => {
                self.mirror_inputs(target, &values);
                target
            }
            IndicatorEdit::Style(target) => {
                self.mirror_style(target);
                target
            }
        };
        self.note_indicator_edit_at(target.tab, target.side);
    }

    fn slot_at(&self, tab: u64, side: PaneSide, slot: SlotId) -> Option<TabSlot> {
        Some(TabSlot {
            workspace: self.tabs.owner(),
            pane: self.pane_at(tab, side)?.id,
            tab,
            side,
            slot,
        })
    }

    /// Add one indicator to one pane, with no fan-out and no dirty mark —
    /// the primitive both the mirror and the materialisation are built on.
    pub(in crate::app) fn add_indicator_at(
        &mut self,
        tab: u64,
        side: PaneSide,
        kind: &SavedKind,
    ) -> Option<SlotId> {
        let source = match kind {
            // Unresolved on purpose: the worker owns the catalog, and an id
            // it does not know becomes an error slot naming it.
            SavedKind::Native { id } => IndicatorSource::Native {
                id: id.clone(),
                values: Vec::new(),
            },
            SavedKind::Script { name } => {
                // A script the library lacks or cannot read still takes a
                // slot, an error slot saying so: the layout addresses its
                // panes by entry index, so a missing slot would land every
                // later edit one entry off.
                let loaded = self.indicators.script_library.load(name);
                let file_info = (loaded.index)
                    .and_then(|index| self.indicators.script_library.file_info(index));
                let slot = match loaded.text {
                    Ok(text) => {
                        let pane = self.pane_mut_at(tab, side)?;
                        pane.add_indicator(IndicatorSource::Script {
                            name: name.clone(),
                            text,
                        })
                    }
                    Err(message) => {
                        if loaded.index.is_some() {
                            tracing::warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "INDICATOR_SCRIPT_UNREADABLE",
                                script = %name,
                                error = %message,
                                action = "error_slot_shown",
                                "cannot read an indicator script"
                            );
                        } else {
                            tracing::warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "INDICATOR_STATE_SCRIPT_MISSING",
                                script = %name,
                                action = "error_slot_shown",
                                "the layout references a script the library no longer has"
                            );
                        }
                        let pane = self.pane_mut_at(tab, side)?;
                        let slot =
                            (pane.indicators).allocate_slot(IndicatorSource::script_kind_id(name));
                        let title = quantick_stores::script_library::script_title(name);
                        for event in unloaded_script_events(slot, title, message) {
                            pane.indicators.apply(event);
                        }
                        slot
                    }
                };
                let owner = self
                    .slot_at(tab, side, slot)
                    .expect("existing indicator pane");
                self.indicators
                    .registry
                    .register_restored(owner, kind.clone());
                if let (Some(index), Some((_, mtime))) = (loaded.index, file_info) {
                    self.indicators.registry.watch_script(owner, index, mtime);
                }
                return Some(slot);
            }
        };
        let slot = self.pane_mut_at(tab, side)?.add_indicator(source);
        self.indicators.registry.register_restored(
            self.slot_at(tab, side, slot)
                .expect("existing indicator pane"),
            kind.clone(),
        );
        Some(slot)
    }

    /// Take every layout slot off one pane, view and worker alike.
    pub(super) fn remove_layout_indicators_at(&mut self, tab: u64, side: PaneSide) {
        for slot in self.layout_slots_at(tab, side) {
            self.remove_indicator_silently(
                self.slot_at(tab, side, slot)
                    .expect("existing indicator pane"),
            );
        }
    }

    /// Remove one slot with no fan-out and no dirty mark.
    fn remove_indicator_silently(&mut self, target: TabSlot) {
        let pane = self
            .tabs
            .by_id_mut(target.tab)
            .and_then(|tab| tab.pane_at_mut(target.side.index()));
        self.indicators.registry.remove(pane, target);
    }

    /// Put a whole saved set on one pane, as [`mirror::materialise`] plans it.
    pub(in crate::app) fn materialize_indicators_at(
        &mut self,
        tab: u64,
        side: PaneSide,
        set: &[SavedIndicator],
    ) {
        for entry in mirror::materialise(set, SavedInput::to_value) {
            let Some(slot) = self.add_indicator_at(tab, side, entry.kind) else {
                continue;
            };
            let owner = self
                .slot_at(tab, side, slot)
                .expect("existing indicator pane");
            match entry.inputs {
                SavedInputs::Bind(values) => {
                    if let Some(pane) = self.pane_mut_at(tab, side) {
                        pane.indicator_worker
                            .send(IndicatorCommand::SetInputs { slot, values });
                    }
                }
                SavedInputs::Dropped { saved, readable } => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_STATE_INPUTS_DROPPED",
                    kind = ?entry.kind,
                    saved,
                    readable,
                    action = "declared_defaults_used",
                    "saved indicator inputs could not be read; using the declared defaults"
                ),
                SavedInputs::Defaults => {}
            }
            let registry = &mut self.indicators.registry;
            if entry.hidden {
                registry.defer_hidden(owner);
            }
            if entry.mouse_vertical_line {
                registry.defer_mouse_vertical_line(owner);
            }
            if let Some(styles) = entry.plot_styles {
                let plots = styles.iter().copied().map(SavedPlotStyle::to_override);
                registry.defer_style(owner, StyleOverride::from_plots(plots.collect()));
            }
        }
    }

    /// A recipe's set replaces what the pane shows, and its own entries with it.
    /// Drawings stay: they are the market's.
    pub(in crate::app) fn show_pane_indicators(
        &mut self,
        tab: u64,
        side: PaneSide,
        set: &[SavedIndicator],
    ) {
        self.remove_layout_indicators_at(tab, side);
        let layout = self.pane_own_layout(tab, side);
        if let Some(entries) = self.store.session_mut().indicators_mut(layout) {
            *entries = set.to_vec();
        }
        self.materialize_indicators_at(tab, side, set);
        self.mark_layouts_dirty();
    }

    /// Each pane named shows its set, in whichever tab it stands.
    pub(in crate::app) fn show_sets(&mut self, sets: Vec<(u64, Vec<SavedIndicator>)>) {
        for (pane, set) in sets {
            if let Some((tab, side)) = self.pane_address(pane) {
                self.show_pane_indicators(tab, side, &set);
            }
        }
    }

    /// Apply queued hide flags and styles to the views that now exist.
    ///
    /// A queued hide means "hide this once it is born": views are born
    /// visible, so applying it is a toggle. Whoever queues one dequeues it
    /// again on the matching unhide ([`Self::mirror_hidden`]).
    pub(in crate::app) fn apply_pending_indicator_state(&mut self) {
        self.indicators.registry.apply_pending(
            &mut super::super::indicator_operations::IndicatorViewAdapter(self.tabs),
        );
    }

    /// The origin pane's layout and the index of the slot in it — the two
    /// coordinates every mirror writes to.
    fn edit_coordinates(&self, origin: TabSlot) -> Option<(LayoutId, usize)> {
        if origin.workspace != self.tabs.owner()
            || self.pane_at(origin.tab, origin.side)?.id != origin.pane
        {
            return None;
        }
        let slots = self.layout_slots_at(origin.tab, origin.side);
        let index = mirror::layout_index(&slots, &origin.slot)?;
        Some((self.pane_own_layout(origin.tab, origin.side), index))
    }

    /// Record `edit` in the origin's layout entry and return what every other
    /// pane on that layout must do; `None` for a slot the layout does not carry.
    fn mirror(&mut self, origin: TabSlot, edit: LayoutEdit) -> Option<Vec<Mirrored>> {
        let at = self.edit_coordinates(origin)?;
        let registry = &self.indicators.registry;
        let panes = self.tabs.iter_with_ids().flat_map(|(tab_id, tab)| {
            tab.panes().map(move |(pane, side)| mirror::PaneSlots {
                pane: (tab_id, side),
                layout: pane.layout.view.layout(),
                slots: registry.layout_slots_at(tab_id, side),
            })
        });
        let panes = panes.collect();
        let entries = self.store.session_mut().indicators_mut(at.0);
        Some(mirror::mirror(
            entries,
            at,
            &(origin.tab, origin.side),
            edit,
            panes,
        ))
    }

    /// [`Self::mirror`] for an edit of an existing slot: the slots it reaches.
    fn mirror_slots(&mut self, origin: TabSlot, edit: LayoutEdit) -> Option<Vec<TabSlot>> {
        let actions = self.mirror(origin, edit)?;
        let slots = actions.into_iter().filter_map(|action| match action {
            MirrorAction::Edit { pane, slot } => self.slot_at(pane.0, pane.1, slot),
            MirrorAction::Add { .. } => None,
        });
        Some(slots.collect())
    }

    /// The origin's own view, for the edit a mirror copies.
    fn view_at(&self, at: TabSlot) -> Option<&IndicatorView> {
        let pane = self.pane_at(at.tab, at.side)?;
        pane.indicators
            .all()
            .iter()
            .find(|view| view.slot == at.slot)
    }

    /// Mirror an add: the layout gains the entry, and the same kind goes on
    /// every other pane showing it, so the new indicator is on every such
    /// chart the frame it was asked for.
    /// The caller marks save intent once after completing the origin edit.
    pub(in crate::app) fn mirror_add(&mut self, origin: TabSlot, kind: &SavedKind) {
        let actions = self.mirror(origin, LayoutEdit::Add(kind.clone()));
        for action in actions.unwrap_or_default() {
            if let MirrorAction::Add {
                pane: (tab, side),
                kind,
            } = action
            {
                self.add_indicator_at(tab, side, &kind);
            }
        }
    }

    /// Mirror a removal by layout index: the entry goes, and the slot at that
    /// position on every other pane of the layout with it.
    /// The caller marks save intent once after removing the origin.
    pub(in crate::app) fn mirror_remove(&mut self, origin: TabSlot) {
        for target in self
            .mirror_slots(origin, LayoutEdit::Remove)
            .unwrap_or_default()
        {
            self.remove_indicator_silently(target);
        }
    }

    /// Mirror an eye toggle: the entry records it, and the same position on
    /// every other pane of the layout follows — now, or once its view is born.
    pub(in crate::app) fn mirror_hidden(&mut self, origin: TabSlot) {
        let Some(hidden) = self.view_at(origin).map(|view| view.hidden) else {
            return;
        };
        let Some(targets) = self.mirror_slots(origin, LayoutEdit::Hidden(hidden)) else {
            return;
        };
        for target in targets {
            let Some(pane) = self.pane_mut_at(target.tab, target.side) else {
                continue;
            };
            let view = pane.indicators.all().iter().find(|v| v.slot == target.slot);
            match view.map(|view| view.hidden) {
                Some(live) if live != hidden => pane.indicators.toggle_hidden(target.slot),
                Some(_) => {}
                // Unborn: the queue says what its first frame should be. An
                // unhide takes a queued hide back out, or the view would be
                // born and hidden a moment later.
                None => (self.indicators.registry).set_deferred_hidden(target, hidden),
            }
        }
        self.mark_layouts_dirty();
    }

    /// Mirror committed inputs: the entry records the values the trader
    /// applied, and the same position on every other pane of the layout is
    /// sent them.
    ///
    /// Committed, never previewed: the settings dialog's live preview goes
    /// to the origin's worker alone and never reaches here, so a slider
    /// mid-drag can never land in the file or on another chart.
    pub(in crate::app) fn mirror_inputs(
        &mut self,
        origin: TabSlot,
        values: &[quantick_indicators::InputValue],
    ) {
        let saved = values.iter().map(SavedInput::from_value).collect();
        let Some(targets) = self.mirror_slots(origin, LayoutEdit::Inputs(saved)) else {
            return;
        };
        for target in targets {
            if let Some(pane) = self.pane_mut_at(target.tab, target.side) {
                let (slot, values) = (target.slot, values.to_vec());
                pane.indicator_worker
                    .send(IndicatorCommand::SetInputs { slot, values });
            }
        }
        self.mark_layouts_dirty();
    }

    /// Mirror the origin's style layer: the entry records it, and the same
    /// position on every other pane of the layout wears it.
    pub(in crate::app) fn mirror_style(&mut self, origin: TabSlot) {
        let Some(style) = self.view_at(origin).map(|view| view.style.clone()) else {
            return;
        };
        let plots = style.plots().iter().copied();
        let saved = plots.map(SavedPlotStyle::from_override).collect();
        let Some(targets) = self.mirror_slots(origin, LayoutEdit::Styles(saved)) else {
            return;
        };
        for target in targets {
            let pane = self.pane_mut_at(target.tab, target.side);
            match pane.and_then(|pane| pane.indicators.view_mut(target.slot)) {
                Some(view) => view.style = style.clone(),
                None => (self.indicators.registry).replace_deferred_style(target, style.clone()),
            }
        }
        self.mark_layouts_dirty();
    }

    /// An indicator edit happened on a pane. The mirror that made it has
    /// already written the layout; this starts the save clock for edits
    /// that reach the layout by no other path.
    pub(in crate::app) fn note_indicator_edit_at(&mut self, _tab: u64, _side: PaneSide) {
        self.mark_layouts_dirty();
    }
}
