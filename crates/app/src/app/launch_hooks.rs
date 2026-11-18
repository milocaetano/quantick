//! The launch phase: what runs after workspace restoration and before the
//! first frame, and the scenario appliers that stage a capture's state in it.
//!
//! Not a method on the app: the composition root captures every launch input
//! in `main` and hands them to the constructor, which calls
//! [`apply_launch_phase`] once over its [`OwnersDesk`]. Each applier takes
//! only the owners it stages and calls their own methods; the few that also
//! run a registered call borrow through the desk beside the ports they name; the value grammar lives in
//! `quantick_control_schema::launch_values`. Two production steps live here
//! because the scenarios are ordered around them — `seed_new_panes` and
//! recording the layers the launch staged. Every applier that reads a
//! `QUANTICK_*` compiles only with its harness family (or under test), from
//! inputs the root captured ([`ScenarioInputs`]); a default build runs the
//! two steps alone.
//!
//! **The order is the contract.** These are not independent switches applied
//! in any convenient sequence -- several are read at one point precisely
//! because of what has and has not happened yet:
//!
//! - `QUANTICK_REPLAY_DAY_BEFORE` is read *before anything loads a session*,
//!   because that is the frame the setting is consulted on.
//! - `seed_new_panes` puts the active layout on the first tab's panes
//!   *before any autostart hook*, because the file is what the user actually
//!   had open.
//! - The autostart hooks run *after* the config defaults and the restored
//!   workspace, because an env var is an explicit request for this one run
//!   and must win over both.
//! - `QUANTICK_WORKSPACE_SAVE` and its file siblings run last, so what they
//!   write is the state every hook before them produced.
//!
//! So the appliers are called in the order [`apply_launch_phase`] lists them.
//! Reordering them changes what a launch opens with, and no compiler catches
//! it.

#[cfg(any(feature = "scenario-harness", test))]
use crate::dock::DockTab;
#[cfg(any(feature = "scenario-harness", test))]
use crate::hooks::ScenarioInputs;
#[cfg(any(feature = "scenario-harness", test))]
use crate::indicator_worker::IndicatorSource;
#[cfg(any(feature = "scenario-harness", test))]
use crate::indicators::state_file::SavedKind;
#[cfg(any(feature = "scenario-harness", test))]
use crate::surfaces::replay_browser::ReplayBrowserExt as _;
#[cfg(any(feature = "scenario-harness", test))]
use crate::tab::CanvasLayout;
#[cfg(any(feature = "drawing-harness", test))]
use crate::toolrail::ToolRailExt as _;
#[cfg(any(feature = "scenario-harness", test))]
use crate::{orderflow_panel::open_on, ui_state::PanelSection};
#[cfg(any(feature = "scenario-harness", test))]
use quantick_control_schema::launch_values as values;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_control_schema::{desk, launch_calls, layout};
#[cfg(any(feature = "scenario-harness", test))]
use quantick_feed::history_reach;

#[cfg(any(feature = "scenario-harness", test))]
use super::AUTOSTART_NATIVES;
use super::owners_desk::OwnersDesk;
#[cfg(any(feature = "scenario-harness", test))]
use super::workspace_runtime::WorkspaceRuntime;
use super::{GatewayPort, TabsPort};
#[cfg(any(feature = "scenario-harness", test))]
use crate::{surfaces::Surfaces, tab::Tab};

