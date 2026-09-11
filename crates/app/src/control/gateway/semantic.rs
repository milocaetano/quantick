//! The frame emitter: what changed in the window since the last frame.
//!
//! Every frame the gateway is enabled, the window's shape is compared against
//! the baseline this file owns, and each difference is recorded as one event.
//! The comparison must allocate nothing on a quiet frame, which is why the
//! baseline holds owned copies of exactly the values that name a change, and
//! why the keys below are small enough to compare in place.

use serde_json::{Value, json};

use quantick_control::id::{EventKind, ModuleId};

use crate::app::QuantickApp;
use crate::metrics;

use super::super::feed::connection_state;
use super::super::interaction::{SelectionIdentity, selection_identity, selection_snapshot};
use super::super::journal::NewEvent;
use super::ControlAccess;

/// What the frame emitter remembers between frames: owned copies of the
/// values that name a change, refreshed only where one happened, so a quiet
/// frame compares in place and allocates nothing.
pub(super) struct SemanticBaseline {
    active_tab_id: u64,
    focused_pane_id: u64,
    selection: SelectionIdentity,
    tabs: Vec<TabKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TabKey {
    tab_id: u64,
    feed_id: String,
    symbol: String,
    connection: &'static str,
    /// `(playing, finished)` while a replay is linked.
    replay: Option<(bool, bool)>,
    /// What the trader has placed on each pane. Rebuilt only when it stops
    /// matching — see [`analysis_matches`].
    panes: Vec<PaneAnalysisKey>,
}

/// One pane's indicators and drawings, as the journal compares them.
///
/// This is a baseline, not a projection: it holds the least that can tell a
/// change apart, so the per-frame comparison touches integers and booleans and
/// allocates nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneAnalysisKey {
    pane_id: u64,
    /// Which half of the split this was. Kept so a pane the layout *closed*
    /// can still name its side in the events that retire what it held — by
    /// then there is no pane left to ask.
    side: crate::pane::PaneSide,
    indicators: Vec<IndicatorKey>,
    drawings: Vec<DrawingKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndicatorKey {
    slot: u64,
    kind: std::sync::Arc<str>,
    /// An evaluation error or a failed hot reload. Either one is what a client
    /// waiting on a compile is waiting for.
    failing: bool,
}

impl IndicatorKey {
    fn matches(&self, view: &crate::indicators::IndicatorView) -> bool {
        self.slot == view.slot.0
            && *self.kind == *view.kind
            && self.failing == (view.error.is_some() || view.stale.is_some())
    }

