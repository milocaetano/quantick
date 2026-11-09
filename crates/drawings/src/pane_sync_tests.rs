//! Panes of one market sharing its all-timeframes marks through the store.
use super::*;
use crate::{ChartPoint, DRAWING_TOOLS, DrawingScope};
use quantick_workspace::layout_document::SavedDrawing;
use quantick_workspace::market_settings::{DrawingKey, MarketSettings};

const T0: i64 = 1_700_000_000_000;

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("registered test tool")
}

fn level_at(drawings: &mut Drawings<DrawingTool>, bar: f32, price: f64) {
    let time = Some(T0 + i64::from(bar as i32) * 60_000);
    assert!(drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(bar, price, time)
    ));
}

fn prices(drawings: &Drawings<DrawingTool>) -> Vec<f64> {
    drawings.items().iter().map(|d| d.points[0].price).collect()
}

/// One chart of the market: its store and the shared set it last loaded.
struct Pane {
    key: DrawingKey,
    bars: &'static str,
    drawings: Drawings<DrawingTool>,
    base: Vec<SavedDrawing>,
}

impl Pane {
    fn open(settings: &MarketSettings, key: DrawingKey, bars: &'static str) -> Self {
        let mut pane = Self {
            key,
            bars,
            drawings: Drawings::default(),
            base: Vec::new(),
        };
        adopt_pane(
            &mut pane.drawings,
            &mut pane.base,
            settings,
            &pane.key,
            bars,
        );
        pane
    }

    fn persist(&mut self, settings: &mut MarketSettings) -> PaneWrite {
        persist_pane(
            &mut self.drawings,
            &mut self.base,
            settings,
            &self.key,
            self.bars,
        )
    }

    fn refresh(&mut self, settings: &MarketSettings) {
        refresh_shared(
            &mut self.drawings,
            &mut self.base,
            settings,
            &self.key,
            self.bars,
        );
    }
}

fn flow() -> DrawingKey {
    DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), None)
}

fn context() -> DrawingKey {
    DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), Some("time:5m".to_owned()))
}

#[test]
fn a_drawing_without_a_market_instant_opens_on_its_chart_only() {
    let mut drawings = Drawings::default();
    assert!(drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0)));
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
    assert!(!drawings.items()[0].all_timeframes());
    level_at(&mut drawings, 2.0, 90.0);
    assert_eq!(drawings.items()[1].scope, DrawingScope::AllCharts);
    assert!(drawings.items()[1].all_timeframes());
}

#[test]
fn a_pasted_copy_is_kept_to_its_chart_and_forgets_the_legacy_pin() {
    let mut drawings = Drawings::default();
    level_at(&mut drawings, 2.0, 90.0);
    let mut source = drawings.items()[0].clone();
    source.legacy_unpinned = true;
    let id = drawings.paste(&source, 3.0);
    let copy = &drawings.items()[drawings.index_of(id).unwrap()];
    assert_eq!(copy.scope, DrawingScope::ThisChart, "a copy has no instant");
    assert!(!copy.legacy_unpinned);
}

#[test]
fn choosing_a_scope_forgets_the_legacy_pin() {
    let mut drawings = Drawings::default();
    level_at(&mut drawings, 2.0, 90.0);
    let drawing = &mut drawings.items_mut()[0];
    drawing.legacy_unpinned = true;
    drawing.set_scope(DrawingScope::AllCharts);
    drawing.set_scope(DrawingScope::ThisChart);
    assert!(!drawing.legacy_unpinned);
    assert_eq!(drawing.scope, DrawingScope::ThisChart);
}

#[test]
fn an_old_shared_entry_without_instants_stays_unpinned() {
    let mut drawings = Drawings::default();
    assert!(drawings.place(tool("horizontal-line"), ChartPoint::at(1.0, 100.0)));
    let mut old = SavedDrawing::from_drawing(&drawings.items()[0]);
    old.shared = true;
    let mut settings = MarketSettings::default();
    settings.replace(&flow(), vec![old], |_| true);

    let mut pane = Pane::open(&settings, flow(), "tick:100");
    assert!(pane.drawings.items()[0].legacy_unpinned);
    pane.drawings.items_mut()[0].points[0].price = 101.0;
    pane.drawings.rename_at(0, "moved");
    pane.persist(&mut settings);
    assert_eq!(settings.drawings(&flow())[0].bars, None, "never pinned");
}

