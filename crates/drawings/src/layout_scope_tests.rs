//! Which charts of a market show a mark: kept to its chart by default; on all
//! timeframes, every chart showing its layout; with "All windows" on, every
//! chart of its market whatever layout it shows; never another market.
use crate::{
    ChartPoint, DRAWING_TOOLS, DeleteOutcome, DrawingAuthor, DrawingScope, DrawingTool, Drawings,
    PaneMarks, SavedDrawingExt, adopt_pane, persist_pane, put_away_pane, refresh_shared,
    retarget_layout,
};
use quantick_workspace::layout_document::{LayoutId, SavedDrawing};
use quantick_workspace::market_settings::{DrawingKey, MarketSettings};
use quantick_workspace::pane_layout::PaneLayout;

const T0: i64 = 1_700_000_000_000;
const LEVELS: LayoutId = LayoutId(1);
const FLOWS: LayoutId = LayoutId(2);

fn horizontal_line() -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .expect("registered test tool")
}

/// A dated level, turned on all timeframes the way the trader does it: a
/// pick recorded as one edit of the store.
fn level_on_all_timeframes(drawings: &mut Drawings<DrawingTool>, bar: f32, price: f64) {
    level_here(drawings, bar, price);
    let index = drawings.items().len() - 1;
    let before = drawings.items()[index].clone();
    drawings.items_mut()[index].set_scope(DrawingScope::AllCharts);
    drawings.record_edit_of(index, before);
}

/// A dated level, as a click places it.
fn level_here(drawings: &mut Drawings<DrawingTool>, bar: f32, price: f64) {
    let time = Some(T0 + i64::from(bar as i32) * 60_000);
    assert!(drawings.place(horizontal_line(), ChartPoint::at_time(bar, price, time)));
}

fn flow(asset: &str) -> DrawingKey {
    DrawingKey::of_pane("binance", asset.to_owned(), None)
}

fn context(bars: &str) -> DrawingKey {
    DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), Some(bars.to_owned()))
}

/// One chart: the layout it shows, the key and bars it holds, its marks and
/// the shared set it last loaded.
struct Chart {
    key: DrawingKey,
    bars: &'static str,
    drawings: Drawings<DrawingTool>,
    base: Vec<SavedDrawing>,
}

impl Chart {
    fn open(
        settings: &MarketSettings,
        layout: LayoutId,
        key: DrawingKey,
        bars: &'static str,
    ) -> Self {
        let mut chart = Self {
            key,
            bars,
            drawings: Drawings::default(),
            base: Vec::new(),
        };
        chart.drawings.set_layout(Some(layout));
        adopt_pane(
            &mut chart.drawings,
            &mut chart.base,
            settings,
            &chart.key,
            bars,
        );
        chart
    }

    fn persist(&mut self, settings: &mut MarketSettings) {
        persist_pane(
            &mut self.drawings,
            &mut self.base,
            settings,
            &self.key,
            self.bars,
        );
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

    /// The chart cut to other bars: what it held goes back to the store, and
    /// it takes what the new key shows. Its layout does not move.
    fn switch_bars(&mut self, settings: &mut MarketSettings, bars: &'static str) {
        put_away_pane(
            &mut self.drawings,
            &mut self.base,
            settings,
            &self.key,
            self.bars,
        );
        self.key = context(bars);
        self.bars = bars;
        adopt_pane(
            &mut self.drawings,
            &mut self.base,
            settings,
            &self.key,
            bars,
        );
    }

    /// The marks this chart paints, hit-tests and lists.
    fn shown(&self) -> Vec<f64> {
        (0..self.drawings.items().len())
            .filter(|&index| self.drawings.in_layout(index))
            .map(|index| self.drawings.items()[index].points[0].price)
            .collect()
    }
}

#[test]
fn a_new_mark_is_kept_to_its_chart_and_records_its_layout() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(FLOWS));
    level_here(&mut drawings, 2.0, 90.0);
    let mark = &drawings.items()[0];
    assert_eq!(mark.scope, DrawingScope::ThisChart, "the default");
    assert!(!mark.all_timeframes());
    assert_eq!(mark.layout, Some(FLOWS));
    assert!(!mark.all_windows, "off by default");
    assert!(!mark.in_all_windows(&[]));
}

