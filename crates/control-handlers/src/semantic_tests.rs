//! The frame emitter over a fake source: the first frame records nothing,
//! and every later frame records only what changed, in the journal's words.

use quantick_chart::indicator_views::IndicatorViews;
use quantick_control_host::wire::{PaneSideDto, available};
use quantick_control_schema::interaction::SelectionSnapshot;
use quantick_drawings::{DrawingTool, Drawings};
use serde_json::{Value, json};

use super::*;

struct Pane {
    id: u64,
    indicators: IndicatorViews,
    drawings: Drawings<DrawingTool>,
}

impl Pane {
    fn new(id: u64) -> Self {
        Self {
            id,
            indicators: IndicatorViews::new(),
            drawings: Drawings::default(),
        }
    }
}

impl AnalysisPane for Pane {
    fn pane_id(&self) -> u64 {
        self.id
    }
    fn indicators(&self) -> &IndicatorViews {
        &self.indicators
    }
    fn drawings(&self) -> &Drawings<DrawingTool> {
        &self.drawings
    }
}

struct Tab {
    id: u64,
    feed_id: String,
    symbol: String,
    connection: &'static str,
    replay: Option<(bool, bool)>,
    panes: Vec<Pane>,
}

impl Tab {
    fn new(id: u64, symbol: &str) -> Self {
        Self {
            id,
            feed_id: "mt5".to_owned(),
            symbol: symbol.to_owned(),
            connection: "live",
            replay: None,
            panes: vec![Pane::new(id * 10)],
        }
    }
}

impl SemanticTab for Tab {
    fn tab_id(&self) -> u64 {
        self.id
    }
    fn feed_id(&self) -> &str {
        &self.feed_id
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn connection(&self) -> &'static str {
        self.connection
    }
    fn replay(&self) -> Option<(bool, bool)> {
        self.replay
    }
    fn visit_panes(&self, visit: &mut dyn FnMut(PaneSideDto, &dyn AnalysisPane)) {
        for (slot, pane) in self.panes.iter().enumerate() {
            let side = if slot == 0 {
                PaneSideDto::Flow
            } else {
                PaneSideDto::Time
            };
            visit(side, pane);
        }
    }
}

struct Source {
    active: usize,
    focused_pane: u64,
    drawing: Option<(u64, u64)>,
    tabs: Vec<Tab>,
}

impl Source {
    fn new(tabs: Vec<Tab>) -> Self {
        let focused_pane = tabs[0].panes[0].id;
        Self {
            active: 0,
            focused_pane,
            drawing: None,
            tabs,
        }
    }
}

impl SemanticSource for Source {
    fn active_tab_id(&self) -> u64 {
        self.tabs[self.active].id
    }
    fn focused_pane_id(&self) -> u64 {
        self.focused_pane
    }
    fn selection(&self) -> SelectionIdentity {
        SelectionIdentity {
            active_tab_id: self.active_tab_id(),
            focused_pane_id: self.focused_pane,
            focused_pane_side: quantick_chart_interaction::pane::PaneSide::Flow,
            drawing: self.drawing,
            paper_trade_row: None,
        }
    }
    fn selection_snapshot(&self) -> SelectionSnapshot {
        SelectionSnapshot {
            active_tab_id: quantick_control::wire::WireU64::new(self.active_tab_id()),
            focused_pane_id: quantick_control::wire::WireU64::new(self.focused_pane),
            focused_pane_side: PaneSideDto::Flow,
            drawing: None,
            paper_trade_row: None,
            event_row: available(),
        }
    }
    fn tab_count(&self) -> usize {
        self.tabs.len()
    }
    fn visit_tabs(&self, visit: &mut dyn FnMut(&dyn SemanticTab)) {
        for tab in &self.tabs {
            visit(tab);
        }
    }
}

#[derive(Default)]
struct Journal(Vec<(String, Value)>);

impl SemanticJournal for Journal {
    fn record_observed(&mut self, _module: &str, kind: &str, payload: Value, now: i64) {
        assert_eq!(now, 1_700_000_000_000, "the frame's clock, handed in");
        self.0.push((kind.to_owned(), payload));
    }
}

const NOW: i64 = 1_700_000_000_000;

fn frame(baseline: &mut SemanticBaseline, source: &Source) -> Vec<(String, Value)> {
    let mut journal = Journal::default();
    baseline.emit(source, &mut journal, NOW);
    journal.0
}

#[test]
fn a_quiet_frame_records_nothing() {
    let source = Source::new(vec![Tab::new(1, "WIN"), Tab::new(2, "BTC")]);
    let mut baseline = SemanticBaseline::capture(&source);
    assert!(frame(&mut baseline, &source).is_empty());
}

#[test]
fn a_tab_switch_records_the_activation_and_the_focus() {
    let mut source = Source::new(vec![Tab::new(1, "WIN"), Tab::new(2, "BTC")]);
    let mut baseline = SemanticBaseline::capture(&source);
    source.active = 1;
    source.focused_pane = 20;
    let events = frame(&mut baseline, &source);
    assert_eq!(
        events,
        vec![
            (
                "workspace.tab.activated".to_owned(),
                json!({ "tab_id": "2" })
            ),
            (
                "workspace.focus.changed".to_owned(),
                json!({ "tab_id": "2", "pane_id": "20" })
            ),
            (
                "interaction.selection.changed".to_owned(),
                json!({ "selection": source.selection_snapshot() })
            ),
        ]
    );
    assert!(frame(&mut baseline, &source).is_empty(), "then quiet again");
}

