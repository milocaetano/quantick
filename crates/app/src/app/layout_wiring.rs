//! How the app keeps every pane equal to *its* layout ([`crate::layouts`]):
//! the indicator fan-out, the per-market drawing swap, and the layout
//! operations the strip, the menu, the keyboard and the control plane all
//! call.
//!
//! A child of `app` rather than a sibling so it can reach the app's own
//! fields: this *is* app logic, split off only so the file that holds it can
//! be read in one sitting.
//!
//! **A layout per pane.** The workspace holds one book of layouts, and each
//! pane shows one of them ([`ChartPane::layout`]). Two panes side by side
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

use std::time::Instant;

use crate::indicator_style::StyleOverride;
use crate::indicator_worker::{IndicatorCommand, IndicatorEvent, IndicatorSource, SlotId};
use crate::indicators::state_file::{SavedIndicator, SavedInput, SavedKind, SavedPlotStyle};
use crate::layouts::{self, DrawingKey, LayoutBook, LayoutError, LayoutId, Loaded, SavedDrawing};
use crate::pane::{ChartPane, DrawingDrag, PaneIndex, PaneSide};

use super::chrome::ChromeState;
use super::{QuantickApp, TabSlot};
use crate::workspace_store::LayoutSave;

mod indicators;
mod strip;

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
        self.workspace.layouts().book()
    }

    /// Record that the book changed. The flag and the clock are stamped
    /// together inside [`crate::workspace_store::LayoutStore`]; here they were
    /// two assignments a caller could half-perform.
    fn mark_layouts_dirty(&mut self) {
        self.workspace.layouts_mut().mark_changed(Instant::now());
    }

    /// Carry out what the store decided. The decision is not made here and
    /// cannot be: `take_save` and `take_flush` own the debounce and the blocked
    /// condition, and consume the pending change before this runs.
    fn act_on(&mut self, decision: LayoutSave) {
        match decision {
            LayoutSave::Wait => {}
            LayoutSave::Write => layouts::save(self.workspace.layouts_path(), self.layouts()),
            LayoutSave::Blocked => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUTS_SAVE_BLOCKED",
                path = %self.workspace.layouts_path().display(),
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
        let decision = self.workspace.layouts_mut().take_save(Instant::now());
        self.act_on(decision);
    }

    /// Write the file now, whatever the debounce says — the way out on exit,
    /// and the moment before a bundle export reads it.
    pub(super) fn flush_layouts(&mut self) {
        self.persist_changed_drawings();
        let decision = self.workspace.layouts_mut().take_flush();
        self.act_on(decision);
    }

    // ------------------------------------------------------------------
    // Which pane shows which layout
    // ------------------------------------------------------------------

    /// The layout a pane shows: its own, or the book's default for a pane
    /// that has not been given one yet.
    pub(crate) fn pane_layout(&self, tab: u64, side: PaneSide) -> LayoutId {
        self.pane_at(tab, side)
            .and_then(|pane| pane.layout)
            .filter(|id| self.layouts().get(*id).is_some())
            .unwrap_or_else(|| self.layouts().active_id())
    }

    /// The layout the focused pane of the active tab shows — what the strip
    /// lights and what `Alt+N` switches.
    pub(crate) fn focused_pane_layout(&self) -> LayoutId {
        let (tab, side) = self.focused_target();
        self.pane_layout(tab, side)
    }

    /// The focused pane's address, for the calls that act on it.
    fn focused_target(&self) -> (u64, PaneSide) {
        let tab = self.active_tab();
        (tab.id, tab.focused_side())
    }

    /// How many panes show `layout` — the same question [`Self::panes_on`]
    /// answers, for the callers that want only the count and would otherwise
    /// heap-allocate a list per frame to call `.len()` on it.
    fn panes_on_count(&self, layout: LayoutId) -> usize {
        self.tabs
            .iter()
            .flat_map(|tab| tab.panes())
            .filter(|(pane, _)| pane.layout == Some(layout))
            .count()
    }

    /// Every (tab, pane) showing `layout`, flow first per tab.
    fn panes_on(&self, layout: LayoutId) -> Vec<(u64, PaneSide)> {
        self.tabs
            .iter()
            .flat_map(|tab| {
                tab.panes()
                    .filter(move |(pane, _)| pane.layout == Some(layout))
                    .map(move |(_, side)| (tab.id, side))
            })
            .collect()
    }

    /// Give a pane its layout's name for its header.
    fn refresh_layout_label(&mut self, tab: u64, side: PaneSide) {
        let name = self
            .pane_at(tab, side)
            .and_then(|pane| pane.layout)
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

    /// Whether a pane can be swapped under the trader right now.
    ///
    /// A strategy armed on a region names that drawing; putting the drawing
    /// away would orphan the instance and drop it with no reason given. A
    /// gesture in flight — a drag, a half-placed object — addresses the
    /// store by index, and a swap under it would land on another layout's
    /// object. Both are the trader's to finish first, and the refusal says so.
    fn pane_swap_refusal(&self, tab: u64, side: PaneSide) -> Option<LayoutError> {
        let pane = self.pane_at(tab, side)?;
        if !pane.strategies.anchors.is_empty() {
            return Some(LayoutError::StrategyArmed);
        }
        if pane.drawings.in_gesture()
            || pane.drawings.draft().is_some()
            || !matches!(pane.gestures.drag, DrawingDrag::None)
        {
            return Some(LayoutError::GestureInFlight);
        }
        None
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
        if self.layouts().get(id).is_none() || !self.pane_is_real(tab, side) {
            return Err(LayoutError::Unknown);
        }
        let from = self.pane_layout(tab, side);
        // What the pane *shows*, not what its field holds: a pane that has
        // not been given a layout explicitly shows the book's default, and
        // comparing the raw `Option` would tear that pane down and build it
        // back identical — every script on it recompiled — to arrive where it
        // already was, and then report the move as a change.
        if from == id {
            if let Some(pane) = self.pane_mut_at(tab, side) {
                pane.layout = Some(id);
            }
            return Ok(false);
        }
        if let Some(refusal) = self.pane_swap_refusal(tab, side) {
            return Err(refusal);
        }
        // Whatever was being typed into a note on this pane belongs to the
        // layout going out, and is committed to it before the store is
        // swapped.
        self.leave_pane_gestures(tab, side);
        self.persist_changed_drawings();
        self.put_away_drawings(tab, side);
        self.remove_layout_indicators_at(tab, side);
        if let Some(pane) = self.pane_mut_at(tab, side) {
            pane.layout = Some(id);
            // This *is* the seed. Every other place that materialises a set
            // says so; leaving the flag off here lets `seed_new_panes` put
            // the same set on a second time, and a pane holding two copies
            // maps every later edit to the wrong layout entry.
            pane.layout_seeded = true;
        }
        let set = self
            .workspace
            .layouts()
            .book()
            .get(id)
            .map(|layout| layout.indicators.clone())
            .unwrap_or_default();
        self.materialize_indicators_at(tab, side, &set);
        self.bring_out_drawings(tab, side);
        self.refresh_layout_label(tab, side);
        // The trader's last pick is what a pane that opens next takes.
        let _ = self.workspace.layouts_mut().book_mut().switch(id);
        self.mark_layouts_dirty();
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
        if !self.pane_is_real(tab, side) {
            return Err(LayoutError::Unknown);
        }
        if let Some(refusal) = self.pane_swap_refusal(tab, side) {
            return Err(refusal);
        }
        let id = self.workspace.layouts_mut().book_mut().create(name)?;
        self.mark_layouts_dirty();
        self.switch_pane_layout(tab, side, id)?;
        Ok(id)
    }

    pub(crate) fn rename_layout(&mut self, id: LayoutId, name: &str) -> Result<bool, LayoutError> {
        let changed = self.workspace.layouts_mut().book_mut().rename(id, name)?;
        if changed {
            self.refresh_all_layout_labels();
            self.mark_layouts_dirty();
        }
        Ok(changed)
    }

    /// Delete a layout. Every pane showing it moves to its left neighbour
    /// first, so no pane is ever left on a layout that no longer exists.
    pub(crate) fn delete_layout(&mut self, id: LayoutId) -> Result<(), LayoutError> {
        if self.layouts().get(id).is_none() {
            return Err(LayoutError::Unknown);
        }
        if self.layouts().layouts().len() == 1 {
            return Err(LayoutError::Last);
        }
        let index = self.layouts().index_of(id).unwrap_or(0);
        let neighbour = self
            .workspace
            .layouts()
            .book()
            .at(index.saturating_sub(1))
            .filter(|layout| layout.id != id)
            .or_else(|| self.layouts().at(index + 1))
            .map(|layout| layout.id)
            .ok_or(LayoutError::Last)?;
        let showing = self.panes_on(id);
        // Every pane is checked before any is moved, so a refusal leaves the
        // layout and every pane exactly as they were.
        for (tab, side) in &showing {
            if let Some(refusal) = self.pane_swap_refusal(*tab, *side) {
                return Err(refusal);
            }
        }
        for (tab, side) in showing {
            self.switch_pane_layout(tab, side, neighbour)?;
        }
        self.workspace.layouts_mut().book_mut().delete(id)?;
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
    fn pane_address_exists(index: usize) -> bool {
        index <= crate::canvas_layout::MAX_CONTEXT_PANES
    }

    pub(super) fn apply_pane_layouts_hook(&mut self, names: &str) {
        let tab_id = self.active_tab().id;
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
                None => match self.workspace.layouts_mut().book_mut().create(Some(&name)) {
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
                if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
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
        if self.surfaces.drawing_chrome.inline_edit_is_on(tab, side) {
            self.end_inline_text_edit();
        }
        self.surfaces
            .drawing_chrome
            .drop_edit_baseline_on(tab, side);
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

    /// Every (tab, pane) there is, flow first per tab.
    fn layout_pane_targets(&self) -> Vec<(u64, PaneSide)> {
        self.tabs
            .iter()
            .flat_map(|tab| tab.sides().map(move |side| (tab.id, side)))
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
            .iter_mut()
            .find(|candidate| candidate.id == tab)
            .and_then(|candidate| candidate.pane_at_mut(side.index()))
    }

    /// See [`Self::pane_mut_at`].
    fn pane_at(&self, tab: u64, side: PaneSide) -> Option<&ChartPane> {
        self.tabs
            .iter()
            .find(|candidate| candidate.id == tab)
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
        let (focused_tab, focused_side) = self.focused_target();
        for (tab, side) in unseeded {
            let named = self
                .pane_at(tab, side)
                .and_then(|pane| pane.layout)
                .filter(|id| self.layouts().get(*id).is_some());
            // Its own tab's flow chart first, the focused pane only after:
            // a stack built in a *background* tab — a control-plane preset
            // lands on one by id — belongs beside the chart it opened next
            // to, not beside whatever market the trader happens to be
            // looking at somewhere else.
            let layout = named.unwrap_or_else(|| {
                self.pane_at(tab, PaneSide::Flow)
                    .filter(|pane| pane.layout_seeded)
                    .and_then(|pane| pane.layout)
                    .filter(|id| self.layouts().get(*id).is_some())
                    .or_else(|| {
                        self.pane_at(focused_tab, focused_side)
                            .filter(|pane| pane.layout_seeded)
                            .and_then(|pane| pane.layout)
                            .filter(|id| self.layouts().get(*id).is_some())
                    })
                    .unwrap_or_else(|| self.layouts().active_id())
            });
            if let Some(pane) = self.pane_mut_at(tab, side) {
                pane.layout_seeded = true;
                pane.layout = Some(layout);
            }
            let set = self
                .workspace
                .layouts()
                .book()
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
        let tab = self.tabs.iter().find(|candidate| candidate.id == tab)?;
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
        if let Some(target) = self.workspace.layouts_mut().book_mut().get_mut(layout) {
            target.set_drawings(&key, items);
        }
    }

    /// Adopt the pane's layout's drawings for the pane's current market.
    fn bring_out_drawings(&mut self, tab: u64, side: PaneSide) {
        let Some(key) = self.drawing_key(tab, side) else {
            return;
        };
        let layout = self.pane_layout(tab, side);
        let saved = self
            .workspace
            .layouts()
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
            if let Some(target) = self.workspace.layouts_mut().book_mut().get_mut(layout) {
                target.set_drawings(&key, items);
            }
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
                .iter()
                .flat_map(|other| {
                    other
                        .panes()
                        .filter(|(pane, _)| {
                            pane.drawings_key.as_ref() == Some(&key)
                                && pane.layout == Some(layout)
                                && !pane.drawings.in_gesture()
                        })
                        .map(move |(_, other_side)| (other.id, other_side))
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