#[test]
fn turned_on_all_timeframes_a_mark_belongs_to_the_layout_it_was_turned_on_in() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    level_here(&mut drawings, 2.0, 90.0);
    drawings.set_layout(Some(FLOWS));
    let before = drawings.items()[0].clone();
    drawings.items_mut()[0].set_scope(DrawingScope::AllCharts);
    drawings.record_edit_of(0, before);
    let mark = &drawings.items()[0];
    assert!(mark.all_timeframes());
    assert_eq!(mark.layout, Some(FLOWS));
    assert!(mark.shows_in(Some(FLOWS), &[]));
    assert!(!mark.shows_in(Some(LEVELS), &[]));
}

#[test]
fn the_layout_keeps_its_mark_across_a_bar_rule_switch() {
    let mut settings = MarketSettings::default();
    let mut chart = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut chart.drawings, 2.0, 90.0);
    chart.persist(&mut settings);

    chart.switch_bars(&mut settings, "time:15m");
    assert_eq!(chart.shown(), [90.0], "at 15m");
    assert!(chart.drawings.is_visible(0));
    chart.switch_bars(&mut settings, "renko:20");
    assert_eq!(chart.shown(), [90.0], "at renko");
}

#[test]
fn every_chart_of_the_market_on_the_same_layout_shows_it() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    owner.persist(&mut settings);

    let twin = Chart::open(&settings, LEVELS, flow("BTCUSDT"), "tick:100");
    assert_eq!(twin.shown(), [90.0]);
    assert!(twin.drawings.is_visible(0));
}

#[test]
fn a_chart_on_another_layout_shows_it_only_with_all_windows_on() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    owner.persist(&mut settings);

    let mut other = Chart::open(&settings, FLOWS, flow("BTCUSDT"), "tick:100");
    assert_eq!(other.drawings.items().len(), 1, "held, for merges");
    assert!(other.shown().is_empty(), "neither painted nor listed");
    assert!(!other.drawings.is_visible(0), "nor hit-tested");
    assert_eq!(other.drawings.shown_count(), 0, "nor counted");

    assert!(owner.drawings.set_all_windows_at(0, true));
    assert!(owner.drawings.in_all_windows(0));
    owner.persist(&mut settings);
    other.refresh(&settings);
    assert_eq!(other.shown(), [90.0]);
    assert!(other.drawings.is_visible(0));
    assert_eq!(other.drawings.shown_count(), 1);

    assert!(owner.drawings.undo(), "one undo step");
    owner.persist(&mut settings);
    other.refresh(&settings);
    assert!(other.shown().is_empty(), "back to its own layout");
}

#[test]
fn a_chart_switched_to_another_layout_drops_the_mark_and_takes_it_back() {
    let mut settings = MarketSettings::default();
    let mut chart = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut chart.drawings, 2.0, 90.0);
    chart.drawings.select(Some(0));
    chart.persist(&mut settings);

    chart.drawings.set_layout(Some(FLOWS));
    assert!(chart.shown().is_empty());
    assert_eq!(chart.drawings.selected(), None, "the selection let go");
    chart.drawings.set_layout(Some(LEVELS));
    assert_eq!(chart.shown(), [90.0]);
}

#[test]
fn a_chart_of_another_market_never_shows_it() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    assert!(owner.drawings.set_all_windows_at(0, true));
    owner.persist(&mut settings);

    let same_layout = Chart::open(&settings, LEVELS, flow("ETHUSDT"), "tick:100");
    assert!(same_layout.drawings.items().is_empty());
    let other_layout = Chart::open(&settings, FLOWS, flow("ETHUSDT"), "tick:100");
    assert!(other_layout.drawings.items().is_empty());
}

#[test]
fn a_mark_kept_to_its_chart_ignores_the_layout() {
    let mut settings = MarketSettings::default();
    let mut chart = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_here(&mut chart.drawings, 2.0, 90.0);
    chart.persist(&mut settings);
    chart.drawings.set_layout(Some(FLOWS));
    assert_eq!(chart.shown(), [90.0], "it is the chart's, not the layout's");
}

