//! Bubble settings belong to an asset: what one market's tab changes never
//! dresses another market, and a market shown again comes back as it was left.
use super::*;
use crate::bubble_presets::BubblePreset;
use quantick_orderflow::LaneWindow;
use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore};
use quantick_stores::bubble_assets::{AssetBubbles, AssetSource};

pub(super) fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../../config/feeds.toml")).expect("shipped feeds parse")
}

pub(super) fn look(app: &QuantickApp) -> BubblePreset {
    let tape = app.active_tab().tape();
    BubblePreset::capture(tape.active_preset_for_test(), tape.cached_config())
}

fn candle_aggression(app: &QuantickApp) -> bool {
    app.active_tab()
        .flow_pane
        .layer_switched_on(ChartLayer::CandleAggression, &app.style)
}

/// The tab's own symbol switch, without a live feed behind it.
pub(super) fn select_market(app: &mut QuantickApp, feed: &str, symbol: &str) {
    let tab = app.active_tab_mut();
    tab.feed_id = feed.to_owned();
    tab.symbol = symbol.to_owned();
    tab.tape_mut().reset_for_symbol(symbol);
    with_config(app, |tab, config| {
        tab.apply_asset_bubbles_after_switch(config);
    });
}

/// Edits made on the mini index while it is on screen.
fn edit_win(app: &mut QuantickApp) -> BubblePreset {
    let tape = app.active_tab_mut().tape_mut();
    tape.set_live_lane_window(LaneWindow::Fixed { ms: 7_000 });
    assert!(tape.edit_config(|config| config.set_ignore_opening_burst_in_scale(true)));
    look(app)
}

#[test]
fn a_win_edit_survives_a_round_trip_through_btc_in_one_tab() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    assert!(app.active_tab().tape().cached_config().native_tape());
    let edited = edit_win(&mut app);
    select_market(&mut app, "binance", "BTCUSDT");
    assert!(
        !app.active_tab().tape().cached_config().native_tape(),
        "BTC opens without the mini index tape"
    );
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert_eq!(look(&app), edited, "WIN comes back exactly as it was left");
}

#[test]
fn a_win_edit_leaves_btc_on_its_own_look() {
    let mut app = app_on(shipped_config(), "binance", "BTCUSDT");
    let btc = look(&app);
    select_market(&mut app, "metatrader-b3", "WINV26");
    edit_win(&mut app);
    select_market(&mut app, "binance", "BTCUSDT");
    assert_eq!(look(&app), btc, "BTC is untouched by the WIN edit");
    assert!(
        !app.active_tab()
            .tape()
            .cached_config()
            .volume_dots
            .ignore_opening_burst_in_scale
    );
}

#[test]
fn another_markets_declared_look_does_not_follow_the_tab_to_btc() {
    let mut app = app_on(shipped_config(), "binance", "BTCUSDT");
    let btc = look(&app);
    select_market(&mut app, "metatrader-b3", "WDO$N");
    assert_eq!(look(&app).name, "live lane pie");
    select_market(&mut app, "binance", "BTCUSDT");
    assert_eq!(look(&app), btc, "BTC keeps its own look, not WDO's");
}

#[test]
fn candle_aggression_switched_on_win_stays_with_win() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    switch_layer(&mut app, ChartLayer::CandleAggression, true);
    assert!(candle_aggression(&app));
    select_market(&mut app, "binance", "BTCUSDT");
    assert!(
        !candle_aggression(&app),
        "BTC does not inherit WIN's candle aggression"
    );
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert!(candle_aggression(&app), "WIN gets its own switch back");
}

#[test]
fn the_flow_opening_scale_set_on_win_stays_with_win() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_flow_opening(true)
    );
    select_market(&mut app, "binance", "BTCUSDT");
    assert!(!app.active_tab().tape().ignore_flow_opening());
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert!(app.active_tab().tape().ignore_flow_opening());
}

#[test]
fn a_future_win_roll_opens_on_the_mini_index_look() {
    let mut config = shipped_config();
    config
        .feeds
        .iter_mut()
        .find(|feed| feed.id == "metatrader-b3")
        .expect("the B3 feed")
        .symbols
        .push("WINZ26".to_owned());
    let app = app_on(config, "metatrader-b3", "WINZ26");
    assert!(
        app.active_tab().tape().cached_config().native_tape(),
        "a dated roll the config never named is still the mini index"
    );
    assert_eq!(look(&app).name, "mini index regions");
}

#[test]
fn the_mini_dollar_beside_it_is_not_the_mini_index() {
    let app = app_on(shipped_config(), "metatrader-b3", "WDO$N");
    assert!(!app.active_tab().tape().cached_config().native_tape());
    assert_eq!(look(&app).name, "live lane pie");
}

