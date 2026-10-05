//! "Save changes for this asset", and the promise under it: a bubble edit
//! made while one asset is on screen is never saved for another — not
//! through the per-asset store, the presets file, the chart-layers file or
//! the cockpit. Panel edits go through the operation the panel's control
//! calls; the rest through the hotkey, the menu or the control action an
//! agent calls.
use super::asset_bubbles_tests::{look, maintain, select_market, shipped_config};
use super::*;
use crate::bubble_presets::{BubblePreset, BubblePresetFile, embedded, load_from};
use crate::control::ActionOrigin;
use quantick_stores::bubble_asset_store::{AssetBinding, AssetBubblesStore, SaveSwitch};
use quantick_stores::bubble_assets::{AssetBubbles, BUBBLES_LAYER_DEFAULT};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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

/// One control action on the active tab's flow pane, through the registry
/// every caller's action goes through, and its result.
fn act(app: &mut QuantickApp, action: &str, mut input: Value) -> Value {
    input["tab_id"] = json!(app.tabs.active_id().to_string());
    input["pane_id"] = json!(app.active_tab().flow_pane.id.to_string());
    app.control_action(action, 1, ActionOrigin::Human, input)
        .unwrap_or_else(|error| panic!("{action}: {error:?}"))
}

fn set_layer(app: &mut QuantickApp, layer: ChartLayer, visible: bool) {
    let input = json!({ "layer_id": layer.id(), "visible": visible });
    act(app, "layers.visibility.set", input);
}

/// Ctrl+B on the flow pane. Beside the native tape the bubbles wait for a
/// native price grid no test feed builds, so the tape goes first.
fn ctrl_b(app: &mut QuantickApp) {
    switch_layer(app, ChartLayer::NativeTape, false);
    let before = layer_on(app, ChartLayer::Bubbles);
    let press = key_press_with(egui::Key::B, egui::Modifiers::CTRL);
    run_frame_with_events(app, &egui::Context::default(), vec![press]);
    assert_ne!(
        layer_on(app, ChartLayer::Bubbles),
        before,
        "Ctrl+B switched them"
    );
}

#[test]
fn the_bubbles_layer_opens_on_the_layers_own_default() {
    assert_eq!(BUBBLES_LAYER_DEFAULT, ChartLayer::Bubbles.0.default_on);
    assert!(!ChartLayer::Bubbles.persisted(), "not in chart-layers.toml");
}

/// The trader's decision: the aggression bubbles switch (Ctrl+B) is the
/// asset's, like candle aggression — not a chart-layers switch for every
/// market, and an old global `bubbles = false` there no longer applies.
#[test]
fn ctrl_b_on_win_is_wins_alone_and_the_old_global_switch_is_ignored() {
    let dir = crate::scratch::ScratchDir::new("asset-bubbles-layer");
    let layers_path = dir.join("chart-layers.toml");
    std::fs::write(&layers_path, "version = 1\n[layers]\nbubbles = false\n").expect("a file");
    let mut app = two_win_tabs();
    app.workspace.set_chart_layers_path(layers_path.clone());
    app.layer_wiring().restore();
    assert!(
        layer_on(&app, ChartLayer::Bubbles),
        "the old global is ignored"
    );

    ctrl_b(&mut app);
    maintain(&mut app);
    assert!(!layer_on(&app, ChartLayer::Bubbles));
    assert_eq!(stored_win().map(|win| win.bubbles), Some(false));
    let written = std::fs::read_to_string(&layers_path).expect("chart layers");
    assert_eq!(
        written, "version = 1\n[layers]\nbubbles = false\n",
        "not written"
    );
    app.tabs.select(1);
    assert!(
        !layer_on(&app, ChartLayer::Bubbles),
        "the other WIN tab wears it"
    );
    select_market(&mut app, "binance", "BTCUSDT");
    assert!(layer_on(&app, ChartLayer::Bubbles), "BTC keeps its own");
    select_market(&mut app, "metatrader-b3", "WIN$N");
    assert!(!layer_on(&app, ChartLayer::Bubbles), "WIN shown again");

    // Saving off, the switch is held on the tab like every other setting.
    act(
        &mut app,
        "orderflow.bubbles.save_changes.set",
        json!({ "save_changes": false }),
    );
    set_layer(&mut app, ChartLayer::Bubbles, true);
    maintain(&mut app);
    assert!(layer_on(&app, ChartLayer::Bubbles));
    assert_eq!(
        stored_win().map(|win| win.bubbles),
        Some(false),
        "held, not saved"
    );
    app.tabs.select(0);
    assert!(
        !layer_on(&app, ChartLayer::Bubbles),
        "the first tab shows what is saved"
    );
}