/// Run the launch phase over a constructed app, in the order this module
/// fixes. Called once, from `new_with_workspace`, after the saved workspace
/// is restored.
pub(super) fn apply_launch_phase<D: OwnersDesk + GatewayPort + TabsPort>(
    desk: &mut D,
    #[cfg(any(feature = "scenario-harness", test))] env: &ScenarioInputs,
    #[cfg(any(feature = "control-harness", test))]
    control_launch: super::control_host::ControlLaunch,
    #[cfg(any(feature = "drawing-harness", test))] rail_launch: crate::toolrail::ToolRailLaunch,
) {
    #[cfg(any(feature = "scenario-harness", test))]
    book_and_strip(desk.owners().runtime.active_tab_mut(), env);
    #[cfg(any(feature = "control-harness", test))]
    {
        let owners = desk.owners();
        (owners.control).apply_launch(control_launch, &mut owners.surfaces.access_panel);
    }
    #[cfg(any(feature = "drawing-harness", test))]
    {
        let owners = desk.owners();
        if owners
            .surfaces
            .toolrail
            .apply_launch(rail_launch)
            .favorites_staged
        {
            owners.runtime.workspace.session_mut().stage_favorites();
        }
    }
    #[cfg(any(feature = "scenario-harness", test))]
    history(&mut desk.owners().runtime.history, env);
    // Captured drawing scenarios apply here rather than on the first
    // drawn frame: the demo appliers run earlier in that frame and ask
    // whether the inspector is open, so a hook another hook depends on has
    // to be in place before any of them. They live with the fields they
    // set — see `surfaces::drawing_chrome::apply_launch_hooks`.
    #[cfg(any(feature = "quick-range-harness", feature = "drawing-harness", test))]
    crate::surfaces::drawing_chrome::apply_launch_hooks(&mut desk.owners().surfaces.drawing_chrome);
    #[cfg(any(feature = "scenario-harness", test))]
    tape(desk, env);
    #[cfg(any(feature = "scenario-harness", test))]
    indicator(desk.owners().runtime.active_tab_mut(), env);
    // Put the active layout on the first tab's panes before any autostart
    // hook: the file is what the user actually had open.
    desk.layouts().seed_new_panes();
    #[cfg(any(feature = "scenario-harness", test))]
    {
        layout(desk, env);
        let owners = desk.owners();
        replay(owners.surfaces, owners.runtime.active_tab(), env);
        let owners = desk.owners();
        dock_and_report(owners.runtime, owners.surfaces, env);
        calls::on_flow_pane(desk, env, "QUANTICK_PRICE_RANGE", launch_calls::price_range);
        workspace(desk, env);
    }
    // An env var is not a user edit: what the autostart hooks switched on
    // must not be written back as though the user had asked for it every
    // launch from now on. Same rule the indicator state follows.
    let runtime = desk.owners().runtime;
    let staged_layers = runtime
        .active_tab()
        .flow_pane
        .layer_read()
        .mask(&runtime.style);
    runtime.workspace.layers_mut().record(staged_layers);
    #[cfg(any(feature = "scenario-harness", test))]
    toast(desk.owners().runtime, env);
}

/// The hooks with a registered action, run as an agent would through
/// `quantick_control_schema::launch_calls`, which reads each value.
#[cfg(any(feature = "scenario-harness", test))]
mod calls {
    use super::super::{GatewayPort, TabsPort};
    use crate::hooks::ScenarioInputs as Env;
    use quantick_control_schema::launch_calls::{self as launch, LaunchCall};

    fn run<W: GatewayPort + ?Sized>(window: &mut W, call: Option<LaunchCall>) {
        if let Some((id, input)) = call {
            window.run_hook_action(id, input, crate::control::ActionOrigin::LaunchHook);
        }
    }

    /// A hook addressed to the active tab's flow pane, the pane it always set.
    pub(super) fn on_flow_pane<W>(window: &mut W, env: &Env, hook: &str, read: launch::PaneCall)
    where
        W: GatewayPort + TabsPort + ?Sized,
    {
        let tabs = window.tab_reads().tabs();
        let pane = tabs.get(tabs.active_index()).map(|tab| tab.flow_pane.id);
        let (tab, pane) = (tabs.active_id(), pane.unwrap_or_default());
        run(
            window,
            env.var(hook).and_then(|value| read(&value, tab, pane)),
        );
    }

    pub(super) fn layout_tab<W: GatewayPort + ?Sized>(window: &mut W, env: &Env) {
        let name = env.var("QUANTICK_LAYOUT_TAB");
        if let Some(name) = name.and_then(|name| crate::layouts::clean_name(&name)) {
            let known = window.workspace().layout_state().layouts().by_name(&name);
            let known = known.is_some();
            run(window, Some(launch::layout_tab(&name, known)));
        }
    }

    /// A `1` switch for a call that takes no input.
    pub(super) fn when_one<W: GatewayPort + ?Sized>(
        window: &mut W,
        env: &Env,
        hook: &str,
        id: &'static str,
    ) {
        run(window, launch::when_one(env.var(hook).as_deref(), id));
    }
}

/// The order book and the live strip: the two the map opens with.
#[cfg(any(feature = "scenario-harness", test))]
fn book_and_strip(tab: &mut Tab, env: &ScenarioInputs) {
    // Dev/ops can open the map without a click.
    if values::one(env.var("QUANTICK_BOOK_AUTOSTART").as_deref()) {
        tab.tape_mut().set_depth_visible(true);
    }
    // Same convenience for the live strip; its pixels stay
    // capability-gated either way (see live_strip_width).
    if values::one(env.var("QUANTICK_LIVE_STRIP_AUTOSTART").as_deref()) {
        tab.flow_pane.set_layer_visible(
            crate::chart_layers::ChartLayer::LiveStrip,
            true,
            &mut Default::default(),
        );
    }
}

