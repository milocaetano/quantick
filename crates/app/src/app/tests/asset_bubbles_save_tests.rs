//! "Save changes for this asset", and the promise under it: a bubble edit
//! made while one asset is on screen is never saved for another — not
//! through the per-asset store, the presets file, the chart-layers file or
//! the cockpit.
use super::*;
use crate::bubble_presets::{BubblePreset, BubblePresetFile, PresetSource, embedded};
use quantick_orderflow::LaneWindow;
use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore};
use quantick_stores::bubble_assets::AssetBubbles;
use std::path::{Path, PathBuf};

const ACTION: &str = "orderflow.bubbles.save_changes.set";

fn shipped_config() -> AppConfig {
    toml::from_str(include_str!("../../../config/feeds.toml")).expect("shipped feeds parse")
}

fn look(app: &QuantickApp) -> BubblePreset {
    let tape = app.active_tab().tape();
    BubblePreset::capture(tape.active_preset_for_test(), tape.cached_config())
}

fn maintain(app: &mut QuantickApp) {
    app.layer_wiring().maintain(&egui::Context::default());
}

/// The tab's own market switch, without a live feed behind it.
fn select_market(app: &mut QuantickApp, feed: &str, symbol: &str) {
    let tab = app.active_tab_mut();
    tab.feed_id = feed.to_owned();
    tab.symbol = symbol.to_owned();
    tab.tape_mut().reset_for_symbol(symbol);
    with_config(app, |tab, config| {
        tab.apply_asset_bubbles_after_switch(config);
    });
}

/// The mini index in two tabs, the first one selected.
fn two_win_tabs() -> QuantickApp {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    app.arrangement_adapter()
        .open_tab("binance".to_owned(), "BTCUSDT".to_owned(), None);
    select_market(&mut app, "metatrader-b3", "WIN$N");
    app.tabs.select(0);
    app
}

fn opening_excluded(app: &QuantickApp) -> bool {
    app.active_tab()
        .tape()
        .cached_config()
        .volume_dots
        .ignore_opening_burst_in_scale
}

fn saves_changes(app: &QuantickApp) -> bool {
    let asset = app.active_tab().tape().asset().expect("a bound asset");
    asset.saves_changes()
}

/// What the store on disk holds for the mini index.
fn stored_win() -> Option<AssetBubbles> {
    let store = AssetBubblesStore::load(crate::bubble_presets::assets_path());
    store.get("WIN*").cloned()
}

#[test]
fn with_saving_off_a_win_edit_stays_on_its_tab_until_the_tab_shows_win_again() {
    let mut app = two_win_tabs();
    let tape = app.active_tab_mut().tape_mut();
    assert_eq!(tape.set_save_asset_changes(false), Some(true));
    assert!(tape.set_ignore_opening_burst_in_scale(true));
    maintain(&mut app);
    maintain(&mut app);
    assert!(opening_excluded(&app), "the edit is on screen");
    let why = app.active_tab().tape().asset().expect("bound").unsaved();
    assert!(why.expect("not saved").contains("saving is off"));
    assert_eq!(stored_win(), None, "nothing is written for WIN");

    app.tabs.select(1);
    assert!(
        !opening_excluded(&app),
        "the other WIN tab shows what is stored"
    );
    assert!(!saves_changes(&app), "the switch is WIN's, in every tab");
    app.tabs.select(0);
    assert!(
        opening_excluded(&app),
        "selecting a tab keeps the session's edit"
    );

    select_market(&mut app, "binance", "BTCUSDT");
    assert!(saves_changes(&app), "BTC saves its changes");
    select_market(&mut app, "metatrader-b3", "WINV26");
    assert!(
        !opening_excluded(&app),
        "showing WIN again brings back what is stored"
    );
    assert!(!saves_changes(&app), "and the switch stays off");

    // A restart reads the store back: the switch, and nothing else.
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_opening_burst_in_scale(true)
    );
    maintain(&mut app);
    let reread = AssetBubblesStore::load(crate::bubble_presets::assets_path()).shared();
    with_config(&mut app, |tab, config| {
        tab.bind_asset_bubbles(config, &reread);
    });
    assert!(!opening_excluded(&app), "the session's edit is gone");
    assert!(!saves_changes(&app), "the switch is kept");
}