/// Review of d326c7b7, finding 7: an export while saving is off says the
/// changes held on screen are not in the bundle.
#[test]
fn an_export_names_the_changes_saving_off_holds_back() {
    let mut app = app_on(shipped_config(), "metatrader-b3", "WINV26");
    act(
        &mut app,
        "orderflow.bubbles.save_changes.set",
        json!({ "save_changes": false }),
    );
    act(
        &mut app,
        "orderflow.tape.opening_scale.set",
        json!({ "ignore_opening_burst_in_scale": true }),
    );
    let file = crate::scratch::ScratchFile::new("asset-export-held", "workspace.qws.toml");
    app.workspace_bundle_adapter().export_workspace_to(&file);
    let message = app.surfaces.toast.message().unwrap_or_default().to_owned();
    assert!(message.contains("not included"), "{message}");
    assert!(message.contains("WIN*"), "{message}");
    assert!(
        !message.contains("as last saved"),
        "the switch itself is saved: {message}"
    );
}

#[test]
fn with_saving_off_a_win_edit_stays_on_its_tab_until_the_tab_shows_win_again() {
    let mut app = two_win_tabs();
    let tape = app.active_tab_mut().tape_mut();
    assert_eq!(tape.set_save_asset_changes(false), Some(SaveSwitch::Off));
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
    assert_eq!(
        tape.set_save_asset_changes(true),
        Some(SaveSwitch::ScreenStored)
    );
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

/// Review of d326c7b7..99169799, finding 1: switching saving on stores what
/// the pane shows only when nothing newer is stored for the asset. Behind
/// another tab's filing the pane's screen gives way to the stored settings
/// instead, and the action's result says which happened.
#[test]
fn switching_saving_on_says_whether_the_screen_was_stored_or_gave_way() {
    let mut app = two_win_tabs();
    app.tabs.select(1);
    let off = save_switch(&mut app, false);
    assert_eq!(
        (&off["changed"], &off["screen"]),
        (&json!(true), &json!("kept"))
    );
    assert!(
        app.active_tab_mut()
            .tape_mut()
            .set_ignore_flow_opening(true)
    );
    let on = save_switch(&mut app, true);
    assert_eq!(on["screen"], "stored", "nothing newer is stored");
    // Off again from the panel, before the first tab wore that filing.
    let tape = app.active_tab_mut().tape_mut();
    assert_eq!(tape.set_save_asset_changes(false), Some(SaveSwitch::Off));

    // An edit there, then saving on: the other tab's filing is newer.
    app.tabs.select(0);
    let tape = app.active_tab_mut().tape_mut();
    assert!(tape.set_ignore_opening_burst_in_scale(true));
    let on = save_switch(&mut app, true);
    assert_eq!(on["changed"], true);
    assert_eq!(on["screen"], "replaced_by_stored");
    maintain(&mut app);
    assert!(!opening_excluded(&app), "this tab's screen gave way");
    assert!(
        app.active_tab().tape().ignore_flow_opening(),
        "to the store"
    );
    let stored = stored_win().expect("WIN's settings");
    assert!(stored.flow_ignore_opening);
    assert!(
        !stored.look.volume_dot_ignore_opening_burst_in_scale,
        "nothing of this tab's screen is stored"
    );
    let again = save_switch(&mut app, true);
    assert_eq!(
        (&again["changed"], &again["screen"]),
        (&json!(false), &json!("kept"))
    );
}

/// Where the presets file every market reads stands after an edit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Presets {
    /// Untouched: the shipped presets stay in force.
    Untouched,
    /// The app wrote it at the trader's explicit save or delete.
    Written,
    /// The trader edited it by hand ([`HAND_EDITED`]) and the panel reloaded it.
    HandEdited,
}

/// The presets file the trader edits by hand: the shipped one, its `active`
/// look — the one an undeclared asset like BTC opens on — changed.
const HAND_EDITED: &str = "hand-edited-bubbles.toml";

