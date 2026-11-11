//! The widget panes through the control plane, on a live BTCUSDT tab: each
//! opens, reads back what its window paints, and closes.
use super::asset_bubbles_tests::shipped_config;
use super::*;
use crate::control::ActionOrigin;
use quantick_chart::widgets::liquidations::LIQUIDATIONS_NOT_PROVIDED;
use quantick_chart::widgets::open_interest::OPEN_INTEREST_NOT_PROVIDED;
use quantick_chart::widgets::spot_perp_cvd::SPOT_PERP_NOT_PROVIDED;
use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};
use serde_json::{Value, json};

/// The shipped Binance BTCUSDT tab, holding a book and a tape.
fn live_btc_tab() -> QuantickApp {
    crate::store_home::next_test_home();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (_book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, mut cmd_rx) = mpsc::channel(16);
    let mut app = QuantickApp::new(
        shipped_config(),
        "binance",
        "BTCUSDT",
        BarSpec::Tick(50),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    let level = |price: i64, size: i64| BookLevel::new(price.into(), size.into()).unwrap();
    let tape = app.active_tab_mut().tape_mut();
    tape.set_depth_visible(true);
    tape.handle_depth_event(DepthEvent::Snapshot {
        symbol: "BTCUSDT".to_owned(),
        generation: take_capture_start(&mut cmd_rx),
        observed_at_ms: 1_100,
        effective_at_ms: 999,
        price_step: None,
        snapshot: BookSnapshot::new(
            10,
            vec![level(76_199, 3), level(76_198, 12)],
            vec![level(76_201, 2), level(76_202, 9)],
            BookCoverage::Limited {
                levels_per_side: 1_000,
            },
        ),
    });
    tape.flush_for_test();
    let print = |agg_id: u64, quantity: i64, side| quantick_engine::Trade {
        agg_id,
        timestamp_ms: 1_000 + agg_id as i64,
        price: Decimal::from(76_200),
        quantity: Decimal::from(quantity),
        side,
    };
    let trades = vec![
        print(1, 1, quantick_engine::Side::Buy),
        print(2, 5, quantick_engine::Side::Sell),
        print(3, 4, quantick_engine::Side::Buy),
    ];
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    let tab_id = app.runtime.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    app
}

fn call(app: &mut QuantickApp, capability: &str, id: &str) -> Value {
    app.control_action(capability, 1, ActionOrigin::Human, json!({ "id": id }))
        .unwrap_or_else(|error| panic!("{capability} {id}: {error:?}"))
}

#[test]
fn open_read_close_every_widget() {
    let mut app = live_btc_tab();
    let ctx = egui::Context::default();
    let ids = crate::widget_surfaces::WIDGET_SURFACES.map(|spec| spec.id);
    assert_eq!(
        ids,
        [
            "depth",
            "big_trades",
            "liquidations",
            "open_interest",
            "spot_perp_cvd"
        ]
    );
    let not_provided = [
        LIQUIDATIONS_NOT_PROVIDED,
        OPEN_INTEREST_NOT_PROVIDED,
        SPOT_PERP_NOT_PROVIDED,
    ];
    for id in ids {
        let opened = call(&mut app, "widget.open", id);
        assert_eq!(opened["open"], true, "{id}: {opened}");
        // A new window lays itself out on its first frame and paints on the next.
        run_frame(&mut app, &ctx);
        let painted = painted_text(&run_frame(&mut app, &ctx));
        let read = call(&mut app, "widget.read", id);
        assert_eq!(read["open"], true, "{id}");
        assert_eq!(read["market"], "BTCUSDT · Binance", "{id}");
        let texts: Vec<&str> = read["texts"]
            .as_array()
            .expect("texts")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        match id {
            "depth" | "big_trades" => {
                assert_eq!(read["provided"], true, "{id}");
                assert!(read["rows"].as_u64() > Some(0), "{id} holds rows: {read}");
                assert!(read.get("empty").is_none(), "{id}: {read}");
                assert!(texts.contains(&"76200.00"), "{id}: {texts:?}");
                assert!(
                    painted.iter().any(|text| text == "76200.00"),
                    "{id} painted"
                );
            }
            _ => {
                assert_eq!(read["provided"], false, "{id}");
                assert_eq!(read["rows"], 0, "{id}");
                let empty = read["empty"].as_str().expect("an empty state");
                assert!(not_provided.contains(&empty), "{id}: {empty}");
                assert!(painted.iter().any(|text| text == empty), "{id} painted");
            }
        }
        let closed = call(&mut app, "widget.close", id);
        assert_eq!(closed["open"], false, "{id}");
        assert_eq!(call(&mut app, "widget.read", id)["open"], false, "{id}");
    }
    let unknown = app.control_action("widget.open", 1, ActionOrigin::Human, json!({"id": "tape"}));
    assert!(unknown.is_err(), "an unknown pane is refused");
}

#[test]
fn big_trades_reads_the_feed_config_threshold() {
    let mut app = live_btc_tab();
    let read = call(&mut app, "widget.read", "big_trades");
    let chips: Vec<(&str, bool)> = read["chips"]
        .as_array()
        .expect("chips")
        .iter()
        .map(|chip| (chip["text"].as_str().unwrap(), chip["on"] == true))
        .collect();
    assert!(chips.contains(&("250k", true)), "{chips:?}");
    assert_eq!(
        read["rows"], 2,
        "the $381k and $305k prints clear $250k: {read}"
    );
}