#[test]
fn switching_saving_on_again_saves_what_the_tab_shows_and_the_other_tab_wears_it() {
    let mut app = two_win_tabs();
    let tape = app.active_tab_mut().tape_mut();
    tape.set_save_asset_changes(false);
    assert!(tape.set_ignore_opening_burst_in_scale(true));
    maintain(&mut app);
    app.tabs.select(1);
    let other = app.active_tab_mut().tape_mut();
    assert!(
        other.set_ignore_flow_opening(true),
        "the other tab's own edit"
    );
    maintain(&mut app);

    app.tabs.select(0);
    let tape = app.active_tab_mut().tape_mut();
    assert_eq!(tape.set_save_asset_changes(true), Some(true));
    maintain(&mut app);
    maintain(&mut app);
    let stored = stored_win().expect("WIN's settings are saved");
    assert!(stored.look.volume_dot_ignore_opening_burst_in_scale);
    assert!(!stored.flow_ignore_opening, "not the other tab's held edit");
    assert_eq!(
        app.active_tab().tape().asset().expect("bound").unsaved(),
        None
    );

    app.tabs.select(1);
    assert!(opening_excluded(&app), "the other tab wears what was saved");
    assert!(
        !app.active_tab().tape().ignore_flow_opening(),
        "and drops what it held"
    );
}

fn input(app: &QuantickApp, save_changes: bool) -> Value {
    json!({
        "tab_id": app.tabs.active_id().to_string(),
        "pane_id": app.active_tab().flow_pane.id.to_string(),
        "save_changes": save_changes,
    })
}

/// The flow pane's `asset` block in `orderflow.bubbles`.
fn asset_read(app: &mut QuantickApp, client: &mut LocalClient) -> Value {
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let (response, _) = unkeyed_call(
        app,
        client,
        "snapshot.read",
        json!({ "scopes": ["orderflow.bubbles"] }),
    );
    success_result(&response)["scopes"]["orderflow.bubbles"]["value"]["tabs"]
        .as_array()
        .expect("tabs")
        .iter()
        .flat_map(|tab| tab["panes"].as_array().expect("panes"))
        .find(|pane| pane["pane_id"] == pane_id)
        .expect("the flow pane is readable")["bubbles"]["asset"]
        .clone()
}

#[test]
fn save_changes_is_the_assets_permission_checked_retry_safe_and_read_back() {
    assert_eq!(
        quantick_control_schema::bubble_save::descriptor().persistence,
        quantick_control::registry::EffectPersistence::Durable,
        "the switch is stored for the asset"
    );
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("save-changes");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let before = asset_read(&mut app, &mut observer);
    assert_eq!(before["save_changes"], true, "on until switched off");

    let payload = input(&app, false);
    let (denied, _) = unkeyed_call(&mut app, &mut observer, ACTION, payload.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    assert_eq!(asset_read(&mut app, &mut observer), before);
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "save-first",
        ACTION,
        payload.clone(),
        "save-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "save-retry",
        ACTION,
        payload,
        "save-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    let result = success_result(&first);
    assert_eq!(result["changed"], true);
    assert_eq!(result["save_changes"], false);
    assert_eq!(result["asset"], before["key"]);
    maintain(&mut app);
    let off = asset_read(&mut app, &mut observer);
    assert_eq!(off["save_changes"], false);
    assert_eq!(off["saved"], true, "nothing on screen differs yet");

    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_opening_burst_in_scale(true)
    );
    maintain(&mut app);
    let held = asset_read(&mut app, &mut observer);
    assert_eq!(held["saved"], false, "a held edit is not saved");
    let why = held["save_error"].as_str().expect("why").to_owned();
    assert!(why.contains("saving is off"), "{why}");
    let payload = input(&app, false);
    let (noop, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&noop)["changed"], false);

    let mut context = input(&app, true);
    context["pane_id"] = json!(app.active_tab().time_panes[0].id.to_string());
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, context);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    let mut unknown = input(&app, true);
    unknown["every_asset"] = json!(true);
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, unknown);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    assert_eq!(asset_read(&mut app, &mut observer)["save_changes"], false);

    let payload = input(&app, true);
    let (on, _) = unkeyed_call(&mut app, &mut cockpit, ACTION, payload);
    assert_eq!(success_result(&on)["changed"], true);
    maintain(&mut app);
    let saved = asset_read(&mut app, &mut observer);
    assert_eq!(saved["save_changes"], true);
    assert_eq!(saved["saved"], true, "what the pane shows is saved");
    assert_eq!(saved["source"], "stored");
    disable_test_gateway(&mut app, &ctx);
}

