//! Owned indicator identities, provenance and deferred view state for one workspace.
//! Hosts retain allocation and workers; adapters retain clocks, file I/O and layout mirroring.
use crate::indicator_style::StyleOverride;
use quantick_chart_interaction::pane::PaneSide;
use quantick_indicator_session::{IndicatorSource, SlotId};
use quantick_workspace::{indicator_document::SavedKind, lifetime::WorkspaceId};
use std::collections::BTreeSet;
use std::time::SystemTime;

/// An indicator slot together with the tab and pane that own it.
///
/// Slot ids are allocated per pane, so the id alone identifies nothing once
/// there are two panes, let alone two tabs: without the rest, removing one
/// tab's slot 0 would drop another's bookkeeping for its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TabSlot {
    pub workspace: WorkspaceId,
    pub pane: u64,
    pub tab: u64,
    pub side: PaneSide,
    pub slot: SlotId,
}

impl TabSlot {
    pub fn address(self) -> quantick_workspace::lifetime::ResourceAddress {
        quantick_workspace::lifetime::ResourceAddress {
            workspace: self.workspace,
            tab: self.tab,
            pane: self.pane,
            slot: self.slot.0,
        }
    }
}

/// The two host effects needed by attachment and removal. No focus, layout,
/// library, compilation policy or authority can be changed through this port.
pub trait IndicatorHost {
    fn pane_id(&self) -> u64;
    fn add(&mut self, source: IndicatorSource) -> SlotId;
    fn remove(&mut self, slot: SlotId);
}

/// Resolves the existing headless view; the registry owns deferred application policy.
pub trait IndicatorViewHost {
    fn view_mut(&mut self, target: TabSlot) -> Option<&mut crate::indicator_views::IndicatorView>;
}

/// Registry state is changed only by lifecycle operations, never by lending its collections.
pub struct IndicatorRegistry {
    workspace: WorkspaceId,
    slot_kinds: Vec<(TabSlot, SavedKind)>,
    operator_slots: BTreeSet<TabSlot>,
    script_files: Vec<(TabSlot, usize, SystemTime)>,
    pending_hidden: Vec<TabSlot>,
    pending_styles: Vec<(TabSlot, StyleOverride)>,
    pending_mouse_vertical_lines: Vec<TabSlot>,
}

/// A numeric slot exists, but none of its registered targets is operator-owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotOperatorOwned;

/// The adapter has enough information to mirror a human change without the
/// operation borrowing layout state. Operator overlays have no layout entry.
pub struct IndicatorAttachment {
    pub target: TabSlot,
    pub layout_entry: Option<SavedKind>,
}

impl IndicatorRegistry {
    /// Native ids, including unknown ones, reach the worker without fallback.
    pub fn attach_native(
        &mut self,
        host: &mut impl IndicatorHost,
        (tab, side): (u64, PaneSide),
        id: &str,
    ) -> IndicatorAttachment {
        let slot = host.add(IndicatorSource::Native {
            id: id.to_owned(),
            values: Vec::new(),
        });
        let target = TabSlot {
            workspace: self.workspace,
            pane: host.pane_id(),
            tab,
            side,
            slot,
        };
        let kind = SavedKind::native(id);
        self.slot_kinds.push((target, kind.clone()));
        IndicatorAttachment {
            target,
            layout_entry: Some(kind),
        }
    }

    pub fn attach_script(
        &mut self,
        host: &mut impl IndicatorHost,
        (tab, side): (u64, PaneSide),
        name: String,
        text: String,
        by_operator: bool,
    ) -> IndicatorAttachment {
        let slot = host.add(IndicatorSource::Script {
            name: name.clone(),
            text,
        });
        let target = TabSlot {
            workspace: self.workspace,
            pane: host.pane_id(),
            tab,
            side,
            slot,
        };
        let kind = SavedKind::Script { name };
        self.slot_kinds.push((target, kind.clone()));
        if by_operator {
            self.operator_slots.insert(target);
        }
        IndicatorAttachment {
            target,
            layout_entry: (!by_operator).then_some(kind),
        }
    }

