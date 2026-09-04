// The `app.rs` unit tests, split by the subsystem each one exercises.
//
// They stay child modules of `crate::app` rather than moving to
// `crates/app/tests/`: an integration test is a separate crate and sees only
// `quantick-app`'s public API, while these reach `QuantickApp`'s private
// items. A child module sees its ancestor's private items, so the split costs
// no widened visibility anywhere in production code.
//
// The shared harness -- `test_app`, the `run_frame` family, the paint readers
// -- lives here in the parent, and one `use super::*` per file is all any of
// them needs. That single glob carries `crate::app`'s own imports down too:
// this module's `use super::*` binds them here, and a child sees an
// ancestor's private bindings, glob-imported ones included. So the scope
// inside these files is the scope the tests had while they lived in `app.rs`,
// reached in one hop rather than two.
//
// The `_tests` suffix on every file is what makes that one hop enough, and it
// is load-bearing rather than decorative. Siblings glob each other in through
// the same `use super::*`, so a module named `drawings` would shadow the
// crate's own `drawings` inside all twelve files at once -- five of the twelve
// subsystem names collide with a real module that way. Suffixing all twelve
// keeps the naming uniform instead of renaming only the ones that clash today.
//
// This was learned the expensive way: the first attempt used the bare names,
// hit 90 ambiguity errors, and was misread as a rule about globs not
// propagating -- which sent a second `use crate::app::*` into every file to
// work around a problem that was really the shadowing. There is no such rule.
// The suffix fixes it; the second glob was redundant and is gone.

use crate::plot_area::plot_split;

mod chart_view_tests;
mod control_plane_tests;
mod drawings_tests;
mod feeds_sources_tests;
mod indicators_tests;
mod input_ui_tests;
mod layers_tests;
mod orderflow_tests;
mod panes_layout_tests;
mod paper_trading_tests;
mod toolrail_tests;
mod workspaces_tests;

use super::*;
use crate::chart::PriceScale;

use rust_decimal::Decimal;
use tokio::sync::mpsc;

use quantick_feed_binance::depth::DepthEvent;

use crate::canvas_layout::CANVAS_DIVIDER_PX;
use crate::config::{AppConfig, FeedCapabilities, FeedConfig, ProviderKind};
use crate::drawings::{ChartPoint, MAX_DRAWING_WIDTH_PX, PresetHost};
// The drawing chrome moved out to its own module; the tests that drive it
// through `QuantickApp` stay here, and reach its numbers by name.
use crate::feed::{FeedConnectionState, FeedEvent, FeedNotice};
use crate::pane::DEFAULT_PANE_FRACTION;
use crate::pane::DrawingDrag;
use crate::surfaces::drawing_chrome::inline_editor::INLINE_TEXT_HINT;
use crate::surfaces::drawing_chrome::{
    DRAWING_INSPECTOR_DEFAULT_POSITION, DRAWING_MANAGER_GAP_PX, INSPECTOR_AUTO_PIN_CHART_WIDTH_PX,
    INSPECTOR_MIN_WIDTH_PX, InspectorTab,
};
use crate::tab::BOOK_GENERATION_STRIDE;
use crate::time_header;
use crate::viewport::Viewport;

/// The slot a stored fractional anchor sits on, asked of the one owner.
///
/// A test that recomputes it — `bar.floor()`, as five of them used to —
/// is a second copy of the projection rule, free to disagree with the
/// production one and to keep passing while it does. See
/// [`Viewport::slot_of`].
fn slot_of(bar: f32) -> usize {
    Viewport::slot_of(bar).expect("a placed anchor sits on a slot")
}

/// Run a tab operation that needs the config, splitting the borrow the
/// way the frame loop does.
fn with_config<R>(app: &mut QuantickApp, f: impl FnOnce(&mut Tab, &AppConfig) -> R) -> R {
    let (tab, config) = app.active_with_config();
    f(tab, config)
}

/// A pane indicator with `values` as its single plot, delivered the way
/// the worker delivers one.
fn add_pane_indicator(app: &mut QuantickApp, title: &str, values: Vec<f64>) -> SlotId {
    let slot = app
        .active_tab_mut()
        .flow_pane
        .indicators
        .allocate_slot("test.indicator");
    rebuild_pane_indicator(app, slot, title, values);
    slot
}

/// The same indicator, recomputed — an edited input, a hot reload, older
/// trades re-cutting the series.
fn rebuild_pane_indicator(app: &mut QuantickApp, slot: SlotId, title: &str, values: Vec<f64>) {
    app.active_tab_mut()
        .flow_pane
        .indicators
        .apply(IndicatorEvent::rebuilt(
            slot,
            quantick_indicators::IndicatorDescriptor {
                title: title.to_owned(),
                short_title: None,
                overlay: false,
                plots: vec![quantick_indicators::PlotSpec {
                    id: quantick_indicators::PlotId::new(0),
                    title: title.to_owned(),
                    style: quantick_indicators::PlotStyle::Line,
                    base_color: quantick_indicators::Rgba8::opaque(255, 255, 255),
                    width: 1.0,
                    offset: 0,
                    marker: None,
                }],
                inputs: Vec::new(),
                fills: Vec::new(),
            },
            vec![values],
        ));
}

/// The `(lo, hi)` a pane is drawing with right now: its manual range if the
/// user has taken control of the axis, else the fit the last frame made.
fn pane_range(app: &QuantickApp, slot: SlotId) -> (f64, f64) {
    let view = app
        .active_tab()
        .flow_pane
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == slot)
        .expect("the pane is still there");
    let auto = view.last_auto.expect("a frame fitted this pane");
    view.scale.resolve(auto)
}

/// Whether a pane is still auto-fitting its values.
fn pane_is_auto(app: &QuantickApp, slot: SlotId) -> bool {
    app.active_tab()
        .flow_pane
        .indicators
        .all()
        .iter()
        .find(|view| view.slot == slot)
        .expect("the pane is still there")
        .scale
        .is_auto()
}

/// The gutter band beside pane `index`, computed the way the frame does.
fn pane_gutter(app: &QuantickApp, index: usize) -> egui::Rect {
    let pane = &app.active_tab().flow_pane;
    let areas = plot_split(
        pane.last_plot_area.expect("a frame has been drawn"),
        pane.live_strip_width(app.active_tab().capabilities(&app.config)),
        pane.indicators
            .pane_sizing(&mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES]),
    );
    areas.pane_gutters[index]
}

/// The plot band of pane `index` — where its curve is drawn, as opposed to
/// the gutter where its numbers are.
fn pane_body(app: &QuantickApp, index: usize) -> egui::Rect {
    let pane = &app.active_tab().flow_pane;
    let areas = plot_split(
        pane.last_plot_area.expect("a frame has been drawn"),
        pane.live_strip_width(app.active_tab().capabilities(&app.config)),
        pane.indicators
            .pane_sizing(&mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES]),
    );
    areas.indicator_panes[index].rect
}

/// The bar the viewport has at its right edge — how far the chart is
/// panned along time.
fn right_edge(app: &QuantickApp) -> f32 {
    let pane = &app.active_tab().flow_pane;
    pane.viewport.right_edge_bar(pane.slots())
}

/// The pane band as the last drawn frame carved it.
fn pane_slots(app: &QuantickApp) -> Vec<crate::indicators::PaneSlot> {
    let pane = &app.active_tab().flow_pane;
    plot_split(
        pane.last_plot_area.expect("a frame has been drawn"),
        pane.live_strip_width(app.active_tab().capabilities(&app.config)),
        pane.indicators
            .pane_sizing(&mut [crate::indicators::PaneSizing::Auto; crate::indicators::MAX_PANES]),
    )
    .indicator_panes
}

/// An app showing every pane it allows, drawn once at `size`.
fn app_with_full_pane_band(
    ctx: &egui::Context,
    size: egui::Vec2,
) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, cmd_rx) = app_with_history(200);
    for index in 0..crate::indicators::MAX_PANES {
        add_pane_indicator(
            &mut app,
            &format!("pane{index}"),
            (0..200).map(f64::from).collect(),
        );
    }
    run_frame_at(&mut app, ctx, size);
    (app, cmd_rx)
}

/// A minimal one-feed, two-symbol config for the app tests.
fn test_config() -> AppConfig {
    AppConfig {
        default_feed: "binance".to_string(),
        default_symbol: "TESTUSDT".to_string(),
        feeds: vec![FeedConfig {
            id: "binance".to_string(),
            name: "Binance".to_string(),
            provider: ProviderKind::Binance,
            symbols: vec!["TESTUSDT".to_string(), "ETHUSDT".to_string()],
            bubble_preset: None,
            symbol_bubble_presets: Default::default(),
            default_layout: None,
            default_bars: None,
            record_deals: false,
        }],
        metatrader: Default::default(),
        paper: Default::default(),
        deals: Default::default(),
        history: Default::default(),
    }
}