/// One edit made while the mini index is on screen, as the panel, a menu
/// or a control call makes it. `presets` is the presets file a panel save
/// writes to.
struct Edit {
    name: &'static str,
    apply: fn(&mut QuantickApp, &Path),
    /// Whether the edit is an explicit write to the presets file.
    writes_presets: bool,
    /// Whether the edit changes what the mini index shows — a row that
    /// changes nothing proves nothing.
    moves_win: bool,
}

fn writer(path: &Path) -> impl FnOnce(&BubblePresetFile) -> Result<PathBuf, String> {
    let path = path.to_path_buf();
    move |document| crate::bubble_presets::save_to(path, document)
}

fn refused(name: &str) -> impl FnOnce(&BubblePresetFile) -> Result<PathBuf, String> {
    let name = name.to_owned();
    move |_| panic!("'{name}' reached the presets file")
}

fn toggle(app: &mut QuantickApp, layer: ChartLayer) {
    let on = layer_on(app, layer);
    switch_layer(app, layer, !on);
}

const EDITS: &[Edit] = &[
    Edit {
        name: "a panel value",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            tape.edit_config_for_test(|config| config.bubbles.max_radius += 3.0);
        },
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "a preset picked",
        apply: |app, _| assert!(app.active_tab_mut().tape_mut().apply_preset("dense tape")),
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "the panel's defaults restored",
        apply: |app, _| app.active_tab_mut().tape_mut().reset_bubble_visuals(),
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "the presets file reloaded",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            tape.reload_presets_from((embedded(), PresetSource::Embedded, None));
        },
        writes_presets: false,
        moves_win: false,
    },
    Edit {
        name: "a look saved under a new name",
        apply: |app, presets| {
            let tape = app.active_tab_mut().tape_mut();
            tape.edit_config_for_test(|config| config.bubbles.max_radius += 3.0);
            tape.set_preset_name_draft_for_test("my win");
            tape.save_preset_with(writer(presets));
        },
        writes_presets: true,
        moves_win: true,
    },
    Edit {
        name: "a look other assets open on, saved over or deleted",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            tape.edit_config_for_test(|config| config.bubbles.max_radius += 3.0);
            for name in ["default", "mini index regions", "live lane pie"] {
                tape.set_preset_name_draft_for_test(name);
                tape.save_preset_with(refused(name));
                tape.delete_preset_with(refused(name));
            }
        },
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "a preset no asset opens on, deleted",
        apply: |app, presets| {
            let tape = app.active_tab_mut().tape_mut();
            tape.set_preset_name_draft_for_test("3d spheres");
            tape.delete_preset_with(writer(presets));
        },
        writes_presets: true,
        moves_win: false,
    },
    Edit {
        name: "the tape's opening scale",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            assert!(tape.set_ignore_opening_burst_in_scale(true));
        },
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "the candles' opening scale",
        apply: |app, _| {
            assert!(
                app.active_tab_mut()
                    .tape_mut()
                    .set_ignore_flow_opening(true)
            )
        },
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "the tape window chosen",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            tape.set_live_lane_window(LaneWindow::Fixed { ms: 9_000 });
        },
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "candle aggression",
        apply: |app, _| switch_layer(app, ChartLayer::CandleAggression, true),
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "the native tape",
        apply: |app, _| toggle(app, ChartLayer::NativeTape),
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "tape only",
        apply: |app, _| toggle(app, ChartLayer::TapeOnly),
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "volume dots",
        apply: |app, _| toggle(app, ChartLayer::BubbleOverlapMerge),
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "lane marks",
        apply: |app, _| toggle(app, ChartLayer::LaneMarks),
        writes_presets: false,
        moves_win: true,
    },
    Edit {
        name: "saving switched off, then a panel value",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            assert_eq!(tape.set_save_asset_changes(false), Some(true));
            tape.edit_config_for_test(|config| config.bubbles.max_radius += 3.0);
        },
        writes_presets: false,
        moves_win: true,
    },
];

