//! Panes of one market sharing its all-timeframes marks through the store.
use super::*;
use crate::{ChartPoint, DRAWING_TOOLS, DrawingScope};
use quantick_workspace::layout_document::SavedDrawing;
use quantick_workspace::market_settings::{DrawingKey, MarketSettings};
use quantick_workspace::market_settings_store::MarketSettingsStore;
use std::path::PathBuf;

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

/// A copy has no market instant until something dates it: kept to its chart
/// until then, and never claiming all timeframes.
#[test]
fn a_pasted_copy_waits_on_its_chart_and_forgets_the_legacy_pin() {
    let mut drawings = Drawings::default();
    level_at(&mut drawings, 2.0, 90.0);
    let mut source = drawings.items()[0].clone();
    source.legacy_unpinned = true;
    let id = drawings.paste(&source, 3.0);
    let copy = &drawings.items()[drawings.index_of(id).unwrap()];
    assert!(!copy.all_timeframes(), "a copy has no instant");
    assert!(!copy.legacy_unpinned);
}

/// A copy of a mark every timeframe shows joins them once all its anchors
/// carry market time; until then it is written to its own chart.
#[test]
fn a_copy_of_a_mark_on_all_timeframes_joins_them_once_dated() {
    let mut settings = MarketSettings::default();
    let mut pane = Pane::open(&settings, flow(), "tick:100");
    level_at(&mut pane.drawings, 2.0, 90.0);
    let copy = pane.drawings.duplicate_selected(3.0).expect("a copy").copy;
    let index = pane.drawings.index_of(copy).unwrap();
    assert!(!pane.drawings.items()[index].all_timeframes(), "undated");
    pane.persist(&mut settings);
    assert_eq!(settings.shared_drawings(&flow().market).len(), 1);
    assert_eq!(settings.drawings(&flow()).len(), 1, "it waits on its chart");

    pane.drawings.set_times(index, &[Some(T0 + 5 * 60_000)]);
    assert!(pane.drawings.items()[index].all_timeframes(), "dated");
    pane.persist(&mut settings);
    assert_eq!(settings.shared_drawings(&flow().market).len(), 2);
    assert!(settings.drawings(&flow()).is_empty(), "and left its chart");
}