    /// Legacy v1 addressing: insertion-order first operator-owned match.
    /// Whole targets remain intact internally even when local numbers collide.
    pub fn operator_target(&self, slot: u64) -> Result<Option<TabSlot>, NotOperatorOwned> {
        let mut known = false;
        for (owner, _) in self.slot_kinds.iter() {
            if owner.slot.0 == slot {
                known = true;
                if self.operator_slots.contains(owner) {
                    return Ok(Some(*owner));
                }
            }
        }
        if known {
            Err(NotOperatorOwned)
        } else {
            Ok(None)
        }
    }

    /// Also used by silent layout removal. A pane that has already gone has
    /// no host effect, but its bookkeeping must still be retired.
    pub fn remove(&mut self, host: Option<&mut impl IndicatorHost>, target: TabSlot) {
        if let Some(host) = host {
            host.remove(target.slot);
        }
        self.slot_kinds.retain(|(owner, _)| *owner != target);
        self.operator_slots.remove(&target);
        self.script_files.retain(|(owner, ..)| *owner != target);
        self.pending_hidden.retain(|owner| *owner != target);
        self.pending_styles.retain(|(owner, _)| *owner != target);
        self.pending_mouse_vertical_lines
            .retain(|owner| *owner != target);
    }
}

impl IndicatorRegistry {
    pub fn new(workspace: WorkspaceId) -> Self {
        Self {
            workspace,
            slot_kinds: Vec::new(),
            operator_slots: BTreeSet::new(),
            script_files: Vec::new(),
            pending_hidden: Vec::new(),
            pending_styles: Vec::new(),
            pending_mouse_vertical_lines: Vec::new(),
        }
    }
    pub fn workspace(&self) -> WorkspaceId {
        self.workspace
    }
    pub fn slot_kinds(&self) -> &[(TabSlot, SavedKind)] {
        &self.slot_kinds
    }
    pub fn operator_slots(&self) -> &BTreeSet<TabSlot> {
        &self.operator_slots
    }
    pub fn script_files(&self) -> &[(TabSlot, usize, SystemTime)] {
        &self.script_files
    }
    pub fn pending_hidden(&self) -> &[TabSlot] {
        &self.pending_hidden
    }
    pub fn pending_styles(&self) -> &[(TabSlot, StyleOverride)] {
        &self.pending_styles
    }
    pub fn pending_mouse_vertical_lines(&self) -> &[TabSlot] {
        &self.pending_mouse_vertical_lines
    }
    pub fn contains(&self, target: TabSlot) -> bool {
        self.kind(target).is_some()
    }
    pub fn kind(&self, target: TabSlot) -> Option<&SavedKind> {
        self.slot_kinds
            .iter()
            .find(|(owner, _)| *owner == target)
            .map(|(_, kind)| kind)
    }
    /// Register a restored human slot after the adapter allocated its worker or error view.
    pub fn register_restored(&mut self, target: TabSlot, kind: SavedKind) -> bool {
        if target.workspace != self.workspace || self.contains(target) {
            return false;
        }
        self.slot_kinds.push((target, kind));
        true
    }
    /// Launch-only indicators stay live but do not enter layout persistence.
    pub fn exclude_from_layout(&mut self, target: TabSlot) {
        self.slot_kinds.retain(|(owner, _)| *owner != target);
    }
    pub fn watch_script(&mut self, target: TabSlot, library_index: usize, seen: SystemTime) {
        self.script_files.push((target, library_index, seen));
    }
    /// Record the supplied observation before attempting the adapter's file read.
    pub fn observe_script(&mut self, target: TabSlot, seen: SystemTime) {
        if let Some((_, _, previous)) = self
            .script_files
            .iter_mut()
            .find(|(owner, ..)| *owner == target)
        {
            *previous = seen;
        }
    }
    pub fn defer_hidden(&mut self, target: TabSlot) {
        self.pending_hidden.push(target);
    }
    pub fn set_deferred_hidden(&mut self, target: TabSlot, hidden: bool) {
        self.pending_hidden.retain(|owner| *owner != target);
        if hidden {
            self.defer_hidden(target);
        }
    }
    pub fn defer_style(&mut self, target: TabSlot, style: StyleOverride) {
        self.pending_styles.push((target, style));
    }
    pub fn replace_deferred_style(&mut self, target: TabSlot, style: StyleOverride) {
        self.pending_styles.retain(|(owner, _)| *owner != target);
        self.defer_style(target, style);
    }
    pub fn defer_mouse_vertical_line(&mut self, target: TabSlot) {
        self.pending_mouse_vertical_lines.push(target);
    }
    /// Retry only registered slots whose worker has not published a view yet.
    /// Preserve application order: hides, styles, then non-overlay mouse guides.
    pub fn apply_pending(&mut self, host: &mut impl IndicatorViewHost) {
        for target in std::mem::take(&mut self.pending_hidden) {
            if let Some(view) = host.view_mut(target) {
                view.hidden = !view.hidden;
            } else if self.contains(target) {
                self.defer_hidden(target);
            }
        }
        for (target, style) in std::mem::take(&mut self.pending_styles) {
            if let Some(view) = host.view_mut(target) {
                view.style = style;
            } else if self.contains(target) {
                self.defer_style(target, style);
            }
        }
        for target in std::mem::take(&mut self.pending_mouse_vertical_lines) {
            match host.view_mut(target) {
                Some(view) if !view.descriptor.overlay => view.mouse_vertical_line = true,
                None if self.contains(target) => self.defer_mouse_vertical_line(target),
                Some(_) | None => {}
            }
        }
    }
    pub fn clear(&mut self) {
        *self = Self::new(self.workspace);
    }
    /// Tab shutdown retires all metadata, including work waiting for a first view.
    pub fn forget_tab(&mut self, tab: u64) {
        self.slot_kinds.retain(|(owner, _)| owner.tab != tab);
        self.operator_slots.retain(|owner| owner.tab != tab);
        self.script_files.retain(|(owner, ..)| owner.tab != tab);
        self.pending_hidden.retain(|owner| owner.tab != tab);
        self.pending_styles.retain(|(owner, _)| owner.tab != tab);
        self.pending_mouse_vertical_lines
            .retain(|owner| owner.tab != tab);
    }
    /// Human slots in insertion order; operator overlays never become layout entries.
    pub fn layout_slots_at(&self, tab: u64, side: PaneSide) -> Vec<SlotId> {
        self.slot_kinds
            .iter()
            .map(|(owner, _)| owner)
            .filter(|owner| owner.tab == tab && owner.side == side)
            .filter(|owner| !self.operator_slots.contains(owner))
            .map(|owner| owner.slot)
            .collect()
    }
    /// Follow a successful context-pane move without changing stable pane identity.
    pub fn move_context_pane(&mut self, tab: u64, from: usize, to: usize) {
        let reside = |owner: &mut TabSlot| {
            if owner.tab == tab {
                owner.side = remap_side(owner.side, from, to);
            }
        };
        for (owner, _) in &mut self.slot_kinds {
            reside(owner);
        }
        for (owner, _) in &mut self.pending_styles {
            reside(owner);
        }
        for owner in &mut self.pending_hidden {
            reside(owner);
        }
        for owner in &mut self.pending_mouse_vertical_lines {
            reside(owner);
        }
        for (owner, ..) in &mut self.script_files {
            reside(owner);
        }
        self.operator_slots = self
            .operator_slots
            .iter()
            .copied()
            .map(|mut owner| {
                reside(&mut owner);
                owner
            })
            .collect();
    }
}

/// The position after a context-pane move; UI-held captured targets use the same mapping.
pub fn remap_side(side: PaneSide, from: usize, to: usize) -> PaneSide {
    let index = side.index();
    PaneSide::from_index(if index == from {
        to
    } else if from < to && index > from && index <= to {
        index - 1
    } else if to < from && index >= to && index < from {
        index + 1
    } else {
        index
    })
}

#[cfg(test)]
mod tests;