#[test]
fn clearing_a_chart_spares_the_marks_it_does_not_show() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    owner.persist(&mut settings);

    let mut other = Chart::open(&settings, FLOWS, flow("BTCUSDT"), "tick:100");
    level_here(&mut other.drawings, 3.0, 80.0);
    assert_eq!(other.shown(), [80.0]);
    assert!(!other.drawings.all_locked());
    other.drawings.set_all_locked(true);
    assert!(other.drawings.all_locked());
    let foreign = other
        .drawings
        .items()
        .iter()
        .find(|d| d.layout == Some(LEVELS));
    assert!(
        !foreign.unwrap().locked,
        "the other layout's mark untouched"
    );
    assert_eq!(other.drawings.delete_all(), 1, "only what it shows");
    other.persist(&mut settings);
    owner.refresh(&settings);
    assert_eq!(owner.shown(), [90.0]);
}

/// A mark from before layouts were recorded shows on every chart of its
/// market, as it always has, and reads as All windows on.
#[test]
fn a_legacy_mark_shows_on_every_layout_until_turned_off_here() {
    let mut settings = MarketSettings::default();
    let mut writer = Chart::open(&settings, LEVELS, flow("BTCUSDT"), "tick:100");
    writer.drawings.set_layout(None);
    level_on_all_timeframes(&mut writer.drawings, 2.0, 90.0);
    assert_eq!(writer.drawings.items()[0].layout, None);
    writer.persist(&mut settings);

    let mut chart = Chart::open(&settings, FLOWS, context("time:5m"), "time:5m");
    let other = Chart::open(&settings, LEVELS, flow("BTCUSDT"), "tick:100");
    assert_eq!(chart.shown(), [90.0]);
    assert_eq!(other.shown(), [90.0]);
    assert!(chart.drawings.in_all_windows(0));

    assert!(chart.drawings.set_all_windows_at(0, false));
    let mark = &chart.drawings.items()[0];
    assert_eq!(mark.layout, Some(FLOWS), "claimed by this layout");
    assert!(!mark.in_all_windows(&[]));
    assert_eq!(chart.shown(), [90.0]);
}

/// Read from a file written before layouts were recorded, a shared mark
/// saves back byte for byte through a write of its market's set.
#[test]
fn a_legacy_entry_round_trips_byte_for_byte_through_a_write() {
    let mut writer = Drawings::default();
    level_on_all_timeframes(&mut writer, 2.0, 90.0);
    let legacy = SavedDrawing::from_drawing(&writer.items()[0]);
    assert_eq!(legacy.layout, None);
    let mut settings = MarketSettings::default();
    let key = flow("BTCUSDT");
    let _ = crate::store_marks(&mut settings, &key, "tick:100", writer.items(), &[]);
    let text = quantick_workspace::market_settings::to_text(&settings).unwrap();
    assert!(!text.contains("layout"), "{text}");
    let mut settings = quantick_workspace::market_settings::parse_settings(&text).unwrap();

    let mut chart = Chart::open(&settings, FLOWS, key, "tick:100");
    assert_eq!(chart.shown(), [90.0], "on every layout");
    assert!(chart.drawings.in_all_windows(0));
    level_on_all_timeframes(&mut chart.drawings, 3.0, 80.0);
    chart.persist(&mut settings);
    let shared = settings.shared_drawings(&chart.key.market);
    assert_eq!(shared.len(), 2, "the set was written");
    let back = shared.iter().find(|entry| entry.id == legacy.id).unwrap();
    assert_eq!(*back, legacy);
    assert_eq!(
        toml::to_string(back).unwrap(),
        toml::to_string(&legacy).unwrap()
    );
}

#[test]
fn a_paste_belongs_to_the_layout_it_lands_in_and_to_no_other() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    assert!(drawings.set_all_windows_at(0, true));
    let source = drawings.items()[0].clone();

    let mut target = Drawings::default();
    target.set_layout(Some(FLOWS));
    let id = target.paste(&source, 3.0);
    let copy = &target.items()[target.index_of(id).unwrap()];
    assert_eq!(copy.layout, Some(FLOWS));
    assert!(!copy.all_windows);
}