/// How far back a launch reaches, and how it gets there.
#[cfg(any(feature = "scenario-harness", test))]
fn history(history: &mut quantick_session::HistorySettings, env: &ScenarioInputs) {
    if let Some(raw) = env.var("QUANTICK_HISTORY_REACH_SPAN_MINUTES") {
        // Read before `QUANTICK_HISTORY_REACH`: it is what that hook's legacy
        // `span` token means, as it is in a saved workspace.
        match raw.trim().parse::<u32>() {
            Ok(minutes) => history.set_span_minutes(minutes),
            Err(_) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_REACH_SPAN_HOOK_UNREADABLE",
                value = %raw,
                action = "keep_current_span",
                "QUANTICK_HISTORY_REACH_SPAN_MINUTES is not a whole number of minutes"
            ),
        }
    }
    // What the main click loads, as `hours:N` or `sessions:N` (or a token the
    // old reach menu saved). Set explicitly, it overrides the workspace: a
    // validation run must be able to pin what it photographs. An unknown
    // token is refused out loud with the grammar, never silently defaulted.
    if let Some(token) = env.var("QUANTICK_HISTORY_REACH") {
        let span = history.history_reach_span_minutes;
        match history_reach::HistoryReach::from_legacy_span(&token, span) {
            Some(reach) => history.history_reach = reach,
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_REACH_HOOK_UNKNOWN",
                token = %token,
                error = %history_reach::HistoryReach::parse(&token).err().map_or_else(String::new, |error| error.to_string()),
                action = "keep_current_reach",
                "QUANTICK_HISTORY_REACH names no reach this build has"
            ),
        }
    }
    if let Some(value) = env.var("QUANTICK_VENUE_LEAD_IN") {
        // `1` and `0`, and nothing else understood. A typo must not decide
        // a switch the trader set: read as a bare truthiness test, `true`
        // or `on` would silently turn the lead-in *off* and overwrite what
        // the workspace saved, and a capture run would photograph the off
        // state while reporting it as on.
        match values::binary(&value) {
            Some(on) => history.venue_lead_in = on,
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "VENUE_LEAD_IN_HOOK_UNKNOWN",
                value = %value.trim(),
                action = "keep_current_setting",
                "QUANTICK_VENUE_LEAD_IN takes 1 or 0"
            ),
        }
    }
    // Nonsense is refused rather than guessed: a typo leaves the trader's own
    // setting alone instead of silently flipping it.
    if let Some(on) = env
        .var("QUANTICK_PROGRESSIVE_HISTORY")
        .and_then(|v| values::binary(&v))
    {
        history.progressive_history = on;
    }
}

