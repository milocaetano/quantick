//! The frame emitter's semantic journal: what changed since the last frame
//! about the tabs, the focus, the selection, the feeds, the replays and what
//! the trader placed on each pane.
//!
//! The window is read through [`SemanticSource`], one tab at a time as
//! [`SemanticTab`]; the baseline is compared in place — a handful of integer
//! and string comparisons — and refreshed only where something changed, so a
//! quiet frame allocates nothing. The events go to a [`SemanticJournal`],
//! which the control plane's event journal already is; the frame hands the
//! time in, nothing here reads a clock.

use quantick_chart::indicator_views::IndicatorView;
use quantick_control::id::{EventKind, ModuleId};
use quantick_control_host::{
    journal::{EventJournal, NewEvent},
    wire::PaneSideDto,
};
use quantick_control_schema::interaction::SelectionSnapshot;
use quantick_drawings::{Drawing, DrawingTool};
use serde_json::{Value, json};

use crate::analysis::AnalysisPane;

/// The side of a tab's split a pane sits on, as the selection names it: a
/// context chart keeps its slot, so a focus moving between two stacked
/// context charts is a change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FocusedSide {
    Flow,
    Time(usize),
}

/// What identifies the selection — the ids, never the content — so a frame
/// compares a few integers and builds the owned snapshot only on a change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionIdentity {
    pub active_tab_id: u64,
    pub focused_pane_id: u64,
    pub focused_pane_side: FocusedSide,
    /// `(pane_id, drawing_id)` of the selected drawing, if any.
    pub drawing: Option<(u64, u64)>,
    pub paper_trade_row: Option<usize>,
}

/// One open tab as the emitter reads it.
pub trait SemanticTab {
    /// The tab's stable id.
    fn tab_id(&self) -> u64;
    /// The feed the tab is on.
    fn feed_id(&self) -> &str;
    /// The symbol the tab is on.
    fn symbol(&self) -> &str;
    /// The feed connection's wire name.
    fn connection(&self) -> &'static str;
    /// `(playing, finished)` while a replay is linked.
    fn replay(&self) -> Option<(bool, bool)>;
    /// Visit every pane in the order the analysis scopes publish them — the
    /// one walk shared with the scopes, so the journal's order and the wire's
    /// order cannot drift apart.
    fn visit_panes(&self, visit: &mut dyn FnMut(PaneSideDto, &dyn AnalysisPane));
}

/// The window as the emitter reads it once a frame.
pub trait SemanticSource {
    /// The tab on screen.
    fn active_tab_id(&self) -> u64;
    /// The pane the keyboard drives on the tab on screen.
    fn focused_pane_id(&self) -> u64;
    /// What is selected, by id.
    fn selection(&self) -> SelectionIdentity;
    /// The selection as the selection scope publishes it — built only for
    /// the event, so "changed" means the same thing to the journal and to a
    /// capture.
    fn selection_snapshot(&self) -> SelectionSnapshot;
    /// How many tabs are open.
    fn tab_count(&self) -> usize;
    /// Visit every open tab in screen order.
    fn visit_tabs(&self, visit: &mut dyn FnMut(&dyn SemanticTab));
}

/// Where the emitter's observations land.
pub trait SemanticJournal {
    /// Record one observed change, with no actor, at `now`.
    fn record_observed(&mut self, module: &str, kind: &str, payload: Value, now: i64);
}