#[test]
fn all_windows_is_never_on_for_a_mark_kept_to_its_chart() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    level_here(&mut drawings, 2.0, 90.0);
    assert!(!drawings.set_all_windows_at(0, true), "refused");
    assert!(!drawings.items()[0].all_windows);
    drawings.select(Some(0));
    crate::update(
        &mut drawings,
        crate::DrawingIntent::ToggleAllWindowsSelected,
    );
    assert!(!drawings.items()[0].all_windows, "the toggle too");
    let scope = DrawingScope::ThisChart;
    assert!(!drawings.pick_where_shown(0, scope, Some(true)));
    assert!(!drawings.in_all_windows(0), "it shows on no other chart");
    assert_eq!(drawings.undo_depth(), 1, "only the placement");
}

/// A mark another layout lends this chart through "All windows" is that
/// layout's: clearing or locking this chart leaves it as its owner left it.
#[test]
fn clearing_or_locking_a_chart_spares_a_mark_lent_by_another_layout() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    assert!(owner.drawings.set_all_windows_at(0, true));
    owner.persist(&mut settings);

    let mut other = Chart::open(&settings, FLOWS, flow("BTCUSDT"), "tick:100");
    level_here(&mut other.drawings, 3.0, 80.0);
    assert_eq!(other.drawings.shown_count(), 2, "the lent mark shows");
    assert_eq!(other.drawings.owned_count(), 1, "but is not its own");
    other.drawings.set_all_locked(true);
    assert!(other.drawings.all_locked());
    let lent = |chart: &Chart| {
        let mut items = chart.drawings.items().iter();
        items.find(|d| d.layout == Some(LEVELS)).cloned()
    };
    assert!(!lent(&other).unwrap().locked, "not locked");
    assert_eq!(other.drawings.delete_all(), 1, "only its own");
    assert!(lent(&other).is_some(), "not cleared");
    other.persist(&mut settings);
    owner.refresh(&settings);
    assert_eq!(owner.shown(), [90.0]);
    assert!(owner.drawings.in_all_windows(0), "still in all windows");
}

/// A store never named a layout remembers only the new marks it still
/// holds: the ones deleted and gone from the history are let go.
#[test]
fn the_marks_waiting_for_a_layout_are_only_the_ones_held() {
    let mut drawings = Drawings::default();
    for _ in 0..3 {
        level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    }
    assert_eq!(drawings.unclaimed.len(), 3);
    drop(drawings.take_all());
    level_here(&mut drawings, 2.0, 90.0);
    assert_eq!(
        drawings.unclaimed,
        [drawings.items()[0].id],
        "the gone let go"
    );

    for _ in 0..3 * crate::UNDO_HISTORY_LIMIT {
        level_here(&mut drawings, 2.0, 90.0);
        assert!(drawings.delete_all() > 0);
    }
    assert!(
        drawings.unclaimed.len() <= crate::UNDO_HISTORY_LIMIT,
        "bounded"
    );
}

#[test]
fn layout_and_all_windows_survive_the_file() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(FLOWS));
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    assert!(drawings.set_all_windows_at(0, true));
    let saved = SavedDrawing::from_drawing(&drawings.items()[0]);
    assert_eq!(saved.layout, Some(FLOWS));
    assert!(saved.all_windows);
    let back = saved.to_drawing(drawings.items()[0].id).unwrap();
    assert_eq!(back, drawings.items()[0]);
}

/// A mark that stops showing on a chart leaves its selection there, and
/// nothing selected there can delete it.
#[test]
fn a_mark_that_leaves_a_layout_leaves_its_selection_and_cannot_be_deleted_there() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    assert!(owner.drawings.set_all_windows_at(0, true));
    owner.persist(&mut settings);
    let mut other = Chart::open(&settings, FLOWS, flow("BTCUSDT"), "tick:100");

    other.drawings.select(Some(0));
    assert_eq!(other.drawings.selected(), Some(0), "shown, so selectable");
    assert!(other.drawings.set_all_windows_at(0, false));
    assert_eq!(other.drawings.items()[0].layout, Some(LEVELS));
    assert!(other.shown().is_empty(), "back to its own layout");
    assert_eq!(other.drawings.selected(), None, "the selection let go");
    other.drawings.select(Some(0));
    assert_eq!(other.drawings.selected(), None, "and cannot be taken again");
    assert_eq!(
        other.drawings.delete_selected(true),
        DeleteOutcome::NothingSelected
    );
    assert_eq!(other.drawings.items().len(), 1, "Delete removed nothing");
}

