//! `chart.context.read` through the fake window.
use super::*;
use crate::indicator_host::IndicatorRefusal;
use crate::test_support::{FakePane, FakeTab, FakeWindow};
use quantick_drawings::{
    ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingStyle, NewDrawing, PaneKey,
};
use quantick_stores::indicator_state::{SavedIndicator, SavedInput, SavedKind};
use quantick_workspace::layout_document::LayoutId;
use rust_decimal::Decimal;
use serde_json::json;

fn place(pane: &mut FakePane, tool_id: &str, prices: &[f64]) {
    place_on(&mut pane.drawings, &DrawingBand::Price, tool_id, prices);
}

fn place_on(
    drawings: &mut Drawings<DrawingTool>,
    band: &DrawingBand,
    tool_id: &str,
    prices: &[f64],
) {
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == tool_id)
        .unwrap();
    for (bar, price) in prices.iter().enumerate() {
        let point = ChartPoint::at_time(bar as f32, *price, Some(1_000 + bar as i64));
        drawings.place_with(tool, band, point, |tool| NewDrawing {
            style: DrawingStyle::default(),
            payload: tool.default_payload(),
        });
    }
}

fn armed_on(drawing: DrawingId) -> ArmedReading<DrawingId> {
    ArmedReading {
        drawing,
        preset: "BF buy 1x1".to_owned(),
        side: Side::Buy,
        quantity: Decimal::from(2),
        tp_mult: Decimal::from(3),
        sl_mult: Decimal::ONE,
        on_break: "retest_limit",
        execution: "alarm_only",
        rearm: "one_shot",
        state: "armed",
        disarm_reason: None,
        badge: "BF buy 1x1 · alarm only".to_owned(),
    }
}

/// Tab 1 on BTCUSDT; its pane 2 holds a zone and a line, an EMA, and a
/// strategy on the zone plus one on a drawing since deleted.
fn window() -> FakeWindow {
    let mut pane = FakePane::with_trades(2, 50, []);
    place(&mut pane, "rectangle", &[100.0, 105.5]);
    place(&mut pane, "trend-line", &[101.25, 102.0]);
    let zone = pane.drawings.items()[0].id;
    pane.armed = vec![armed_on(zone), armed_on(DrawingId(99))];
    let mut tab = FakeTab::new(1, "BTCUSDT");
    tab.panes.push(pane);
    let mut window = FakeWindow::new(vec![tab]);
    let ema = SavedIndicator {
        kind: SavedKind::native("native.ema"),
        hidden: true,
        mouse_vertical_line: false,
        inputs: vec![SavedInput::Int(21)],
        plot_styles: Vec::new(),
    };
    window.indicators.saved = vec![(5, ema, true)];
    window
}

fn call(window: &mut FakeWindow, pane: &str) -> Result<Value, ControlError> {
    let input = json!({ "tab_id": "1", "pane_id": pane });
    request(window, &mut (), &FakeWindow::assistant(), &input)
}

#[test]
fn one_call_reads_market_bars_indicators_drawings_and_strategies() {
    let read = call(&mut window(), "2").unwrap();
    assert_eq!(read["feed_id"], "fake");
    assert_eq!(read["symbol"], "BTCUSDT");
    assert_eq!(read["bar_spec"]["kind"], "tick");
    assert_eq!(read["bar_spec"]["parameter"], "50");
    assert_eq!(read["indicators_total"], "1");
    let ema = &read["indicators"][0];
    assert_eq!(
        (&ema["kind"], &ema["id"], &ema["hidden"]),
        (&json!("native"), &json!("native.ema"), &json!(true))
    );
    assert_eq!(
        ema["inputs"][0]["value"],
        json!({"type": "int", "value": 21})
    );
    assert_eq!(read["drawings_total"], "2");
    let zone = &read["drawings"][0];
    assert_eq!(zone["name"], "zone-1");
    assert_eq!(zone["kind"], "rectangle");
    assert_eq!(zone["band"], "price");
    assert_eq!(zone["prices"], json!(["100", "105.5"]));
    assert_eq!(zone["selected"], false);
    let line = &read["drawings"][1];
    assert_eq!(line["name"], "line-1");
    assert_eq!(line["prices"], json!(["101.25", "102"]));
    assert_eq!(line["selected"], true, "the last placement is selected");
    assert_eq!(read["strategies_total"], "2");
    let on_zone = &read["strategies"][0];
    assert_eq!(on_zone["drawing_name"], "zone-1");
    assert_eq!(on_zone["side"], "buy");
    assert_eq!(on_zone["quantity"], "2");
    assert_eq!(
        (&on_zone["tp_mult"], &on_zone["sl_mult"]),
        (&json!("3"), &json!("1"))
    );
    assert_eq!(on_zone["on_break"], "retest_limit");
    assert_eq!(on_zone["execution"], "alarm_only");
    assert_eq!(on_zone["state"], "armed");
    assert_eq!(read["strategies"][1]["drawing_name"], Value::Null);
}

