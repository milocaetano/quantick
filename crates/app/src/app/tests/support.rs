use super::*;

/// The window's test doors: its entry point and its read views.
impl QuantickApp {
    pub(crate) fn arrangement_state(&self) -> arrangement_adapter::ArrangementRead<'_> {
        arrangement_adapter::ArrangementRead {
            tabs: &self.runtime.tabs,
            config: &self.config,
            toolrail: &self.surfaces.toolrail,
            tz: &self.runtime.tz,
            dock: &self.surfaces.dock.model_state,
            show_perf: self.health.show_perf,
            record_deals: self.chrome.record_deals,
            history: &self.runtime.history,
            drawing_chrome: &self.surfaces.drawing_chrome,
            workspace_book: self.runtime.workspace.book.book(),
        }
    }

    pub(crate) fn workspace_state(&self) -> workspace_save_adapter::WorkspaceRead<'_> {
        workspace_save_adapter::WorkspaceRead {
            arrangement: self.arrangement_state(),
            session: self.runtime.workspace.session(),
            replay_view: &self.surfaces.replay_browser,
        }
    }

    /// Create the app on `config`, opening one tab on `feed_id`/`symbol`
    /// (already streaming through `feed`) and bar `spec`, with no saved
    /// workspace to restore.
    ///
    /// The window itself always has one (`main` reads the file before it
    /// spawns a feed), so this is the tests' entry point: a case about the
    /// chart is not a case about what the last session left on disk.
    #[must_use]
    pub fn new(
        config: AppConfig,
        feed_id: impl Into<String>,
        symbol: impl Into<String>,
        spec: impl Into<crate::state::BarConfiguration>,
        feed: FeedHandle,
    ) -> Self {
        Self::new_with_workspace(
            config,
            feed_id,
            symbol,
            spec,
            feed,
            ui_state::Workspace::default(),
            AppLaunch::default(),
        )
    }

    /// Whether the toolbar's heatmap lamp is lit.
    ///
    /// The *switch*, not what capture lets through it — the same reading the
    /// layer file was taught in 848cba0, and for a sibling reason. A lamp lit
    /// from `depth_visible()` (`enabled && show_depth`) reports the heatmap off
    /// for as long as book capture is starting, and forever on a source with no
    /// book: the trader sees an unlit button, presses it, and switches the
    /// layer they wanted *off*. The button already has an honest way to say a
    /// source cannot fill it — `.enabled(...)` carrying its
    /// `disabled_explanation` — so the lamp beside it answers the only other
    /// question there is. The header takes the pane's reading directly: a
    /// second production entry point to the same answer is how the two drift.
    #[must_use]
    pub(super) fn heatmap_lamp_on(&self) -> bool {
        self.active_tab()
            .flow_pane
            .layer_switched_on(ChartLayer::Heatmap, &self.runtime.style)
    }
}

/// Run a tab operation that needs the config, splitting the borrow the
/// way the frame loop does.
pub(super) fn with_config<R>(
    app: &mut QuantickApp,
    f: impl FnOnce(&mut Tab, &AppConfig) -> R,
) -> R {
    let (tab, config) = app.active_with_config();
    f(tab, config)
}

/// A minimal one-feed, two-symbol config for the app tests.
pub(super) fn test_config() -> AppConfig {
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
            asset_families: Vec::new(),
            asset_aliases: Default::default(),
            symbol_asset_modes: Default::default(),
            symbol_widgets: Default::default(),
            symbol_icons: Default::default(),
            contract_rolls: Default::default(),
            default_layout: None,
            default_bars: None,
            record_deals: false,
            short_name: None,
            // The fixtures' fake feeds never say they connected and drain on
            // made-up clocks (`|| 0`): no connect or tape age limit, so only
            // an offline link and the latency pause entries here.
            sync: quantick_feed::sync::SyncLimits {
                max_tape_age_ms: i64::MAX,
                max_connecting_ms: i64::MAX,
                ..Default::default()
            },
        }],
        broker: Default::default(),
        metatrader: Default::default(),
        paper: Default::default(),
        deals: Default::default(),
        history: Default::default(),
    }
}