    fn of(view: &crate::indicators::IndicatorView) -> Self {
        Self {
            slot: view.slot.0,
            kind: std::sync::Arc::clone(&view.kind),
            failing: view.error.is_some() || view.stale.is_some(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DrawingKey {
    id: u64,
    locked: bool,
    hidden: bool,
    /// Whether the trader gave it a name — never the name, which is their own
    /// text and does not belong in the journal any more than on the wire.
    named: bool,
    /// `(actor kind, client name)` when something other than the trader's hand
    /// placed it.
    author: Option<(String, String)>,
}

impl DrawingKey {
    fn matches(&self, drawing: &crate::drawings::Drawing) -> bool {
        self.id == drawing.id.0
            && self.locked == drawing.locked
            && self.hidden == drawing.hidden
            && self.named == drawing.name.is_some()
            && match (&self.author, &drawing.author) {
                (None, None) => true,
                (Some((kind, name)), Some(author)) => {
                    kind == &author.actor_kind && name == &author.client_name
                }
                (None, Some(_)) | (Some(_), None) => false,
            }
    }

    fn of(drawing: &crate::drawings::Drawing) -> Self {
        Self {
            id: drawing.id.0,
            locked: drawing.locked,
            hidden: drawing.hidden,
            named: drawing.name.is_some(),
            author: drawing
                .author
                .as_ref()
                .map(|author| (author.actor_kind.clone(), author.client_name.clone())),
        }
    }
}

/// A drawing's author as the journal carries it. Absent is the trader's own
/// hand; anything else names who acted, under the same rule that labels an
/// inferred side.
fn author_payload(drawing: &crate::drawings::Drawing) -> Value {
    drawing.author.as_ref().map_or(Value::Null, |author| {
        json!({
            "actor_kind": author.actor_kind,
            "client_name": author.client_name,
        })
    })
}

fn tab_key(tab: &crate::tab::Tab) -> TabKey {
    TabKey {
        tab_id: tab.id,
        feed_id: tab.active.0.clone(),
        symbol: tab.active.1.clone(),
        connection: connection_state(tab.feed_connection),
        replay: replay_key(tab),
        panes: pane_analysis_keys(tab),
    }
}

/// The panes of one tab, in the order both analysis scopes publish them.
///
/// [`crate::tab::Tab::panes`] and not a local `Vec`: this is walked by
/// [`analysis_matches`] on every frame, and the version of this comparison
/// #223's review removed was removed for allocating exactly one vector per
/// frame. One walk shared with the scopes also keeps the journal's order and
/// the wire's order from drifting apart.
fn analysis_panes(
    tab: &crate::tab::Tab,
) -> impl Iterator<Item = (&crate::pane::ChartPane, crate::pane::PaneSide)> {
    tab.panes()
}

fn pane_analysis_keys(tab: &crate::tab::Tab) -> Vec<PaneAnalysisKey> {
    analysis_panes(tab)
        .map(|(pane, side)| PaneAnalysisKey {
            pane_id: pane.id,
            side,
            indicators: pane.indicators.all().iter().map(IndicatorKey::of).collect(),
            drawings: pane.drawings.items().iter().map(DrawingKey::of).collect(),
        })
        .collect()
}

/// Whether the stored baseline still describes this tab's panes.
///
/// Compared in place and allocating nothing: a quiet frame walks two slices
/// and returns. Only a frame that answers `false` pays for
/// [`pane_analysis_keys`] and the events below.
fn analysis_matches(tab: &crate::tab::Tab, stored: &[PaneAnalysisKey]) -> bool {
    let mut stored = stored.iter();
    let matched = analysis_panes(tab).all(|(pane, _side)| {
        let Some(key) = stored.next() else {
            return false;
        };
        {
            let indicators = pane.indicators.all();
            let drawings = pane.drawings.items();
            pane.id == key.pane_id
                && indicators.len() == key.indicators.len()
                && drawings.len() == key.drawings.len()
                && indicators
                    .iter()
                    .zip(&key.indicators)
                    .all(|(view, stored)| stored.matches(view))
                && drawings
                    .iter()
                    .zip(&key.drawings)
                    .all(|(drawing, stored)| stored.matches(drawing))
        }
    });
    // Every stored pane must have been consumed too: a closed pane leaves the
    // iterator short, and a tab that lost one has changed.
    matched && stored.next().is_none()
}

fn replay_key(tab: &crate::tab::Tab) -> Option<(bool, bool)> {
    tab.replay
        .as_ref()
        .map(|link| (link.status.is_playing(), link.status.is_finished()))
}

impl ControlAccess {
    /// Record the semantic changes since the last frame: tab, focus,
    /// selection, feed connection and market, replay state. The baseline is
    /// compared in place — a handful of integer and string comparisons — and
    /// refreshed only where something changed, so a quiet frame allocates
    /// nothing; with access disabled nothing runs at all, the journal starts
    /// when the human opens the door and records changes, not the state it
    /// found.
    pub(super) fn emit_semantic_changes(&mut self, app: &QuantickApp) {
        let tabs = app.control_tabs();
        let active = &tabs[app
            .control_active_tab_index()
            .min(tabs.len().saturating_sub(1))];
        let active_tab_id = active.id;
        let focused_pane_id = active.pane(active.focused_side()).id;
        let selection = selection_identity(app);
        let Some(mut baseline) = self.semantic_baseline.take() else {
            self.semantic_baseline = Some(SemanticBaseline {
                active_tab_id,
                focused_pane_id,
                selection,
                tabs: tabs.iter().map(tab_key).collect(),
            });
            return;
        };
        let now = metrics::wall_clock_ms();
        if baseline.active_tab_id != active_tab_id {
            baseline.active_tab_id = active_tab_id;
            self.record_observed(
                "workspace",
                "workspace.tab.activated",
                json!({ "tab_id": active_tab_id.to_string() }),
                now,
            );
        }
        if baseline.focused_pane_id != focused_pane_id {
            baseline.focused_pane_id = focused_pane_id;
            self.record_observed(
                "workspace",
                "workspace.focus.changed",
                json!({
                    "tab_id": active_tab_id.to_string(),
                    "pane_id": focused_pane_id.to_string(),
                }),
                now,
            );
        }
        if baseline.selection != selection {
            baseline.selection = selection;
            // The owned snapshot is built only now, for the event: it is the
            // same projection the selection scope publishes, so "changed"
            // means the same thing to the journal and to a capture.
            self.record_observed(
                "interaction",
                "interaction.selection.changed",
                json!({ "selection": selection_snapshot(app) }),
                now,
            );
        }
        for tab in tabs {
            match baseline.tabs.iter_mut().find(|old| old.tab_id == tab.id) {
                None => {
                    let key = tab_key(tab);
                    self.record_observed(
                        "workspace",
                        "workspace.tab.opened",
                        json!({ "tab_id": key.tab_id.to_string(), "feed_id": key.feed_id, "symbol": key.symbol }),
                        now,
                    );
                    baseline.tabs.push(key);
                }
                Some(old) => {
                    if old.feed_id != tab.active.0 || old.symbol != tab.active.1 {
                        old.feed_id.clone_from(&tab.active.0);
                        old.symbol.clone_from(&tab.active.1);
                        self.record_observed(
                            "feed",
                            "feed.market.changed",
                            json!({ "tab_id": tab.id.to_string(), "feed_id": old.feed_id, "symbol": old.symbol }),
                            now,
                        );
                    }
                    let connection = connection_state(tab.feed_connection);
                    if old.connection != connection {
                        old.connection = connection;
                        self.record_observed(
                            "feed",
                            "feed.connection.changed",
                            json!({ "tab_id": tab.id.to_string(), "state": connection }),
                            now,
                        );
                    }
                    let replay = replay_key(tab);
                    if old.replay != replay {
                        old.replay = replay;
                        self.record_observed(
                            "replay",
                            "replay.state.changed",
                            json!({
                                "tab_id": tab.id.to_string(),
                                "active": replay.is_some(),
                                "playing": replay.map(|(playing, _)| playing),
                                "finished": replay.map(|(_, finished)| finished),
                            }),
                            now,
                        );
                    }
                    // One comparison for both analysis scopes: it walks two
                    // slices per pane and allocates only once it has something
                    // to say.
                    if !analysis_matches(tab, &old.panes) {
                        self.record_analysis_changes(tab, &old.panes, now);
                        old.panes = pane_analysis_keys(tab);
                    }
                }
            }
        }
        if baseline.tabs.len() != tabs.len()
            || baseline
                .tabs
                .iter()
                .any(|old| !tabs.iter().any(|tab| tab.id == old.tab_id))
        {
            baseline.tabs.retain(|old| {
                let open = tabs.iter().any(|tab| tab.id == old.tab_id);
                if !open {
                    self.record_observed(
                        "workspace",
                        "workspace.tab.closed",
                        json!({ "tab_id": old.tab_id.to_string() }),
                        now,
                    );
                }
                open
            });
        }
        self.semantic_baseline = Some(baseline);
    }

    /// Journal what changed about one tab's indicators and drawings.
    ///
    /// Reached only from a frame where [`analysis_matches`] already answered
    /// `false`, so the allocation here is paid once per change and never on a
    /// quiet frame. Identity drives the diff: an indicator is its slot, a
    /// drawing is its id, and a list that merely reordered emits nothing.
    ///
    /// A slot is an address, not an identity — the pane hands it out again
    /// after a remove — so a slot whose kind changed is reported as the
    /// detach and the attach it really is, never as one instance mutating.
    fn record_analysis_changes(
        &mut self,
        tab: &crate::tab::Tab,
        stored: &[PaneAnalysisKey],
        now: i64,
    ) {
        let tab_id = tab.id.to_string();
        for (pane, side) in analysis_panes(tab) {
            let was = stored.iter().find(|key| key.pane_id == pane.id);
            let previous_indicators = was.map(|key| key.indicators.as_slice()).unwrap_or(&[]);
            let previous_drawings = was.map(|key| key.drawings.as_slice()).unwrap_or(&[]);
            let side = crate::control::PaneSideDto::from(side);
            let pane_id = pane.id.to_string();

            let indicators = pane.indicators.all();
            let drawings = pane.drawings.items();

            for view in indicators {
                match previous_indicators
                    .iter()
                    .find(|key| key.slot == view.slot.0)
                {
                    None => self.record_indicator_attached(
                        &tab_id,
                        &pane_id,
                        side,
                        view.slot.0,
                        view.kind.as_ref(),
                        now,
                    ),
                    // The pane reused the address for another constructor
                    // between two frames. Two events, because two instances.
                    Some(key) if *key.kind != *view.kind => {
                        self.record_indicator_detached(
                            &tab_id,
                            &pane_id,
                            side,
                            key.slot,
                            key.kind.as_ref(),
                            now,
                        );
                        self.record_indicator_attached(
                            &tab_id,
                            &pane_id,
                            side,
                            view.slot.0,
                            view.kind.as_ref(),
                            now,
                        );
                    }
                    Some(key) => {
                        let failing = view.error.is_some() || view.stale.is_some();
                        if key.failing != failing {
                            self.record_observed(
                                "analysis",
                                "analysis.indicator.compile_state.changed",
                                json!({
                                    "tab_id": tab_id,
                                    "pane_id": pane_id,
                                    "pane_side": side,
                                    "slot_id": view.slot.0.to_string(),
                                    "kind": view.kind.as_ref(),
                                    "failing": failing,
                                }),
                                now,
                            );
                        }
                    }
                }
            }
            for key in previous_indicators {
                if !indicators.iter().any(|view| view.slot.0 == key.slot) {
                    self.record_indicator_detached(
                        &tab_id,
                        &pane_id,
                        side,
                        key.slot,
                        key.kind.as_ref(),
                        now,
                    );
                }
            }

            for drawing in drawings {
                match previous_drawings.iter().find(|key| key.id == drawing.id.0) {
                    None => self.record_observed(
                        "analysis",
                        "analysis.drawing.created",
                        json!({
                            "tab_id": tab_id,
                            "pane_id": pane_id,
                            "pane_side": side,
                            "drawing_id": drawing.id.0.to_string(),
                            "tool_id": drawing.tool.id(),
                            "author": author_payload(drawing),
                        }),
                        now,
                    ),
                    Some(key) if !key.matches(drawing) => self.record_observed(
                        "analysis",
                        "analysis.drawing.edited",
                        json!({
                            "tab_id": tab_id,
                            "pane_id": pane_id,
                            "pane_side": side,
                            "drawing_id": drawing.id.0.to_string(),
                            "tool_id": drawing.tool.id(),
                            "locked": drawing.locked,
                            "hidden": drawing.hidden,
                            // Presence, never the text: the journal is held to
                            // the same rule as the wire.
                            "user_label_present": drawing.name.is_some(),
                            "author": author_payload(drawing),
                        }),
                        now,
                    ),
                    Some(_) => {}
                }
            }
            for key in previous_drawings {
                if !drawings.iter().any(|drawing| drawing.id.0 == key.id) {
                    self.record_drawing_removed(&tab_id, &pane_id, side, key.id, now);
                }
            }
        }

        // A pane the layout closed takes its whole arrangement with it. Walked
        // separately because the loop above only visits panes that still
        // exist, and a client left holding indicators and marks that are gone
        // is exactly what this journal is for.
        for key in stored {
            if analysis_panes(tab).any(|(pane, _side)| pane.id == key.pane_id) {
                continue;
            }
            let side = crate::control::PaneSideDto::from(key.side);
            let pane_id = key.pane_id.to_string();
            for indicator in &key.indicators {
                self.record_indicator_detached(
                    &tab_id,
                    &pane_id,
                    side,
                    indicator.slot,
                    indicator.kind.as_ref(),
                    now,
                );
            }
            for drawing in &key.drawings {
                self.record_drawing_removed(&tab_id, &pane_id, side, drawing.id, now);
            }
        }
    }

    fn record_indicator_attached(
        &mut self,
        tab_id: &str,
        pane_id: &str,
        side: crate::control::PaneSideDto,
        slot: u64,
        kind: &str,
        now: i64,
    ) {
        self.record_observed(
            "analysis",
            "analysis.indicator.attached",
            json!({
                "tab_id": tab_id,
                "pane_id": pane_id,
                "pane_side": side,
                "slot_id": slot.to_string(),
                "kind": kind,
            }),
            now,
        );
    }

    fn record_indicator_detached(
        &mut self,
        tab_id: &str,
        pane_id: &str,
        side: crate::control::PaneSideDto,
        slot: u64,
        kind: &str,
        now: i64,
    ) {
        self.record_observed(
            "analysis",
            "analysis.indicator.detached",
            json!({
                "tab_id": tab_id,
                "pane_id": pane_id,
                "pane_side": side,
                "slot_id": slot.to_string(),
                "kind": kind,
            }),
            now,
        );
    }

    fn record_drawing_removed(
        &mut self,
        tab_id: &str,
        pane_id: &str,
        side: crate::control::PaneSideDto,
        drawing_id: u64,
        now: i64,
    ) {
        self.record_observed(
            "analysis",
            "analysis.drawing.removed",
            json!({
                "tab_id": tab_id,
                "pane_id": pane_id,
                "pane_side": side,
                "drawing_id": drawing_id.to_string(),
            }),
            now,
        );
    }

    fn record_observed(&mut self, module: &str, kind: &str, payload: Value, now: i64) {
        self.journal.record(
            NewEvent {
                module_id: ModuleId::new(module).expect("static module ID is valid"),
                kind: EventKind::new(kind).expect("static event kind is valid"),
                actor: None,
                payload,
            },
            now,
        );
    }
}