#[test]
fn an_unknown_pane_or_tab_is_refused() {
    let mut window = window();
    let error = call(&mut window, "9").unwrap_err();
    assert!(error.message.contains("unknown pane"), "{}", error.message);
    let input = json!({ "tab_id": "8", "pane_id": "2" });
    assert!(request(&mut window, &mut (), &FakeWindow::assistant(), &input).is_err());
}

#[test]
fn a_malformed_input_is_refused() {
    let input = json!({ "tab_id": "1" });
    assert!(request(&mut window(), &mut (), &FakeWindow::assistant(), &input).is_err());
}

#[test]
fn the_drawing_list_stops_at_its_cap_and_reports_the_total() {
    let mut window = window();
    let pane = &mut window.tabs[0].panes[0];
    for _ in 0..MAX_DRAWINGS {
        place(pane, "horizontal-line", &[100.0]);
    }
    let read = call(&mut window, "2").unwrap();
    assert_eq!(read["drawings"].as_array().unwrap().len(), MAX_DRAWINGS);
    assert_eq!(read["drawings_total"], (MAX_DRAWINGS + 2).to_string());
}

#[test]
fn an_indicator_owner_that_refuses_refuses_the_read() {
    let mut window = window();
    window.indicators.refusal = Some(IndicatorRefusal::UnknownTabOrPane);
    let error = call(&mut window, "2").unwrap_err();
    assert_eq!(error.message, "unknown tab or pane");
}

#[test]
fn a_mark_held_for_another_layout_is_not_listed_nor_named_for_its_strategy() {
    let mut window = window();
    let pane = &mut window.tabs[0].panes[0];
    let mut levels = Drawings::default();
    levels.set_layout(Some(LayoutId(1)));
    place_on(&mut levels, &DrawingBand::Price, "rectangle", &[90.0, 95.0]);
    levels.items_mut()[0].set_scope(quantick_drawings::DrawingScope::AllCharts);
    let held = levels.items()[0].id;
    pane.drawings = Drawings::default();
    pane.drawings.set_layout(Some(LayoutId(2)));
    pane.drawings.adopt(levels.items().iter().cloned());
    place_on(
        &mut pane.drawings,
        &DrawingBand::Price,
        "trend-line",
        &[1.0, 2.0],
    );
    pane.armed = vec![armed_on(held)];
    let read = call(&mut window, "2").unwrap();
    assert_eq!(read["drawings_total"], "1");
    assert_eq!(read["drawings"][0]["kind"], "trend-line");
    assert_eq!(read["drawings"][0]["selected"], true);
    assert_eq!(read["strategies"][0]["drawing_name"], Value::Null);
}

#[test]
fn a_drawing_on_an_indicator_band_names_that_band() {
    let mut window = window();
    let cvd = DrawingBand::Indicator(PaneKey {
        kind: "native.cvd".into(),
        ordinal: 0,
    });
    let pane = &mut window.tabs[0].panes[0];
    place_on(&mut pane.drawings, &cvd, "horizontal-line", &[-1250.0]);
    let read = call(&mut window, "2").unwrap();
    let level = &read["drawings"][2];
    assert_eq!(
        level["band"],
        json!({"indicator": {"kind": "native.cvd", "ordinal": 0}})
    );
    assert_eq!(level["prices"], json!(["-1250"]));
}

#[test]
fn a_store_hidden_all_reads_every_mark_hidden() {
    let mut window = window();
    window.tabs[0].panes[0].drawings.set_all_hidden(true);
    let read = call(&mut window, "2").unwrap();
    let drawings = read["drawings"].as_array().unwrap();
    assert!(drawings.iter().all(|mark| mark["hidden"] == true));
}

#[test]
fn the_strategy_list_stops_at_its_cap_and_reports_the_total() {
    let mut window = window();
    let pane = &mut window.tabs[0].panes[0];
    pane.armed = (0..MAX_STRATEGIES as u64 + 5)
        .map(|id| armed_on(DrawingId(id)))
        .collect();
    let read = call(&mut window, "2").unwrap();
    assert_eq!(read["strategies"].as_array().unwrap().len(), MAX_STRATEGIES);
    assert_eq!(read["strategies_total"], (MAX_STRATEGIES + 5).to_string());
}