#[test]
fn a_copy_of_a_mark_kept_to_its_chart_stays_there_once_dated() {
    let mut drawings = Drawings::default();
    level_at(&mut drawings, 2.0, 90.0);
    drawings.items_mut()[0].set_scope(DrawingScope::ThisChart);
    let source = drawings.items()[0].clone();
    let id = drawings.paste(&source, 3.0);
    let index = drawings.index_of(id).unwrap();
    drawings.set_times(index, &[Some(T0 + 5 * 60_000)]);
    assert!(!drawings.items()[index].all_timeframes());
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

/// Another chart's write does not cost this one the undo of its own edits to
/// the marks every chart shows: a placement undoes here, and the set is
/// written without it.
#[test]
fn this_charts_placement_still_undoes_after_another_chart_writes() {
    let mut settings = MarketSettings::default();
    let mut pane = Pane::open(&settings, flow(), "tick:100");
    level_at(&mut pane.drawings, 1.0, 100.0);
    pane.persist(&mut settings);
    let mut other = Pane::open(&settings, context(), "time:5m");
    level_at(&mut other.drawings, 2.0, 200.0);
    other.persist(&mut settings);
    pane.refresh(&settings);
    assert_eq!(prices(&pane.drawings), [100.0, 200.0]);

    assert!(pane.drawings.undo(), "the placement made here undoes");
    assert_eq!(prices(&pane.drawings), [200.0], "and only it");
    pane.persist(&mut settings);
    other.refresh(&settings);
    assert_eq!(prices(&other.drawings), [200.0], "every chart loses it");
    assert!(pane.drawings.redo());
    assert_eq!(prices(&pane.drawings), [100.0, 200.0]);
}

#[test]
fn this_charts_move_and_delete_still_undo_after_another_chart_writes() {
    let mut settings = MarketSettings::default();
    let mut pane = Pane::open(&settings, flow(), "tick:100");
    level_at(&mut pane.drawings, 1.0, 100.0);
    level_at(&mut pane.drawings, 2.0, 200.0);
    pane.persist(&mut settings);
    let before = pane.drawings.items()[0].clone();
    pane.drawings.items_mut()[0].points[0].price = 110.0;
    pane.drawings.record_edit_of(0, before);
    pane.drawings.select(Some(1));
    let _ = pane.drawings.delete_selected(false);
    pane.persist(&mut settings);

    let mut other = Pane::open(&settings, context(), "time:5m");
    level_at(&mut other.drawings, 3.0, 300.0);
    other.persist(&mut settings);
    pane.refresh(&settings);
    assert_eq!(prices(&pane.drawings), [110.0, 300.0]);

    assert!(pane.drawings.undo(), "the delete made here undoes");
    assert_eq!(prices(&pane.drawings), [110.0, 300.0, 200.0]);
    assert!(pane.drawings.undo(), "and so does the move");
    assert_eq!(prices(&pane.drawings), [100.0, 300.0, 200.0]);
    pane.persist(&mut settings);
    other.refresh(&settings);
    let mut shown = prices(&other.drawings);
    shown.sort_by(f64::total_cmp);
    assert_eq!(shown, [100.0, 200.0, 300.0], "written for every chart");
}

fn tick(bars: u32) -> String {
    format!("tick:{bars}")
}

/// One chart as the app drives it: its marks, its layout bookkeeping and the
/// bars it is cut to.
struct Chart {
    drawings: Drawings<DrawingTool>,
    layout: PaneLayout<u32>,
    now: u32,
}

impl Chart {
    fn open(settings: &MarketSettings, key: DrawingKey, now: u32) -> Self {
        let mut chart = Self {
            drawings: Drawings::default(),
            layout: PaneLayout::default(),
            now,
        };
        chart.layout.drawings_key = Some(key.clone());
        chart.layout.drawings_spec = Some(now);
        chart.marks().sync(PaneSync::Adopt, settings, &key);
        chart.layout.saved_revision = chart.drawings.revision();
        chart
    }

    fn marks(&mut self) -> PaneMarks<'_, u32> {
        PaneMarks {
            drawings: &mut self.drawings,
            layout: &mut self.layout,
            now: self.now,
            rule: tick,
        }
    }

    /// Take from the store what [`PaneMarks::follow_write`] asks for, as the
    /// app does; whether it took anything.
    fn follow(
        &mut self,
        settings: &MarketSettings,
        write: Option<(&DrawingKey, PaneWrite)>,
        busy: bool,
    ) -> bool {
        let Some((key, sync)) = self.marks().follow_write(write, busy) else {
            return false;
        };
        self.marks().sync(sync, settings, &key);
        self.layout.saved_revision = self.drawings.revision();
        true
    }
}

/// A frame's writes as the app makes them: every chart with edits not yet
/// written writes them, and every other chart a write reaches follows it.
fn write_all(charts: &mut [&mut Chart], settings: &mut MarketSettings) {
    for writer in 0..charts.len() {
        let chart = &mut charts[writer];
        if !chart.layout.drawings_dirty(chart.drawings.revision()) {
            continue;
        }
        let (key, write) = chart.marks().persist(settings).expect("a keyed chart");
        if !write.changed() {
            continue;
        }
        for (index, twin) in charts.iter_mut().enumerate() {
            if index != writer {
                twin.follow(settings, Some((&key, write)), false);
            }
        }
    }
}

fn saved_prices(entries: &[SavedDrawing]) -> Vec<f64> {
    let mut prices: Vec<f64> = entries.iter().map(|entry| entry.points[0].price).collect();
    prices.sort_by(f64::total_cmp);
    prices
}

