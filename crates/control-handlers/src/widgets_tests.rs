use quantick_chart::widgets::depth::DEPTH_EMPTY;
use quantick_chart::widgets::liquidations::LIQUIDATIONS_NOT_PROVIDED;
use quantick_chart::widgets::open_interest::OPEN_INTEREST_NOT_PROVIDED;
use quantick_chart::widgets::spot_perp_cvd::SPOT_PERP_NOT_PROVIDED;
use quantick_engine::{Side, Trade};

use super::*;

const SIZE: (f32, f32) = (400.0, 300.0);

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn trade(agg_id: u64, timestamp_ms: i64, price: &str, quantity: &str, side: Side) -> Trade {
    Trade {
        agg_id,
        timestamp_ms,
        price: d(price),
        quantity: d(quantity),
        side,
    }
}

fn tape() -> TradeTape {
    [
        trade(1, 1_000, "76200", "0.1", Side::Buy),
        trade(2, 2_000, "76201", "4", Side::Sell),
        trade(3, 3_000, "76199.5", "0.2", Side::Buy),
    ]
    .into_iter()
    .collect()
}

fn ladder() -> BookLadder {
    BookLadder {
        best_bid: None,
        best_ask: None,
        bids: Vec::new(),
        asks: Vec::new(),
    }
}

fn feed<'a>(
    trades: &'a TradeTape,
    setup: &'a WidgetSetup,
    capabilities: FeedCapabilities,
) -> WidgetFeed<'a> {
    WidgetFeed {
        tab_id: 7,
        capabilities,
        ladder: None,
        earlier: None,
        fallback_tick: d("0.5"),
        trades,
        setup,
        base_unit: "BTC",
        tz: TzOffset::new(0),
        now_ms: 3_500,
    }
}

fn usd() -> WidgetSetup {
    WidgetSetup {
        notional_unit: NotionalKind::Usd,
        big_trade_min: Some(250_000),
        ..WidgetSetup::default()
    }
}

fn spec(id: &'static str, scene: SceneBuilder) -> WidgetSpec {
    WidgetSpec {
        id,
        title: "T",
        gate: |_| true,
        scene,
    }
}

#[test]
fn big_trades_lists_the_prints_over_the_configured_threshold() {
    let (trades, setup) = (tape(), usd());
    let scene = big_trades_scene(
        &feed(&trades, &setup, FeedCapabilities::none()),
        SIZE.0,
        SIZE.1,
    );
    assert_eq!(scene.rows, 1, "only the 4 BTC sell clears $250k");
    let texts: Vec<&str> = scene.texts().collect();
    assert!(texts.contains(&"SELL"), "{texts:?}");
    assert!(texts.contains(&"76201.0"), "{texts:?}");
    assert!(scene.chips.contains(&("250k".to_owned(), true)));
}

#[test]
fn depth_needs_a_book_and_says_which_is_missing() {
    let (trades, setup) = (tape(), usd());
    let none = FeedCapabilities::none();
    let scene = depth_scene(&feed(&trades, &setup, none), SIZE.0, SIZE.1);
    assert_eq!(scene.empty.as_deref(), Some(DEPTH_NOT_PROVIDED));
    let book = FeedCapabilities {
        book_capture: true,
        ..none
    };
    let empty_book = ladder();
    let booked = WidgetFeed {
        ladder: Some((&empty_book, 3_000)),
        ..feed(&trades, &setup, book)
    };
    let scene = depth_scene(&booked, SIZE.0, SIZE.1);
    assert!(
        scene.rows > 0,
        "the tape centres the ladder on an empty book"
    );
    assert!(scene.texts().any(|text| text == "76200.0"));
    let scene = depth_scene(&feed(&trades, &setup, book), SIZE.0, SIZE.1);
    assert_eq!(scene.empty.as_deref(), Some(DEPTH_EMPTY));
}