/// An app wired to in-memory channels, plus the test's ends of them: send
/// feed events in, observe feed commands out. No egui, no network.
fn test_app() -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
    mpsc::Sender<DepthEvent>,
) {
    // A cockpit of its own for this app: every store resolves under the
    // scratch home this bumps to, so two apps built on one thread never
    // restore each other's arrangement — the isolation the per-call
    // counters used to give, kept even under `--test-threads=1`.
    crate::store_home::next_test_home();
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let app = QuantickApp::new(
        test_config(),
        "binance",
        "TESTUSDT",
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
    (app, evt_tx, cmd_rx, book_tx)
}

fn test_app_with_notices() -> (
    QuantickApp,
    mpsc::Sender<FeedNotice>,
    (mpsc::Sender<FeedEvent>, mpsc::Sender<DepthEvent>),
) {
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, _cmd_rx) = mpsc::channel(16);
    let (notice_tx, notice_rx) = mpsc::channel(8);
    let app = QuantickApp::new(
        test_config(),
        "binance",
        "TESTUSDT",
        BarSpec::Tick(50),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: notice_rx,
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    (app, notice_tx, (evt_tx, book_tx))
}

/// Take the `SetBookCapture` command every test app queues at
/// construction and return its generation. Capture follows the feed, so
/// there is exactly one of these in flight before any test acts.
fn take_capture_start(commands: &mut mpsc::Receiver<FeedCommand>) -> u64 {
    match commands
        .try_recv()
        .expect("capture starts with the feed, not with the toggle")
    {
        FeedCommand::SetBookCapture {
            enabled: true,
            initial_generation,
        } => initial_generation,
        _ => panic!("unexpected command"),
    }
}

fn enable_heatmap_with_snapshot(app: &mut QuantickApp, commands: &mut mpsc::Receiver<FeedCommand>) {
    use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot};

    let generation = take_capture_start(commands);
    app.active_tab_mut().tape_mut().set_depth_visible(true);
    app.active_tab_mut()
        .tape_mut()
        .handle_depth_event(DepthEvent::Snapshot {
            symbol: "TESTUSDT".to_owned(),
            generation,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            price_step: None,
            snapshot: BookSnapshot::new(
                10,
                vec![BookLevel::new(Decimal::from(99), Decimal::from(5)).unwrap()],
                vec![BookLevel::new(Decimal::from(101), Decimal::from(6)).unwrap()],
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        });
    app.active_tab_mut().tape_mut().flush_for_test();
    assert_eq!(app.active_tab_mut().tape_mut().health().active_levels, 2);
}

fn trade(agg_id: u64) -> quantick_engine::Trade {
    quantick_engine::Trade {
        agg_id,
        timestamp_ms: 1_000 + agg_id as i64 * 100,
        price: Decimal::from(100) + Decimal::new(agg_id as i64 % 20, 1),
        quantity: Decimal::ONE,
        side: if agg_id.is_multiple_of(2) {
            quantick_engine::Side::Buy
        } else {
            quantick_engine::Side::Sell
        },
    }
}

/// An app holding `count` backfilled trades, built into tick(1) bars — one
/// bar per trade, the finest series a spec change can coarsen.
fn app_with_history(count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, evt_tx, cmd_rx, _book_tx) = test_app();
    // A bare canvas, the one every caller here was written against: the
    // strip stands beside the price axis and takes width from the candles,
    // so leaving it on moves every hard-coded pointer coordinate in the
    // drawing and inspector tests onto it — a collision about layout,
    // never about what those tests assert. Which layers a launch opens
    // with is a different question, and `test_app` keeps the shipped
    // answer intact for the test that reads it
    // (`each_layer_switch_moves_exactly_one_owner`).
    app.active_tab_mut().flow_pane.live_strip_visible = false;
    app.active_tab_mut().flow_pane.tick_n = 1;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    let trades: Vec<_> = (1..=count).map(trade).collect();
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    app.active_tab_mut().drain_feed();
    assert_eq!(app.active_tab().flow_pane.state.bars().len() as u64, count);
    (app, cmd_rx)
}

/// An app whose source quotes prices and nothing else: no book, no traded
/// volume — what a live CFD bridge publishes.
fn app_without_depth() -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (_evt_tx, evt_rx) = mpsc::channel(64);
    let (_book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let app = QuantickApp::new(
        test_config(),
        "binance",
        "TESTUSDT",
        BarSpec::Tick(50),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(FeedCapabilities::none()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
    );
    (app, cmd_rx)
}

/// How many switches each pane's menu lays out at its top level. The two
/// menus split one list ([`ChartLayer::on_tape`]), so counting either from
/// the list is what keeps a new layer from silently landing on the wrong
/// canvas's menu.
fn chart_menu_entries() -> usize {
    ChartLayer::ALL
        .into_iter()
        .filter(|layer| !layer.on_tape())
        .count()
}

fn tape_menu_entries() -> usize {
    ChartLayer::ALL
        .into_iter()
        .filter(|layer| layer.on_tape())
        .count()
}

/// The active tab's flow pane beside the chrome a canvas frame hands it.
fn with_flow_pane<R>(
    app: &mut QuantickApp,
    body: impl FnOnce(&mut ChartPane, &mut pane::PaneChrome<'_>) -> R,
) -> R {
    let capabilities = app.active_tab().capabilities(&app.config);
    let side_inferred = app.active_tab().side_note(&app.config).is_some();
    let mut begin_text_edit = false;
    let QuantickApp {
        tabs,
        active_tab,
        toolrail,
        drawing_presets,
        style,
        tz,
        layer_actions,
        footprint_config,
        ..
    } = app;
    let tab = &mut tabs[*active_tab];
    let mut chrome = pane::PaneChrome {
        toolrail,
        presets: drawing_presets,
        begin_text_edit: &mut begin_text_edit,
        style,
        tz: *tz,
        feed_gaps: &[],
        symbol: &tab.symbol,
        paper: &mut tab.paper,
        paper_takes_input: true,
        paper_hud_here: true,
        // One pane in hand and no tab around it: there is no other pane
        // whose shared marks could be under the pointer.
        shared_pick: None,
        shared: pane::SharedInteraction::default(),
        capabilities,
        side_inferred,
        footprint: footprint_config,
        layers: layer_actions,
    };
    body(&mut tab.flow_pane, &mut chrome)
}

/// Switch a layer exactly as the menu does, including the hop the app makes
/// for the ones a pane does not own.
fn switch_layer(app: &mut QuantickApp, layer: ChartLayer, visible: bool) {
    with_flow_pane(app, |pane, chrome| {
        pane.set_layer_visible(layer, visible, chrome.layers);
    });
    app.apply_layer_actions();
}

/// Whether the active tab's flow pane is painting `layer`.
fn layer_on(app: &QuantickApp, layer: ChartLayer) -> bool {
    app.active_tab().flow_pane.layer_visible(layer, &app.style)
}

/// A one-print recording on disk, read back through the replay crate's
/// own loader — the cheapest honest recording a test can hand a tab.
fn recording_at(dir: &std::path::Path) -> quantick_replay::Session {
    std::fs::create_dir_all(dir).expect("the scratch folder is writable");
    let mut text = quantick_replay::format::write_header(&quantick_replay::format::WriteHeader {
        symbol: "TESTUSDT".to_owned(),
        timezone: quantick_replay::UtcOffset::UTC,
        side_source: "venue".to_owned(),
        source: None,
    });
    quantick_replay::format::write_trade(
        &mut text,
        &trade(2),
        None,
        quantick_replay::UtcOffset::UTC,
    );
    let path = dir.join("20260316.csv");
    std::fs::write(&path, text).expect("the recording is written");
    quantick_replay::Session::load(&path, quantick_replay::ParseOptions::default())
        .expect("the recording this test just wrote parses")
}

/// How many rectangles the frame painted — candle bodies dominate it, so
/// it stands in for "how many candles were drawn" when a test holds the
/// one-bar-one-candle law at the paint level rather than at an accessor.
fn painted_rects(output: &egui::FullOutput) -> usize {
    fn walk(shape: &egui::Shape, found: &mut usize) {
        match shape {
            egui::Shape::Rect(_) => *found += 1,
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, found);
                }
            }
            _ => {}
        }
    }
    let mut found = 0;
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

/// Every string the frame painted, panels and chart alike.
fn painted_text(output: &egui::FullOutput) -> Vec<String> {
    fn walk(shape: &egui::Shape, found: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => found.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, found);
                }
            }
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
}

/// Whether the frame drew the price axis over the test's price range —
/// the labels only exist once the chart really scaled and painted itself.
fn has_price_axis(texts: &[String]) -> bool {
    texts.iter().any(|text| {
        text.parse::<f64>()
            .is_ok_and(|price| (95.0..=115.0).contains(&price))
    })
}

fn run_frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    run_frame_with_events(app, ctx, Vec::new())
}

fn run_frame_with_events(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_frame_with_modifiers(app, ctx, events, egui::Modifiers::NONE)
}

