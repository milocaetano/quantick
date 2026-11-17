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
    assert!(!mark.in_all_windows());
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
    assert!(mark.shows_in(Some(FLOWS)));
    assert!(!mark.shows_in(Some(LEVELS)));
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
    assert!(owner.drawings.items()[0].in_all_windows());
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
    assert!(chart.drawings.items()[0].in_all_windows());

    assert!(chart.drawings.set_all_windows_at(0, false));
    let mark = &chart.drawings.items()[0];
    assert_eq!(mark.layout, Some(FLOWS), "claimed by this layout");
    assert!(!mark.in_all_windows());
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
    assert!(chart.drawings.items()[0].in_all_windows());
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
    assert!(drawings.set_all_windows_at(0, true), "the pick is kept");
    assert!(
        !drawings.items()[0].in_all_windows(),
        "but it shows on no other chart"
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