/// Two charts with edits not yet written in one frame: the second is not
/// brought to the first's write over its own edits, it merges its write, and
/// no edit is lost or doubled.
#[test]
fn two_charts_with_unwritten_edits_in_one_frame_keep_both() {
    let mut settings = MarketSettings::default();
    let mut flow_chart = Chart::open(&settings, flow(), 1);
    level_at(&mut flow_chart.drawings, 1.0, 100.0);
    level_at(&mut flow_chart.drawings, 2.0, 200.0);
    write_all(&mut [&mut flow_chart], &mut settings);
    let mut context_chart = Chart::open(&settings, context(), 5);

    let before = flow_chart.drawings.items()[0].clone();
    flow_chart.drawings.items_mut()[0].points[0].price = 101.0;
    flow_chart.drawings.record_edit_of(0, before);
    level_at(&mut context_chart.drawings, 3.0, 300.0);
    let before = context_chart.drawings.items()[1].clone();
    context_chart.drawings.items_mut()[1].set_scope(DrawingScope::ThisChart);
    context_chart.drawings.record_edit_of(1, before);
    write_all(&mut [&mut flow_chart, &mut context_chart], &mut settings);

    let shared = settings.shared_drawings(&flow().market);
    assert_eq!(saved_prices(&shared), [101.0, 300.0], "move and new mark");
    let own = settings.drawings(&context());
    assert_eq!(saved_prices(own), [200.0], "kept to the context chart");
    assert_eq!(prices(&context_chart.drawings), [101.0, 300.0, 200.0]);
    assert_eq!(prices(&flow_chart.drawings), [101.0, 300.0]);
}

/// A chart busy when another wrote (a gesture or a draft in flight) is not
/// brought to the write then, and catches up once free, even when its gesture
/// ended with nothing to write.
#[test]
fn a_chart_busy_when_another_wrote_catches_up_once_free() {
    let mut settings = MarketSettings::default();
    let mut flow_chart = Chart::open(&settings, flow(), 1);
    let mut context_chart = Chart::open(&settings, context(), 5);
    level_at(&mut flow_chart.drawings, 1.0, 100.0);
    let (key, write) = flow_chart.marks().persist(&mut settings).unwrap();

    let reached = Some((&key, write));
    assert!(!context_chart.follow(&settings, reached, true), "busy");
    assert!(context_chart.drawings.items().is_empty());
    assert!(context_chart.follow(&settings, None, false), "free again");
    assert_eq!(prices(&context_chart.drawings), [100.0]);
    assert!(!context_chart.follow(&settings, None, false), "once");
}

fn market_store() -> MarketSettingsStore {
    let path = PathBuf::from("market-settings.toml");
    MarketSettingsStore::new(MarketSettings::default(), path, true)
}

/// A chart held at bars a mark is pinned to asks the store again once it
/// changes: the mark gone, the re-cut moves the bars in place.
#[test]
fn a_recut_blocked_by_a_pinned_mark_moves_once_the_mark_is_gone() {
    let mut store = market_store();
    let mut chart = Chart::open(store.settings(), flow(), 1);
    level_at(&mut chart.drawings, 1.0, 100.0);
    chart.drawings.items_mut()[0].set_scope(DrawingScope::ThisChart);
    chart.marks().persist(store.settings_mut());

    chart.now = 2;
    chart.marks().follow_store(&store);
    chart.marks().follow_store(&store);
    assert_eq!(chart.layout.drawings_spec, Some(1), "pinned to the held bars");

    chart.drawings.select(Some(0));
    let _ = chart.drawings.delete_selected(true);
    chart.marks().persist(store.settings_mut());
    chart.marks().follow_store(&store);
    assert_eq!(chart.layout.drawings_spec, Some(2), "asked again, it moves");
}

#[test]
fn putting_a_chart_away_forgets_a_blocked_recut() {
    let mut store = market_store();
    let mut chart = Chart::open(store.settings(), flow(), 1);
    level_at(&mut chart.drawings, 1.0, 100.0);
    chart.drawings.items_mut()[0].set_scope(DrawingScope::ThisChart);
    chart.marks().persist(store.settings_mut());
    chart.now = 2;
    chart.marks().follow_store(&store);
    assert!(chart.layout.recut_checked.is_some(), "blocked");

    chart.marks().put_away(store.settings_mut());
    assert!(chart.layout.recut_checked.is_none());
}
