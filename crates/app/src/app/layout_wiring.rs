//! How the app keeps every pane equal to *its* layout ([`crate::layouts`]):
//! the indicator fan-out, the drawings a market keeps, and the layout
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
//! **Drawings: put away and brought out.** Each pane holds what one
//! [`DrawingKey`] shows at its bars (`market_settings::pane_marks`), and no
//! layout. A layout switch leaves it on the pane; a tab moving market, or a
//! chart to other bars, stores it and adopts the new key's, ids kept. A pane's
//! revision counter is compared each frame; when it moved, the set goes to the
//! market store, the panes it reaches are rebuilt and the save follows.
//!
//! Per-frame cost: integer and short string compares per pane. Nothing here
//! allocates on a quiet frame.

use crate::state::BarConfiguration;
use crate::surfaces::drawing_chrome::DrawingChromeExt as _;
use quantick_drawings::{PaneMarks, PaneSync};
use quantick_workspace::market_settings::{MarketSettings, PaneWrite};
use quantick_workspace::session::{LayoutEffect, PaneFacts, Selection};
use std::time::Instant;

use crate::indicator_worker::{IndicatorCommand, SlotId};
use crate::layouts::{self, DrawingKey, LayoutBook, LayoutError, LayoutId};
use crate::pane::{ChartPane, DrawingDrag, PaneIndex, PaneSide};

use super::TabSlot;
use crate::workspace_store::LayoutStore;
use quantick_stores::market_settings;

mod bar_progress;
mod indicators;
pub(super) use indicators::saved_inputs;
pub(crate) use indicators::set_indicator_mouse_vertical_line;
mod strip;

/// Whether a pane can be swapped under the trader right now.
///
/// A gesture in flight — a drag, a half-placed object — may address a band of
/// an indicator the switch takes off. It is the trader's to finish first, and
/// the refusal says so.
fn facts_of(pane: &ChartPane) -> PaneFacts {
    PaneFacts {
        gesture_in_flight: pane.drawings.in_gesture()
            || pane.drawings.draft().is_some()
            || !matches!(pane.gestures.drag, DrawingDrag::None),
    }
}

/// Re-anchor what a pane took from the store, once it has bars to anchor on.
fn settle_marks(pane: &mut ChartPane) {
    if pane.slots() == 0 {
        pane.defer_reanchor();
    } else {
        let slots = pane.slots();
        pane.reanchor_drawings(slots);
    }
    pane.layout.saved_revision = pane.drawings.revision();
}

/// A pane's side of the market store (see `quantick_drawings::PaneMarks`).
fn marks_of(pane: &mut ChartPane) -> PaneMarks<'_, BarConfiguration> {
    let now = *pane.state.spec();
    let (drawings, layout) = (&mut pane.drawings, &mut pane.layout);
    let rule = BarConfiguration::to_config_string;
    PaneMarks {
        drawings,
        layout,
        now,
        rule,
    }
}

/// Read-only layout projection over the actual session and current pane topology.
pub(crate) struct LayoutRead<'a> {
    pub(super) tabs: &'a crate::app::arrangement_host::ArrangementHost,
    pub(super) active: usize,
    pub(super) session: &'a quantick_workspace::session::LayoutSession,
    pub(super) market: &'a quantick_workspace::market_settings_store::MarketSettingsStore,
}
impl<'a> LayoutRead<'a> {
    /// A drawing gesture in flight on a pane holding `from`'s or `to`'s marks.
    pub(crate) fn drawing_gesture_on(&self, feed: &str, from: &str, to: &str) -> bool {
        self.tabs.iter_with_ids().any(|(_, tab)| {
            tab.panes().any(|(pane, _)| {
                let source = pane.layout.drawings_source.as_ref();
                pane.drawings.in_gesture()
                    && source.is_some_and(|(venue, symbol)| {
                        venue == feed && (symbol == from || symbol == to)
                    })
            })
        })
    }