/// What the mini index's tab shows: the look, candle aggression, the
/// candles' opening scale and the save switch.
fn win(app: &QuantickApp) -> (BubblePreset, bool, bool, bool) {
    let tab = app.active_tab();
    (
        look(app),
        tab.flow_pane
            .layer_switched_on(ChartLayer::CandleAggression, &app.style),
        tab.tape().ignore_flow_opening(),
        saves_changes(app),
    )
}

/// BTC as its tab shows it, and as a restart would open it on `presets`.
fn btc(app: &mut QuantickApp, presets: &BubblePresetFile) -> (BubblePreset, bool, AssetBubbles) {
    app.tabs.select(1);
    let shown = look(app);
    let candle = app
        .active_tab()
        .flow_pane
        .layer_switched_on(ChartLayer::CandleAggression, &app.style);
    assert!(saves_changes(app), "BTC's own switch");
    app.tabs.select(0);
    let store = AssetBubblesStore::load(crate::bubble_presets::assets_path()).shared();
    let market = ("binance", "BTCUSDT");
    let (_, reopened) = AssetBinding::bind(store, &shipped_config(), market, presets, false);
    (shown, candle, reopened)
}

/// The trader's request: "do not let it save for everyone when I change
/// some configuration". Every kind of bubble edit on the mini index leaves
/// BTC — on screen and after a restart — and the files every market reads
/// as they were. The one shared write is an explicit preset save or delete,
/// and it restyles no other asset either.
#[test]
fn no_bubble_edit_on_win_reaches_btc_or_a_file_every_market_reads() {
    for edit in EDITS {
        let dir = crate::scratch::ScratchDir::new("asset-audit");
        let (layers_path, presets_path) = (dir.join("chart-layers.toml"), dir.join("bubbles.toml"));
        let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
        std::fs::write(
            &layers_path,
            include_str!("../../../config/chart-layers.toml"),
        )
        .expect("a chart-layers file");
        app.workspace.set_chart_layers_path(layers_path.clone());
        app.workspace.set_ui_state_path(dir.join("ui-state.toml"));
        app.layer_wiring().restore();
        app.arrangement_adapter()
            .open_tab("binance".to_owned(), "BTCUSDT".to_owned(), None);
        maintain(&mut app);
        app.tabs.select(0);
        maintain(&mut app);
        let before = btc(&mut app, &embedded());
        app.workspace_save_adapter().save_workspace("audit");
        let read = |path: &Path| std::fs::read(path).unwrap_or_default();
        let (layers, cockpit) = (read(&layers_path), read(app.workspace.ui_state_path()));
        let shown = win(&app);

        (edit.apply)(&mut app, &presets_path);
        maintain(&mut app);
        maintain(&mut app);

        let at = edit.name;
        assert_eq!(win(&app) != shown, edit.moves_win, "{at}: WIN on screen");
        assert_eq!(read(&layers_path), layers, "{at}: chart-layers.toml moved");
        app.workspace_save_adapter().save_workspace("audit");
        assert!(
            read(app.workspace.ui_state_path()) == cockpit,
            "{at}: the cockpit file moved"
        );
        assert_eq!(
            presets_path.exists(),
            edit.writes_presets,
            "{at}: presets file"
        );
        let presets = if edit.writes_presets {
            let (written, _, error) = crate::bubble_presets::load_from(Some(presets_path));
            assert_eq!(error, None, "{at}");
            assert_eq!(written.active, embedded().active, "{at}: the default moved");
            written
        } else {
            embedded()
        };
        assert_eq!(btc(&mut app, &presets), before, "{at}: BTC moved");
    }
}