/// The window every frame-driving test gets unless it asks for another:
/// roomy, so a test about something else is never accidentally a test
/// about a cramped layout.
const TEST_WINDOW: egui::Vec2 = egui::vec2(1400.0, 900.0);
/// The smallest window whose *layout* is still promised: the chrome
/// collapses down to here and clips below it (the drawing rail's Minimal
/// stage, docs/drawing-toolbar-ux.md §2.8). The window itself has no
/// minimum any more — see [`A_DEGENERATE_WINDOW`] — but this is still
/// where the pane band is under real pressure while everything is
/// expected to read, so the layout tests stay aimed at it.
const MIN_WINDOW: egui::Vec2 = egui::vec2(900.0, 560.0);
/// A window dragged down to nothing. `main.rs` sets no
/// `with_min_inner_size`, so this is reachable by a trader with a mouse;
/// nothing is promised about what it looks like, only that the app is
/// still there when the window is dragged back out.
const A_DEGENERATE_WINDOW: egui::Vec2 = egui::vec2(1.0, 1.0);

fn run_frame_with_modifiers(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    run_frame_sized(app, ctx, TEST_WINDOW, events, modifiers)
}

/// A frame at a chosen window size — how a test reaches the layout a
/// smaller window produces without resizing anything global.
fn run_frame_sized(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
        events,
        modifiers,
        ..Default::default()
    };
    ctx.run(input, |ctx| app.draw_frame(ctx, Instant::now()))
}

/// A press-drag-release in a window of `size`.
fn drag_sized(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    start: egui::Pos2,
    end: egui::Pos2,
) {
    run_frame_sized(
        app,
        ctx,
        size,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
        egui::Modifiers::NONE,
    );
    run_frame_sized(
        app,
        ctx,
        size,
        vec![egui::Event::PointerMoved(end)],
        egui::Modifiers::NONE,
    );
    run_frame_sized(
        app,
        ctx,
        size,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
        egui::Modifiers::NONE,
    );
}

fn run_frame_at(app: &mut QuantickApp, ctx: &egui::Context, size: egui::Vec2) {
    run_frame_sized(app, ctx, size, Vec::new(), egui::Modifiers::NONE);
}

fn pointer_button(position: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn click_chart(app: &mut QuantickApp, ctx: &egui::Context, position: egui::Pos2) {
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
    );
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, false),
        ],
    );
}

/// A click with a modifier held, and the move that carries the pointer
/// there — the click-move-click half of a placement, as opposed to a drag.
fn click_chart_with(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    position: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    for pressed in [true, false] {
        run_frame_sized(
            app,
            ctx,
            TEST_WINDOW,
            vec![
                egui::Event::PointerMoved(position),
                pointer_button(position, pressed),
            ],
            modifiers,
        );
    }
}

/// Move the pointer without pressing anything, and answer what got
/// painted — the frames between two clicks, which is where a placement
/// gesture does all of its talking.
fn move_chart_with(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    position: egui::Pos2,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    run_frame_sized(
        app,
        ctx,
        TEST_WINDOW,
        vec![egui::Event::PointerMoved(position)],
        modifiers,
    )
}

fn drag_chart(app: &mut QuantickApp, ctx: &egui::Context, start: egui::Pos2, end: egui::Pos2) {
    run_frame_with_events(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(end)]);
    run_frame_with_events(
        app,
        ctx,
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    );
}

/// What the gear on the context bar does, in one line.
///
/// Selecting a drawing raises the bar, not the panel, so a test that is
/// about the panel's *contents* has to open it first — the same way the
/// trader does. `the_gear_on_the_context_bar_opens_the_inspector` is the
/// test that proves this shortcut matches the real button.
fn open_inspector(app: &mut QuantickApp, ctx: &egui::Context) {
    app.surfaces.drawing_chrome.set_inspector_open(true);
    // Two frames: the first opens the window, the second lets it settle
    // its size and automatic placement before anything reads its rect.
    run_frame(app, ctx);
    run_frame(app, ctx);
}

fn drawing_tool(id: &str) -> drawings::DrawingTool {
    drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .expect("drawing tool is registered")
}

fn arm_drawing_from_toolbox(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    id: &str,
) -> drawings::DrawingTool {
    let drawing = drawing_tool(id);
    let tool = Tool::Drawing(drawing);
    if let Some(button) = app.toolrail.button_rect(tool) {
        click_chart(app, ctx, button.center());
    } else {
        // A folded family member has no direct slot: open the family
        // flyout through the slot's caret zone and arm it by its row —
        // the same pointer path a user takes.
        let family = drawing
            .family()
            .expect("only family members lack a direct slot");
        let slot = drawings::DRAWING_TOOLS
            .into_iter()
            .filter(|candidate| {
                candidate
                    .family()
                    .is_some_and(|candidate_family| candidate_family.id == family.id)
            })
            .find_map(|candidate| app.toolrail.button_rect(Tool::Drawing(candidate)))
            .expect("the family slot was rendered");
        click_chart(app, ctx, slot.max - egui::vec2(4.0, 4.0));
        run_frame(app, ctx);
        let row = app
            .toolrail
            .flyout_row_rect(drawing)
            .expect("the flyout lists the folded member");
        click_chart(app, ctx, row.center());
    }
    assert_eq!(
        app.toolrail.tool(),
        tool,
        "arming {id} through the rail must land"
    );
    drawing
}

/// Count the line segments painted in the drawing colour — how many
/// strokes of *this* object are on screen, across every pane.
fn drawing_strokes(output: &egui::FullOutput) -> usize {
    let color = egui::epaint::ColorMode::Solid(crate::drawings::DEFAULT_DRAWING_COLOR);
    output
        .shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::LineSegment { stroke, .. } => stroke.color == color,
            _ => false,
        })
        .count()
}

/// Twice the area of the triangle the three anchors make, in chart space.
/// Zero exactly when they are collinear — which for a channel means a
/// corridor of no width, and for a triangle no triangle.
fn anchor_cross(points: &[drawings::ChartPoint]) -> f64 {
    let [a, b, c] = points else {
        panic!("a three-anchor object, got {}", points.len());
    };
    (f64::from(b.bar) - f64::from(a.bar)) * (c.price - a.price)
        - (f64::from(c.bar) - f64::from(a.bar)) * (b.price - a.price)
}

/// A press-drag-release with a modifier held for the whole gesture.
fn drag_chart_with(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    start: egui::Pos2,
    end: egui::Pos2,
    modifiers: egui::Modifiers,
) {
    for events in [
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
        vec![egui::Event::PointerMoved(end)],
        vec![egui::Event::PointerMoved(end), pointer_button(end, false)],
    ] {
        run_frame_sized(app, ctx, TEST_WINDOW, events, modifiers);
    }
}

/// The price the trend line holds at `bar` — the height the width anchor
/// is measured above or below.
fn trend_price_at(points: &[drawings::ChartPoint], bar: f32) -> f64 {
    let (a, b) = (&points[0], &points[1]);
    let span = f64::from(b.bar - a.bar);
    if span.abs() < 1e-9 {
        return a.price;
    }
    a.price + (b.price - a.price) * f64::from(bar - a.bar) / span
}

/// A tab split in two, with one shared horizontal line drawn on the flow
/// pane, and the screen position that line occupies on the *time* pane.
///
/// The mark is anchored on a real flow bar (so it carries a real market
/// instant) at the price sitting in the middle of the time pane's window
/// (so a drag has room to move in either direction without leaving the
/// chart). Its y is computed through the time pane's own price scale,
/// which is the whole point: the two panes agree on the price and on
/// nothing else.
fn split_with_a_shared_line(
    ctx: &egui::Context,
) -> (QuantickApp, mpsc::Receiver<FeedCommand>, egui::Pos2) {
    let (mut app, commands) = app_with_history(200);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    // The layout is deferred a frame, so the time pane does not exist yet
    // on the line above — hence these frames before it is configured.
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    // One-second bars, so this fixture's 20 seconds of tape is 20 bars on
    // the time pane against 200 on the flow pane. That difference is what
    // §D7 is about — the two panes agree on market time and on nothing
    // else — and without it the time pane holds a single bar, most of its
    // chart is empty space no instant can be named in, and every gesture
    // below silently does nothing.
    let pane = app
        .active_tab_mut()
        .time_pane_mut()
        .expect("two frames is enough for the deferred layout to build it");
    pane.kind = crate::state::BarKind::Time;
    pane.time_interval_ms = 1_000;
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    assert!(
        app.active_tab()
            .time_pane()
            .is_some_and(|pane| pane.slots() > 5),
        "the time pane must hold a real series, or these tests pass on a              gesture that never reached a bar"
    );

    let (chart, scale) = time_pane_projection(&app);
    // Mid-window price, and an x near the newest bar: both panes can name
    // an instant there, and a drag has room above and below.
    let price = scale.price_at(chart.center().y);
    let slot = 100;
    let time = app
        .active_tab()
        .flow_pane
        .slot_open_time(slot)
        .expect("a closed bar has a time");
    app.active_tab_mut().flow_pane.drawings.place_with(
        drawing_tool("horizontal-line"),
        &drawings::DrawingBand::Price,
        drawings::ChartPoint::at_time(slot as f32 + 0.5, price, Some(time)),
        |tool| drawings::NewDrawing {
            style: drawings::DrawingStyle::default(),
            payload: tool.default_payload(),
        },
    );
    app.active_tab_mut()
        .flow_pane
        .drawings
        .selected_mut()
        .expect("placement selects what it completed")
        .scope = drawings::DrawingScope::AllCharts;
    // Nothing selected to start with, so the assertions cannot pass on the
    // selection placement left behind.
    app.active_tab_mut().flow_pane.drawings.select(None);
    run_frame(&mut app, ctx);

    let (chart, scale) = time_pane_projection(&app);
    (
        app,
        commands,
        egui::pos2(chart.right() - 30.0, scale.y(price)),
    )
}