/// A launch-hook lookup over a fixed table instead of the process
/// environment, so a hook exported in the developer's shell cannot flip a
/// fixture's assertions and every case runs in one `cargo test`.
pub(super) fn fixed_env(
    table: &'static [(&'static str, &'static str)],
) -> impl FnMut(&str) -> Option<std::ffi::OsString> {
    move |name| {
        table
            .iter()
            .find(|(hook, _)| *hook == name)
            .map(|(_, value)| (*value).into())
    }
}

/// An app wired to in-memory channels, plus the test's ends of them: send
/// feed events in, observe feed commands out. No egui, no network.
pub(super) fn test_app() -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
    mpsc::Sender<DepthEvent>,
) {
    test_app_with_launch(AppLaunch::default())
}

/// An agent's call from inside the window, attributed as a remote
/// assistant's: the hooks' own door, minus their untraced origin.
pub(super) trait AgentCall {
    fn run_agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError>;
}

impl AgentCall for QuantickApp {
    fn run_agent_action(
        &mut self,
        capability_id: &str,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, quantick_control::error::ControlError> {
        self.agent_action(capability_id, input, crate::control::ActionOrigin::Remote)
    }
}

pub(super) fn test_app_with_launch(
    launch: AppLaunch,
) -> (
    QuantickApp,
    mpsc::Sender<FeedEvent>,
    mpsc::Receiver<FeedCommand>,
    mpsc::Sender<DepthEvent>,
) {
    test_app_with_workspace_and_launch(ui_state::Workspace::default(), launch)
}

pub(super) fn test_app_with_workspace_and_launch(
    workspace: ui_state::Workspace,
    launch: AppLaunch,
) -> (
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
    let app = QuantickApp::new_with_workspace(
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
        workspace,
        launch,
    );
    (app, evt_tx, cmd_rx, book_tx)
}

pub(super) fn test_app_with_notices() -> (
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
pub(super) fn take_capture_start(commands: &mut mpsc::Receiver<FeedCommand>) -> u64 {
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

pub(super) fn enable_heatmap_with_snapshot(
    app: &mut QuantickApp,
    commands: &mut mpsc::Receiver<FeedCommand>,
) {
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

pub(super) fn trade(agg_id: u64) -> quantick_engine::Trade {
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
pub(super) fn app_with_history(count: u64) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    app_with_history_and_launch(count, AppLaunch::default())
}

pub(super) fn app_with_history_and_launch(
    count: u64,
    launch: AppLaunch,
) -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, evt_tx, cmd_rx, _book_tx) = test_app_with_launch(launch);
    // Omit the forming-bar strip. Drawing fixtures also turn off the tape in
    // bare_canvas; native-tape fixtures retain its shipped source setting.
    app.active_tab_mut().flow_pane.set_layer_visible(
        ChartLayer::LiveStrip,
        false,
        &mut Default::default(),
    );
    app.active_tab_mut()
        .flow_pane
        .spec
        .retain(crate::state::BarSpec::Tick(1));
    app.active_tab_mut().apply_spec_changes();
    app.active_tab_mut().apply_spec_changes();
    let trades: Vec<_> = (1..=count).map(trade).collect();
    evt_tx.try_send(FeedEvent::Backfilled(trades)).unwrap();
    let tab_id = app.runtime.tabs.active_id();
    app.active_tab_mut().drain_feed(tab_id);
    assert_eq!(
        app.active_tab().flow_pane.series.state.bars().len() as u64,
        count
    );
    (app, cmd_rx)
}

/// An app whose source quotes prices and nothing else: no book, no traded
/// volume — what a live CFD bridge publishes.
pub(super) fn app_without_depth() -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
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

/// A one-print recording on disk, read back through the replay crate's
/// own loader — the cheapest honest recording a test can hand a tab.
pub(super) fn recording_at(dir: &std::path::Path) -> quantick_replay::Session {
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

/// A saved cockpit with configuration autosave explicitly enabled, so popup
/// movement can update its position without creating a new arrangement.
pub(super) fn with_a_saved_workspace(app: &mut QuantickApp, ctx: &egui::Context, name: &str) {
    app.runtime
        .workspace
        .set_ui_state_path(scratch_ui_state(name));
    app.runtime.workspace.session_mut().set_save_on_exit(true);
    run_frame(app, ctx);
    app.workspace_save_adapter().save_workspace("test");
    app.surfaces.toast.model_state = Default::default();
    assert!(
        app.runtime.workspace.ui_state_path().exists(),
        "the cockpit is on disk"
    );
}

/// One app on `config`, opened on `feed_id`/symbol — the smallest harness
/// that exercises the constructor path (where a feed's declared bubble
/// preset is applied).
pub(super) fn app_on(config: AppConfig, feed_id: &str, symbol: &str) -> QuantickApp {
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

/// A scratch workspace path, so a test never writes the real cockpit.
///
/// `thread_dir` rather than a `ScratchDir`, because the path is handed to the
/// app and the caller keeps nothing it could drop: every call site here reads
/// `set_ui_state_path(scratch_ui_state(..))`, and a value dropped at the end
/// of that statement would take the folder with it before the app ever wrote
/// to it. The directory goes when the test's thread ends instead.
pub(super) fn scratch_ui_state(name: &str) -> std::path::PathBuf {
    crate::scratch::thread_dir("app-ui-state").join(format!("{name}.toml"))
}

/// A feed handle wired to channels nothing sends on, for a tab a test
/// opens but does not drive.
pub(super) fn stub_feed() -> (FeedHandle, TabEnds) {
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
pub(super) struct TabEnds {
    pub(super) events: mpsc::Sender<FeedEvent>,
    #[expect(dead_code, reason = "held open so the tab's channel is not closed")]
    pub(super) book: mpsc::Sender<DepthEvent>,
    #[expect(dead_code, reason = "held open so the tab's channel is not closed")]
    pub(super) commands: mpsc::Receiver<FeedCommand>,
}

/// Open a second market the way the `+` does, minus the real feed spawn:
/// the same bookkeeping runs, over channels the test drives.
pub(super) fn open_second_tab(app: &mut QuantickApp, ctx: &egui::Context, symbol: &str) -> TabEnds {
    app.apply_tab_action(TabAction::New);
    assert!(
        app.surfaces.market_palette.model_state.is_open(),
        "the + opens the picker"
    );
    app.surfaces
        .market_palette
        .update(quantick_ui_model::market_palette::Intent::Close);

    let (evt_tx, evt_rx) = mpsc::channel(64);
    let (book_tx, book_rx) = mpsc::channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    app.arrangement_adapter().adopt_tab(
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

/// A scratch sidecar path, so a test never touches the real one. Handed to
/// the app, so it takes `thread_dir` for the reason [`scratch_ui_state`]
/// gives.
pub(super) fn symbols_scratch(name: &str) -> std::path::PathBuf {
    crate::scratch::thread_dir("symbols-app").join(format!("{name}.toml"))
}

/// A config with two MetaTrader feeds, one of them mapping a port — the
/// shape where adding the wrong symbol used to write a file that killed
/// the next launch.
pub(super) fn two_metatrader_feeds() -> AppConfig {
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
                asset_families: Vec::new(),
                asset_aliases: Default::default(),
                symbol_asset_modes: Default::default(),
                symbol_widgets: Default::default(),
                symbol_icons: Default::default(),
                contract_rolls: Default::default(),
                default_layout: None,
                default_bars: None,
                record_deals: false,
                short_name: None,
                sync: Default::default(),
            },
            FeedConfig {
                id: "b3".to_string(),
                name: "MetaTrader 5 — B3".to_string(),
                provider: ProviderKind::MetaTrader,
                symbols: vec!["WIN$N".to_string()],
                bubble_preset: None,
                symbol_bubble_presets: Default::default(),
                asset_families: Vec::new(),
                asset_aliases: Default::default(),
                symbol_asset_modes: Default::default(),
                symbol_widgets: Default::default(),
                symbol_icons: Default::default(),
                contract_rolls: Default::default(),
                default_layout: None,
                default_bars: None,
                record_deals: false,
                short_name: None,
                sync: Default::default(),
            },
        ],
        broker: Default::default(),
        metatrader: crate::config::MetaTraderSettings {
            ports,
            ..Default::default()
        },
        paper: Default::default(),
        deals: Default::default(),
        history: Default::default(),
    }
}