/// Another layout may edit a mark it shows; kept to its chart from there, the
/// mark is that chart's own from then on.
#[test]
fn kept_to_its_chart_from_another_layout_the_mark_becomes_that_charts() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    assert!(owner.drawings.set_all_windows_at(0, true));
    owner.persist(&mut settings);
    let mut other = Chart::open(&settings, FLOWS, flow("BTCUSDT"), "tick:100");

    let before = other.drawings.items()[0].clone();
    other.drawings.items_mut()[0].set_scope(DrawingScope::ThisChart);
    other.drawings.record_edit_of(0, before);
    assert_eq!(other.drawings.items()[0].layout, Some(FLOWS));
    other.persist(&mut settings);
    owner.refresh(&settings);
    assert!(owner.shown().is_empty(), "the owner let it go");
    assert_eq!(other.shown(), [90.0]);
}

/// The assistant sweep is app-wide: every mark a store holds counts and goes,
/// shown on its layout or not.
#[test]
fn the_assistant_sweep_takes_marks_another_layout_owns() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    drawings.set_author_at(
        0,
        Some(DrawingAuthor {
            actor_kind: "agent".to_owned(),
            client_name: "assistant".to_owned(),
        }),
    );
    let mut other = Drawings::default();
    other.set_layout(Some(FLOWS));
    other.adopt(drawings.items().iter().cloned());
    assert_eq!(other.shown_count(), 0);
    assert_eq!(other.authored_count(), 1);
    assert_eq!(other.remove_authored(), 1);
    assert!(other.items().is_empty());
}

/// A deleted layout hands its marks to the layout its charts move to, in the
/// store and in every chart holding them, so no mark is left on a layout no
/// chart can show.
#[test]
fn a_deleted_layouts_marks_follow_its_charts_to_the_next_layout() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, FLOWS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    owner.persist(&mut settings);
    let mut other = Chart::open(&settings, LEVELS, flow("BTCUSDT"), "tick:100");
    assert!(other.shown().is_empty());

    let mut owner_layout = PaneLayout::<()> {
        shared_base: std::mem::take(&mut owner.base),
        ..PaneLayout::default()
    };
    let mut other_layout = PaneLayout::<()> {
        shared_base: std::mem::take(&mut other.base),
        ..PaneLayout::default()
    };
    let marks = |drawings, layout| PaneMarks {
        drawings,
        layout,
        now: (),
        rule: |()| String::new(),
    };
    let panes = [
        marks(&mut owner.drawings, &mut owner_layout),
        marks(&mut other.drawings, &mut other_layout),
    ];
    assert!(retarget_layout(&mut settings, FLOWS, LEVELS, panes));
    owner.base = owner_layout.shared_base;
    other.base = other_layout.shared_base;
    owner.drawings.set_layout(Some(LEVELS));
    assert_eq!(owner.shown(), [90.0]);
    assert_eq!(other.shown(), [90.0]);
    let market = &owner.key.market;
    assert_eq!(settings.shared_drawings(market)[0].layout, Some(LEVELS));

    // Level with the store: a write from either changes nothing.
    let before = settings.clone();
    owner.persist(&mut settings);
    other.persist(&mut settings);
    assert_eq!(settings, before);
}

#[test]
fn the_shown_list_is_the_marks_in_layout_with_their_indexes() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    level_here(&mut drawings, 1.0, 80.0);
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    drawings.set_layout(Some(FLOWS));
    level_here(&mut drawings, 3.0, 100.0);
    let shown: Vec<usize> = drawings.shown().map(|(index, _)| index).collect();
    let in_layout = (0..3).filter(|&index| drawings.in_layout(index));
    assert_eq!(shown, in_layout.collect::<Vec<_>>());
    assert_eq!(shown, [0, 2], "the other layout's mark is not listed");
    assert_eq!(drawings.shown_count(), 2);
}

