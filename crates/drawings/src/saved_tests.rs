//! A pane's marks through the market store: drawn on one cut of a market,
//! read back on another.
use super::*;
use crate::{DRAWING_TOOLS, Drawings};
use quantick_workspace::market_settings::{parse_settings, to_text};

const AT_15M: &str = "time:15m";
const MINUTE_MS: i64 = 60_000;

fn tool(id: &str) -> DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("registered test tool")
}

fn flow(asset: &str) -> DrawingKey {
    DrawingKey::of_pane("binance", asset.to_owned(), None)
}

fn context(asset: &str, bars: &str) -> DrawingKey {
    DrawingKey::of_pane("binance", asset.to_owned(), Some(bars.to_owned()))
}

/// A level placed by hand on a pane, at a bar with a market instant.
fn drawn(price: f64) -> Drawings<DrawingTool> {
    let mut drawings = Drawings::default();
    assert!(drawings.place(
        tool("horizontal-line"),
        ChartPoint::at_time(4.0, price, Some(1_700_000_000_000 + 4 * 15 * MINUTE_MS)),
    ));
    drawings
}

fn prices(marks: &[Drawing<DrawingTool>]) -> Vec<f64> {
    marks.iter().map(|mark| mark.points[0].price).collect()
}

/// Every cut of the market a trader can open: the flow chart at ticks and at
/// minutes, and context charts at other intervals and at renko bricks.
fn every_cut() -> [(DrawingKey, &'static str); 5] {
    [
        (flow("BTCUSDT"), "tick:100"),
        (flow("BTCUSDT"), "time:5m"),
        (context("BTCUSDT", "time:5m"), "time:5m"),
        (context("BTCUSDT", "renko:20"), "renko:20"),
        (context("BTCUSDT", AT_15M), AT_15M),
    ]
}

#[test]
fn a_drawing_on_15m_shows_on_5m_tick_and_renko_of_its_market() {
    let mut settings = MarketSettings::default();
    let pane = drawn(100.0);
    assert_eq!(pane.items()[0].scope, DrawingScope::AllCharts);
    let write = store_marks(
        &mut settings,
        &context("BTCUSDT", AT_15M),
        AT_15M,
        pane.items(),
    );
    assert!(write.all_timeframes);

    for (key, bars) in every_cut() {
        assert_eq!(
            prices(&pane_marks(&settings, &key, bars)),
            [100.0],
            "{key:?} at {bars}"
        );
    }
    assert!(
        pane_marks(&settings, &flow("ETHUSDT"), "tick:100").is_empty(),
        "never on another market"
    );
}

#[test]
fn a_drawing_kept_to_this_chart_shows_only_at_its_bars_and_comes_back() {
    let mut settings = MarketSettings::default();
    let on_15m = context("BTCUSDT", AT_15M);
    let mut pane = drawn(100.0);
    pane.selected_mut().expect("just placed").scope = DrawingScope::ThisChart;
    store_marks(&mut settings, &on_15m, AT_15M, pane.items());

    for (key, bars) in every_cut() {
        let expected: &[f64] = if key == on_15m { &[100.0] } else { &[] };
        assert_eq!(
            prices(&pane_marks(&settings, &key, bars)),
            expected,
            "{key:?} at {bars}"
        );
    }

    // The same pane cut to 5m and back: hidden, then restored.
    let on_5m = context("BTCUSDT", "time:5m");
    let away = pane_marks(&settings, &on_5m, "time:5m");
    store_marks(&mut settings, &on_5m, "time:5m", &away);
    assert_eq!(prices(&pane_marks(&settings, &on_15m, AT_15M)), [100.0]);
}

#[test]
fn opting_out_on_the_flow_chart_pins_the_mark_to_the_flow_bars_it_was_on() {
    let mut settings = MarketSettings::default();
    let mut pane = drawn(100.0);
    pane.selected_mut().expect("just placed").scope = DrawingScope::ThisChart;
    store_marks(&mut settings, &flow("BTCUSDT"), "tick:100", pane.items());

    assert_eq!(
        prices(&pane_marks(&settings, &flow("BTCUSDT"), "tick:100")),
        [100.0]
    );
    assert!(pane_marks(&settings, &flow("BTCUSDT"), "tick:200").is_empty());
    assert_eq!(
        settings.drawings(&flow("BTCUSDT"))[0].bars.as_deref(),
        Some("tick:100")
    );
}

#[test]
fn an_entry_from_before_pinning_comes_back_unpinned_and_is_written_so() {
    let mut settings = MarketSettings::default();
    let mut old = SavedDrawing::from_drawing(&drawn(100.0).items()[0]);
    old.shared = false;
    settings.replace(&flow("BTCUSDT"), vec![old.clone()], |_| true);

    let shown = pane_marks(&settings, &flow("BTCUSDT"), "tick:100");
    assert!(shown[0].legacy_unpinned);
    assert_eq!(
        prices(&pane_marks(&settings, &flow("BTCUSDT"), "time:1m")),
        [100.0],
        "at every bar rule of its chart, as before"
    );

    let mut edited = shown;
    edited[0].points[0].price = 101.0;
    store_marks(&mut settings, &flow("BTCUSDT"), "tick:100", &edited);
    let written = &settings.drawings(&flow("BTCUSDT"))[0];
    assert_eq!(written.bars, None, "an edit does not pin an old mark");
    assert_eq!(written.points[0].price, 101.0);
}

#[test]
fn both_kinds_survive_a_save_and_a_load() {
    let mut settings = MarketSettings::default();
    let mut pane = drawn(100.0);
    assert!(pane.place(
        tool("horizontal-line"),
        ChartPoint::at_time(5.0, 90.0, Some(1_700_000_000_000 + 5 * 15 * MINUTE_MS)),
    ));
    pane.selected_mut().expect("just placed").scope = DrawingScope::ThisChart;
    store_marks(
        &mut settings,
        &context("BTCUSDT", AT_15M),
        AT_15M,
        pane.items(),
    );

    let loaded = parse_settings(&to_text(&settings).unwrap()).unwrap();
    assert_eq!(loaded, settings);
    let back = pane_marks(&loaded, &context("BTCUSDT", AT_15M), AT_15M);
    assert_eq!(prices(&back), [100.0, 90.0]);
    assert_eq!(back[0].scope, DrawingScope::AllCharts);
    assert_eq!(back[1].scope, DrawingScope::ThisChart);
    assert_eq!(
        prices(&pane_marks(&loaded, &flow("BTCUSDT"), "tick:100")),
        [100.0]
    );
}