/// The tape and everything drawn on it: the drawing chrome's own five, the
/// aggression layer, the lanes, the window, the footprint and the budgets.
#[cfg(any(feature = "scenario-harness", test))]
fn tape<D: OwnersDesk + GatewayPort + TabsPort>(desk: &mut D, env: &ScenarioInputs) {
    let owners = desk.owners();
    let tab = owners.runtime.active_tab_mut();
    // Same convenience for the aggression layer (bubbles + the live
    // column's footprint), held for the run like the window: never filed.
    if values::one(env.var("QUANTICK_BUBBLES_AUTOSTART").as_deref()) {
        tab.tape_mut()
            .hold_for_run(|held| held.bubbles = Some(true));
    }
    // The chart upside down, through the very setter the axis menu's
    // checkbox calls. The inverted frame is otherwise only reachable by
    // a long axis drag no scripted run can perform. Both panes of a
    // split layout: the hook exists so one capture audits every
    // price-mapped surface at once, and a half-inverted frame would
    // silently audit the time pane the right way up.
    if values::one(env.var("QUANTICK_INVERTED").as_deref().map(str::trim)) {
        for pane in tab.panes_mut() {
            pane.price_view.set_inverted(true);
        }
    }
    calls::on_flow_pane(desk, env, "QUANTICK_TAPE", launch_calls::tape);
    let owners = desk.owners();
    let tab = owners.runtime.active_tab_mut();
    // The tape's own layer switches. The two panes are configured apart and
    // the tape's menu is a right-click a scripted run cannot perform, so the
    // state behind it needs a door of its own — the state, not a second
    // way of drawing it: each entry calls the very setter the menu's
    // checkbox calls. Unlisted layers stay as they were, which is what
    // keeps this hook from being a second opinion about the whole tape. A
    // typo leaves the tape alone rather than guessing at a layer: a capture
    // of the wrong state is worse than a capture of the default one.
    if let Some(value) = env.var("QUANTICK_TAPE_LAYERS") {
        let layers = values::lane_layers(&value);
        let tape = tab.tape_mut();
        if let Some(on) = layers.depth {
            tape.set_lane_depth_visible(on);
        }
        if let Some(on) = layers.bubbles {
            tape.set_lane_bubbles_enabled(on);
        }
    }
    // How much market time the tape shows: `auto` follows the bars, a
    // duration pins it (`90s`, `2min`, `120000ms`, or bare milliseconds).
    // Nonsense is refused rather than guessed at, so a typo photographs
    // the default instead of an invented window. Navigation held for the
    // run, never filed: it reaches the asset a replay autostart switches to.
    if let Some(value) = env.var("QUANTICK_TAPE_WINDOW")
        && let Some(window) = quantick_orderflow::LaneWindow::parse(&value)
    {
        tab.tape_mut()
            .hold_for_run(|held| held.window = Some(window));
    }
    // Same convenience for the candle footprint — the same field the
    // pane's layer menu writes, so a validation run sees exactly what a
    // click would show.
    if owners.chrome.harness.footprint() {
        tab.flow_pane.presentation.footprint.visible = true;
    }
    // Every style by its own id, resolved through the same registry the
    // panel's selector and the TOML read. A style reachable by click but
    // not by name is a style the second operator cannot pick, and one
    // more list to keep in step by hand.
    if let Some(value) = env.var("QUANTICK_FOOTPRINT_STYLE") {
        match crate::footprint_config::FootprintStyle::from_id(value.trim()) {
            Some(style) => owners.runtime.footprint_config.style = style,
            // Named and unknown is a typo in a validation script, and a
            // silent fallback to the default would have it photograph the
            // wrong style and call it a pass.
            None => tracing::warn!(
                requested = %value,
                known = ?crate::footprint_config::FootprintStyle::ALL
                    .map(crate::footprint_config::FootprintStyle::id),
                "QUANTICK_FOOTPRINT_STYLE names no known style; keeping the current one",
            ),
        }
    }
    // The zoom, scriptable: the footprint's detail levels are functions
    // of candle width, and a validation run cannot drag a scroll wheel.
    // Same clamp as the gesture (see Viewport::set_px_per_bar).
    if let Some(px) = owners.chrome.harness.candle_width() {
        owners
            .runtime
            .active_tab_mut()
            .flow_pane
            .model
            .viewport
            .open_at_px_per_bar(px);
    }
    // The bubble budget, scriptable. The fold is the one bubble state a
    // capture cannot otherwise reach: it needs a tape dense enough to
    // exhaust a budget of seven hundred, which is a market condition and
    // not a setting. `QUANTICK_BUBBLE_BUDGET=8` squeezes the same budget
    // the frame always spends, through the same field the projection
    // reads, so what a screenshot shows is what a busy session shows —
    // folded marks wearing their ring and their count.
    if let Some(budget) = env
        .var("QUANTICK_BUBBLE_BUDGET")
        .and_then(|v| values::positive(&v))
    {
        for tab in owners.runtime.tabs.iter_mut() {
            tab.tape_mut().set_primitive_budget(budget);
        }
    }
    // A starved tape, scriptable — the state this whole fix is about. The
    // bubbles trailing the lane's right edge, and past its window leaving
    // it empty, happen when the book keeps arriving and nothing prints. No
    // setting produces that and no capture can wait for the market to do
    // it, so `QUANTICK_TAPE_STARVE_AFTER_MS=8000` stops feeding the tape
    // eight seconds in and lets the book run. Nothing is forged: the
    // prints are withheld through the feed's own call, and the axis then
    // reports the age it actually observes.
    let starve = env.var("QUANTICK_TAPE_STARVE_AFTER_MS");
    if let Some(after_ms) = starve.and_then(|v| values::non_negative_ms(&v)) {
        for tab in owners.runtime.tabs.iter_mut() {
            tab.tape_mut().set_starve_tape_after_ms(after_ms);
        }
    }
}

/// The indicators a launch opens with, and whether their legend is folded.
#[cfg(any(feature = "scenario-harness", test))]
fn indicator(tab: &mut Tab, env: &ScenarioInputs) {
    // Same convenience for indicators: open with the two M1 natives on
    // (EMA overlay + CVD pane), through the same code path the toolbar
    // menu takes, so a scripted validation run needs no clicks.
    if values::one(env.var("QUANTICK_INDICATORS_AUTOSTART").as_deref()) {
        for id in AUTOSTART_NATIVES {
            tab.flow_pane.add_indicator(IndicatorSource::Native {
                id: (*id).to_owned(),
                values: Vec::new(),
            });
        }
    }
    // The folded legend, reachable from a clean launch: without it the
    // collapsed state is un-photographable by an agent, and a surface no
    // harness can reach is a surface no visual QA covers. Goes through
    // `IndicatorState::set_legend_collapsed`, the same call the chevron and the
    // menu entry make — never a field poked from the side.
    if values::one(env.var("QUANTICK_LEGEND_COLLAPSED").as_deref()) {
        let pane = tab.focused_pane_mut();
        super::indicator_manager::IndicatorState::set_legend_collapsed(pane, true);
    }
}