#[test]
fn the_all_timeframes_switch_picks_the_scope_and_pins_it() {
    let mut drawings = Drawings::default();
    level_here(&mut drawings, 1.0, 80.0);
    let mark = &mut drawings.items_mut()[0];
    mark.set_all_timeframes(true);
    assert_eq!(mark.scope, DrawingScope::AllCharts);
    assert!(mark.all_timeframes());
    mark.set_all_timeframes(false);
    assert_eq!(mark.scope, DrawingScope::ThisChart);
    assert!(!mark.legacy_unpinned, "a pick pins it");
}

/// A mark whose layout the book no longer holds (a bundle imported without
/// its layouts, a starter book after a corrupt file) shows on every chart of
/// its market, never on none; a book that holds the layout again takes it
/// back, and a pick on a chart makes it that chart's layout's.
#[test]
fn a_mark_whose_layout_the_book_lost_shows_on_every_chart() {
    const LOST: LayoutId = LayoutId(9);
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LOST, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    owner.persist(&mut settings);
    let mut other = Chart::open(&settings, FLOWS, flow("BTCUSDT"), "tick:100");
    other.drawings.set_live_layouts([LOST, FLOWS]);
    assert!(
        other.shown().is_empty(),
        "its layout is held: its own alone"
    );

    other.drawings.set_live_layouts([LEVELS, FLOWS]);
    assert_eq!(other.shown(), [90.0], "lost layout: every chart shows it");
    assert!(other.drawings.is_visible(0), "painted and hit-tested");
    assert!(other.drawings.in_all_windows(0), "and says so");
    other.drawings.select(Some(0));
    assert_eq!(other.drawings.selected(), Some(0), "selectable");
    assert_eq!(
        other.drawings.items()[0].layout,
        Some(LOST),
        "never rewritten"
    );

    other.drawings.set_live_layouts([LOST, FLOWS]);
    assert!(other.shown().is_empty(), "a book holding it takes it back");
    assert_eq!(other.drawings.selected(), None, "its selection let go");

    other.drawings.set_live_layouts([LEVELS, FLOWS]);
    assert!(other.drawings.set_all_windows_at(0, false), "a pick here");
    assert_eq!(other.drawings.items()[0].layout, Some(FLOWS), "re-homed");
    assert_eq!(other.shown(), [90.0]);
    other.drawings.set_layout(Some(LEVELS));
    assert!(other.shown().is_empty(), "this layout's alone now");
}

/// Cleared from a chart, a mark whose layout the book lost goes like any
/// other it shows: none is left where nothing can reach it.
#[test]
fn clearing_a_chart_takes_the_marks_whose_layout_the_book_lost() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LayoutId(9)));
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    drawings.set_layout(Some(FLOWS));
    drawings.set_live_layouts([LEVELS, FLOWS]);
    assert_eq!(drawings.delete_all(), 1);
    assert!(drawings.items().is_empty());
}

/// A mark made before the host named the chart's layout (an agent call
/// before the first frame) belongs to the first layout named, in the history
/// too, and is never saved as one from before layouts; a mark read from
/// before layouts stays as it was read.
#[test]
fn a_mark_made_before_the_layout_was_named_is_claimed_by_the_first_one() {
    let mut read = Drawings::default();
    level_on_all_timeframes(&mut read, 1.0, 80.0);
    let legacy = SavedDrawing::from_drawing(&read.items()[0]);
    assert_eq!(legacy.layout, None, "as written before layouts");
    let mut drawings = Drawings::default();
    drawings.adopt(legacy.to_drawing(crate::DrawingId(0)));
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    let pasted = drawings.paste(&drawings.items()[1].clone(), 1.0);
    assert!(drawings.items().iter().all(|mark| mark.layout.is_none()));

    drawings.set_layout(Some(FLOWS));
    let layouts: Vec<_> = drawings.items().iter().map(|mark| mark.layout).collect();
    assert_eq!(layouts, [None, Some(FLOWS), Some(FLOWS)], "legacy stays");
    assert_eq!(drawings.index_of(pasted), Some(2));
    assert!(drawings.undo(), "the paste undone");
    assert_eq!(
        drawings.items()[1].layout,
        Some(FLOWS),
        "history claimed too"
    );
    assert!(
        SavedDrawing::from_drawing(&drawings.items()[1])
            .layout
            .is_some()
    );

    drawings.set_layout(Some(LEVELS));
    assert_eq!(drawings.items()[1].layout, Some(FLOWS), "claimed once");
}