    pub(crate) fn layouts(&self) -> &'a LayoutBook {
        self.session.book()
    }
    pub(crate) fn pane_layout(&self, tab: u64, side: PaneSide) -> LayoutId {
        let named = self
            .tabs
            .by_id(tab)
            .and_then(|tab| tab.pane_at(side.index()))
            .and_then(ChartPane::layout_id);
        self.session
            .book()
            .strip_layout(self.session.resolve_layout(named))
    }
    pub(crate) fn focused_pane_layout(&self) -> LayoutId {
        let tab_id = self.tabs.id_at(self.active);
        let tab = &self.tabs[self.active];
        self.pane_layout(tab_id, tab.focused_side())
    }
    /// The indicators the focused pane shows, or would show, on strip layout `id`.
    pub(crate) fn shown_count(&self, id: LayoutId) -> usize {
        let pane = self.tabs[self.active].focused_pane().id;
        self.session.shown_count(pane, id)
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
    pub(super) indicator_settings:
        &'a mut crate::surfaces::indicator_settings::IndicatorSettingsSurface,
    pub(super) dialogs: &'a mut crate::surfaces::layout_dialogs::LayoutDialogsSurface,
}

/// The layouts and the market store this cockpit opens with, the drawings an
/// older file left in its layouts already moved to their markets.
pub(super) fn open_layout_store(config: &crate::config::AppConfig) -> LayoutStore {
    let path = layouts::default_path();
    let (book, blocked) =
        layouts::load_or_starter(&path, &crate::indicators::state_file::default_path());
    let config = config.clone();
    let assets =
        std::rc::Rc::new(move |venue: &str, symbol: &str| config.market_asset(venue, symbol));
    market_settings::open_layout_store(book, path, blocked, layouts::market_settings_path(), assets)
}