/// The layout strip and the pane layouts. Runs after `seed_new_panes` -- see
/// the module's order rule.
#[cfg(any(feature = "scenario-harness", test))]
fn layout<D: OwnersDesk + GatewayPort + TabsPort>(desk: &mut D, env: &ScenarioInputs) {
    calls::layout_tab(desk, env);
    // One layout per pane, by name, in pane-address order (`flow,top,bottom`):
    // a capture of two charts on two layouts side by side. Names the book
    // lacks are created empty; an empty entry leaves that pane alone.
    if let Some(names) = env.var("QUANTICK_PANE_LAYOUTS") {
        desk.layouts().apply_pane_layouts_hook(&names);
    }
    if values::one(env.var("QUANTICK_LAYOUT_RENAME").as_deref()) {
        let active = desk.owners().runtime.layout_state().focused_pane_layout();
        desk.layouts().begin_layout_rename(active);
    }
    if values::one(env.var("QUANTICK_LAYOUT_DELETE").as_deref()) {
        let active = desk.owners().runtime.layout_state().focused_pane_layout();
        let delete = crate::layout_strip::StripAction::Delete(active);
        desk.layouts().apply_strip_action(delete);
    }
    // Scripted validation runs can open with library scripts loaded:
    // a comma-separated list of script names, each through the same
    // code path the indicator library takes.
    if let Some(names) = env.var("QUANTICK_INDICATOR_SCRIPTS_AUTOSTART") {
        for name in values::names(&names) {
            let runtime = desk.owners().runtime;
            let library = runtime.indicators.script_library.entries();
            if !library.iter().any(|entry| entry.name == name) {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNKNOWN",
                    script = %name,
                    action = "autostart_entry_skipped",
                    "autostart names a script the library does not have"
                );
                continue;
            }
            // Straight onto the focused pane, with no mirror: an env var is
            // not a user edit. Without this, a scripted validation run
            // appended its own scripts to the layout and they opened by
            // themselves on the next plain launch — config presence
            // activating something, which the rules forbid. The natives
            // hook above never registers a kind, so it is already inert.
            let (tab, side) = (
                runtime.tabs.active_id(),
                runtime.active_tab().focused_side(),
            );
            let kind = SavedKind::Script {
                name: name.to_owned(),
            };
            let Some(slot) = desk.layouts().add_indicator_at(tab, side, &kind) else {
                continue;
            };
            let runtime = desk.owners().runtime;
            let pane = runtime.tabs.by_id(tab).expect("launch tab").pane(side).id;
            let workspace = runtime.id();
            let slot = super::TabSlot {
                workspace,
                pane,
                tab,
                side,
                slot,
            };
            runtime.indicators.registry.exclude_from_layout(slot);
        }
    }
}

