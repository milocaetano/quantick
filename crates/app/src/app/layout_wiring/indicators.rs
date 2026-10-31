//! The indicator half of the layout wiring: putting one indicator on one
//! pane, materialising a layout's saved set, and mirroring an add, a
//! removal, a hide, an input edit or a style edit onto every pane showing
//! the same layout.

use super::LayoutAdapter;
use super::TabSlot;
use crate::indicator_style::StyleOverride;
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind, SavedPlotStyle};
use crate::indicators::state_file::{SavedInputExt, SavedPlotStyleExt};
use crate::layouts::LayoutId;
use crate::pane::PaneSide;

mod guide;
pub(crate) use guide::set_indicator_mouse_vertical_line;

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

    /// Where a slot sits in its pane's layout, or `None` for a slot the
    /// layout does not carry — an operator's, or one a validation hook added.
    fn layout_index_of(&self, target: TabSlot) -> Option<usize> {
        self.layout_slots_at(target.tab, target.side)
            .iter()
            .position(|slot| *slot == target.slot)
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
                let index = self
                    .indicators
                    .script_library
                    .entries()
                    .iter()
                    .position(|candidate| candidate.name == *name);
                // A script the library no longer has still takes a slot — an
                // error slot saying so. The layout addresses its panes by
                // entry index, and a pane with one slot fewer than its layout
                // has entries would have every edit after the gap land one
                // entry off; and a row that says "not in the library" is the
                // honest picture of a set the trader cannot see whole.
                let read = match index {
                    Some(index) => self.indicators.script_library.read(index),
                    None => {
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "INDICATOR_STATE_SCRIPT_MISSING",
                            script = %name,
                            action = "error_slot_shown",
                            "the layout references a script the library no longer has"
                        );
                        Some(Err(format!("{name} is not in the script library")))
                    }
                };
                let file_info =
                    index.and_then(|index| self.indicators.script_library.file_info(index));
                let slot = match read {
                    Some(Ok(text)) => {
                        let pane = self.pane_mut_at(tab, side)?;
                        pane.add_indicator(IndicatorSource::Script {
                            name: name.clone(),
                            text,
                        })
                    }
                    Some(Err(message)) => {
                        if index.is_some() {
                            tracing::warn!(
                                target: "quantick::app",
                                schema_version = 1_u8,
                                event_code = "INDICATOR_SCRIPT_UNREADABLE",
                                script = %name,
                                error = %message,
                                action = "error_slot_shown",
                                "cannot read an indicator script"
                            );
                        }
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
                let owner = self
                    .slot_at(tab, side, slot)
                    .expect("existing indicator pane");
                self.indicators
                    .registry
                    .register_restored(owner, kind.clone());
                if let (Some(index), Some((_, mtime))) = (index, file_info) {
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

    /// Put a whole saved set on one pane: add, bind inputs, queue the hide
    /// and the style until the worker's first `Rebuilt` gives them a view.
    pub(in crate::app) fn materialize_indicators_at(
        &mut self,
        tab: u64,
        side: PaneSide,
        set: &[SavedIndicator],
    ) {
        for entry in set {
            let Some(slot) = self.add_indicator_at(tab, side, &entry.kind) else {
                continue;
            };
            let owner = self
                .slot_at(tab, side, slot)
                .expect("existing indicator pane");
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
            self.indicators.registry.defer_hidden(owner);
        }
        guide::queue_saved(self, owner, entry);
        if !entry.plot_styles.is_empty() {
            self.indicators.registry.defer_style(
                owner,
                StyleOverride::from_plots(
                    entry
                        .plot_styles
                        .iter()
                        .copied()
                        .map(SavedPlotStyle::to_override)
                        .collect(),
                ),
            );
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
        let index = self.layout_index_of(origin)?;
        Some((self.pane_layout(origin.tab, origin.side), index))
    }

    /// A layout's entry at a layout index, for an edit to write.
    fn layout_entry_mut(&mut self, layout: LayoutId, index: usize) -> Option<&mut SavedIndicator> {
        self.store
            .session_mut()
            .indicators_mut(layout)?
            .get_mut(index)
    }

    /// The other panes an edit on `origin` reaches: every pane on the same
    /// layout, minus the origin.
    fn mirror_targets(&self, origin: TabSlot, layout: LayoutId) -> Vec<(u64, PaneSide)> {
        self.panes_on(layout)
            .into_iter()
            .filter(|target| *target != (origin.tab, origin.side))
            .collect()
    }

    /// Mirror an add: the layout gains the entry, and the same kind goes on
    /// every other pane showing it, so the new indicator is on every such
    /// chart the frame it was asked for. Inputs start empty — "the declared
    /// defaults" — until the trader commits some.
    /// The caller marks save intent once after completing the origin edit.
    pub(in crate::app) fn mirror_add(&mut self, origin: TabSlot, kind: &SavedKind) {
        let Some((layout, index)) = self.edit_coordinates(origin) else {
            return;
        };
        let entry = SavedIndicator {
            kind: kind.clone(),
            hidden: false,
            mouse_vertical_line: false,
            inputs: Vec::new(),
            plot_styles: Vec::new(),
        };
        if let Some(target) = self.store.session_mut().indicators_mut(layout) {
            if index <= target.len() {
                target.insert(index, entry);
            } else {
                target.push(entry);
            }
        }
        for (tab, side) in self.mirror_targets(origin, layout) {
            self.add_indicator_at(tab, side, kind);
        }
    }

    /// Mirror a removal by layout index: the entry goes, and the slot at that
    /// position on every other pane of the layout with it.
    /// The caller marks save intent once after removing the origin.
    pub(in crate::app) fn mirror_remove(&mut self, origin: TabSlot) {
        let Some((layout, index)) = self.edit_coordinates(origin) else {
            return;
        };
        if let Some(target) = self.store.session_mut().indicators_mut(layout)
            && index < target.len()
        {
            target.remove(index);
        }
        for (tab, side) in self.mirror_targets(origin, layout) {
            if let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() {
                self.remove_indicator_silently(
                    self.slot_at(tab, side, slot)
                        .expect("existing indicator pane"),
                );
            }
        }
    }

    /// Mirror an eye toggle: the entry records it, and the same position on
    /// every other pane of the layout follows — now, or once its view is born.
    pub(in crate::app) fn mirror_hidden(&mut self, origin: TabSlot) {
        let Some((layout, index)) = self.edit_coordinates(origin) else {
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
        if let Some(entry) = self.layout_entry_mut(layout, index) {
            entry.hidden = hidden;
        }
        for (tab, side) in self.mirror_targets(origin, layout) {
            let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() else {
                continue;
            };
            let owner = self
                .slot_at(tab, side, slot)
                .expect("existing indicator pane");
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
                    self.indicators.registry.set_deferred_hidden(owner, hidden);
                }
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
        let Some((layout, index)) = self.edit_coordinates(origin) else {
            return;
        };
        if let Some(entry) = self.layout_entry_mut(layout, index) {
            entry.inputs = values.iter().map(SavedInput::from_value).collect();
        }
        for (tab, side) in self.mirror_targets(origin, layout) {
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
    /// position on every other pane of the layout wears it.
    pub(in crate::app) fn mirror_style(&mut self, origin: TabSlot) {
        let Some((layout, index)) = self.edit_coordinates(origin) else {
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
        if let Some(entry) = self.layout_entry_mut(layout, index) {
            entry.plot_styles = style
                .plots()
                .iter()
                .copied()
                .map(SavedPlotStyle::from_override)
                .collect();
        }
        for (tab, side) in self.mirror_targets(origin, layout) {
            let Some(slot) = self.layout_slots_at(tab, side).get(index).copied() else {
                continue;
            };
            let owner = self
                .slot_at(tab, side, slot)
                .expect("existing indicator pane");
            match self
                .pane_mut_at(tab, side)
                .and_then(|pane| pane.indicators.view_mut(slot))
            {
                Some(view) => view.style = style.clone(),
                None => {
                    self.indicators
                        .registry
                        .replace_deferred_style(owner, style.clone());
                }
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
