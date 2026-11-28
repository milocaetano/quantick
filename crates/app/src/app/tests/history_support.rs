use super::*;

/// A minute candle at `minute`, priced so a fold is visible in the result.
pub(super) fn venue_candle(minute: i64, seed: i64) -> quantick_engine::Bar {
    let open_time = minute * quantick_feed::OHLCV_BASE_INTERVAL_MS;
    quantick_engine::Bar {
        open_time,
        close_time: open_time + quantick_feed::OHLCV_BASE_INTERVAL_MS - 1,
        open: Decimal::from(100 + seed),
        high: Decimal::from(110 + seed),
        low: Decimal::from(90 + seed),
        close: Decimal::from(105 + seed),
        buy_volume: Decimal::from(2),
        sell_volume: Decimal::from(3),
        trade_count: 7,
    }
}

/// A trade one minute after the one before it, so a fixture fills an M1
/// pane with real bars rather than one long forming candle.
pub(super) fn minute_trade(minute: u64) -> quantick_engine::Trade {
    minute_trade_at(minute as i64)
}

/// The same, for a minute that may sit before the fixture's first — what
/// "load older" delivers.
pub(super) fn minute_trade_at(minute: i64) -> quantick_engine::Trade {
    let mut trade = trade(minute.unsigned_abs() + 1);
    trade.timestamp_ms = minute * quantick_feed::OHLCV_BASE_INTERVAL_MS + 1_000;
    trade
}

/// Venue candles for the minutes *before* the fixture trades, which start
/// in minute 0 — so the prefix never overlaps the engine's own bars.
pub(super) fn venue_history(count: i64) -> Vec<quantick_engine::Bar> {
    (-count..0)
        .map(|m| venue_candle(m, m.rem_euclid(5)))
        .collect()
}

/// Venue candles for a range of minutes, so a test can hand over one
/// progressive slice at a time. Minutes are negative: the fixture trades
/// start in minute 0, and the prefix sits before them.
pub(super) fn venue_history_range(from_minute: i64, to_minute: i64) -> Vec<quantick_engine::Bar> {
    (from_minute..to_minute)
        .map(|m| venue_candle(m, m.rem_euclid(5)))
        .collect()
}

/// What one queued `FetchOhlcv` asked for: whether it wanted a progressive
/// answer (`slice_ms`) and where its newest edge was (`before_ms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OhlcvAsk {
    pub(super) slice_ms: Option<i64>,
    pub(super) before_ms: Option<i64>,
}

/// Every FetchOhlcv sitting in the command channel, in order.
///
/// One drain, not one per field: a receiver is consumed by reading it, so
/// two helpers over the same channel meant the second call always saw an
/// empty queue — a trap a test would fail on for the wrong reason.
pub(super) fn drain_ohlcv_fetches(commands: &mut mpsc::Receiver<FeedCommand>) -> Vec<OhlcvAsk> {
    let mut asked = Vec::new();
    while let Ok(command) = commands.try_recv() {
        if let FeedCommand::FetchOhlcv {
            slice_ms,
            before_ms,
            ..
        } = command
        {
            asked.push(OhlcvAsk {
                slice_ms,
                before_ms,
            });
        }
    }
    asked
}

/// How many candle requests went out.
pub(super) fn drain_ohlcv_requests(commands: &mut mpsc::Receiver<FeedCommand>) -> usize {
    drain_ohlcv_fetches(commands).len()
}

/// The `slice_ms` every queued FetchOhlcv asked for, in order — what says
/// whether the tab asked for a progressive answer or the old single one.
pub(super) fn drain_ohlcv_slice_requests(
    commands: &mut mpsc::Receiver<FeedCommand>,
) -> Vec<Option<i64>> {
    drain_ohlcv_fetches(commands)
        .into_iter()
        .map(|ask| ask.slice_ms)
        .collect()
}

/// The `before_ms` every queued FetchOhlcv asked for, in order — what says
/// whether the tab asked for the opening span or reached back past what it
/// already holds.
pub(super) fn drain_ohlcv_before_requests(
    commands: &mut mpsc::Receiver<FeedCommand>,
) -> Vec<Option<i64>> {
    drain_ohlcv_fetches(commands)
        .into_iter()
        .map(|ask| ask.before_ms)
        .collect()
}

/// A split app whose feed reports candle history, with the channel ends
/// the test drives.
pub(super) fn history_app(
    ctx: &egui::Context,
) -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
) {
    app_backfilled_with(ctx, (0..200).map(minute_trade).collect())
}

/// [`history_app`] on a tape of a chosen magnitude and grid — the two
/// facts the row-sizing rule reads. Depth capture stays off, as it is
/// there: a chart no depth snapshot will ever hand a price to is the case
/// that sizing exists for.
pub(super) fn tape_app(
    ctx: &egui::Context,
    first_price: Decimal,
    step: Decimal,
) -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
) {
    app_backfilled_with(ctx, grid_trades(first_price, step))
}

pub(super) fn app_backfilled_with(
    ctx: &egui::Context,
    trades: Vec<quantick_engine::Trade>,
) -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
) {
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let mut app = QuantickApp::new(
        test_config(),
        "binance",
        "TESTUSDT",
        BarSpec::Tick(1),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(FeedCapabilities {
                transport: true,
                book_capture: false,
                history_paging: true,
                history_boundary: None,
                traded_volume: true,
                deal_counter: false,
                ohlcv_history: true,
                ohlcv_generation: 0,
                ohlcv_daily_generation: 0,
                ohlcv_aggressor_split: false,
                native_candle_intervals: false,
                liquidations: false,
                order_routing: false,
            }),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    let _ = book_tx;
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.drain_tabs();
    run_frame(&mut app, ctx);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    (app, evt_tx, cmd_rx)
}

/// Two hundred prints walking a `step`-wide grid from `first_price` — the
/// two facts the row-sizing rule reads off a tape. The prices move rather
/// than repeat because a grid is named from the *distances* between
/// prints, and the same price twice is no distance at all.
pub(super) fn grid_trades(first_price: Decimal, step: Decimal) -> Vec<quantick_engine::Trade> {
    (0..200)
        .map(|i| quantick_engine::Trade {
            agg_id: i + 1,
            timestamp_ms: i as i64 * quantick_feed::OHLCV_BASE_INTERVAL_MS + 1_000,
            price: first_price + step * Decimal::from(i % 20),
            quantity: Decimal::ONE,
            side: if i.is_multiple_of(2) {
                quantick_engine::Side::Buy
            } else {
                quantick_engine::Side::Sell
            },
        })
        .collect()
}

/// Every `LoadOlder` sitting in the command channel, as the counts asked
/// for. The reach turns one press into a run of these.
pub(super) fn drain_load_older(commands: &mut mpsc::Receiver<FeedCommand>) -> Vec<usize> {
    let mut asks = Vec::new();
    while let Ok(command) = commands.try_recv() {
        if let FeedCommand::LoadOlder { count } = command {
            asks.push(count);
        }
    }
    asks
}