/// The replay transport. `QUANTICK_REPLAY_DAY_BEFORE` is read first, before
/// anything loads a session -- see the module's order rule.
#[cfg(any(feature = "scenario-harness", test))]
fn replay(surfaces: &mut Surfaces, tab: &Tab, env: &ScenarioInputs) {
    let browser = &mut surfaces.replay_browser;
    // Whether a recording opens with the day before it joined in front.
    // Read before anything loads a session, because that is the frame the
    // setting is consulted on. Staged rather than chosen: a validation run
    // states the screen it wants to photograph, and must not write a QA
    // preference into the trader's workspace — the same rule the replay
    // folder follows.
    if let Some(value) = env.var("QUANTICK_REPLAY_DAY_BEFORE") {
        // Refused rather than guessed, like the autostart hook below it: a
        // typo that quietly meant "off" would photograph a single-day
        // chart under a run that believed it had staged a join, which is
        // the one state this hook exists to reach.
        match values::flag(&value) {
            Some(enabled) => {
                browser.stage_day_before(enabled);
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "REPLAY_DAY_BEFORE_STAGED",
                    enabled,
                    requested = value.trim(),
                    "the day before was staged for this run"
                );
            }
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_DAY_BEFORE_UNREADABLE",
                requested = value.trim(),
                action = "left_as_the_workspace_has_it",
                "the day-before hook takes 0 or 1; this run keeps the trader's own setting"
            ),
        }
    }
    // Same convenience for Market Replay: scan the folder in force — the
    // hook, else the stored pick, else the documents home — and play its
    // first session. The same code path a click takes, so a scripted run
    // and a person get the same behaviour.
    // `1` loads and plays, as it always has. `paused` loads and waits,
    // which is what a person now gets when they open a recording, and a
    // state no other hook can reach.
    if let Some(play) = values::replay_autostart(env.var("QUANTICK_REPLAY_AUTOSTART").as_deref()) {
        let speed = values::replay_speed(env.var("QUANTICK_REPLAY_SPEED").as_deref());
        // Which recording, when the folder holds more than one. The
        // scan lists them oldest first, so without this a folder of days
        // always opens the one that can have nothing joined in front of
        // it — the single state this hook family exists to avoid.
        let day = values::non_blank(env.var("QUANTICK_REPLAY_SESSION").as_deref());
        let started = browser.autostart(speed, day.as_deref(), play);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_AUTOSTART",
            // The folder actually scanned, not the environment variable:
            // once a stored pick can supply it, reading the hook back
            // would report an empty folder for a run that scanned a full
            // one — a log that lies about the input it acted on.
            folder = browser.folder_in_use(),
            speed,
            day = day.as_deref().unwrap_or("(first)"),
            day_before = browser.day_before(),
            play,
            started,
            action = if started { "load_first_session" } else { "open_browser" },
            "market replay autostart"
        );
    }
    // The session list, opened outright. The browser is one menu entry
    // deep and a validation run has no mouse, so without this the half
    // that shows what a trader already has is the one half no capture can
    // reach — and "I could not find my recordings" is a report about that
    // window, not about the list inside it.
    if values::one(env.var("QUANTICK_REPLAY_BROWSER").as_deref()) {
        browser.open_browser();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_BROWSER_AUTOSTART",
            folder = browser.folder_in_use(),
            "opened the session browser"
        );
    }
    // The download half of the same browser. Reached on its own because a
    // scripted run has to photograph the Get data tab without a click, and
    // it is a different screen from the session list beside it. Takes the
    // same path the tab click takes — including, for a bare `1`, the
    // chart's own instrument, because that is what clicking the tab now
    // fills the field with and a hook that opened it emptier than a click
    // would photograph a screen no person ever sees.
    if let Some(value) = env.var(crate::replay_view::GET_DATA_ENV) {
        let symbol = match value.trim() {
            "1" | "" => tab.symbol.clone(),
            symbol => symbol.to_string(),
        };
        browser.open_get_data(Some(&symbol));
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_GET_DATA_AUTOSTART",
            symbol = symbol.as_str(),
            "opened the replay download tab"
        );
    }
}

/// The dock and the paper report: the named tab, the calendar, the ledger
/// and the report list.
#[cfg(any(feature = "scenario-harness", test))]
fn dock_and_report(runtime: &mut WorkspaceRuntime, surfaces: &mut Surfaces, env: &ScenarioInputs) {
    let tz = runtime.tz;
    let tab = runtime.active_tab_mut();
    // Same convenience for the dock: open a named tab, so a scripted
    // validation run shows a panel without a click.
    if let Some(name) = env.var("QUANTICK_DOCK_TAB") {
        // Read by the owners' own wire names, the words a scene reports.
        let id = name.trim();
        let section = PanelSection::ALL.into_iter().find(|at| at.id() == id);
        let dock_tab = DockTab::ALL.into_iter().find(|at| at.id() == id);
        match (section, dock_tab) {
            (Some(section), _) => open_on(tab, section),
            (None, Some(dock_tab)) => surfaces.dock.open_tab(dock_tab),
            (None, None) => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "DOCK_TAB_AUTOSTART_UNKNOWN",
                tab = %name,
                action = "dock_left_as_is",
                "QUANTICK_DOCK_TAB names no dock tab"
            ),
        }
    }
    let account = tab.paper.account_mut();
    // And for the performance report window — the Report… button's own
    // path, so a scripted run can show it.
    if values::one(env.var("QUANTICK_PAPER_REPORT_AUTOSTART").as_deref()) {
        account.autostart_report();
    }
    if let Some(value) = env.var("QUANTICK_PAPER_REPORT_SCROLL") {
        account.report.arm_scroll_probe(&value);
    }
    // The calendar the report grew: reachable open, on a chosen day or
    // a chosen span, with no clicks at all.
    if let Some(spec) = env.var("QUANTICK_PAPER_CALENDAR") {
        match crate::paper_calendar::parse_selection(&spec) {
            Some(selection) => account.autostart_calendar(selection),
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_CALENDAR_AUTOSTART_UNKNOWN",
                spec = %spec,
                action = "calendar_left_closed",
                "QUANTICK_PAPER_CALENDAR is not 1, YYYY-MM-DD or YYYY-MM-DD..YYYY-MM-DD"
            ),
        }
    }
    // Which instrument's saved history the ledger lists.
    if let Some(spec) = env.var("QUANTICK_LEDGER_SCOPE") {
        match crate::paper_trading::LedgerScope::from_hook(&spec) {
            Some(scope) => account.set_ledger_scope(scope),
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LEDGER_SCOPE_AUTOSTART_UNKNOWN",
                scope = %spec,
                action = "ledger_left_on_the_chart",
                "QUANTICK_LEDGER_SCOPE wants `chart`, `all`, or a symbol folder name"
            ),
        }
    }
    // And the ledger past its first page of saved history — a state
    // only a click on "show older" otherwise reaches.
    if let Some(text) = env.var("QUANTICK_LEDGER_PAGES") {
        match values::positive(&text) {
            Some(pages) => account.autostart_ledger_pages(pages),
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LEDGER_PAGES_AUTOSTART_UNKNOWN",
                pages = %text,
                action = "ledger_left_on_its_first_page",
                "QUANTICK_LEDGER_PAGES wants a whole number of pages, one or more"
            ),
        }
    }
    // Every day folded shut — the one-line-per-day read, which is
    // otherwise a click on each header.
    if values::one(env.var("QUANTICK_LEDGER_FOLD").as_deref()) {
        account.autostart_folded_days(tz);
    }
    // The report's trade list is open by default, so the hook is how a
    // capture reaches it collapsed.
    if let Some(value) = env.var("QUANTICK_PAPER_REPORT_LIST") {
        account.set_report_list_open(value.trim() != "0");
    }
}