/// The time pane's chart rect and price scale, as it last drew them.
fn time_pane_projection(app: &QuantickApp) -> (egui::Rect, PriceScale) {
    let time_pane = app
        .active_tab()
        .time_pane()
        .expect("the split built a time pane");
    let chart = time_pane.last_chart_area.expect("the time pane drew");
    let (lo, hi) = time_pane
        .price_view
        .resolve(time_pane.last_auto_range.expect("the pane has a range"));
    let scale = PriceScale::from_range(
        lo,
        hi,
        time_pane.last_chart_top,
        time_pane.last_chart_top + time_pane.last_chart_height,
    );
    (chart, scale)
}

/// A canvas point at price-line `y` that is provably clear of the open
/// inspector.
///
/// These proofs are about the canvas gesture, not about where the
/// placement rule currently puts the panel — so they ask the panel where
/// it is instead of encoding an answer that changes whenever the Style
/// tab grows a row.
fn canvas_point_clear_of_inspector(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    y: f32,
) -> egui::Pos2 {
    run_frame(app, ctx);
    let clear = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .filter(|rect| rect.y_range().contains(y))
        .map_or(400.0, |rect| rect.right() + 40.0);
    egui::pos2(clear, y)
}

/// A chrome section that says nothing except where the popup was parked —
/// every other field at what a fresh cockpit has, so a test that restores
/// one is testing the position and not the dock.
fn chrome_with_popup_at(position: Option<[f32; 2]>) -> ui_state::SavedChrome {
    ui_state::SavedChrome {
        timezone_minutes: 0,
        dock_visible: true,
        dock_tab: None,
        rail_visible: true,
        rail_dock: ui_state::SavedRailDock::Left,
        perf_readings: false,
        legacy_favorite_tools: Vec::new(),
        progressive_history: true,
        history_reach: None,
        history_reach_span_minutes: None,
        venue_lead_in: false,
        record_deals: None,
        inspector_position: position,
    }
}

/// Put a horizontal line on the chart at that height and leave it there.
/// Two of them is the setup every "the next drawing too" claim needs.
fn draw_horizontal_line(app: &mut QuantickApp, ctx: &egui::Context, price_y: f32) {
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    click_chart(app, ctx, egui::pos2(700.0, price_y));
    run_frame(app, ctx);
}

/// Open the popup and drag its title bar by `delta`, returning where it
/// ended up. The gesture the whole feature is about.
fn park_the_popup(app: &mut QuantickApp, ctx: &egui::Context, delta: egui::Vec2) -> egui::Pos2 {
    open_inspector(app, ctx);
    let popup = ctx
        .memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the properties popup is open");
    // Left of the trailing icons, which is the grip a trader gets.
    let grip = egui::pos2(popup.left() + 60.0, popup.top() + 14.0);
    drag_chart(app, ctx, grip, grip + delta);
    // The write is queued during the release frame and flushed at the top
    // of the next one, where every other workspace write lives.
    run_frame(app, ctx);
    app.surfaces
        .drawing_chrome
        .inspector_pos()
        .expect("the drag records a position")
}

/// A cockpit already on disk, which is what every session after the first
/// one starts from. The autosave updates a workspace; it does not invent
/// one.
fn with_a_saved_workspace(app: &mut QuantickApp, ctx: &egui::Context, name: &str) {
    app.workspace.set_ui_state_path(scratch_ui_state(name));
    run_frame(app, ctx);
    app.save_workspace("test");
    app.surfaces.toast.clear();
    assert!(
        app.workspace.ui_state_path().exists(),
        "the cockpit is on disk"
    );
}

/// Place a tool by its registry id through the pointer, anchor by anchor,
/// and hand back its index. The mission's tools need one click and two
/// respectively, so the helper takes the anchors as a list rather than
/// assuming either.
fn place_drawing(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    id: &str,
    anchors: &[egui::Pos2],
) -> usize {
    app.toolrail.arm(Tool::Drawing(drawing_tool(id)));
    for anchor in anchors {
        click_chart(app, ctx, *anchor);
    }
    run_frame(app, ctx);
    app.drawing_pane()
        .drawings
        .items()
        .iter()
        .rposition(|drawing| drawing.tool.id() == id)
        .unwrap_or_else(|| panic!("{id} was placed"))
}

/// Select `index` the way a click on the object does, raise the properties
/// popup through the gear's own door, and report where it opened.
fn select_and_open_popup(app: &mut QuantickApp, ctx: &egui::Context, index: usize) -> egui::Pos2 {
    app.drawing_pane_mut().drawings.select(Some(index));
    run_frame(app, ctx);
    open_inspector(app, ctx);
    // egui keeps an Area's last rect after the Area stops being shown, so
    // `area_rect` alone cannot tell "open here" from "closed, and here is
    // where it used to be" — and the last place it was is exactly the
    // value that would make a position assertion pass on a popup that is
    // no longer floating. Ask the app what it drew before reading egui.
    assert!(
        app.surfaces.drawing_chrome.inspector_open(),
        "the gear's door is open"
    );
    assert!(
        !app.surfaces.drawing_chrome.inspector_pinned(),
        "and the popup is floating, not docked"
    );
    assert_eq!(
        app.drawing_pane().drawings.selected(),
        Some(index),
        "on the object this call selected"
    );
    ctx.memory(|memory| memory.area_rect(egui::Id::new("drawing_inspector")))
        .expect("the properties popup is open")
        .min
}

fn run_sized_frame(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    run_frame_sized(app, ctx, size, events, egui::Modifiers::NONE)
}

fn click_sized(app: &mut QuantickApp, ctx: &egui::Context, size: egui::Vec2, position: egui::Pos2) {
    run_sized_frame(
        app,
        ctx,
        size,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, true),
        ],
    );
    run_sized_frame(
        app,
        ctx,
        size,
        vec![
            egui::Event::PointerMoved(position),
            pointer_button(position, false),
        ],
    );
}

fn key_press(key: egui::Key) -> egui::Event {
    key_press_with(key, egui::Modifiers::NONE)
}

fn key_press_with(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Whether any painted line segment uses `color` — proof a stroke of that
/// colour reached the screen (or, negated, that a hidden object did not).
fn painted_line_with_color(output: &egui::FullOutput, color: egui::Color32) -> bool {
    output.shapes.iter().any(|clipped| match &clipped.shape {
        egui::Shape::LineSegment { stroke, .. } => {
            stroke.color == egui::epaint::ColorMode::Solid(color)
        }
        _ => false,
    })
}

fn time_zoom(app: &QuantickApp) -> f32 {
    app.active_tab()
        .time_pane()
        .expect("time pane")
        .viewport
        .px_per_bar()
}

/// One app on `config`, opened on `feed_id`/symbol — the smallest harness
/// that exercises the constructor path (where a feed's declared bubble
/// preset is applied).
fn app_on(config: AppConfig, feed_id: &str, symbol: &str) -> QuantickApp {
    let (_evt_tx, evt_rx) = mpsc::channel(8);
    let (_book_tx, book_rx) = mpsc::channel(8);
    let (cmd_tx, _cmd_rx) = mpsc::channel(8);
    QuantickApp::new(
        config,
        feed_id,
        symbol,
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
    )
}

/// An app with `count` trades of history, split, and laid out by two real
/// frames so both panes have reported their rects.
fn split_app(ctx: &egui::Context, count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, commands) = app_with_history(count);
    run_frame(&mut app, ctx);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, ctx);
    run_frame(&mut app, ctx);
    (app, commands)
}

/// Let every pane's indicator worker finish what it was sent, then apply
/// its events — the two steps the frame loop takes, made deterministic.
fn settle_indicators(app: &mut QuantickApp) {
    for pane in app.active_tab_mut().panes_mut() {
        pane.indicator_worker.flush();
        pane.apply_indicator_events();
    }
}

/// A point inside the pane on `side`, for a click that focuses it.
fn pane_point(app: &QuantickApp, side: PaneSide) -> egui::Pos2 {
    app.active_tab()
        .pane(side)
        .last_chart_area
        .expect("the pane reported its rect")
        .center()
}