#[test]
fn a_tab_records_its_market_connection_and_replay_as_they_change() {
    let mut source = Source::new(vec![Tab::new(1, "WIN")]);
    let mut baseline = SemanticBaseline::capture(&source);
    source.tabs[0].symbol = "WDO".to_owned();
    source.tabs[0].connection = "reconnecting";
    source.tabs[0].replay = Some((true, false));
    let events = frame(&mut baseline, &source);
    assert_eq!(
        events,
        vec![
            (
                "feed.market.changed".to_owned(),
                json!({ "tab_id": "1", "feed_id": "mt5", "symbol": "WDO" })
            ),
            (
                "feed.connection.changed".to_owned(),
                json!({ "tab_id": "1", "state": "reconnecting" })
            ),
            (
                "replay.state.changed".to_owned(),
                json!({ "tab_id": "1", "active": true, "playing": true, "finished": false })
            ),
        ]
    );
}

#[test]
fn opening_and_closing_tabs_records_each_once() {
    let mut source = Source::new(vec![Tab::new(1, "WIN")]);
    let mut baseline = SemanticBaseline::capture(&source);
    source.tabs.push(Tab::new(2, "BTC"));
    assert_eq!(
        frame(&mut baseline, &source),
        vec![(
            "workspace.tab.opened".to_owned(),
            json!({ "tab_id": "2", "feed_id": "mt5", "symbol": "BTC" })
        )]
    );
    source.tabs.remove(0);
    source.focused_pane = 20;
    let events = frame(&mut baseline, &source);
    let kinds: Vec<&str> = events.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "workspace.tab.activated",
            "workspace.focus.changed",
            "interaction.selection.changed",
            "workspace.tab.closed",
        ]
    );
    assert_eq!(events[3].1, json!({ "tab_id": "1" }));
    assert!(frame(&mut baseline, &source).is_empty());
}

#[test]
fn a_pane_the_layout_opened_or_closed_changes_the_tab_without_an_event_when_empty() {
    let mut source = Source::new(vec![Tab::new(1, "WIN")]);
    let mut baseline = SemanticBaseline::capture(&source);
    source.tabs[0].panes.push(Pane::new(11));
    assert!(
        frame(&mut baseline, &source).is_empty(),
        "an empty pane has nothing to attach"
    );
    source.tabs[0].panes.pop();
    assert!(
        frame(&mut baseline, &source).is_empty(),
        "and nothing to retire"
    );
}

/// The journal reports what happened to a mark a pane shows, never which
/// marks its layout shows: one it only holds for merges is no event, a
/// switch of layout neither creates nor removes one, and a mark it reported
/// is reported gone when it is really deleted, shown there or not.
#[test]
fn the_journal_sees_only_the_marks_a_panes_layout_shows() {
    use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingScope};
    use quantick_drawings::{DrawingStyle, NewDrawing};
    use quantick_workspace::layout_document::LayoutId;
    let kinds = |events: Vec<(String, Value)>| -> Vec<String> {
        events.into_iter().map(|(kind, _)| kind).collect()
    };
    let mut source = Source::new(vec![Tab::new(1, "WIN")]);
    let drawings = &mut source.tabs[0].panes[0].drawings;
    drawings.set_layout(Some(LayoutId(1)));
    let mut baseline = SemanticBaseline::capture(&source);

    let drawings = &mut source.tabs[0].panes[0].drawings;
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line");
    let point = ChartPoint::at_time(3.0, 100.0, Some(NOW));
    let fresh = |tool: DrawingTool| NewDrawing {
        style: DrawingStyle::default(),
        payload: tool.default_payload(),
    };
    drawings.set_layout(Some(LayoutId(2)));
    assert!(drawings.place_with(tool.unwrap(), &DrawingBand::Price, point, fresh));
    let before = drawings.items()[0].clone();
    drawings.items_mut()[0].set_scope(DrawingScope::AllCharts);
    drawings.record_edit_of(0, before);
    drawings.set_layout(Some(LayoutId(1)));
    assert_eq!(drawings.items().len(), 1, "held");
    assert!(
        frame(&mut baseline, &source).is_empty(),
        "not shown: no event"
    );

    let pane = &mut source.tabs[0].panes[0];
    pane.drawings.set_layout(Some(LayoutId(2)));
    assert!(
        frame(&mut baseline, &source).is_empty(),
        "shown by a switch: not created"
    );
    source.tabs[0].panes[0].drawings.set_locked_at(0, true);
    let events = frame(&mut baseline, &source);
    assert_eq!(kinds(events), ["analysis.drawing.edited"], "edited here");
    let pane = &mut source.tabs[0].panes[0];
    pane.drawings.set_layout(Some(LayoutId(1)));
    assert!(
        frame(&mut baseline, &source).is_empty(),
        "hidden by a switch: not removed"
    );

    let drawings = &mut source.tabs[0].panes[0].drawings;
    let id = drawings.items()[0].id;
    assert!(drawings.remove_by_id(id));
    let events = frame(&mut baseline, &source);
    assert_eq!(kinds(events), ["analysis.drawing.removed"], "really gone");
}