/// The canvas layout, the workspace file, and the toast that says a launch
/// rescued something.
#[cfg(any(feature = "scenario-harness", test))]
fn workspace<D: OwnersDesk + GatewayPort + TabsPort>(desk: &mut D, env: &ScenarioInputs) {
    let owners = desk.owners();
    let tab_id = owners.runtime.tabs.active_id();
    // The window menu hung from the toolbar's list door, as its click does.
    if let Some(value) = env.var("QUANTICK_WORKSPACE_MENU")
        && !owners
            .chrome
            .window_menu
            .tab_workspace
            .stage(tab_id, &value)
    {
        tracing::warn!(target: "quantick::app", value = %value, "QUANTICK_WORKSPACE_MENU names no workspace surface");
    }
    // Open on a named canvas layout, through the same path the View menu
    // takes. An env var is an explicit request for this run, so it wins
    // over a feed's declared `default_layout`.
    if let Some(name) = env.var("QUANTICK_LAYOUT") {
        match crate::tab::layout::named(&name) {
            Some(layout) => owners.runtime.active_tab_mut().set_layout(layout),
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUT_AUTOSTART_UNKNOWN",
                layout = %name,
                action = "layout_left_as_is",
                accepted = %crate::canvas_layout::LAYOUT_PRESETS
                    .iter()
                    .map(|preset| preset.id)
                    .collect::<Vec<_>>()
                    .join(", "),
                // Built from the registry rather than spelled out: a
                // hand-written list goes stale the day a preset is added,
                // and a run that mistypes an id deserves the real one.
                "QUANTICK_LAYOUT names no canvas layout"
            ),
        }
    }
    let collapse = layout::COLLAPSE_CAPABILITY_ID;
    calls::when_one(desk, env, "QUANTICK_PANE_COLLAPSED", collapse);
    let flow = layout::FLOW_COLLAPSE_CAPABILITY_ID;
    calls::when_one(desk, env, "QUANTICK_FLOW_PANE_COLLAPSED", flow);
    let tab = desk.owners().runtime.active_tab_mut();
    let upper = env.var("QUANTICK_CONTEXT_UPPER_COLLAPSED");
    let lower = env.var("QUANTICK_CONTEXT_LOWER_COLLAPSED");
    if tab.canvas.layout == CanvasLayout::TimeTimeAndFlow
        && let Some(upper) = values::context_fold(upper.as_deref(), lower.as_deref())
    {
        tab.canvas.fold_context_pane(upper);
    }
    let save = desk::DESK_SAVE_CAPABILITY_ID;
    calls::when_one(desk, env, "QUANTICK_WORKSPACE_SAVE", save);
    // A chart's ⋯ menu open, or one of its rows pressed, as a click does.
    let menu = env.var("QUANTICK_PANE_MENU");
    if let Some((slot, row)) = menu
        .as_deref()
        .and_then(quantick_workspace::canvas::menu_hook)
    {
        use quantick_ui_model::remove_chart::Intent;
        let intent = row.map_or(Intent::SetMenuOpen(Some(slot)), |row| {
            Intent::Pick(slot, row)
        });
        let owners = desk.owners();
        let tab_id = owners.runtime.tabs.active_id();
        owners.surfaces.remove_chart.update_scoped(tab_id, intent);
    }
    // The three file entries, reachable with no click for the same reason
    // (`.claude/skills/ui-harness`). Each runs the menu entry's own code
    // past the OS dialog — the dialog is the one thing a scripted run
    // cannot drive, so the path is given instead of picked. They really
    // write and really replace the cockpit, so point `QUANTICK_UI_STATE`
    // and its sibling stores at scratchpad files first.
    if let Some(path) = env.var("QUANTICK_WORKSPACE_EXPORT") {
        desk.workspace_file()
            .export_workspace_to(std::path::Path::new(&path));
    }
    if let Some(path) = env.var("QUANTICK_WORKSPACE_IMPORT") {
        desk.workspace_file()
            .import_workspace_from(std::path::Path::new(&path));
    }
}