/// One edit made while the mini index is on screen. `apply` gets the path a
/// panel save writes the presets file to.
struct Edit {
    name: &'static str,
    apply: fn(&mut QuantickApp, &Path),
    presets: Presets,
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

fn toggle_by_call(app: &mut QuantickApp, layer: ChartLayer) {
    let on = layer_on(app, layer);
    set_layer(app, layer, !on);
}

fn panel_value(app: &mut QuantickApp) {
    let tape = app.active_tab_mut().tape_mut();
    tape.edit_config_for_test(|config| config.bubbles.max_radius += 3.0);
}

fn save_switch(app: &mut QuantickApp, on: bool) -> Value {
    act(
        app,
        "orderflow.bubbles.save_changes.set",
        json!({ "save_changes": on }),
    )
}

const EDITS: &[Edit] = &[
    Edit {
        name: "a panel value",
        apply: |app, _| panel_value(app),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "a preset picked",
        apply: |app, _| assert!(app.active_tab_mut().tape_mut().apply_preset("dense tape")),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "the panel's defaults restored",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            tape.press_reset_bubble_visuals_for_test();
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "the presets file edited by hand, reloaded",
        apply: |app, presets| {
            let mut edited = embedded();
            "dense tape".clone_into(&mut edited.active);
            let path = presets.with_file_name(HAND_EDITED);
            crate::bubble_presets::save_to(path.clone(), &edited).expect("a hand edit");
            app.active_tab_mut()
                .tape_mut()
                .press_reload_presets_for_test(path);
        },
        presets: Presets::HandEdited,
        moves_win: false,
    },
    Edit {
        name: "a look saved under a new name",
        apply: |app, presets| {
            panel_value(app);
            let tape = app.active_tab_mut().tape_mut();
            tape.set_preset_name_draft_for_test("my win");
            tape.press_save_preset_for_test(writer(presets));
        },
        presets: Presets::Written,
        moves_win: true,
    },
    Edit {
        name: "a look other assets open on, saved over or deleted",
        apply: |app, _| {
            panel_value(app);
            let tape = app.active_tab_mut().tape_mut();
            for name in ["default", "mini index regions", "live lane pie"] {
                tape.set_preset_name_draft_for_test(name);
                tape.press_save_preset_for_test(refused(name));
                tape.press_delete_preset_for_test(refused(name));
            }
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "a preset no asset opens on, deleted",
        apply: |app, presets| {
            let tape = app.active_tab_mut().tape_mut();
            tape.set_preset_name_draft_for_test("3d spheres");
            tape.press_delete_preset_for_test(writer(presets));
        },
        presets: Presets::Written,
        moves_win: false,
    },
    Edit {
        name: "the tape's opening scale, by control call",
        apply: |app, _| {
            let input = json!({ "ignore_opening_burst_in_scale": true });
            act(app, "orderflow.tape.opening_scale.set", input);
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "the candles' opening scale, from the layer menu",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            assert!(tape.set_ignore_flow_opening(true));
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "the tape window chosen",
        apply: |app, _| {
            let tape = app.active_tab_mut().tape_mut();
            tape.set_live_lane_window(quantick_orderflow::LaneWindow::Fixed { ms: 9_000 });
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "candle aggression, from the layer menu",
        apply: |app, _| switch_layer(app, ChartLayer::CandleAggression, true),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "the native tape, from the layer menu",
        apply: |app, _| toggle(app, ChartLayer::NativeTape),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "tape only",
        apply: |app, _| toggle(app, ChartLayer::TapeOnly),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "volume dots, by control call",
        apply: |app, _| toggle_by_call(app, ChartLayer::BubbleOverlapMerge),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "lane marks",
        apply: |app, _| toggle(app, ChartLayer::LaneMarks),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "aggression bubbles, Ctrl+B",
        apply: |app, _| ctrl_b(app),
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "aggression bubbles, by control call",
        apply: |app, _| {
            switch_layer(app, ChartLayer::NativeTape, false);
            toggle_by_call(app, ChartLayer::Bubbles);
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "saving switched off, by control call",
        apply: |app, _| {
            save_switch(app, false);
        },
        presets: Presets::Untouched,
        moves_win: false,
    },
    Edit {
        name: "saving switched off, then a panel value",
        apply: |app, _| {
            save_switch(app, false);
            panel_value(app);
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
    Edit {
        name: "saving off, a panel value, saving on again",
        apply: |app, _| {
            save_switch(app, false);
            panel_value(app);
            maintain(app);
            save_switch(app, true);
        },
        presets: Presets::Untouched,
        moves_win: true,
    },
];

/// What the mini index's tab shows: the look, candle aggression, the
/// candles' opening scale and the aggression bubbles — not the save switch,
/// so a row that only switches it moves nothing here.
fn win(app: &QuantickApp) -> (BubblePreset, bool, bool, bool) {
    let tab = app.active_tab();
    (
        look(app),
        tab.flow_pane
            .layer_switched_on(ChartLayer::CandleAggression, &app.style),
        tab.tape().ignore_flow_opening(),
        layer_on(app, ChartLayer::Bubbles),
    )
}

/// BTC as its tab shows it — look, candle aggression, bubbles — and as a
/// restart on `presets` would open it.
fn btc(
    app: &mut QuantickApp,
    presets: &BubblePresetFile,
) -> (BubblePreset, bool, bool, AssetBubbles) {
    app.tabs.select(1);
    let shown = (
        look(app),
        layer_on(app, ChartLayer::CandleAggression),
        layer_on(app, ChartLayer::Bubbles),
    );
    assert!(saves_changes(app), "BTC's own switch");
    app.tabs.select(0);
    let store = AssetBubblesStore::load(crate::bubble_presets::assets_path()).shared();
    let (_, reopened) = bind_btc(store, presets);
    (shown.0, shown.1, shown.2, reopened)
}

fn bind_btc(
    store: quantick_stores::bubble_asset_store::SharedAssetBubbles,
    presets: &BubblePresetFile,
) -> (AssetBinding, AssetBubbles) {
    let market = ("binance", "BTCUSDT");
    AssetBinding::bind(store, &shipped_config(), market, presets, false)
}

/// BTC as `presets` declares it, nobody having tuned it.
fn btc_declared(presets: &BubblePresetFile) -> (BubblePreset, bool, bool, AssetBubbles) {
    let (_, settings) = bind_btc(AssetBubblesStore::default().shared(), presets);
    (
        settings.look.clone(),
        settings.candle_aggression,
        settings.bubbles,
        settings,
    )
}

/// The presets file at `path`, which must parse: a corrupt write falls back
/// to the shipped presets and would pass every other check.
fn parsed(path: PathBuf, at: &str) -> BubblePresetFile {
    let (presets, _, error) = load_from(Some(path));
    assert_eq!(error, None, "{at}: the presets file does not parse");
    presets
}

/// The trader's request: "do not let it save for everyone when I change
/// some configuration". Every kind of bubble edit on the mini index leaves
/// BTC — on screen and after a restart — as the presets file in force
/// declares it, and the files every market reads as they were. The one
/// shared write is an explicit preset save or delete, and it restyles no
/// other asset either; a hand-edited presets file reloaded from the WIN tab
/// restyles BTC exactly as that file says, and no further.
#[test]
fn no_bubble_edit_on_win_reaches_btc_or_a_file_every_market_reads() {
    for edit in EDITS {
        // A cockpit of its own per row, or one row's WIN settings open the next.
        crate::store_home::next_test_home();
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
        // A first frame lays out the panes the feed declares, which the
        // cockpit records; a hotkey row needs a frame, so every row has one.
        run_frame(&mut app, &egui::Context::default());
        maintain(&mut app);
        let at = edit.name;
        let shipped = embedded();
        assert!(
            btc(&mut app, &shipped) == btc_declared(&shipped),
            "{at}: BTC opens declared"
        );
        app.workspace_save_adapter().save_workspace("audit");
        let read = |path: &Path| std::fs::read(path).unwrap_or_default();
        let (layers, cockpit) = (read(&layers_path), read(app.workspace.ui_state_path()));
        let shown = win(&app);

        (edit.apply)(&mut app, &presets_path);
        maintain(&mut app);
        maintain(&mut app);

        assert_eq!(win(&app) != shown, edit.moves_win, "{at}: WIN on screen");
        assert_eq!(read(&layers_path), layers, "{at}: chart-layers.toml moved");
        app.workspace_save_adapter().save_workspace("audit");
        assert!(
            read(app.workspace.ui_state_path()) == cockpit,
            "{at}: the cockpit file moved"
        );
        let written = edit.presets == Presets::Written;
        assert_eq!(presets_path.exists(), written, "{at}: presets file");
        let in_force = match edit.presets {
            Presets::Untouched => embedded(),
            Presets::Written => parsed(presets_path, at),
            Presets::HandEdited => parsed(dir.join(HAND_EDITED), at),
        };
        if written {
            assert_eq!(
                in_force.active,
                embedded().active,
                "{at}: the default moved"
            );
        }
        let after = btc(&mut app, &in_force);
        assert!(after == btc_declared(&in_force), "{at}: BTC moved");
    }
}