/// Where `price` sits on `side`'s axis, computed the way that pane's own
/// frame computes it.
fn price_y(app: &QuantickApp, side: PaneSide, price: f64) -> f32 {
    let pane = app.active_tab().pane(side);
    let chart = pane.last_chart_area.expect("the pane reported its rect");
    let auto = pane.last_auto_range.expect("the pane fitted a range");
    let (lo, hi) = pane.price_view.resolve(auto);
    PriceScale::from_range(lo, hi, chart.top(), chart.bottom()).y(price)
}

/// A scratch workspace path, so a test never writes the real cockpit.
fn scratch_ui_state(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "quantick-app-ui-state-{name}-{}-{:?}.toml",
        std::process::id(),
        std::thread::current().id()
    ))
}

/// A frame carrying the window's close request, which is the only signal
/// the exit save has to work from.
fn close_requested_frame(app: &mut QuantickApp, ctx: &egui::Context) {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, TEST_WINDOW)),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    let _ = ctx.run(input, |ctx| app.draw_frame(ctx, Instant::now()));
}

/// A tool the tests can star without caring which one it is.
fn starrable_tool() -> crate::drawings::DrawingTool {
    crate::drawings::DrawingTool::by_id("measure").expect("a registered drawing tool")
}

/// A level for the layout tests: one anchor, in market time, on the pane.
fn place_level(app: &mut QuantickApp, side: PaneSide, price: f64) {
    let tool = crate::drawings::DrawingTool::by_id("horizontal-line").expect("the tool");
    let time = app
        .active_tab()
        .pane(side)
        .slot_open_time(0)
        .expect("bars to anchor on");
    let placed = app.active_tab_mut().pane_mut(side).drawings.place(
        tool,
        crate::drawings::ChartPoint {
            bar: 0.5,
            price,
            time_ms: Some(time),
        },
    );
    assert!(placed, "a horizontal line places on one anchor");
}

fn drawings_on(app: &QuantickApp, side: PaneSide) -> Vec<f64> {
    app.active_tab()
        .pane(side)
        .drawings
        .items()
        .iter()
        .map(|drawing| drawing.points[0].price)
        .collect()
}

// ---- workspace tabs (§11) ----

/// A feed handle wired to channels nothing sends on, for a tab a test
/// opens but does not drive.
fn stub_feed() -> (FeedHandle, TabEnds) {
    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    (
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
        TabEnds {
            events: evt_tx,
            book: book_tx,
            commands: cmd_rx,
        },
    )
}

/// The ends of a tab's feed, kept alive by the test so its channels stay
/// open exactly as a running feed thread would keep them.
struct TabEnds {
    events: mpsc::Sender<FeedEvent>,
    #[expect(dead_code, reason = "held open so the tab's channel is not closed")]
    book: mpsc::Sender<DepthEvent>,
    #[expect(dead_code, reason = "held open so the tab's channel is not closed")]
    commands: mpsc::Receiver<FeedCommand>,
}

/// Open a second market the way the `+` does, minus the real feed spawn:
/// the same bookkeeping runs, over channels the test drives.
fn open_second_tab(app: &mut QuantickApp, ctx: &egui::Context, symbol: &str) -> TabEnds {
    app.apply_tab_action(TabAction::New);
    assert!(
        app.surfaces.source_picker.is_open(),
        "the + opens the picker"
    );
    app.surfaces.source_picker.close();

    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    app.adopt_tab(
        "binance".to_owned(),
        symbol.to_owned(),
        FeedHandle {
            events: evt_rx,
            book_events: book_rx,
            notices: feed::silent_notices(),
            capabilities: feed::fixed_capabilities(ProviderKind::Binance.capabilities()),
            latency: feed::unsplit_latency(),
            commands: cmd_tx,
            replay: None,
        },
        None,
    );
    run_frame(app, ctx);
    TabEnds {
        events: evt_tx,
        book: book_tx,
        commands: cmd_rx,
    }
}

// ---- venue candle history in the time pane ----