impl LayoutAdapter<'_> {
    /// Promote exactly the selected live object, then verify its committed copy.
    pub(crate) fn resolve_alert_target(
        &mut self,
        selector: quantick_control_schema::alerts::DrawingSelector,
    ) -> Result<
        quantick_alerts::DurableDrawingRef,
        quantick_control_schema::alerts::TargetResolveError,
    > {
        use quantick_control_schema::alerts::TargetResolveError as Error;
        use quantick_workspace::drawing_identity;
        self.persist_changed_drawings();
        let tab = self.tabs.by_id(selector.tab_id).ok_or(Error::Missing)?;
        let (pane, side) = tab
            .panes()
            .find(|(pane, _)| pane.id == selector.pane_id)
            .ok_or(Error::Missing)?;
        let mut key = self
            .drawing_key(selector.tab_id, side)
            .ok_or(Error::Missing)?;
        if pane.layout.drawings_key.as_ref() != Some(&key) || pane.layout.is_behind() {
            return Err(Error::SourceChanged);
        }
        if facts_of(pane).gesture_in_flight {
            return Err(Error::Busy);
        }
        let id = quantick_drawings::DrawingId(selector.drawing_id);
        let mut matches = pane.drawings.items().iter().filter(|item| item.id == id);
        let drawing = matches.next().ok_or(Error::Missing)?;
        if matches.next().is_some() {
            return Err(Error::Ambiguous);
        }
        if drawing.all_timeframes() {
            key = DrawingKey::all_timeframes(key.market);
        }
        let existing = drawing.durable_identity.clone();
        let workspace = self.store.market().path().to_string_lossy().into_owned();
        if existing
            .as_ref()
            .is_some_and(|identity| !drawing_identity::belongs_to(identity, &key))
        {
            return Err(Error::SourceChanged);
        }
        // Known mirrors may hold stale undo state while a gesture is active.
        if self
            .tabs
            .iter()
            .flat_map(|tab| tab.panes())
            .any(|(pane, _)| {
                pane.layout
                    .drawings_key
                    .as_ref()
                    .is_some_and(|held| held.market == key.market)
                    && facts_of(pane).gesture_in_flight
            })
        {
            return Err(Error::Busy);
        }
        let proposed = match existing {
            Some(identity) => identity,
            None => {
                let mut bytes = [0_u8; 32];
                getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
                let entropy = bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                drawing_identity::issue(&workspace, &key, id.0, &entropy)
                    .map_err(|_| Error::Unavailable)?
            }
        };
        let identity = self
            .pane_mut_at(selector.tab_id, side)
            .ok_or(Error::Missing)?
            .drawings
            .ensure_identity(id, proposed)
            .map_err(|_| Error::Ambiguous)?;
        self.persist_changed_drawings();
        market_settings::commit(self.store.market_mut(), std::slice::from_ref(&key))
            .map_err(Error::WriteFailed)?;
        let text = std::fs::read_to_string(self.store.market().path())
            .map_err(|error| Error::WriteFailed(error.to_string()))?;
        let document = market_settings::parse_settings(&text)
            .map_err(|error| Error::WriteFailed(error.to_string()))?;
        drawing_identity::resolve(
            &identity,
            document
                .drawings(&key)
                .iter()
                .filter_map(|item| item.durable_identity.as_ref()),
        )
        .map_err(|_| Error::Ambiguous)?;
        Ok(identity)
    }

    /// Take every indicator off every pane.
    ///
    /// Straight off each pane's own collection rather than by walking
    /// `slot_kinds`: that list is bookkeeping for the state *file*, and an
    /// indicator can be on a pane without being in it — the autostart hooks
    /// add without registering, and `exclude_from_layout` removes
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
        self.indicators.registry.clear();
        self.mark_layouts_dirty();
    }

    fn active_tab(&self) -> &crate::tab::Tab {
        &self.tabs[self.active]
    }
    fn note_workspace(&mut self, message: String) {
        crate::surfaces::toast::note(self.toast, message, Instant::now());
    }
    fn end_inline_text_edit(&mut self) {
        self.drawing_chrome.commit_inline_text(self.tabs);
    }

    // ------------------------------------------------------------------
    // Boot and file
    // ------------------------------------------------------------------

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

    fn mark_market_dirty(&mut self) {
        self.store.market_mut().mark_changed(Instant::now());
    }

    /// Everything the frame owes the layouts: seed panes that just appeared,
    /// follow tabs that changed market, notice drawings that changed, and
    /// write the file once the last change has settled.
    pub(super) fn maintain_layouts(&mut self) {
        // First, so a twin brought to a claimed or left set has no edit to lose.
        self.persist_changed_drawings();
        self.seed_new_panes();
        self.follow_market_changes();
        self.persist_changed_drawings();
        market_settings::save_settled(self.store, Instant::now());
    }

    /// Write the file now, whatever the debounce says — the way out on exit,
    /// and the moment before a bundle export reads it.
    pub(super) fn flush_layouts(&mut self) {
        self.persist_changed_drawings();
        market_settings::flush(self.store, Instant::now());
    }

    // ------------------------------------------------------------------
    // Which pane shows which layout
    // ------------------------------------------------------------------

    /// The layout a pane shows: its own, or the book's default for a pane
    /// that has not been given one yet.
    pub(crate) fn pane_layout(&self, tab: u64, side: PaneSide) -> LayoutId {
        let own = self.pane_own_layout(tab, side);
        self.store.session().book().strip_layout(own)
    }

    /// The entry an edit on this pane writes to: its own copy, once it has one.
    fn pane_own_layout(&self, tab: u64, side: PaneSide) -> LayoutId {
        self.store
            .session()
            .resolve_layout(self.pane_at(tab, side).and_then(ChartPane::layout_id))
    }

    /// Every seeded pane on its tab's copy of its strip layout; see
    /// [`quantick_workspace::session::LayoutSession::own_copies`].
    pub(in crate::app) fn own_indicator_copies(&mut self) {
        let panes = self
            .tabs
            .iter_with_ids()
            .flat_map(|(id, tab)| tab.panes().map(move |(pane, _)| (id, pane.id)));
        let owned = self.store.session_mut().own_copies(panes);
        let reshow: Vec<_> = owned
            .reshow
            .iter()
            .filter_map(|id| self.pane_address(*id))
            .collect();
        for (tab, side) in reshow {
            let layout = self.layouts().get(self.pane_own_layout(tab, side));
            let set = layout.map(|layout| layout.indicators.clone());
            self.show_pane_indicators(tab, side, &set.unwrap_or_default());
        }
        if owned.changed {
            self.mark_layouts_dirty();
        }
    }

    /// Where the pane with `id` stands.
    fn pane_address(&self, id: u64) -> Option<(u64, PaneSide)> {
        self.tabs.iter_with_ids().find_map(|(tab_id, tab)| {
            let mut panes = tab.panes();
            panes.find_map(|(pane, side)| (pane.id == id).then_some((tab_id, side)))
        })
    }

    /// Attach the core's read-only membership view to an actual pane.
    fn register_layout_pane(&mut self, tab: u64, side: PaneSide) -> Result<u64, LayoutError> {
        let pane = self.pane_at(tab, side).ok_or(LayoutError::Unknown)?;
        let (id, named) = (pane.id, pane.layout_id());
        let view = self.store.session_mut().register_in(tab, id, named);
        if let Some(pane) = self.pane_mut_at(tab, side) {
            pane.layout.view = view;
            pane.layout.opening = None;
        }
        Ok(id)
    }

    /// The focused pane's address, for the calls that act on it.
    fn focused_target(&self) -> (u64, PaneSide) {
        let tab_id = self.tabs.active_id();
        let tab = self.active_tab();
        (tab_id, tab.focused_side())
    }

    /// How many panes show strip layout `layout`, on it or on a copy: the
    /// panes its delete moves. Counted, not collected: read every frame.
    pub(in crate::app) fn panes_on_count(&self, layout: LayoutId) -> usize {
        let session = self.store.session();
        self.tabs
            .iter()
            .flat_map(|tab| tab.panes())
            .filter(|(pane, _)| session.shows_origin(&pane.layout.view, layout))
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
                        .map(move |(pane, side)| ((tab_id, side), &pane.layout.view))
                }),
            )
            .collect()
    }

    /// Give a pane its strip layout's name for its header.
    fn refresh_layout_label(&mut self, tab: u64, side: PaneSide) {
        let name = self
            .pane_at(tab, side)
            .and_then(ChartPane::layout_id)
            .and_then(|id| self.layouts().get(self.layouts().strip_layout(id)))
            .map(|layout| layout.name.clone())
            .unwrap_or_default();
        if let Some(pane) = self.pane_mut_at(tab, side) {
            pane.layout.label = name;
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
    /// Its layout slots are taken off and the new layout's set is put on. Its
    /// drawings stay: they are the market's. Panes on other layouts are
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
        // Onto the tab's copy now, so an edit before the next frame is the tab's.
        self.own_indicator_copies();
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
                .pane_address(selection.pane())
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
                    tab.canvas.set_opening_layout(side, id);
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
    /// hides and styles) names a context chart by its slot. A move that left it
    /// behind would have the layout address the wrong pane's slots. The drawings
    /// travel with the pane: their key names its role, not its place. The one
    /// door for the View menu and the control
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
        self.indicators.registry.move_context_pane(tab_id, from, to);
        self.indicator_settings.update(
            quantick_ui_model::indicator_settings::Intent::ContextPaneMoved {
                tab: tab_id,
                from,
                to,
            },
        );
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
        self.indicators.registry.layout_slots_at(tab, side)
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

    /// The market a tab's panes are showing: see `market_settings::shown_market`.
    fn market_of(tab: &crate::tab::Tab) -> (&str, &str) {
        let recording = tab.link.replay.as_ref().map(|_| tab.symbol.as_str());
        let live = (tab.active.0.as_str(), tab.active.1.as_str());
        quantick_workspace::market_settings::shown_market(live, recording)
    }

    /// The market and role a pane's drawings belong to right now.
    fn drawing_key(&self, tab: u64, side: PaneSide) -> Option<DrawingKey> {
        let pane = self.pane_at(tab, side)?;
        let (venue, symbol) = Self::market_of(self.tabs.by_id(tab)?);
        let bars = (side != PaneSide::Flow).then(|| pane.state.spec().to_config_string());
        let asset = self.store.market().asset(venue, symbol);
        Some(DrawingKey::of_pane(venue, asset, bars))
    }

    /// A pane and the market store beside it, borrowed apart.
    fn pane_and_store(
        &mut self,
        tab: u64,
        side: PaneSide,
    ) -> Option<(&mut ChartPane, &mut MarketSettings)> {
        let pane = self.tabs.by_id_mut(tab)?.pane_at_mut(side.index())?;
        Some((pane, self.store.market_mut().settings_mut()))
    }

    /// Serialise what a pane holds into the market store under the key the pane
    /// says it holds, and empty the pane.
    fn put_away_drawings(&mut self, tab: u64, side: PaneSide) -> PaneWrite {
        let Some((pane, market)) = self.pane_and_store(tab, side) else {
            return PaneWrite::default();
        };
        marks_of(pane).put_away(market)
    }

    /// Adopt the drawings of the pane's current market and role. A context
    /// chart first claims what a legacy file kept under its address.
    fn bring_out_drawings(&mut self, tab: u64, side: PaneSide) {
        let Some(key) = self.drawing_key(tab, side) else {
            return;
        };
        // A twin mid-gesture would write its stale set over a claim when it
        // lets go; the slot waits in the file until the next time it is asked.
        let busy = self
            .tabs
            .iter()
            .flat_map(|tab| tab.panes())
            .any(|(pane, _)| {
                pane.layout.drawings_key.as_ref() == Some(&key) && facts_of(pane).gesture_in_flight
            });
        let market = self.store.market_mut();
        let claimed = !busy && market.settings_mut().claim_for(&key, side.index());
        if claimed {
            self.mark_market_dirty();
        }
        let grown = claimed.then(|| key.clone());
        let source = self.tabs.by_id(tab).map(Self::market_of);
        let source = source.map(|(venue, symbol)| (venue.to_owned(), symbol.to_owned()));
        self.adopt_marks(tab, side, &key, PaneSync::Adopt);
        let Some(pane) = self.pane_mut_at(tab, side) else {
            return;
        };
        pane.layout.drawings_key = Some(key);
        pane.layout.drawings_source = source;
        pane.layout.drawings_spec = Some(*pane.state.spec());
        if let Some(key) = grown {
            self.refresh_twins(Some((tab, side, &key, PaneWrite::ALL)));
        }
    }

    /// Take what `key` shows at the pane's held bars from the store, the `sync`
    /// way (`quantick_drawings::PaneSync`).
    fn adopt_marks(&mut self, tab: u64, side: PaneSide, key: &DrawingKey, sync: PaneSync) {
        let Some((pane, market)) = self.pane_and_store(tab, side) else {
            return;
        };
        marks_of(pane).sync(sync, market, key);
        settle_marks(pane);
    }

    /// A contract roll's half of the drawings. Contracts of one asset are one
    /// market, so nothing moves; a roll onto another asset copies what is on
    /// screen to it, and a pane already there shows the result instead of
    /// writing its old set back. The roll's rule refused a gesture on either
    /// contract before this.
    pub(crate) fn carry_market_drawings(&mut self, feed: &str, from: &str, to: &str) -> usize {
        self.persist_changed_drawings();
        let (to, carried) = self.store.market_mut().carry_roll(feed, from, to);
        if carried == 0 {
            return 0;
        }
        let holding: Vec<(u64, PaneSide, DrawingKey)> = self
            .tabs
            .iter_with_ids()
            .flat_map(|(tab_id, tab)| {
                tab.panes().filter_map(move |(pane, side)| {
                    let key = pane.layout.drawings_key.clone()?;
                    Some((tab_id, side, key))
                })
            })
            .filter(|(_, _, key)| key.market == to)
            .collect();
        for (tab, side, key) in holding {
            self.adopt_marks(tab, side, &key, PaneSync::Readopt);
        }
        self.mark_market_dirty();
        carried
    }

    /// A tab whose market moved out from under a pane, or a chart cut to other
    /// bars: the drawings go to the key they were drawn on and the new key's
    /// come out. A roll inside one asset lands on the same key. A chart
    /// cut to other bars with a strategy live stays on the key it holds until the
    /// strategy stops: it names one of these drawings, and a re-cut is no reason
    /// to kill it. Moving the tab to another market still does, as it always has.
    pub(super) fn follow_market_changes(&mut self) {
        let moved: Vec<(u64, PaneSide)> = self
            .tabs
            .iter_with_ids()
            .flat_map(|(tab_id, tab)| {
                let (feed, symbol) = Self::market_of(tab);
                tab.panes()
                    .filter(move |(pane, _)| {
                        let recuttable = pane.strategies.anchors.watching() == 0;
                        let bars = recuttable.then(|| pane.state.spec());
                        pane.layout.needs_rekey(feed, symbol, bars)
                    })
                    .map(move |(_, side)| (tab_id, side))
            })
            .collect();
        for (tab, side) in moved {
            self.leave_pane_gestures(tab, side);
            let left = self
                .pane_at(tab, side)
                .and_then(|pane| pane.layout.drawings_key.clone());
            let write = self.put_away_drawings(tab, side);
            self.bring_out_drawings(tab, side);
            if let Some(left) = left.filter(|_| write.changed()) {
                self.refresh_twins(Some((tab, side, &left, write)));
                self.mark_market_dirty();
            }
        }
    }

    /// Any pane whose drawings changed since they were last written has its
    /// set copied into the market store, at the bars it is cut to.
    pub(super) fn persist_changed_drawings(&mut self) {
        self.follow_quiet_recuts();
        self.refresh_twins(None);
        let changed: Vec<(u64, PaneSide)> = self
            .tabs
            .iter_with_ids()
            .flat_map(|(tab_id, tab)| {
                tab.panes()
                    .filter(|(pane, _)| {
                        pane.layout.drawings_dirty(pane.drawings.revision())
                            && !pane.drawings.in_gesture()
                    })
                    .map(move |(_, side)| (tab_id, side))
            })
            .collect();
        for (tab, side) in changed {
            let Some((pane, market)) = self.pane_and_store(tab, side) else {
                continue;
            };
            let Some((key, write)) = marks_of(pane).persist(market) else {
                continue;
            };
            if write.merged {
                settle_marks(pane);
            }
            if write.changed() {
                self.mark_market_dirty();
                self.refresh_twins(Some((tab, side, &key, write)));
            }
        }
    }

    /// Each frame before a write: `quantick_drawings::PaneMarks::follow_store`.
    fn follow_quiet_recuts(&mut self) {
        let store = self.store.market();
        for pane in self.tabs.iter_mut().flat_map(|tab| tab.panes_mut()) {
            marks_of(pane).follow_store(store);
        }
    }

    /// Every other pane a write reaches ([`PaneWrite::reaches`]), or with none
    /// every pane that waited one out, is brought to the store once it can take
    /// it (`quantick_drawings::PaneMarks::follow_write`), or its next edit
    /// writes a stale set over it. The selection crosses the rebuild by id.
    fn refresh_twins(&mut self, write: Option<(u64, PaneSide, &DrawingKey, PaneWrite)>) {
        let writer = write.map(|(tab, side, ..)| (tab, side));
        let twins: Vec<(u64, PaneSide, bool)> = self
            .tabs
            .iter_with_ids()
            .flat_map(|(id, tab)| tab.panes().map(move |(pane, side)| (id, side, pane)))
            .filter(|&(id, side, p)| writer.map_or(p.layout.is_behind(), |at| at != (id, side)))
            .map(|(id, side, pane)| (id, side, facts_of(pane).gesture_in_flight))
            .collect();
        let reach = write.map(|(_, _, key, write)| (key, write));
        for (tab, side, busy) in twins {
            let follow = |pane: &mut ChartPane| marks_of(pane).follow_write(reach, busy);
            if let Some((key, sync)) = self.pane_mut_at(tab, side).and_then(follow) {
                self.leave_pane_gestures(tab, side);
                self.adopt_marks(tab, side, &key, sync);
            }
        }
    }
}
