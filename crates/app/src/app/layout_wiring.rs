//! How the app keeps every pane equal to *its* layout ([`crate::layouts`]):
//! the indicator fan-out, the per-market drawing swap, and the layout
//! operations the strip, the menu, the keyboard and the control plane all
//! call.
//!
//! The adapter borrows only layout-related ports. The headless session owns
//! document and membership policy; this shell executes its ordered effects.
//!
//! **A layout per pane.** The workspace holds one book of layouts, and each
//! pane shows one of them ([`ChartPane::layout_id`]). Two panes side by side
//! may show two — a CVD on the flow chart, a lone average on the context
//! chart — or the same one, in which case they are two readings of one set
//! and are kept equal. The strip and the number keys switch the *focused*
//! pane; a pane that opens takes the focused pane's layout. The book's
//! `active` is the trader's last pick, and the default for a pane that has
//! none.
//!
//! **Indicators: the layout is edited, then mirrored to its panes.** An edit
//! on one pane — add, remove, hide, retune, restyle — is written into that
//! pane's layout entry *from the edit itself* (the kind added, the index
//! removed, the values the trader committed) and then applied to every other
//! pane showing the same layout, in every tab, by *layout index*: the n-th
//! indicator of a pane is the n-th of every pane on that layout. Nothing here
//! reads a view back to learn what the layout holds: a view is the worker's
//! answer, which lands later and may be a preview the trader will discard.
//!
//! Operator-attached scripts (the annotate tier) stay on the pane they were
//! attached to and out of every layout: they are an agent's overlay, removed
//! by the agent, and were never part of what a trader keeps.
//!
//! **Drawings: put away and brought out.** Each pane holds the drawings of
//! one [`DrawingKey`] under its own layout at a time. When its tab moves to
//! another market, or the pane switches layout, what it holds is serialised
//! under the old key and the new key's set is adopted — with the ids it had,
//! so a strategy armed on a region and an annotation an agent placed still
//! name their object, and anchored by market time. A pane's revision counter
//! is compared each frame; when it moved, the set is written into its layout
//! and the debounced save follows.
//!
//! Per-frame cost: one integer compare and two short string compares per
//! pane, and a flag test per pane for seeding. Nothing here allocates on a
//! quiet frame.

use crate::layouts::SavedDrawingExt;
use quantick_workspace::session::{LayoutEffect, PaneFacts, Selection};
use std::time::Instant;

use crate::indicator_worker::{IndicatorCommand, SlotId};
use crate::layouts::{self, DrawingKey, LayoutBook, LayoutError, LayoutId, Loaded, SavedDrawing};
use crate::pane::{ChartPane, DrawingDrag, PaneIndex, PaneSide};

use super::TabSlot;
use crate::workspace_store::LayoutSave;

mod indicators;
pub(crate) use indicators::set_indicator_mouse_vertical_line;
mod strip;

/// The feed half of a drawing key while a tab plays a recording.
///
/// A recording is its own market: its prices are the recorded day's, not
/// the live venue's, so a level drawn on it belongs to the recording's
/// symbol under this name and never lands on the live chart of the same
/// symbol, nor on another tab streaming it.
const REPLAY_FEED_KEY: &str = "replay";

/// Whether a pane can be swapped under the trader right now.
///
/// A strategy armed on a region names that drawing; putting the drawing
/// away would orphan the instance and drop it with no reason given. A
/// gesture in flight — a drag, a half-placed object — addresses the
/// store by index, and a swap under it would land on another layout's
/// object. Both are the trader's to finish first, and the refusal says so.
fn facts_of(pane: &ChartPane) -> PaneFacts {
    PaneFacts {
        strategy_armed: !pane.strategies.anchors.is_empty(),
        gesture_in_flight: pane.drawings.in_gesture()
            || pane.drawings.draft().is_some()
            || !matches!(pane.gestures.drag, DrawingDrag::None),
    }
}