/// A minute candle at `minute`, priced so a fold is visible in the result.
fn venue_candle(minute: i64, seed: i64) -> quantick_engine::Bar {
    let open_time = minute * crate::feed::OHLCV_BASE_INTERVAL_MS;
    quantick_engine::Bar {
        open_time,
        close_time: open_time + crate::feed::OHLCV_BASE_INTERVAL_MS - 1,
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
fn minute_trade(minute: u64) -> quantick_engine::Trade {
    minute_trade_at(minute as i64)
}

/// The same, for a minute that may sit before the fixture's first — what
/// "load older" delivers.
fn minute_trade_at(minute: i64) -> quantick_engine::Trade {
    let mut trade = trade(minute.unsigned_abs() + 1);
    trade.timestamp_ms = minute * crate::feed::OHLCV_BASE_INTERVAL_MS + 1_000;
    trade
}

/// Venue candles for the minutes *before* the fixture trades, which start
/// in minute 0 — so the prefix never overlaps the engine's own bars.
fn venue_history(count: i64) -> Vec<quantick_engine::Bar> {
    (-count..0)
        .map(|m| venue_candle(m, m.rem_euclid(5)))
        .collect()
}

/// Venue candles for a range of minutes, so a test can hand over one
/// progressive slice at a time. Minutes are negative: the fixture trades
/// start in minute 0, and the prefix sits before them.
fn venue_history_range(from_minute: i64, to_minute: i64) -> Vec<quantick_engine::Bar> {
    (from_minute..to_minute)
        .map(|m| venue_candle(m, m.rem_euclid(5)))
        .collect()
}

/// What one queued `FetchOhlcv` asked for: whether it wanted a progressive
/// answer (`slice_ms`) and where its newest edge was (`before_ms`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OhlcvAsk {
    slice_ms: Option<i64>,
    before_ms: Option<i64>,
}

/// Every FetchOhlcv sitting in the command channel, in order.
///
/// One drain, not one per field: a receiver is consumed by reading it, so
/// two helpers over the same channel meant the second call always saw an
/// empty queue — a trap a test would fail on for the wrong reason.
fn drain_ohlcv_fetches(commands: &mut mpsc::Receiver<FeedCommand>) -> Vec<OhlcvAsk> {
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
fn drain_ohlcv_requests(commands: &mut mpsc::Receiver<FeedCommand>) -> usize {
    drain_ohlcv_fetches(commands).len()
}

/// The `slice_ms` every queued FetchOhlcv asked for, in order — what says
/// whether the tab asked for a progressive answer or the old single one.
fn drain_ohlcv_slice_requests(commands: &mut mpsc::Receiver<FeedCommand>) -> Vec<Option<i64>> {
    drain_ohlcv_fetches(commands)
        .into_iter()
        .map(|ask| ask.slice_ms)
        .collect()
}

/// The `before_ms` every queued FetchOhlcv asked for, in order — what says
/// whether the tab asked for the opening span or reached back past what it
/// already holds.
fn drain_ohlcv_before_requests(commands: &mut mpsc::Receiver<FeedCommand>) -> Vec<Option<i64>> {
    drain_ohlcv_fetches(commands)
        .into_iter()
        .map(|ask| ask.before_ms)
        .collect()
}

/// A split app whose feed reports candle history, with the channel ends
/// the test drives.
fn history_app(
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
fn tape_app(
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

fn app_backfilled_with(
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
                book_capture: false,
                history_paging: true,
                traded_volume: true,
                deal_counter: false,
                ohlcv_history: true,
                ohlcv_generation: 0,
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
fn grid_trades(first_price: Decimal, step: Decimal) -> Vec<quantick_engine::Trade> {
    (0..200)
        .map(|i| quantick_engine::Trade {
            agg_id: i + 1,
            timestamp_ms: i as i64 * crate::feed::OHLCV_BASE_INTERVAL_MS + 1_000,
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

/// A fixed-range profile placed on the flow pane with the footprint layer
/// switched off — the state the trader reported the jagged profile from,
/// and the one where nothing but the profile itself wants the ladders.
fn place_range_profile_with_the_layer_off(app: &mut QuantickApp) {
    let frvp = crate::drawings::DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == crate::frvp::TOOL_ID)
        .expect("frvp is registered");
    let pane = app.active_tab_mut().pane_mut(PaneSide::Flow);
    pane.set_layer_visible(
        ChartLayer::Footprint,
        false,
        &mut chart_layers::LayerActions::default(),
    );
    assert!(
        !pane
            .drawings
            .place(frvp, crate::drawings::ChartPoint::at(1.0, 100.0))
    );
    assert!(
        pane.drawings
            .place(frvp, crate::drawings::ChartPoint::at(20.0, 105.0))
    );
}

/// The grouping the flow pane's profile actually folded at, read off the
/// drawing rather than off the chart state — the ladder's row width and
/// the profile's are meant to be one number, and a test that read only the
/// state could not tell whether they had come apart.
fn folded_profile_group(app: &QuantickApp) -> Decimal {
    app.active_tab().pane(PaneSide::Flow).drawings.items()[0]
        .payload
        .as_any()
        .downcast_ref::<crate::drawings::FrvpPayload>()
        .expect("frvp payload")
        .cache
        .as_ref()
        .expect("a placed range over a live tape folded")
        .profile
        .as_ref()
        .expect("the range covers bars that have tape")
        .0
        .group()
}

/// Every `LoadOlder` sitting in the command channel, as the counts asked
/// for. The reach turns one press into a run of these.
fn drain_load_older(commands: &mut mpsc::Receiver<FeedCommand>) -> Vec<usize> {
    let mut asks = Vec::new();
    while let Ok(command) = commands.try_recv() {
        if let FeedCommand::LoadOlder { count } = command {
            asks.push(count);
        }
    }
    asks
}

// ---- adding a symbol from the picker (§11) ----

/// A scratch sidecar path, so a test never touches the real one.
fn symbols_scratch(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "quantick-symbols-app-{}-{}.toml",
        name,
        std::process::id()
    ))
}

/// A config with two MetaTrader feeds, one of them mapping a port — the
/// shape where adding the wrong symbol used to write a file that killed
/// the next launch.
fn two_metatrader_feeds() -> AppConfig {
    let mut ports = std::collections::BTreeMap::new();
    ports.insert("US500".to_string(), 9102_u16);
    AppConfig {
        default_feed: "tickmill".to_string(),
        default_symbol: "US500".to_string(),
        feeds: vec![
            FeedConfig {
                id: "tickmill".to_string(),
                name: "MetaTrader 5 — Tickmill".to_string(),
                provider: ProviderKind::MetaTrader,
                symbols: vec!["US500".to_string()],
                bubble_preset: None,
                symbol_bubble_presets: Default::default(),
                default_layout: None,
                default_bars: None,
                record_deals: false,
            },
            FeedConfig {
                id: "b3".to_string(),
                name: "MetaTrader 5 — B3".to_string(),
                provider: ProviderKind::MetaTrader,
                symbols: vec!["WIN$N".to_string()],
                bubble_preset: None,
                symbol_bubble_presets: Default::default(),
                default_layout: None,
                default_bars: None,
                record_deals: false,
            },
        ],
        metatrader: crate::config::MetaTraderSettings {
            ports,
            ..Default::default()
        },
        paper: Default::default(),
        deals: Default::default(),
        history: Default::default(),
    }
}

fn observer_instance() -> quantick_control::id::InstanceId {
    quantick_control::id::InstanceId::from_bytes([0x42; 16])
}

fn observer_scope(name: &str) -> quantick_control::id::SnapshotScopeId {
    quantick_control::id::SnapshotScopeId::new(name).expect("test scope ID is valid")
}

fn gateway_test_scopes() -> std::collections::BTreeSet<quantick_control::id::PermissionId> {
    [
        "observe",
        "observe.system",
        "observe.workspace",
        "observe.market",
        "observe.chart",
        "observe.indicators",
        "observe.drawings",
        "observe.orderflow",
        "observe.replay",
        "observe.health",
        "observe.attention",
        "observe.events",
    ]
    .into_iter()
    .map(|id| quantick_control::id::PermissionId::new(id).unwrap())
    .collect()
}

fn gateway_test_options() -> quantick_control_local::client::ConnectOptions {
    quantick_control_local::client::ConnectOptions::observer(
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        gateway_test_scopes(),
    )
}

/// A client that asks for the annotate tier as well, for the tests that
/// prove what the trader's grant does and does not open.
fn annotator_test_options() -> quantick_control_local::client::ConnectOptions {
    let mut scopes = gateway_test_scopes();
    for id in [
        "annotate",
        "annotate.attention",
        "annotate.chart",
        "annotate.notification",
        "annotate.script",
    ] {
        scopes.insert(quantick_control::id::PermissionId::new(id).unwrap());
    }
    quantick_control_local::client::ConnectOptions::for_profile(
        "annotator",
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        scopes,
    )
}

/// Grant the annotate tier for the next connection through the panel's
/// own named call — the door the checkboxes and the hook both use.
fn grant_annotate_for_test(app: &mut QuantickApp, scopes: &str) {
    app.control_access
        .as_mut()
        .expect("control access is installed")
        .configure_scopes(scopes)
        .expect("the test grants registered scopes");
}

/// One anchor at the newest bar's open time and its close price — what an
/// agent reads from `chart.window.read` before it annotates.
fn newest_anchor(app: &QuantickApp) -> serde_json::Value {
    let pane = app.active_tab().drawing_pane();
    let slot = pane.slots().saturating_sub(1);
    let time = pane
        .slot_open_time(slot)
        .expect("the newest bar has a time");
    let price = pane
        .closed_bar(slot)
        .and_then(|bar| rust_decimal::prelude::ToPrimitive::to_f64(&bar.close))
        .unwrap_or(1.0);
    serde_json::json!({
        "time_unix_ms": time,
        "price": format!("{price}"),
    })
}

/// One remote call, served by the frames it takes: send, let the
/// application drain its queue, read the reply.
fn remote_call(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    capability: &str,
    payload: serde_json::Value,
) -> quantick_control::wire::ResponseEnvelope {
    let request_id = client
        .send(capability, payload)
        .expect("the request is sent");
    for _ in 0..400 {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, request_id);
    response
}

/// Say that these objects were placed by an assistant, as the gateway's
/// own actor does when an agent calls the same action. Named by id, so a
/// test can never accidentally relabel the trader's own drawing.
fn stamp_agent_author(app: &mut QuantickApp, ids: &[u64]) {
    let pane = app.active_tab_mut().drawing_pane_mut();
    let count = pane.drawings.items().len();
    for index in 0..count {
        if ids.contains(&pane.drawings.items()[index].id.0) {
            pane.drawings.set_author_at(
                index,
                Some(crate::drawings::DrawingAuthor {
                    actor_kind: "agent".to_owned(),
                    client_name: "quantick integration test".to_owned(),
                }),
            );
        }
    }
}

/// What is on the pane, by indicator kind — the shape a detach has to
/// restore exactly.
fn indicator_kinds(app: &QuantickApp) -> Vec<String> {
    app.active_tab()
        .focused_pane()
        .indicators
        .all()
        .iter()
        .map(|view| view.kind.to_string())
        .collect()
}

fn gateway_test_directory(name: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);
    std::env::temp_dir().join(format!(
        "quantick-gateway-test-{}-{name}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ))
}

fn enable_test_gateway(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    directory: &std::path::Path,
    queue_capacity: usize,
) -> std::path::PathBuf {
    app.control_access
        .as_mut()
        .expect("control access is installed")
        .enable_for_test(ctx, directory.to_path_buf(), queue_capacity);
    wait_for_test_gateway_descriptor(app, ctx)
}

fn enable_test_gateway_with_limits(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    directory: &std::path::Path,
    queue_capacity: usize,
    request_timeout: std::time::Duration,
    max_connections: usize,
) -> std::path::PathBuf {
    app.control_access
        .as_mut()
        .expect("control access is installed")
        .enable_for_test_with_limits(
            ctx,
            directory.to_path_buf(),
            queue_capacity,
            request_timeout,
            max_connections,
        );
    wait_for_test_gateway_descriptor(app, ctx)
}

fn wait_for_test_gateway_descriptor(
    app: &mut QuantickApp,
    ctx: &egui::Context,
) -> std::path::PathBuf {
    for _ in 0..400 {
        run_frame(app, ctx);
        if let Some(path) = app
            .control_access
            .as_ref()
            .and_then(crate::control::ControlAccess::descriptor_path_for_test)
        {
            return path;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("test gateway did not publish discovery");
}

fn wait_for_queued_gateway_requests(app: &QuantickApp, expected: usize) {
    for _ in 0..400 {
        if app
            .control_access
            .as_ref()
            .expect("control access is installed")
            .queued_requests_for_test()
            == expected
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("gateway request queue did not reach {expected}");
}

fn disable_test_gateway(app: &mut QuantickApp, ctx: &egui::Context) {
    app.control_access
        .as_mut()
        .expect("control access is installed")
        .disable_for_test();
    for _ in 0..400 {
        run_frame(app, ctx);
        if app
            .control_access
            .as_ref()
            .expect("control access is installed")
            .is_disabled_for_test()
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("test gateway did not stop cleanly");
}

fn response_error(
    response: &quantick_control::wire::ResponseEnvelope,
) -> &quantick_control::error::ControlError {
    match &response.outcome {
        quantick_control::wire::ResponseOutcome::Failure { error } => error,
        quantick_control::wire::ResponseOutcome::Success { .. } => {
            panic!("expected a structured gateway failure")
        }
    }
}

/// Measure the coherent capture of every initial scope in batches:
/// `(best median, best p99, worst of the best batch)` in microseconds,
/// where "best" is the batch with the lowest p99. A noisy neighbour can
/// only make a batch look slower, never faster, so the best batch is the
/// honest reading of the capture's own cost.
/// A workspace with something in every collection the snapshot scopes walk.
///
/// The benchmark below exists to answer "what does a capture cost", and an
/// empty workspace cannot answer it: every loop the analysis, order-flow
/// and session scopes add runs zero times over `app_with_history` alone —
/// no indicators, no drawings, no book ladder, no replay link, no paper
/// rows — so a measurement taken there reports the cost of nine empty
/// projections and calls it flat. This fills each of them through the path
/// the application itself uses, so the number below is the cost of a
/// capture over a working chart.
fn loaded_observer_workspace(bars: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    use quantick_orderbook::{BookCoverage, BookLevel, BookSnapshot, DepthEvent};

    let (mut app, commands) = app_with_history(bars);

    // Indicators: delivered the way the worker delivers them, with a full
    // committed column so the "latest reading" walk has rows to reach.
    for index in 0..BENCH_INDICATORS_PER_PANE {
        let pane = &mut app.active_tab_mut().flow_pane;
        let slot = pane.indicators.allocate_slot("native.ema");
        let descriptor = quantick_indicators::IndicatorDescriptor {
            title: format!("EMA {index}"),
            short_title: None,
            overlay: true,
            plots: vec![quantick_indicators::PlotSpec {
                id: quantick_indicators::PlotId::new(0),
                title: "EMA".to_owned(),
                style: quantick_indicators::PlotStyle::Line,
                base_color: quantick_indicators::Rgba8::opaque(1, 2, 3),
                width: 1.0,
                offset: 0,
                marker: None,
            }],
            fills: Vec::new(),
            inputs: vec![quantick_indicators::InputSpec::Int {
                name: "len".to_owned(),
                title: "Length".to_owned(),
                default: 9,
                min: Some(1),
                max: Some(500),
                step: Some(1),
                options: Vec::new(),
            }],
        };
        let column = (0..bars).map(|bar| bar as f64).collect::<Vec<_>>();
        pane.indicators
            .apply(crate::indicator_worker::IndicatorEvent::rebuilt(
                slot,
                descriptor,
                vec![column],
            ));
    }

    // Drawings: placed through the same call the toolrail makes.
    for index in 0..BENCH_DRAWINGS_PER_PANE {
        let slot = index % (bars as usize).max(1);
        let (time, price) = {
            let pane = &app.active_tab().flow_pane;
            let Some(bar) = pane.closed_bar(slot) else {
                break;
            };
            (
                pane.slot_open_time(slot),
                rust_decimal::prelude::ToPrimitive::to_f64(&bar.close).unwrap_or(0.0),
            )
        };
        let flow = &mut app.active_tab_mut().flow_pane;
        flow.drawings.place_with(
            drawing_tool("horizontal-line"),
            &drawings::DrawingBand::Price,
            ChartPoint::at_time(slot as f32 + 0.5, price, time),
            |tool| drawings::NewDrawing {
                style: drawings::DrawingStyle::default(),
                payload: tool.default_payload(),
            },
        );
    }

    // The book: driven in as a venue snapshot and published, so the L2
    // scope has a full ladder to render into exact decimal strings.
    // Priced well above the ladder depth so the bid side never walks a
    // price down through zero, which the book rightly refuses.
    const BENCH_MID_PRICE: i64 = 1_000;
    let level = |offset: i64, side: i64| {
        BookLevel::new(
            rust_decimal::Decimal::from(BENCH_MID_PRICE + side * (offset + 1)),
            rust_decimal::Decimal::from(offset + 1),
        )
        .expect("a positive price and quantity is a valid level")
    };
    let bids = (0..BENCH_BOOK_LEVELS_PER_SIDE)
        .map(|offset| level(offset, -1))
        .collect::<Vec<_>>();
    let asks = (0..BENCH_BOOK_LEVELS_PER_SIDE)
        .map(|offset| level(offset, 1))
        .collect::<Vec<_>>();
    app.active_tab_mut()
        .tape_mut()
        .handle_depth_event(DepthEvent::Snapshot {
            symbol: "TESTUSDT".to_owned(),
            generation: 1,
            observed_at_ms: 1_100,
            effective_at_ms: 999,
            price_step: None,
            snapshot: BookSnapshot::new(
                10,
                bids,
                asks,
                BookCoverage::Limited {
                    levels_per_side: 1_000,
                },
            ),
        });
    app.active_tab_mut().tape_mut().flush_for_test();

    // Paper: a closed trade ledger and an open position, through the
    // simulator's own entry path.
    let print = |agg_id: u64, price: i64| quantick_engine::Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small ids") * 1_000,
        price: rust_decimal::Decimal::from(price),
        quantity: rust_decimal::Decimal::ONE,
        side: quantick_engine::Side::Buy,
    };
    {
        let paper = &mut app.active_tab_mut().paper;
        paper.seed(&print(0, 100));
        let mut agg = 1;
        for round in 0..BENCH_CLOSED_TRADES {
            paper.market(quantick_engine::Side::Buy);
            paper.on_trade(&print(agg, 100));
            agg += 1;
            paper.close_position();
            paper.on_trade(&print(agg, 101 + (round % 3) as i64));
            agg += 1;
        }
        // Leave one open, so the position branch is walked too.
        paper.market(quantick_engine::Side::Buy);
        paper.on_trade(&print(agg, 100));
    }

    (app, commands)
}

/// The shape of the loaded benchmark workspace. Chosen to sit at or above
/// what a working chart carries, so the measured cost is an upper bound on
/// a real one rather than a best case.
const BENCH_INDICATORS_PER_PANE: usize = 6;
const BENCH_DRAWINGS_PER_PANE: usize = 40;
/// The host clips its published ladder to `LADDER_LEVELS_PER_SIDE`, so
/// asking for more than that measures the clip, not the wire.
const BENCH_BOOK_LEVELS_PER_SIDE: i64 = 128;
const BENCH_CLOSED_TRADES: usize = 120;

fn measure_core_capture_us() -> (u64, u64, u64) {
    const WARMUP_CAPTURES: usize = 25;
    const MEASURED_CAPTURES: usize = 500;
    const BATCHES: usize = 3;

    let (app, _commands) = loaded_observer_workspace(2_000);
    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = registry
        .descriptors()
        .map(|descriptor| descriptor.scope_id.clone())
        .collect::<Vec<_>>();
    let instance = observer_instance();
    for _ in 0..WARMUP_CAPTURES {
        drop(registry.capture(&app, &instance, &scopes).unwrap());
    }
    let mut best = (u64::MAX, u64::MAX, u64::MAX);
    for _ in 0..BATCHES {
        let mut elapsed_us = Vec::with_capacity(MEASURED_CAPTURES);
        for _ in 0..MEASURED_CAPTURES {
            drop(registry.capture(&app, &instance, &scopes).unwrap());
            elapsed_us.push(registry.performance().last_capture_us);
        }
        elapsed_us.sort_unstable();
        let median_us = elapsed_us[elapsed_us.len() / 2];
        let p99_index = (elapsed_us.len() * 99).div_ceil(100).saturating_sub(1);
        let p99_us = elapsed_us[p99_index];
        let worst_us = *elapsed_us.last().unwrap();
        println!(
            "CONTROL_CORE_CAPTURE {{\"capture_median_us\":{median_us},\"capture_p99_us\":{p99_us},\"capture_worst_us\":{worst_us},\"captures\":{MEASURED_CAPTURES}}}"
        );
        if p99_us < best.1 {
            best = (median_us, p99_us, worst_us);
        }
    }
    best
}

/// Measure the maximum chart-window capture (the reviewed 32-bar page)
/// in batches: `(best median, best p99, worst of the best batch)` in
/// microseconds, where "best" is the batch with the lowest p99. A noisy
/// neighbour can only make a batch look slower, never faster, so the best
/// batch is the honest reading of the capture's own cost.
fn measure_max_chart_window_capture_us() -> (u64, u64, u64) {
    use crate::control::chart::{ChartWindowQuery, ChartWindowRange, chart_window};
    use quantick_control::{limits::CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS, wire::WireU64};

    const WARMUP_CAPTURES: usize = 10;
    const MEASURED_CAPTURES: usize = 100;
    const BATCHES: usize = 3;

    let max_page_items = u64::try_from(CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS)
        .expect("the reviewed page limit fits in the wire integer");
    let (app, _commands) = app_with_history(max_page_items);
    let query = ChartWindowQuery {
        tab_id: WireU64::new(app.active_tab().id),
        pane_id: WireU64::new(app.active_tab().flow_pane.id),
        range: ChartWindowRange::Slots {
            start_slot: WireU64::new(0),
            end_slot_exclusive: WireU64::new(max_page_items),
        },
        page_size: CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS,
    };
    let instance = observer_instance();
    for _ in 0..WARMUP_CAPTURES {
        drop(chart_window(&app, &instance, &query, None).unwrap());
    }
    let mut best = (u64::MAX, u64::MAX, u64::MAX);
    for _ in 0..BATCHES {
        let mut elapsed_us = Vec::with_capacity(MEASURED_CAPTURES);
        for _ in 0..MEASURED_CAPTURES {
            let started = std::time::Instant::now();
            drop(chart_window(&app, &instance, &query, None).unwrap());
            elapsed_us.push(u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX));
        }
        elapsed_us.sort_unstable();
        let median_us = elapsed_us[elapsed_us.len() / 2];
        let p99_index = (elapsed_us.len() * 99).div_ceil(100).saturating_sub(1);
        let p99_us = elapsed_us[p99_index];
        let worst_us = *elapsed_us.last().unwrap();
        println!(
            "CONTROL_MAX_CHART_WINDOW_CAPTURE {{\"capture_median_us\":{median_us},\"capture_p99_us\":{p99_us},\"capture_worst_us\":{worst_us},\"captures\":{MEASURED_CAPTURES},\"bars\":{CONTROL_CHART_WINDOW_MAX_PAGE_ITEMS}}}"
        );
        if p99_us < best.1 {
            best = (median_us, p99_us, worst_us);
        }
    }
    best
}

/// Put the pointer over `slot` of the flow pane, the way the cursor tests
/// do, so a mark has a bar to resolve.
fn hover_bar(app: &mut QuantickApp, ctx: &egui::Context, slot: usize) {
    run_frame(app, ctx);
    let position = {
        let pane = &app.active_tab().flow_pane;
        let chart = pane.last_chart_area.expect("the pane reported its rect");
        let right = pane.last_lane_divider_x.unwrap_or_else(|| chart.right());
        egui::pos2(
            pane.viewport.x_center(slot, right, pane.slots()),
            chart.center().y,
        )
    };
    run_frame_with_events(app, ctx, vec![egui::Event::PointerMoved(position)]);
}

fn success_result(response: &quantick_control::wire::ResponseEnvelope) -> serde_json::Value {
    match &response.outcome {
        quantick_control::wire::ResponseOutcome::Success { result } => result.clone(),
        quantick_control::wire::ResponseOutcome::Failure { error } => {
            panic!("expected a success: {error:?}")
        }
    }
}

/// One capture of the scene scope, as a client would read it.
fn observer_scene(app: &QuantickApp) -> serde_json::Value {
    let mut registry = crate::control::standard_registry().unwrap();
    let scope = observer_scope("scene.controls");
    let capture = registry
        .capture(app, &observer_instance(), std::slice::from_ref(&scope))
        .unwrap()
        .into_serialized()
        .unwrap();
    capture.scopes[&scope].value.clone()
}

/// The scene's control IDs, in the order it reports them.
fn scene_control_ids(scene: &serde_json::Value) -> Vec<String> {
    scene["controls"]
        .as_array()
        .expect("the scene reports a control list")
        .iter()
        .map(|control| control["control_id"].as_str().unwrap().to_owned())
        .collect()
}

fn scene_control<'a>(scene: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    scene["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|control| control["control_id"] == id)
        .unwrap_or_else(|| panic!("the scene names {id}"))
}

/// The read scopes an evidence client asks for: everything the safe
/// default grants, plus the two the evidence tier adds — both sensitive,
/// both off until the trader ticks them.
fn evidence_test_options() -> quantick_control_local::client::ConnectOptions {
    let mut scopes = gateway_test_scopes();
    for id in ["observe.evidence", "observe.screenshot"] {
        scopes.insert(quantick_control::id::PermissionId::new(id).unwrap());
    }
    quantick_control_local::client::ConnectOptions::observer(
        "quantick integration test",
        env!("CARGO_PKG_VERSION"),
        scopes,
    )
}

/// The scopes a bundle is captured over in these tests: enough to explain
/// a session, and including the scene, which is what a screenshot
/// correlates against.
const EVIDENCE_TEST_SCOPES: [&str; 6] = [
    "system.info",
    "workspace.summary",
    "feed.status",
    "chart.summary",
    "health.summary",
    "scene.controls",
];

/// One run of resource bytes, decoded through the contract's own wire
/// type rather than a second base64 spelling in this test.
fn decode_wire_base64(encoded: &str) -> Vec<u8> {
    quantick_control::wire::Base64Bytes::new(encoded)
        .expect("the gateway encodes valid base64")
        .decode()
        .expect("what encodes decodes")
}

/// Page a retained bundle back through its own cursor and return the
/// canonical bytes the chunks reassemble into, with the manifest that
/// described them.
fn read_evidence_bundle(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    manifest: &serde_json::Value,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut cursor = serde_json::Value::Null;
    loop {
        let payload = if cursor.is_null() {
            serde_json::json!({ "evidence_id": manifest["evidence_id"] })
        } else {
            serde_json::json!({ "evidence_id": manifest["evidence_id"], "cursor": cursor })
        };
        let response = remote_call(app, ctx, client, "evidence.read", payload);
        let page = success_result(&response);
        for chunk in page["page"]["items"].as_array().unwrap() {
            bytes.extend(decode_wire_base64(chunk["data"].as_str().unwrap()));
        }
        match page["page"]["next_cursor"].clone() {
            serde_json::Value::Null => break,
            next => cursor = next,
        }
    }
    assert_eq!(
        bytes.len(),
        manifest["encoded_bytes"]
            .as_str()
            .unwrap()
            .parse::<usize>()
            .unwrap(),
        "the pages reassemble to exactly the bundle the manifest describes"
    );
    bytes
}

/// One capture that wants a picture, served the way the window serves one:
/// the request parks, the frame delivers the pixels, the capture runs.
///
/// There is no shortcut past the parking. An image is worth exactly one
/// frame — `begin_frame` drops whatever no capture claimed — so a fixture
/// published before the request was sent would be gone by the time it
/// arrived, which is the staleness rule doing its job.
fn capture_with_screenshot(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    payload: serde_json::Value,
    image: crate::control::RawScreenshot,
) -> quantick_control::wire::ResponseEnvelope {
    let request_id = client
        .send("evidence.capture", payload)
        .expect("the request is sent");
    // Wait for the capture to actually park before handing over pixels.
    // Not a nicety: an image is worth one frame, so publishing before the
    // request has parked would have the frame drop it and the capture then
    // wait for one that never comes. Asserted rather than assumed, so a
    // machine slow enough to break the precondition says so here instead
    // of failing later on a confusing assertion about the manifest.
    let parked = (0..PARK_WAIT_FRAMES).any(|_| {
        run_frame(app, ctx);
        app.control_access
            .as_ref()
            .expect("control access is installed")
            .awaiting_screenshot_for_test()
            > 0
    });
    assert!(
        parked,
        "the capture did not park for an image within {PARK_WAIT_FRAMES} frames"
    );
    let mut access = app
        .control_access
        .take()
        .expect("control access is installed");
    access.publish_screenshot_for_test(app, image);
    app.control_access = Some(access);
    for _ in 0..REPLY_WAIT_FRAMES {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().expect("the gateway answered");
    assert_eq!(response.request_id, request_id);
    response
}

/// Frames a test spends waiting for a capture to park on an image.
///
/// Generous: the request crosses a socket and two threads, and these tests
/// run alongside every other crate's test binary. The gateways they use
/// have their request timeout raised for the same reason.
const PARK_WAIT_FRAMES: usize = 600;
/// Frames a test spends waiting for the gateway's reply.
const REPLY_WAIT_FRAMES: usize = 600;

/// A window's worth of pixels a PNG encoder cannot shrink.
///
/// The ramp below deflates to a few kilobytes, which is the wrong fixture
/// for anything about size: it makes every bundle a single chunk and hides
/// the paging path entirely. This one is noise from a fixed generator —
/// reproducible, and it compresses to nothing.
fn incompressible_screenshot(width: u32, height: u32) -> crate::control::RawScreenshot {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for _ in 0..(width as usize * height as usize) {
        // xorshift64*: no dependency, and the same bytes on every machine.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let sample = state.wrapping_mul(0x2545_F491_4F6C_DD1D).to_le_bytes();
        rgba.extend_from_slice(&[sample[0], sample[1], sample[2], 0xff]);
    }
    crate::control::RawScreenshot {
        width_px: width,
        height_px: height,
        pixels_per_point: 1.0,
        rgba: crate::control::ScreenshotPixels::new(move || rgba),
    }
}

/// A window's worth of opaque pixels, the shape the platform hands over.
fn test_screenshot(width: u32, height: u32) -> crate::control::RawScreenshot {
    crate::control::RawScreenshot {
        width_px: width,
        height_px: height,
        pixels_per_point: 1.0,
        rgba: crate::control::ScreenshotPixels::new(move || {
            (0..(width as usize * height as usize))
                .flat_map(|index| {
                    let shade = u8::try_from(index % 251).unwrap_or(0);
                    [shade, 0x20, 0x30, 0xff]
                })
                .collect()
        }),
    }
}