#[test]
fn derivative_panes_say_the_feed_does_not_provide_them() {
    let (trades, setup) = (tape(), usd());
    let feed = feed(&trades, &setup, FeedCapabilities::none());
    let builders: [(SceneBuilder, &str); 3] = [
        (liquidations_scene, LIQUIDATIONS_NOT_PROVIDED),
        (open_interest_scene, OPEN_INTEREST_NOT_PROVIDED),
        (spot_perp_cvd_scene, SPOT_PERP_NOT_PROVIDED),
    ];
    for (build, text) in builders {
        assert_eq!(build(&feed, SIZE.0, SIZE.1).empty.as_deref(), Some(text));
    }
}

#[test]
fn the_deck_rebuilds_a_scene_only_when_its_inputs_change() {
    let (trades, setup) = (tape(), usd());
    let spec = spec("big_trades", big_trades_scene);
    let mut deck = WidgetDeck::default();
    let feed = feed(&trades, &setup, FeedCapabilities::none());
    let first = deck.scene(&spec, &feed, SIZE).clone();
    deck.scenes.get_mut("big_trades").expect("cached").1.rows = 99;
    assert_eq!(
        deck.scene(&spec, &feed, SIZE).rows,
        99,
        "same inputs, cached"
    );
    let mut longer = tape();
    longer.push(trade(4, 3_200, "76200", "5", Side::Buy));
    let moved = WidgetFeed {
        trades: &longer,
        ..feed
    };
    assert_eq!(deck.scene(&spec, &moved, SIZE).rows, first.rows + 1);
}

#[test]
fn the_deck_keeps_the_book_before_the_current_one() {
    let (trades, setup) = (tape(), usd());
    let (old, new) = (ladder(), ladder());
    let mut deck = WidgetDeck::default();
    let at = |ladder, at_ms| WidgetFeed {
        ladder: Some((ladder, at_ms)),
        ..feed(&trades, &setup, FeedCapabilities::none())
    };
    deck.observe(&at(&old, 10));
    deck.observe(&at(&old, 10));
    assert_eq!(deck.books[1], None, "the same book twice is one book");
    deck.observe(&at(&new, 20));
    let times: Vec<Option<i64>> = deck
        .books
        .iter()
        .map(|book| book.as_ref().map(|b| b.1))
        .collect();
    assert_eq!(times, [Some(20), Some(10)]);
}

#[test]
fn open_close_and_read_are_bounded_and_named() {
    let mut deck = WidgetDeck::default();
    let registry = [
        spec("depth", depth_scene),
        spec("big_trades", big_trades_scene),
    ];
    assert!(deck.set_open("big_trades", true));
    assert!(deck.set_open("depth", true));
    deck.set_open("big_trades", true);
    assert_eq!(deck.open_ids(), ["depth", "big_trades"]);
    assert!(!deck.set_open("depth", false));
    assert_eq!(deck.open_ids(), ["big_trades"]);
    let error = find(&registry, "tape").err().expect("unknown id");
    assert!(error.contains("depth, big_trades"), "{error}");
    let caps = FeedCapabilities::none();
    let state = state(&registry[1], &deck, &caps);
    assert!(state.open && state.provided);
    let mut scene = WidgetScene::empty("text", Vec::new(), SIZE.0, SIZE.1);
    let text = scene.prims[0].clone();
    scene.prims = vec![text; MAX_READ_TEXTS + 10];
    let read = read(state, "BTCUSDT · Binance".to_owned(), &scene);
    assert_eq!(read.texts.len(), MAX_READ_TEXTS);
}

#[test]
fn a_hook_list_opens_only_the_panes_the_registry_knows() {
    static REGISTRY: [WidgetSpec; 2] = [
        WidgetSpec {
            id: "depth",
            title: "DEPTH",
            gate: |_| true,
            scene: depth_scene,
        },
        WidgetSpec {
            id: "big_trades",
            title: "BIG TRADES",
            gate: |_| true,
            scene: big_trades_scene,
        },
    ];
    let mut deck = WidgetDeck::default();
    deck.open_listed(&REGISTRY, " big_trades ,tape,,depth");
    assert_eq!(deck.open_ids(), ["big_trades", "depth"]);
}