/// Every shipped market outside the WIN family opens as it did before the
/// native tape existed: no native tape, no tape-only pane, no volume dots, no
/// opening exclusion and no candle aggression.
#[test]
fn no_market_outside_the_win_family_opens_with_the_tape_features() {
    let config = shipped_config();
    for feed in &config.feeds {
        for symbol in feed
            .symbols
            .iter()
            .filter(|symbol| !symbol.starts_with("WIN"))
        {
            let app = app_on(config.clone(), &feed.id, symbol);
            let tape = app.active_tab().tape().cached_config();
            let at = format!("{}/{symbol}", feed.id);
            assert!(!tape.native_tape(), "{at} opens on the native tape");
            assert!(!tape.live_lane.tape_only, "{at} opens tape only");
            assert!(!tape.volume_dots.enabled, "{at} opens on volume dots");
            assert!(
                !tape.volume_dots.ignore_opening_burst_in_scale,
                "{at} excludes the opening"
            );
            assert!(
                !candle_aggression(&app),
                "{at} opens with candle aggression"
            );
        }
    }
}

/// Two tabs, WIN and BTC: candle aggression switched on in the WIN tab is
/// WIN's, so a restart does not open the BTC tab with it.
#[test]
fn candle_aggression_on_a_win_tab_does_not_reach_a_btc_tab_after_a_restart() {
    let dir = crate::scratch::ScratchDir::new("asset-layers");
    let path = dir.join("chart-layers.toml");
    let mut win = app_on(shipped_config(), "metatrader-b3", "WINV26");
    win.workspace.set_chart_layers_path(path.clone());
    let mask = win.active_tab().flow_pane.layer_mask(&win.style);
    win.workspace.layers_mut().record(mask);
    switch_layer(&mut win, ChartLayer::CandleAggression, true);
    win.layer_wiring().maintain(&egui::Context::default());

    let mut btc = app_on(shipped_config(), "binance", "BTCUSDT");
    btc.workspace.set_chart_layers_path(path);
    btc.layer_wiring().restore();
    assert!(
        !candle_aggression(&btc),
        "the BTC tab opens without WIN's candle aggression"
    );
}

/// A recorded mini index session replayed in a BTC tab wears the mini index
/// look, and BTC gets its own back when the replay closes.
#[test]
fn a_win_recording_in_a_btc_tab_wears_the_win_look() {
    let mut app = app_on(shipped_config(), "binance", "BTCUSDT");
    let btc = look(&app);
    select_market(&mut app, "binance", "WINV26");
    assert!(app.active_tab().tape().cached_config().native_tape());
    select_market(&mut app, "binance", "BTCUSDT");
    assert_eq!(look(&app), btc);
}

pub(super) fn maintain(app: &mut QuantickApp) {
    app.layer_wiring().maintain(&egui::Context::default());
}

fn source(app: &QuantickApp) -> AssetSource {
    app.active_tab()
        .tape()
        .look
        .asset()
        .expect("a bound asset")
        .source()
}

/// Review round 1, finding 1: wheeling the tape's window moves the view.
/// It writes nothing and does not make the asset's settings its own.
#[test]
fn wheeling_the_tape_window_is_navigation_not_a_setting() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    let window = app.active_tab().tape().live_lane_window();
    app.active_tab_mut().tape_mut().zoom_live_lane(2.0);
    assert_ne!(app.active_tab().tape().live_lane_window(), window);
    maintain(&mut app);
    assert_eq!(source(&app), AssetSource::Declared);
    assert!(
        !crate::bubble_presets::assets_path().exists(),
        "navigation writes nothing"
    );

    // A window chosen on purpose is a setting, written once.
    let tape = app.active_tab_mut().tape_mut();
    tape.set_live_lane_window(LaneWindow::Fixed { ms: 9_000 });
    maintain(&mut app);
    assert_eq!(source(&app), AssetSource::Stored);
    assert!(crate::bubble_presets::assets_path().is_file());
    let tape = app.active_tab().tape();
    assert_eq!(tape.look.asset().expect("bound").unsaved(), None);
}

/// Review round 1, finding 2: two tabs on the mini index share its
/// settings. An edit in one reaches the other, and the other's next edit
/// keeps it instead of filing its stale look over it.
#[test]
fn an_edit_in_one_win_tab_reaches_the_other_and_survives_its_next_edit() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "BTCUSDT".to_owned(), None);
    select_market(&mut app, "metatrader-b3", "WIN$N");
    assert_eq!(app.tabs.active_index(), 1);
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .edit_config(|config| config.set_ignore_opening_burst_in_scale(true))
    );
    // One frame files the edit, the next one dresses the other tab with it.
    maintain(&mut app);
    maintain(&mut app);

    app.tabs.select(0);
    let tape = app.active_tab().tape();
    assert!(
        tape.cached_config()
            .volume_dots
            .ignore_opening_burst_in_scale,
        "the WINV26 tab wears the WIN$N tab's edit"
    );
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_flow_opening(true)
    );
    maintain(&mut app);
    maintain(&mut app);

    app.tabs.select(1);
    let tape = app.active_tab().tape();
    assert!(tape.ignore_flow_opening(), "and the other way round");
    assert!(
        tape.cached_config()
            .volume_dots
            .ignore_opening_burst_in_scale,
        "the first edit survives the second tab's"
    );
}