/// A pane mirrors another's shared marks by the layout it shows itself,
/// never by the source chart's.
#[test]
fn a_mirror_lends_only_the_marks_the_viewing_layout_shows() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(LEVELS));
    level_on_all_timeframes(&mut drawings, 1.0, 80.0);
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    assert!(drawings.set_all_windows_at(1, true));
    let lent =
        |viewer| -> Vec<usize> { drawings.lent_to(viewer).map(|(index, _)| index).collect() };
    assert_eq!(lent(Some(LEVELS)), [0, 1]);
    assert_eq!(lent(Some(FLOWS)), [1], "another layout: All windows alone");
    let mut hidden = Drawings::default();
    hidden.set_layout(Some(LEVELS));
    level_on_all_timeframes(&mut hidden, 1.0, 80.0);
    hidden.items_mut()[0].hidden = true;
    assert_eq!(
        hidden.lent_to(Some(LEVELS)).count(),
        0,
        "hidden lends nothing"
    );
}

/// Edits addressed by id reach only the marks the chart shows, and the
/// counts and rows a chart reports are of those marks.
#[test]
fn edits_by_id_and_counts_reach_only_the_marks_the_chart_shows() {
    let mut settings = MarketSettings::default();
    let mut owner = Chart::open(&settings, LEVELS, context("time:5m"), "time:5m");
    level_on_all_timeframes(&mut owner.drawings, 2.0, 90.0);
    owner.persist(&mut settings);
    let mut other = Chart::open(&settings, FLOWS, flow("BTCUSDT"), "tick:100");
    level_here(&mut other.drawings, 3.0, 100.0);
    let held = other.drawings.items()[0].id;

    assert_eq!(other.drawings.shown_index_of(held), None, "held, not shown");
    assert_eq!(other.drawings.index_of(held), Some(0));
    assert_eq!(other.drawings.set_own_magnet(held, false), Err(None));
    assert_eq!(other.drawings.shared_count(), 0, "lends nothing it hides");
    assert_eq!(owner.drawings.shared_count(), 1);
    assert_eq!(
        other.drawings.selected_row(),
        Some(0),
        "the placed mark's row"
    );
    let own = other.drawings.items()[1].id;
    assert_eq!(other.drawings.shown_index_of(own), Some(1));
}

/// A chart on a deleted layout moves to the next one with its marks, so the
/// selection it held stays.
#[test]
fn a_deleted_layouts_chart_keeps_its_selection_through_the_move() {
    let mut drawings = Drawings::default();
    drawings.set_layout(Some(FLOWS));
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    drawings.select(Some(0));
    drawings.retarget_layout(FLOWS, LEVELS);
    assert_eq!(drawings.layout(), Some(LEVELS));
    assert_eq!(drawings.items()[0].layout, Some(LEVELS));
    assert_eq!(drawings.selected(), Some(0), "the selection held");
}

/// The chart switches first and its marks follow once the layout is gone:
/// the selection the switch let go of comes back with them, and only that.
#[test]
fn a_chart_switched_off_a_deleted_layout_takes_its_selection_back() {
    let mut drawings = Drawings::default();
    drawings.set_live_layouts([LEVELS, FLOWS]);
    drawings.set_layout(Some(FLOWS));
    level_on_all_timeframes(&mut drawings, 2.0, 90.0);
    drawings.select(Some(0));
    drawings.set_layout(Some(LEVELS));
    assert_eq!(drawings.selected(), None, "the switch let go");
    drawings.set_live_layouts([LEVELS]);
    drawings.retarget_layout(FLOWS, LEVELS);
    assert_eq!(drawings.items()[0].layout, Some(LEVELS));
    assert_eq!(drawings.selected(), Some(0), "taken back");

    drawings.set_live_layouts([LEVELS, FLOWS]);
    drawings.set_layout(Some(FLOWS));
    drawings.select(None);
    drawings.retarget_layout(LEVELS, FLOWS);
    assert_eq!(drawings.items()[0].layout, Some(FLOWS));
    assert_eq!(drawings.selected(), None, "a pick made since stands");
}