/// Read-only layout projection over the actual session and current pane topology.
pub(crate) struct LayoutRead<'a> {
    pub(super) tabs: &'a crate::app::arrangement_host::ArrangementHost,
    pub(super) active: usize,
    pub(super) session: &'a quantick_workspace::session::LayoutSession,
}
impl<'a> LayoutRead<'a> {
    pub(crate) fn layouts(&self) -> &'a LayoutBook {
        self.session.book()
    }
    pub(crate) fn pane_layout(&self, tab: u64, side: PaneSide) -> LayoutId {
        let named = self
            .tabs
            .by_id(tab)
            .and_then(|tab| tab.pane_at(side.index()))
            .and_then(ChartPane::layout_id);
        self.session.resolve_layout(named)
    }
    pub(crate) fn focused_pane_layout(&self) -> LayoutId {
        let tab_id = self.tabs.id_at(self.active);
        let tab = &self.tabs[self.active];
        self.pane_layout(tab_id, tab.focused_side())
    }
}

/// Layout shell ports. No application root or unrelated workspace/surface state is reachable.
pub(crate) struct LayoutAdapter<'a> {
    pub(super) tabs: &'a mut crate::app::arrangement_host::ArrangementHost,
    pub(super) active: usize,
    pub(super) indicators: &'a mut super::indicator_manager::IndicatorState,
    pub(super) store: &'a mut crate::workspace_store::LayoutStore,
    pub(super) drawing_chrome: &'a mut crate::surfaces::DrawingChromeSurface,
    pub(super) toast: &'a mut crate::surfaces::ToastSurface,
    pub(super) rename: &'a mut Option<super::chrome::LayoutRename>,
    pub(super) delete_confirm: &'a mut Option<LayoutId>,
}