/// Review round 1, finding 6: opening a cockpit file dresses the tabs
/// already open with its per-asset settings.
#[test]
fn an_imported_cockpit_dresses_the_open_tabs_with_its_asset_settings() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    let edit = |app: &mut QuantickApp, on: bool| {
        let tape = app.active_tab_mut().tape_mut();
        assert!(tape.edit_config(|config| config.set_ignore_opening_burst_in_scale(on)));
        maintain(app);
    };
    edit(&mut app, true);
    let file = crate::scratch::ScratchFile::new("asset-bundle", "workspace.qws.toml");
    app.workspace_bundle_adapter().export_workspace_to(&file);
    edit(&mut app, false);
    assert_eq!(source(&app), AssetSource::Declared);

    app.workspace_bundle_adapter().import_workspace_from(&file);
    let tape = app.active_tab().tape();
    assert!(
        tape.cached_config()
            .volume_dots
            .ignore_opening_burst_in_scale,
        "the imported WIN settings are on screen"
    );
    assert_eq!(source(&app), AssetSource::Stored);
}

/// Review round 2, finding 2: a dated roll no feed lists — typed in the
/// source picker — opens on its tab feed's look, as before assets existed,
/// not on the presets file's default.
#[test]
fn a_dated_roll_no_feed_lists_opens_on_its_tab_feeds_look() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WDO$N");
    select_market(&mut app, "metatrader-b3", "WDOX26");
    assert_eq!(look(&app).name, "live lane pie");
    let asset = app.active_tab().tape().look.asset().expect("bound");
    assert_eq!(asset.key(), "WDOX26");
    assert_eq!(asset.source(), AssetSource::Declared);
}

/// Review round 2, finding 6: two tabs on the mini index each change a
/// setting before a frame files either. The later filing wins whole — it is
/// filed, not dropped — and the other tab wears it at the next frame.
#[test]
fn an_unfiled_edit_is_filed_even_when_another_tab_filed_first() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "BTCUSDT".to_owned(), None);
    select_market(&mut app, "metatrader-b3", "WIN$N");
    app.tabs.select(0);
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .edit_config(|config| config.set_ignore_opening_burst_in_scale(true))
    );
    app.tabs.select(1);
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_flow_opening(true)
    );
    maintain(&mut app);
    assert!(
        app.active_tab().tape().ignore_flow_opening(),
        "the second tab's edit is on screen, not dropped"
    );
    maintain(&mut app);
    app.tabs.select(0);
    assert!(
        app.active_tab().tape().ignore_flow_opening(),
        "and reaches the first tab"
    );
}

/// Review round 2, finding 4: a window chosen from the menu is the asset's
/// even when the wheel already showed it, and a restart brings it back.
#[test]
fn a_window_chosen_equal_to_the_wheeled_one_survives_a_restart() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    let tape = app.active_tab_mut().tape_mut();
    tape.zoom_live_lane(2.0);
    let wheeled = tape.live_lane_window();
    tape.set_live_lane_window(wheeled);
    maintain(&mut app);
    assert_eq!(source(&app), AssetSource::Stored);

    let reread = quantick_stores::bubble_asset_store::AssetBubblesStore::load(
        crate::bubble_presets::assets_path(),
    )
    .shared();
    with_config(&mut app, |tab, config| {
        tab.bind_asset_bubbles(config, &reread)
    });
    assert_eq!(app.active_tab().tape().live_lane_window(), wheeled);
}

/// Test agent D2 (review round 2, finding 7): the launch window hook is
/// navigation for one run. It reaches the asset a replay switches the tab
/// to after the hook ran, and it files nothing.
#[test]
fn the_launch_window_hook_holds_through_a_switch_and_files_nothing() {
    let launch = AppLaunch {
        scenario: crate::hooks::ScenarioInputs::from_pairs(&[("QUANTICK_TAPE_WINDOW", "90s")]),
        ..AppLaunch::default()
    };
    let (mut app, _evt, _cmd, _book) = test_app_with_launch(launch);
    let held = LaneWindow::Fixed { ms: 90_000 };
    assert_eq!(app.active_tab().tape().live_lane_window(), held);
    app.config = shipped_config();
    select_market(&mut app, "binance", "WINV26");
    let tape = app.active_tab().tape();
    assert_eq!(tape.look.asset().expect("bound").key(), "WIN*");
    assert!(tape.cached_config().native_tape(), "the WIN look is on");
    assert_eq!(tape.live_lane_window(), held, "the held window too");
    maintain(&mut app);
    assert_eq!(source(&app), AssetSource::Declared);
    assert!(
        !crate::bubble_presets::assets_path().exists(),
        "a launch hook writes nothing"
    );
}