/// The toast a launch raises through the simulator's own route.
#[cfg(any(feature = "scenario-harness", test))]
fn toast(runtime: &mut WorkspaceRuntime, env: &ScenarioInputs) {
    // `QUANTICK_TOAST=paper`: a simulator acknowledgement, posted through
    // the panel's own `show_toast`.
    //
    // The surface's own hook can raise a message *in* the lane; only this
    // one proves the route to it, which is the half this change built —
    // the panel's outbox, the drain in `settle_paper_panels`, and the
    // eight-second clock the surface owns. Without it the paper path is
    // reachable from a launch only by waiting for a fill and hoping the
    // shutter lands inside the window: the demo trades within the first
    // second and the message is gone eight seconds later, so a capture
    // run photographs an empty lane and cannot tell that from a defect.
    if env.var("QUANTICK_TOAST").as_deref() == Some("paper") {
        let tabs = &mut runtime.tabs;
        let toast = "SIM: stop filled at 169 790 — flat.".to_owned();
        tabs.runtime_mut(0).paper.show_toast(toast);
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_BOOK_AUTOSTART",
    "QUANTICK_BUBBLES_AUTOSTART",
    "QUANTICK_BUBBLE_BUDGET",
    "QUANTICK_DOCK_TAB",
    "QUANTICK_CONTEXT_LOWER_COLLAPSED",
    "QUANTICK_CONTEXT_UPPER_COLLAPSED",
    "QUANTICK_FOOTPRINT_STYLE",
    "QUANTICK_HISTORY_REACH",
    "QUANTICK_HISTORY_REACH_SPAN_MINUTES",
    "QUANTICK_INDICATORS_AUTOSTART",
    "QUANTICK_INDICATOR_SCRIPTS_AUTOSTART",
    "QUANTICK_INVERTED",
    "QUANTICK_LAYOUT",
    "QUANTICK_PANE_MENU",
    "QUANTICK_LAYOUT_DELETE",
    "QUANTICK_LAYOUT_RENAME",
    "QUANTICK_LAYOUT_TAB",
    "QUANTICK_LEDGER_FOLD",
    "QUANTICK_LEDGER_PAGES",
    "QUANTICK_LEDGER_SCOPE",
    "QUANTICK_LEGEND_COLLAPSED",
    "QUANTICK_LIVE_STRIP_AUTOSTART",
    "QUANTICK_PANE_LAYOUTS",
    "QUANTICK_PANE_COLLAPSED",
    "QUANTICK_FLOW_PANE_COLLAPSED",
    "QUANTICK_PAPER_CALENDAR",
    "QUANTICK_PAPER_REPORT_AUTOSTART",
    "QUANTICK_PAPER_REPORT_LIST",
    "QUANTICK_PAPER_REPORT_SCROLL",
    "QUANTICK_PRICE_RANGE",
    "QUANTICK_PROGRESSIVE_HISTORY",
    "QUANTICK_REPLAY_AUTOSTART",
    "QUANTICK_REPLAY_BROWSER",
    "QUANTICK_REPLAY_DAY_BEFORE",
    "QUANTICK_REPLAY_SESSION",
    "QUANTICK_REPLAY_SPEED",
    "QUANTICK_TAPE",
    "QUANTICK_TAPE_LAYERS",
    "QUANTICK_TAPE_STARVE_AFTER_MS",
    "QUANTICK_TAPE_WINDOW",
    "QUANTICK_VENUE_LEAD_IN",
    "QUANTICK_WORKSPACE_EXPORT",
    "QUANTICK_WORKSPACE_IMPORT",
    "QUANTICK_WORKSPACE_MENU",
    "QUANTICK_WORKSPACE_SAVE"
];