impl LayoutAdapter<'_> {
    /// Take every indicator off every pane.
    ///
    /// Straight off each pane's own collection rather than by walking
    /// `slot_kinds`: that list is bookkeeping for the state *file*, and an
    /// indicator can be on a pane without being in it — the autostart hooks
    /// add without registering, and `forget_last_indicator_state_change` pops
    /// an entry while leaving the indicator on screen. Clearing the list
    /// would have left those behind for the imported set to stack on top of.
    pub(super) fn clear_indicators(&mut self) {
        /// Empty one pane, view and worker alike.
        fn strip(pane: &mut crate::pane::ChartPane) {
            let slots: Vec<SlotId> = pane.indicators.all().iter().map(|view| view.slot).collect();
            for slot in slots {
                pane.indicators.remove(slot);
                pane.indicator_worker.send(IndicatorCommand::Remove(slot));
            }
        }
        for tab in self.tabs.iter_mut() {
            // Every pane the tab holds, not the two it used to. `panes_mut`
            // rather than `pane_mut(Time)`: the latter falls back to the flow
            // pane when a tab was never split, which would strip it twice, and
            // it stops at the *first* context chart — so the second stacked
            // chart kept its indicators while `slot_kinds` was cleared out from
            // under them, and the imported set stacked on top.
            for pane in tab.panes_mut() {
                strip(pane);
            }
        }
        self.indicators.slot_kinds.clear();
        self.indicators.operator_slots.clear();
        self.indicators.script_files.clear();
        self.indicators.pending_hidden.clear();
        self.indicators.pending_styles.clear();
        self.indicators.pending_mouse_vertical_lines.clear();
        self.mark_layouts_dirty();
    }

    fn active_tab(&self) -> &crate::tab::Tab {
        &self.tabs[self.active]
    }
    fn note_workspace(&mut self, message: String) {
        self.toast.note(message, Instant::now());
    }
    fn end_inline_text_edit(&mut self) {
        self.drawing_chrome.commit_inline_text(self.tabs);
    }

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
        self.store.book()
    }

    /// Record that the book changed. The flag and the clock are stamped
    /// together inside [`crate::workspace_store::LayoutStore`]; here they were
    /// two assignments a caller could half-perform.
    fn mark_layouts_dirty(&mut self) {
        self.store.mark_changed(Instant::now());
    }

    /// Carry out what the store decided. The decision is not made here and
    /// cannot be: `take_save` and `take_flush` own the debounce and the blocked
    /// condition, and consume the pending change before this runs.
    fn act_on(&mut self, decision: LayoutSave) {
        match decision {
            LayoutSave::Wait => {}
            LayoutSave::Write => layouts::save(self.store.path(), self.layouts()),
            LayoutSave::Blocked => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUTS_SAVE_BLOCKED",
                path = %self.store.path().display(),
                action = "file_left_untouched",
                "the layouts file could not be read at launch and was not set aside; this session's layouts are not written over it"
            ),
        }
    }

    /// Everything the frame owes the layouts: seed panes that just appeared,
    /// follow tabs that changed market, notice drawings that changed, and
    /// write the file once the last change has settled.
    pub(super) fn maintain_layouts(&mut self) {
        self.seed_new_panes();
        self.follow_market_changes();
        self.persist_changed_drawings();
        let decision = self.store.take_save(Instant::now());
        self.act_on(decision);
    }

    /// Write the file now, whatever the debounce says — the way out on exit,
    /// and the moment before a bundle export reads it.
    pub(super) fn flush_layouts(&mut self) {
        self.persist_changed_drawings();
        let decision = self.store.take_flush();
        self.act_on(decision);
    }

    // ------------------------------------------------------------------
    // Which pane shows which layout
    // ------------------------------------------------------------------

    /// The layout a pane shows: its own, or the book's default for a pane
    /// that has not been given one yet.
    pub(crate) fn pane_layout(&self, tab: u64, side: PaneSide) -> LayoutId {
        self.store
            .session()
            .resolve_layout(self.pane_at(tab, side).and_then(ChartPane::layout_id))
    }

    /// Attach the core's read-only membership view to an actual pane.
    fn register_layout_pane(&mut self, tab: u64, side: PaneSide) -> Result<u64, LayoutError> {
        let pane = self.pane_at(tab, side).ok_or(LayoutError::Unknown)?;
        let (id, named) = (pane.id, pane.layout_id());
        let view = self.store.session_mut().register(id, named);
        if let Some(pane) = self.pane_mut_at(tab, side) {
            pane.layout_view = view;
            pane.opening_layout = None;
        }
        Ok(id)
    }

    /// The focused pane's address, for the calls that act on it.
    fn focused_target(&self) -> (u64, PaneSide) {
        let tab_id = self.tabs.active_id();
        let tab = self.active_tab();
        (tab_id, tab.focused_side())
    }

    /// How many panes show `layout` — the same question [`Self::panes_on`]
    /// answers, for the callers that want only the count and would otherwise
    /// heap-allocate a list per frame to call `.len()` on it.
    fn panes_on_count(&self, layout: LayoutId) -> usize {
        let session = self.store.session();
        self.tabs
            .iter()
            .flat_map(|tab| tab.panes())
            .filter(|(pane, _)| session.shows(&pane.layout_view, layout))
            .count()
    }

    /// Physical order comes from topology; membership selection belongs to the core.
    fn panes_on(&self, layout: LayoutId) -> Vec<(u64, PaneSide)> {
        let session = self.store.session();
        session
            .matching(
                layout,
                self.tabs.iter_with_ids().flat_map(|(tab_id, tab)| {
                    tab.panes()
                        .map(move |(pane, side)| ((tab_id, side), &pane.layout_view))
                }),
            )
            .collect()
    }

    /// Give a pane its layout's name for its header.
    fn refresh_layout_label(&mut self, tab: u64, side: PaneSide) {
        let name = self
            .pane_at(tab, side)
            .and_then(ChartPane::layout_id)
            .and_then(|id| self.layouts().get(id))
            .map(|layout| layout.name.clone())
            .unwrap_or_default();
        if let Some(pane) = self.pane_mut_at(tab, side) {
            pane.layout_label = name;
        }
    }

    fn refresh_all_layout_labels(&mut self) {
        for (tab, side) in self.layout_pane_targets() {
            self.refresh_layout_label(tab, side);
        }
    }

    // ------------------------------------------------------------------
    // Layout operations
    // ------------------------------------------------------------------

    /// Whether a pane can be swapped under the trader right now; see [`facts_of`].
    fn pane_facts(&self, tab: u64, side: PaneSide) -> PaneFacts {
        self.pane_at(tab, side)
            .map_or(PaneFacts::default(), facts_of)
    }

    /// Make `id` the layout one pane shows.
    ///
    /// The pane's drawings go to the layout going out, its layout slots are
    /// taken off, the new layout's set is put on, and the new layout's
    /// drawings for the pane's market come out. Panes on other layouts are
    /// untouched; the book's default moves to `id`, so the next pane to open
    /// takes what the trader last picked.
    pub(crate) fn switch_pane_layout(
        &mut self,
        tab: u64,
        side: PaneSide,
        id: LayoutId,
    ) -> Result<bool, LayoutError> {
        let pane = self.register_layout_pane(tab, side)?;
        let selection = self
            .store
            .session()
            .select(pane, id, self.pane_facts(tab, side))?;
        self.apply_layout_selection(tab, side, selection)?;
        if !selection.changed() {
            return Ok(false);
        }
        let from = selection.from();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "LAYOUT_SWITCHED",
            tab,
            pane = side.index(),
            from = from.0,
            to = id.0,
            name = %self.layouts().get(id).map_or("", |layout| layout.name.as_str()),
            action = "pane_rematerialized",
            "a pane changed layout"
        );
        Ok(true)
    }

    /// Imperative adapter for the domain's explicit ordering, with no layout policy.
    fn apply_layout_selection(
        &mut self,
        tab: u64,
        side: PaneSide,
        selection: Selection,
    ) -> Result<(), LayoutError> {
        let mut committed = None;
        for effect in selection.effects() {
            match effect {
                LayoutEffect::LeaveGestures => self.leave_pane_gestures(tab, side),
                LayoutEffect::PersistChangedDrawings => self.persist_changed_drawings(),
                LayoutEffect::StoreOutgoingDrawings => self.put_away_drawings(tab, side),
                LayoutEffect::DetachIndicators => self.remove_layout_indicators_at(tab, side),
                LayoutEffect::CommitMembership => {
                    committed = Some(self.store.session_mut().commit_selection(selection)?)
                }
                LayoutEffect::MaterializeIndicators => {
                    let set = self
                        .layouts()
                        .get(selection.to())
                        .map(|layout| layout.indicators.clone())
                        .unwrap_or_default();
                    self.materialize_indicators_at(tab, side, &set);
                }
                LayoutEffect::RestoreDrawings => self.bring_out_drawings(tab, side),
                LayoutEffect::RefreshLabel => self.refresh_layout_label(tab, side),
                LayoutEffect::CommitDefault => self
                    .store
                    .session_mut()
                    .finish_selection(committed.take().ok_or(LayoutError::Unknown)?)?,
                LayoutEffect::MarkDirty => self.mark_layouts_dirty(),
            }
        }
        Ok(())
    }

    /// Switch the focused pane of the active tab — what the strip, the View
    /// menu and `Alt+N` do.
    pub(crate) fn switch_layout(&mut self, id: LayoutId) -> Result<bool, LayoutError> {
        let (tab, side) = self.focused_target();
        self.switch_pane_layout(tab, side, id)
    }

    /// Switch the focused pane to the layout at strip position `index`.
    pub(crate) fn switch_layout_index(&mut self, index: usize) -> Result<bool, LayoutError> {
        let id = self.layouts().at(index).ok_or(LayoutError::Unknown)?.id;
        self.switch_layout(id)
    }

    /// Add a layout and put it on the focused pane — a new tab opens where
    /// it was made, which is what a `+` on a strip means everywhere else.
    ///
    /// The switch is checked before the layout is made, so a refusal leaves
    /// the strip as it was rather than with a tab nobody asked to keep.
    pub(crate) fn create_layout(&mut self, name: Option<&str>) -> Result<LayoutId, LayoutError> {
        let (tab, side) = self.focused_target();
        self.create_layout_at(tab, side, name)
    }

    pub(crate) fn rename_layout(&mut self, id: LayoutId, name: &str) -> Result<bool, LayoutError> {
        let changed = self.store.session_mut().rename(id, name)?;
        if changed {
            self.refresh_all_layout_labels();
            self.mark_layouts_dirty();
        }
        Ok(changed)
    }

    /// Delete a layout. Every pane showing it moves to its left neighbour
    /// first, so no pane is ever left on a layout that no longer exists.
    pub(crate) fn delete_layout(&mut self, id: LayoutId) -> Result<(), LayoutError> {
        // Include unseeded panes named by an opening request without materializing them.
        for (tab, side) in self.layout_pane_targets() {
            self.register_layout_pane(tab, side)?;
        }
        let facts = self
            .tabs
            .iter()
            .flat_map(|tab| tab.panes().map(|(pane, _)| (pane.id, facts_of(pane))));
        let selections = self.store.session().plan_delete(id, facts)?;
        for selection in selections {
            let (tab, side) = self
                .tabs
                .iter_with_ids()
                .find_map(|(tab_id, tab)| {
                    tab.panes()
                        .find(|(pane, _)| pane.id == selection.pane())
                        .map(|(_, side)| (tab_id, side))
                })
                .ok_or(LayoutError::Unknown)?;
            self.switch_pane_layout(tab, side, selection.to())?;
        }
        self.store.session_mut().finish_delete(id)?;
        self.mark_layouts_dirty();
        Ok(())
    }

    /// The `QUANTICK_PANE_LAYOUTS` hook: one name per pane address of the
    /// active tab, comma-separated. A name the book lacks is created empty;
    /// an empty entry leaves that pane on what it has.
    /// Whether `index` is an address a canvas can hold at all: `0` the flow
    /// pane, then the context stack up to [`crate::canvas_layout::MAX_CONTEXT_PANES`].
    /// Distinct from `pane_is_real`, which asks whether the pane is standing
    /// *now* — a stack lands a frame after the layout that asked for it.
    #[cfg(any(feature = "scenario-harness", test))]
    fn pane_address_exists(index: usize) -> bool {
        index <= crate::canvas_layout::MAX_CONTEXT_PANES
    }

    #[cfg(any(feature = "scenario-harness", test))]
    pub(super) fn apply_pane_layouts_hook(&mut self, names: &str) {
        let tab_id = self.tabs.active_id();
        for (index, name) in names.split(',').enumerate() {
            let Some(name) = layouts::clean_name(name) else {
                continue;
            };
            let side = PaneSide::from_index(index);
            // Checked before anything is created: an entry past the last
            // address a canvas can hold reaches no pane, and creating its
            // layout first would leave the name behind in the trader's book
            // — written by the debounce — for a pane that was never set.
            if !Self::pane_address_exists(index) {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PANE_LAYOUTS_HOOK_REFUSED",
                    layout = %name,
                    pane = index,
                    action = "entry_ignored",
                    "QUANTICK_PANE_LAYOUTS named a pane address no canvas has"
                );
                continue;
            }
            let id = match self.layouts().by_name(&name).map(|layout| layout.id) {
                Some(id) => id,
                None => match self.store.session_mut().create(Some(&name)) {
                    Ok(id) => {
                        self.mark_layouts_dirty();
                        id
                    }
                    Err(error) => {
                        tracing::warn!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "PANE_LAYOUTS_HOOK_REFUSED",
                            layout = %name,
                            %error,
                            action = "entry_ignored",
                            "QUANTICK_PANE_LAYOUTS could not create the layout"
                        );
                        continue;
                    }
                },
            };
            // A context pane not built yet — the stack lands a frame later —
            // is told what to open on; a built pane is switched now.
            if !self.pane_is_real(tab_id, side) {
                if let Some(tab) = self.tabs.by_id_mut(tab_id) {
                    tab.set_opening_layout(side, id);
                }
                continue;
            }
            if let Err(error) = self.switch_pane_layout(tab_id, side, id) {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "PANE_LAYOUTS_HOOK_REFUSED",
                    layout = %name,
                    pane = index,
                    %error,
                    action = "entry_ignored",
                    "QUANTICK_PANE_LAYOUTS could not switch the pane"
                );
            }
        }
    }

    /// Close whatever off-canvas edit addresses this pane's drawings by
    /// index before its store is swapped: the inline text editor commits to
    /// the set going out, and the inspector's undo baseline is dropped rather
    /// than recorded against another set's object. Other panes' edits are
    /// left alone.
    fn leave_pane_gestures(&mut self, tab: u64, side: PaneSide) {
        if self.drawing_chrome.inline_edit_is_on(tab, side) {
            self.end_inline_text_edit();
        }
        self.drawing_chrome.drop_edit_baseline_on(tab, side);
    }

    /// Whether `side` names a pane the tab has built — `Tab::pane` answers
    /// with the flow pane for a context slot that does not exist yet, which
    /// a caller about to switch a pane must not mistake for the flow pane.
    fn pane_is_real(&self, tab: u64, side: PaneSide) -> bool {
        self.pane_at(tab, side).is_some()
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
        let Some(tab) = self.tabs.by_id_mut(tab_id) else {
            return false;
        };
        if !tab.move_context_pane(tab_id, from, to) {
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
        for (owner, _) in &mut self.indicators.slot_kinds {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        for (owner, _) in &mut self.indicators.pending_styles {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        for owner in &mut self.indicators.pending_hidden {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        for owner in &mut self.indicators.pending_mouse_vertical_lines {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        for (owner, ..) in &mut self.indicators.script_files {
            if owner.tab == tab_id {
                owner.side = reside(owner.side);
            }
        }
        let operator: Vec<TabSlot> = self
            .indicators
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
        self.indicators.operator_slots = operator.into_iter().collect();
        if self.indicators.indicator_settings_target.tab == tab_id {
            self.indicators.indicator_settings_target.side =
                reside(self.indicators.indicator_settings_target.side);
        }
        // The drawings travel with the pane; their key follows its address.
        if let Some(tab) = self.tabs.by_id_mut(tab_id) {
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

    /// Every (tab, pane) there is, flow first per tab.
    fn layout_pane_targets(&self) -> Vec<(u64, PaneSide)> {
        self.tabs
            .iter_with_ids()
            .flat_map(|(tab_id, tab)| tab.sides().map(move |side| (tab_id, side)))
            .collect()
    }

    /// One pane by address, or `None` when the tab or the pane is not there.
    ///
    /// Through `Tab::pane_at`, never `Tab::pane`: the latter answers a
    /// context slot the stack does not have with the *flow* pane, so a bad
    /// address would silently retarget an indicator, a drawing set or a
    /// layout onto the wrong chart instead of being refused.
    fn pane_mut_at(&mut self, tab: u64, side: PaneSide) -> Option<&mut ChartPane> {
        self.tabs
            .by_id_mut(tab)
            .and_then(|candidate| candidate.pane_at_mut(side.index()))
    }

    /// See [`Self::pane_mut_at`].
    fn pane_at(&self, tab: u64, side: PaneSide) -> Option<&ChartPane> {
        self.tabs
            .by_id(tab)
            .and_then(|candidate| candidate.pane_at(side.index()))
    }

    /// The layout's slots on one pane, in layout order: every slot the
    /// bookkeeping knows for the pane, minus the operator's overlays.
    fn layout_slots_at(&self, tab: u64, side: PaneSide) -> Vec<SlotId> {
        self.indicators
            .slot_kinds
            .iter()
            .map(|(owner, _)| *owner)
            .filter(|owner| owner.tab == tab && owner.side == side)
            .filter(|owner| !self.indicators.operator_slots.contains(owner))
            .map(|owner| owner.slot)
            .collect()
    }

    /// Panes that appeared since last frame — a tab opened, a split built —
    /// take a layout: the one a restored workspace named for them, else the
    /// focused pane's, else the book's default. Its indicators go on, and
    /// their market's drawings under it come out.
    pub(super) fn seed_new_panes(&mut self) {
        let unseeded: Vec<(u64, PaneSide)> = self
            .tabs
            .iter_with_ids()
            .flat_map(|(tab_id, tab)| {
                tab.panes()
                    .filter(|(pane, _)| !pane.layout_seeded())
                    .map(move |(_, side)| (tab_id, side))
            })
            .collect();
        if unseeded.is_empty() {
            return;
        }
        let (focused_tab, focused_side) = self.focused_target();
        for (tab, side) in unseeded {
            let Ok(pane) = self.register_layout_pane(tab, side) else {
                continue;
            };
            let flow = self.pane_at(tab, PaneSide::Flow).map(|pane| pane.id);
            let focused = self.pane_at(focused_tab, focused_side).map(|pane| pane.id);
            let Some(layout) = self.store.session_mut().seed(pane, flow, focused) else {
                continue;
            };
            let set = self
                .layouts()
                .get(layout)
                .map(|layout| layout.indicators.clone())
                .unwrap_or_default();
            self.materialize_indicators_at(tab, side, &set);
            self.bring_out_drawings(tab, side);
            self.refresh_layout_label(tab, side);
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
        let tab = self.tabs.by_id(tab)?;
        let (feed, symbol) = Self::market_of(tab);
        Some(DrawingKey {
            feed: feed.to_owned(),
            symbol: symbol.to_owned(),
            pane: side.index(),
        })
    }

    /// Serialise what a pane holds into its layout under the key the pane
    /// says it holds, and empty the pane.
    fn put_away_drawings(&mut self, tab: u64, side: PaneSide) {
        let layout = self.pane_layout(tab, side);
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
        self.store
            .session_mut()
            .store_drawings(layout, &key, items, |tool| {
                crate::drawings::DrawingTool::by_id(tool).is_some()
            });
    }

    /// Adopt the pane's layout's drawings for the pane's current market.
    fn bring_out_drawings(&mut self, tab: u64, side: PaneSide) {
        let Some(key) = self.drawing_key(tab, side) else {
            return;
        };
        let layout = self.pane_layout(tab, side);
        let saved = self
            .store
            .book()
            .get(layout)
            .and_then(|layout| layout.drawings(&key))
            .unwrap_or(&[]);
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
            .iter_with_ids()
            .flat_map(|(tab_id, tab)| {
                let (feed, symbol) = Self::market_of(tab);
                tab.panes()
                    .filter(move |(pane, _)| {
                        pane.layout_seeded()
                            && pane
                                .drawings_key
                                .as_ref()
                                .is_some_and(|key| key.feed != feed || key.symbol != symbol)
                    })
                    .map(move |(_, side)| (tab_id, side))
            })
            .collect();
        for (tab, side) in moved {
            self.leave_pane_gestures(tab, side);
            self.put_away_drawings(tab, side);
            self.bring_out_drawings(tab, side);
            self.mark_layouts_dirty();
        }
    }

    /// Any pane whose drawings changed since they were last written has its
    /// set copied into its layout.
    pub(super) fn persist_changed_drawings(&mut self) {
        let changed: Vec<(u64, PaneSide)> = self
            .tabs
            .iter_with_ids()
            .flat_map(|(tab_id, tab)| {
                tab.panes()
                    .filter(|(pane, _)| {
                        pane.drawings_key.is_some()
                            && pane.drawings.revision() != pane.drawings_saved_revision
                            && !pane.drawings.in_gesture()
                    })
                    .map(move |(_, side)| (tab_id, side))
            })
            .collect();
        for (tab, side) in changed {
            let layout = self.pane_layout(tab, side);
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
            self.store
                .session_mut()
                .store_drawings(layout, &key, items, |tool| {
                    crate::drawings::DrawingTool::by_id(tool).is_some()
                });
            self.mark_layouts_dirty();
            // Two panes on one market and one layout show one set of
            // drawings: the other pane holding this key under this layout
            // is brought to what was just written, rather than keeping a
            // copy that drifts until a switch. Ids travel, so a strategy or
            // an annotation on the twin still names its object.
            //
            // A twin is never *skipped* for having a selection, however
            // tempting: a twin left behind is a twin whose own next edit
            // writes its stale set over this key and takes the drawing just
            // made here with it. The selection is carried across the rebuild
            // by id instead, which is the thing the trader can see. Only a
            // gesture in flight still holds a twin back — that addresses the
            // store by index and a swap under it would land on another
            // object.
            let twins: Vec<(u64, PaneSide)> = self
                .tabs
                .iter_with_ids()
                .flat_map(|(other_id, other)| {
                    other
                        .panes()
                        .filter(|(pane, _)| {
                            pane.drawings_key.as_ref() == Some(&key)
                                && pane.layout_id() == Some(layout)
                                && !pane.drawings.in_gesture()
                        })
                        .map(move |(_, other_side)| (other_id, other_side))
                })
                .filter(|target| *target != (tab, side))
                .collect();
            for (twin_tab, twin_side) in twins {
                let selected_id = self.pane_at(twin_tab, twin_side).and_then(|twin| {
                    let index = twin.drawings.selected()?;
                    Some(twin.drawings.items().get(index)?.id)
                });
                if let Some(twin) = self.pane_mut_at(twin_tab, twin_side) {
                    twin.drawings.take_all();
                }
                self.bring_out_drawings(twin_tab, twin_side);
                // By id, not by index: the set that came back may hold one
                // more object than the one that went away, and the index the
                // trader selected now points at a different mark.
                if let Some(id) = selected_id
                    && let Some(twin) = self.pane_mut_at(twin_tab, twin_side)
                {
                    let index = twin
                        .drawings
                        .items()
                        .iter()
                        .position(|drawing| drawing.id == id);
                    twin.drawings.select(index);
                }
            }
        }
    }
}