impl SemanticJournal for EventJournal {
    fn record_observed(&mut self, module: &str, kind: &str, payload: Value, now: i64) {
        self.record(
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

/// What the frame emitter remembers between frames: owned copies of the
/// values that name a change, refreshed only where one happened, so a quiet
/// frame compares in place and allocates nothing.
pub struct SemanticBaseline {
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
    side: PaneSideDto,
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
    fn matches(&self, view: &IndicatorView) -> bool {
        self.slot == view.slot.0
            && *self.kind == *view.kind
            && self.failing == (view.error.is_some() || view.stale.is_some())
    }

    fn of(view: &IndicatorView) -> Self {
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
    magnet: bool,
    hidden: bool,
    /// Whether the trader gave it a name — never the name, which is their own
    /// text and does not belong in the journal any more than on the wire.
    named: bool,
    /// `(actor kind, client name)` when something other than the trader's hand
    /// placed it.
    author: Option<(String, String)>,
}

impl DrawingKey {
    fn matches(&self, drawing: &Drawing<DrawingTool>) -> bool {
        self.id == drawing.id.0
            && self.locked == drawing.locked
            && self.magnet == drawing.own_magnet()
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

    fn of(drawing: &Drawing<DrawingTool>) -> Self {
        Self {
            id: drawing.id.0,
            locked: drawing.locked,
            magnet: drawing.own_magnet(),
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
fn author_payload(drawing: &Drawing<DrawingTool>) -> Value {
    drawing.author.as_ref().map_or(Value::Null, |author| {
        json!({
            "actor_kind": author.actor_kind,
            "client_name": author.client_name,
        })
    })
}

fn tab_key(tab: &dyn SemanticTab) -> TabKey {
    TabKey {
        tab_id: tab.tab_id(),
        feed_id: tab.feed_id().to_owned(),
        symbol: tab.symbol().to_owned(),
        connection: tab.connection(),
        replay: tab.replay(),
        panes: pane_analysis_keys(tab),
    }
}

fn pane_analysis_keys(tab: &dyn SemanticTab) -> Vec<PaneAnalysisKey> {
    let mut keys = Vec::new();
    tab.visit_panes(&mut |side, pane| {
        keys.push(PaneAnalysisKey {
            pane_id: pane.pane_id(),
            side,
            indicators: pane.indicators().all().iter().map(IndicatorKey::of).collect(),
            drawings: pane.drawings().items().iter().map(DrawingKey::of).collect(),
        });
    });
    keys
}

/// Whether the stored baseline still describes this tab's panes.
///
/// Compared in place and allocating nothing: a quiet frame walks two slices
/// and returns. Only a frame that answers `false` pays for
/// [`pane_analysis_keys`] and the events below.
fn analysis_matches(tab: &dyn SemanticTab, stored: &[PaneAnalysisKey]) -> bool {
    let mut seen = 0;
    let mut matched = true;
    tab.visit_panes(&mut |_side, pane| {
        if !matched {
            return;
        }
        let Some(key) = stored.get(seen) else {
            matched = false;
            return;
        };
        seen += 1;
        let indicators = pane.indicators().all();
        let drawings = pane.drawings().items();
        matched = pane.pane_id() == key.pane_id
            && indicators.len() == key.indicators.len()
            && drawings.len() == key.drawings.len()
            && indicators
                .iter()
                .zip(&key.indicators)
                .all(|(view, stored)| stored.matches(view))
            && drawings
                .iter()
                .zip(&key.drawings)
                .all(|(drawing, stored)| stored.matches(drawing));
    });
    // Every stored pane must have been consumed too: a closed pane leaves the
    // walk short, and a tab that lost one has changed.
    matched && seen == stored.len()
}

/// Whether a tab with `tab_id` is open.
fn tab_open<S: SemanticSource + ?Sized>(source: &S, tab_id: u64) -> bool {
    let mut open = false;
    source.visit_tabs(&mut |tab| open |= tab.tab_id() == tab_id);
    open
}

impl SemanticBaseline {
    /// What the emitter found when access was enabled: recorded as the
    /// baseline, never as events — the journal records changes, not the state
    /// it found.
    pub fn capture<S: SemanticSource + ?Sized>(source: &S) -> Self {
        let mut tabs = Vec::with_capacity(source.tab_count());
        source.visit_tabs(&mut |tab| tabs.push(tab_key(tab)));
        Self {
            active_tab_id: source.active_tab_id(),
            focused_pane_id: source.focused_pane_id(),
            selection: source.selection(),
            tabs,
        }
    }

    /// Record the semantic changes since the last frame: tab, focus,
    /// selection, feed connection and market, replay state, and what each pane
    /// holds. The baseline is compared in place and refreshed only where
    /// something changed, so a quiet frame allocates nothing.
    pub fn emit<S, J>(&mut self, source: &S, journal: &mut J, now: i64)
    where
        S: SemanticSource + ?Sized,
        J: SemanticJournal + ?Sized,
    {
        let active_tab_id = source.active_tab_id();
        let focused_pane_id = source.focused_pane_id();
        let selection = source.selection();
        if self.active_tab_id != active_tab_id {
            self.active_tab_id = active_tab_id;
            journal.record_observed(
                "workspace",
                "workspace.tab.activated",
                json!({ "tab_id": active_tab_id.to_string() }),
                now,
            );
        }
        if self.focused_pane_id != focused_pane_id {
            self.focused_pane_id = focused_pane_id;
            journal.record_observed(
                "workspace",
                "workspace.focus.changed",
                json!({
                    "tab_id": active_tab_id.to_string(),
                    "pane_id": focused_pane_id.to_string(),
                }),
                now,
            );
        }
        if self.selection != selection {
            self.selection = selection;
            // The owned snapshot is built only now, for the event: it is the
            // same projection the selection scope publishes, so "changed"
            // means the same thing to the journal and to a capture.
            journal.record_observed(
                "interaction",
                "interaction.selection.changed",
                json!({ "selection": source.selection_snapshot() }),
                now,
            );
        }
        source.visit_tabs(&mut |tab| {
            let tab_id = tab.tab_id();
            match self.tabs.iter_mut().find(|old| old.tab_id == tab_id) {
                None => {
                    let key = tab_key(tab);
                    journal.record_observed(
                        "workspace",
                        "workspace.tab.opened",
                        json!({ "tab_id": key.tab_id.to_string(), "feed_id": key.feed_id, "symbol": key.symbol }),
                        now,
                    );
                    self.tabs.push(key);
                }
                Some(old) => {
                    if old.feed_id != tab.feed_id() || old.symbol != tab.symbol() {
                        old.feed_id.clear();
                        old.feed_id.push_str(tab.feed_id());
                        old.symbol.clear();
                        old.symbol.push_str(tab.symbol());
                        journal.record_observed(
                            "feed",
                            "feed.market.changed",
                            json!({ "tab_id": tab_id.to_string(), "feed_id": old.feed_id, "symbol": old.symbol }),
                            now,
                        );
                    }
                    let connection = tab.connection();
                    if old.connection != connection {
                        old.connection = connection;
                        journal.record_observed(
                            "feed",
                            "feed.connection.changed",
                            json!({ "tab_id": tab_id.to_string(), "state": connection }),
                            now,
                        );
                    }
                    let replay = tab.replay();
                    if old.replay != replay {
                        old.replay = replay;
                        journal.record_observed(
                            "replay",
                            "replay.state.changed",
                            json!({
                                "tab_id": tab_id.to_string(),
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
                        record_analysis_changes(journal, tab, &old.panes, now);
                        old.panes = pane_analysis_keys(tab);
                    }
                }
            }
        });
        if self.tabs.len() != source.tab_count()
            || self.tabs.iter().any(|old| !tab_open(source, old.tab_id))
        {
            self.tabs.retain(|old| {
                let open = tab_open(source, old.tab_id);
                if !open {
                    journal.record_observed(
                        "workspace",
                        "workspace.tab.closed",
                        json!({ "tab_id": old.tab_id.to_string() }),
                        now,
                    );
                }
                open
            });
        }
    }
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
fn record_analysis_changes<J: SemanticJournal + ?Sized>(
    journal: &mut J,
    tab: &dyn SemanticTab,
    stored: &[PaneAnalysisKey],
    now: i64,
) {
    let tab_id = tab.tab_id().to_string();
    let mut open_panes = Vec::new();
    tab.visit_panes(&mut |side, pane| {
        open_panes.push(pane.pane_id());
        let was = stored.iter().find(|key| key.pane_id == pane.pane_id());
        let previous_indicators = was.map(|key| key.indicators.as_slice()).unwrap_or(&[]);
        let previous_drawings = was.map(|key| key.drawings.as_slice()).unwrap_or(&[]);
        let pane_id = pane.pane_id().to_string();

        let indicators = pane.indicators().all();
        let drawings = pane.drawings().items();

        for view in indicators {
            match previous_indicators
                .iter()
                .find(|key| key.slot == view.slot.0)
            {
                None => record_indicator_attached(
                    journal,
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
                    record_indicator_detached(
                        journal,
                        &tab_id,
                        &pane_id,
                        side,
                        key.slot,
                        key.kind.as_ref(),
                        now,
                    );
                    record_indicator_attached(
                        journal,
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
                        journal.record_observed(
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
                record_indicator_detached(
                    journal,
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
                None => journal.record_observed(
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
                Some(key) if !key.matches(drawing) => journal.record_observed(
                    "analysis",
                    "analysis.drawing.edited",
                    json!({
                        "tab_id": tab_id,
                        "pane_id": pane_id,
                        "pane_side": side,
                        "drawing_id": drawing.id.0.to_string(),
                        "tool_id": drawing.tool.id(),
                        "locked": drawing.locked,
                        "magnet": drawing.own_magnet(),
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
                record_drawing_removed(journal, &tab_id, &pane_id, side, key.id, now);
            }
        }
    });

    // A pane the layout closed takes its whole arrangement with it. Walked
    // separately because the loop above only visits panes that still
    // exist, and a client left holding indicators and marks that are gone
    // is exactly what this journal is for.
    for key in stored {
        if open_panes.contains(&key.pane_id) {
            continue;
        }
        let side = key.side;
        let pane_id = key.pane_id.to_string();
        for indicator in &key.indicators {
            record_indicator_detached(
                journal,
                &tab_id,
                &pane_id,
                side,
                indicator.slot,
                indicator.kind.as_ref(),
                now,
            );
        }
        for drawing in &key.drawings {
            record_drawing_removed(journal, &tab_id, &pane_id, side, drawing.id, now);
        }
    }
}

fn record_indicator_attached<J: SemanticJournal + ?Sized>(
    journal: &mut J,
    tab_id: &str,
    pane_id: &str,
    side: PaneSideDto,
    slot: u64,
    kind: &str,
    now: i64,
) {
    journal.record_observed(
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

fn record_indicator_detached<J: SemanticJournal + ?Sized>(
    journal: &mut J,
    tab_id: &str,
    pane_id: &str,
    side: PaneSideDto,
    slot: u64,
    kind: &str,
    now: i64,
) {
    journal.record_observed(
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

fn record_drawing_removed<J: SemanticJournal + ?Sized>(
    journal: &mut J,
    tab_id: &str,
    pane_id: &str,
    side: PaneSideDto,
    drawing_id: u64,
    now: i64,
) {
    journal.record_observed(
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

#[cfg(test)]
#[path = "semantic_tests.rs"]
mod semantic_tests;