/// Review of d326c7b7..99169799, finding 2: `QUANTICK_BUBBLES_AUTOSTART`
/// holds the aggression bubbles on for one run, like the window hook: it
/// reaches the asset a replay switches the tab to and is never filed, so
/// the trader's stored "bubbles off" survives an unrelated edit — until the
/// switch itself is set on purpose.
#[test]
fn the_bubbles_autostart_hook_holds_through_a_switch_and_files_nothing() {
    let launch = AppLaunch {
        scenario: crate::hooks::ScenarioInputs::from_pairs(&[("QUANTICK_BUBBLES_AUTOSTART", "1")]),
        ..AppLaunch::default()
    };
    let (mut app, _evt, _cmd, _book) = test_app_with_launch(launch);
    assert!(app.active_tab().tape().bubbles_enabled());
    app.config = shipped_config();
    // The trader switched the mini index's bubbles off in an earlier run.
    let store = app.workspace.bubble_assets().clone();
    let presets = app.active_tab().tape().look.presets().clone();
    let win = ("metatrader-b3", "WINV26");
    let (_, declared) = AssetBinding::bind(store.clone(), &app.config, win, &presets, false);
    let off = AssetBubbles {
        bubbles: false,
        ..declared.clone()
    };
    assert!(store.borrow_mut().record("WIN*", &off, &declared));
    maintain(&mut app);
    let stored = || {
        AssetBubblesStore::load(crate::bubble_presets::assets_path())
            .get("WIN*")
            .cloned()
    };

    select_market(&mut app, "binance", "WINV26");
    let tape = app.active_tab().tape();
    assert_eq!(tape.look.asset().expect("bound").key(), "WIN*");
    assert!(tape.bubbles_enabled(), "held on through the switch");
    maintain(&mut app);
    assert_eq!(stored(), Some(off.clone()), "and filed nowhere");
    let tape = app.active_tab_mut().tape_mut();
    assert!(tape.edit_config(|config| config.set_ignore_opening_burst_in_scale(true)));
    maintain(&mut app);
    let filed = stored().expect("WIN's settings");
    assert!(
        filed.look.volume_dot_ignore_opening_burst_in_scale,
        "an edit is filed"
    );
    assert!(!filed.bubbles, "the held switch is not");

    // Set on purpose, the switch is the trader's again: no longer held.
    app.active_tab_mut().tape_mut().set_bubbles_enabled(false);
    maintain(&mut app);
    assert_eq!(stored().map(|win| win.bubbles), Some(false));
    select_market(&mut app, "binance", "BTCUSDT");
    select_market(&mut app, "binance", "WINV26");
    assert!(!app.active_tab().tape().bubbles_enabled(), "released");
}

/// Review round 2, finding 3: an export carries an edit no frame filed
/// yet, and says so when the disk would not take the per-asset settings.
#[test]
fn an_export_files_pending_edits_and_says_when_the_disk_refused_them() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    let tape = app.active_tab_mut().tape_mut();
    assert!(tape.edit_config(|config| config.set_ignore_opening_burst_in_scale(true)));
    let file = crate::scratch::ScratchFile::new("asset-export", "workspace.qws.toml");
    app.workspace_bundle_adapter().export_workspace_to(&file);
    let tape = app.active_tab_mut().tape_mut();
    assert!(tape.edit_config(|config| config.set_ignore_opening_burst_in_scale(false)));
    maintain(&mut app);
    app.workspace_bundle_adapter().import_workspace_from(&file);
    assert!(
        app.active_tab()
            .tape()
            .cached_config()
            .volume_dots
            .ignore_opening_burst_in_scale,
        "the unfiled edit was in the export"
    );

    let path = crate::bubble_presets::assets_path();
    let blocked = path.with_extension("toml.tmp");
    std::fs::create_dir_all(&blocked).expect("a folder where the write goes");
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_flow_opening(true)
    );
    app.workspace_bundle_adapter().export_workspace_to(&file);
    let message = app.surfaces.toast.message().unwrap_or_default().to_owned();
    assert!(message.contains("as last saved"), "{message}");
    std::fs::remove_dir(&blocked).expect("the folder goes");
}