#[test]
fn refreshing_the_shared_marks_keeps_this_charts_history_and_selection() {
    let mut settings = MarketSettings::default();
    let mut other = Pane::open(&settings, flow(), "tick:100");
    level_at(&mut other.drawings, 1.0, 100.0);
    other.persist(&mut settings);

    let mut pane = Pane::open(&settings, context(), "time:5m");
    assert!(
        pane.drawings
            .place(tool("horizontal-line"), ChartPoint::at(2.0, 50.0))
    );
    let mine = pane.drawings.selected_id().unwrap();
    pane.drawings.rename_at(1, "mine");
    let depth = pane.drawings.undo_depth();
    pane.persist(&mut settings);

    other.drawings.items_mut()[0].points[0].price = 105.0;
    other.drawings.rename_at(0, "moved there");
    other.persist(&mut settings);
    pane.refresh(&settings);

    assert_eq!(prices(&pane.drawings), [105.0, 50.0]);
    assert_eq!(pane.drawings.selected_id(), Some(mine), "selection kept");
    assert_eq!(pane.drawings.undo_depth(), depth, "history kept");
    assert!(pane.drawings.undo(), "the rename on this chart undoes");
    assert_eq!(pane.drawings.items()[1].name, None);
    assert_eq!(
        prices(&pane.drawings),
        [105.0, 50.0],
        "and leaves the other chart's edit alone"
    );
}

#[test]
fn undoing_a_scope_change_after_a_refresh_brings_the_mark_back() {
    let mut settings = MarketSettings::default();
    let mut pane = Pane::open(&settings, flow(), "tick:100");
    level_at(&mut pane.drawings, 1.0, 100.0);
    pane.persist(&mut settings);
    let before = pane.drawings.items()[0].clone();
    pane.drawings.items_mut()[0].set_scope(DrawingScope::ThisChart);
    pane.drawings.record_edit_of(0, before);
    pane.persist(&mut settings);
    pane.refresh(&settings);

    assert!(pane.drawings.undo());
    assert_eq!(pane.drawings.items().len(), 1);
    assert_eq!(pane.drawings.items()[0].scope, DrawingScope::AllCharts);
}

#[test]
fn a_new_id_is_never_one_the_market_already_holds() {
    let mut drawings: Drawings<DrawingTool> = Drawings::default();
    drawings.reserve_ids(10);
    level_at(&mut drawings, 1.0, 100.0);
    assert_eq!(drawings.items()[0].id.0, 11);
}

/// What a strategy or an agent named on a chart-only mark keeps naming it
/// when another chart of the market adds a mark for all timeframes.
#[test]
fn a_chart_only_mark_keeps_its_id_when_another_chart_adds_a_shared_one() {
    let mut settings = MarketSettings::default();
    let mut context_pane = Pane::open(&settings, context(), "time:5m");
    assert!(
        context_pane
            .drawings
            .place(tool("horizontal-line"), ChartPoint::at(2.0, 50.0))
    );
    let mine = context_pane.drawings.items()[0].id;
    context_pane.persist(&mut settings);

    let mut flow_pane = Pane::open(&settings, flow(), "tick:100");
    level_at(&mut flow_pane.drawings, 1.0, 100.0);
    let shared = flow_pane.drawings.items()[0].id;
    assert_ne!(shared, mine, "a fresh id above every id of the market");
    flow_pane.persist(&mut settings);
    context_pane.refresh(&settings);

    let index = context_pane.drawings.index_of(mine).unwrap();
    assert_eq!(context_pane.drawings.items()[index].points[0].price, 50.0);
    assert!(context_pane.drawings.index_of(shared).is_some());
}

/// A chart that missed a refresh (a drag was in flight on it) writes only what
/// it changed, not its stale copy over another chart's edit.
#[test]
fn a_stale_chart_merges_its_edit_instead_of_writing_over_anothers() {
    let mut settings = MarketSettings::default();
    let mut first = Pane::open(&settings, flow(), "tick:100");
    level_at(&mut first.drawings, 1.0, 100.0);
    level_at(&mut first.drawings, 2.0, 200.0);
    first.persist(&mut settings);
    let mut second = Pane::open(&settings, context(), "time:5m");

    first.drawings.items_mut()[0].points[0].price = 101.0;
    first.drawings.rename_at(0, "first moved it");
    first.persist(&mut settings);
    // `second` is not refreshed: it edits the other mark on its stale copy.
    second.drawings.items_mut()[1].points[0].price = 201.0;
    second.drawings.rename_at(1, "second moved it");
    let write = second.persist(&mut settings);

    assert!(write.merged, "the writer was behind the store");
    assert_eq!(prices(&second.drawings), [101.0, 201.0], "and caught up");
    let fresh = Pane::open(&settings, flow(), "tick:100");
    assert_eq!(prices(&fresh.drawings), [101.0, 201.0], "both edits kept");
}
