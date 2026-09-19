//! The launch phase: what runs after workspace restoration and before the
//! first frame, and the scenario appliers that stage a capture's state in it.
//!
//! Not a method on the app: the composition root captures every launch input
//! in `main` and hands them to the constructor, which calls
//! [`apply_launch_phase`] once. Two production steps live here because the
//! scenarios are ordered around them — `seed_new_panes` and recording the
//! layers the launch staged. Every applier that reads a `QUANTICK_*` compiles
//! only with its harness family (or under test), from inputs the root
//! captured ([`ScenarioInputs`]); a default build runs the two steps alone.
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
use crate::tab::CanvasLayout;
#[cfg(any(feature = "scenario-harness", test))]
use quantick_feed::history_reach;

use super::LayoutPort;
use super::QuantickApp;
#[cfg(any(feature = "scenario-harness", test))]
use super::{AUTOSTART_NATIVES, parse_tape_window};

/// Run the launch phase over a constructed app, in the order this module
/// fixes. Called once, from `new_with_workspace`, after the saved workspace
/// is restored.
pub(super) fn apply_launch_phase(
    app: &mut QuantickApp,
    #[cfg(any(feature = "scenario-harness", test))] env: &ScenarioInputs,
    #[cfg(any(feature = "control-harness", test))]
    control_launch: super::control_host::ControlLaunch,
    #[cfg(any(feature = "drawing-harness", test))] rail_launch: crate::toolrail::ToolRailLaunch,
) {
    #[cfg(any(feature = "scenario-harness", test))]
    book_and_strip(app, env);
    #[cfg(any(feature = "control-harness", test))]
    app.control.apply_launch(control_launch);
    #[cfg(any(feature = "drawing-harness", test))]
    if app.toolrail.apply_launch(rail_launch).favorites_staged {
        app.workspace.session_mut().stage_favorites();
    }
    #[cfg(any(feature = "scenario-harness", test))]
    history(app, env);
    // Captured drawing scenarios apply here rather than on the first
    // drawn frame: the demo appliers run earlier in that frame and ask
    // whether the inspector is open, so a hook another hook depends on has
    // to be in place before any of them. They live with the fields they
    // set — see `surfaces::drawing_chrome::apply_launch_hooks`.
    #[cfg(any(feature = "quick-range-harness", feature = "drawing-harness", test))]
    crate::surfaces::drawing_chrome::apply_launch_hooks(&mut app.drawings.chrome);
    #[cfg(any(feature = "scenario-harness", test))]
    tape(app, env);
    #[cfg(any(feature = "scenario-harness", test))]
    indicator(app, env);
    // Put the active layout on the first tab's panes before any autostart
    // hook: the file is what the user actually had open.
    app.layout_adapter().seed_new_panes();
    #[cfg(any(feature = "scenario-harness", test))]
    {
        layout(app, env);
        replay(app, env);
        dock_and_report(app, env);
        workspace(app, env);
    }
    // An env var is not a user edit: what the autostart hooks switched on
    // must not be written back as though the user had asked for it every
    // launch from now on. Same rule the indicator state follows.
    let staged_layers = app.active_tab().flow_pane.layer_mask(&app.style);
    app.workspace.layers_mut().record(staged_layers);
    #[cfg(any(feature = "scenario-harness", test))]
    toast(app, env);
}

/// The order book and the live strip: the two the map opens with.
#[cfg(any(feature = "scenario-harness", test))]
fn book_and_strip(app: &mut QuantickApp, env: &ScenarioInputs) {
    // Dev/ops can open the map without a click.
    if env
        .var("QUANTICK_BOOK_AUTOSTART")
        .is_some_and(|value| value == "1")
    {
        app.active_tab_mut().tape_mut().set_depth_visible(true);
    }
    // Same convenience for the live strip; its pixels stay
    // capability-gated either way (see live_strip_width).
    if env
        .var("QUANTICK_LIVE_STRIP_AUTOSTART")
        .is_some_and(|value| value == "1")
    {
        app.active_tab_mut().flow_pane.set_layer_visible(
            crate::chart_layers::ChartLayer::LiveStrip,
            true,
            &mut Default::default(),
        );
    }
}

/// How far back a launch reaches, and how it gets there.
#[cfg(any(feature = "scenario-harness", test))]
fn history(app: &mut QuantickApp, env: &ScenarioInputs) {
    // The switch itself, so both sides of it are reachable without a
    // click. Set explicitly, it also overrides what the workspace saved:
    // a validation run must be able to pin the state it is photographing.
    // The same registry the menu lists from, so a hook can reach every
    // reach the trader can — and an unknown token is refused out loud
    // rather than silently leaving the default in place, which would look
    // like a press that ignored the run it was told to make.
    if let Some(token) = env.var("QUANTICK_HISTORY_REACH") {
        match history_reach::HistoryReach::from_token(&token) {
            Some(reach) => app.history.set_reach(reach),
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HISTORY_REACH_HOOK_UNKNOWN",
                token = %token,
                action = "keep_current_reach",
                "QUANTICK_HISTORY_REACH names no reach this build has"
            ),
        }
    }
    if let Some(raw) = env.var("QUANTICK_HISTORY_REACH_SPAN_MINUTES") {
        // Beside `QUANTICK_HISTORY_REACH`, because the reach and how far it
        // goes are one choice: a hook that could pick `by time` but not say
        // how much time would leave the operator setting half of it.
        match raw.trim().parse::<u32>() {
            Ok(minutes) => app.history.set_span_minutes(minutes),
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
    if let Some(value) = env.var("QUANTICK_VENUE_LEAD_IN") {
        // `1` and `0`, and nothing else understood. A typo must not decide
        // a switch the trader set: read as a bare truthiness test, `true`
        // or `on` would silently turn the lead-in *off* and overwrite what
        // the workspace saved, and a capture run would photograph the off
        // state while reporting it as on.
        match value.trim() {
            "1" => app.history.venue_lead_in = true,
            "0" => app.history.venue_lead_in = false,
            other => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "VENUE_LEAD_IN_HOOK_UNKNOWN",
                value = %other,
                action = "keep_current_setting",
                "QUANTICK_VENUE_LEAD_IN takes 1 or 0"
            ),
        }
    }
    if let Some(value) = env.var("QUANTICK_PROGRESSIVE_HISTORY") {
        match value.trim() {
            "1" => app.history.progressive_history = true,
            "0" => app.history.progressive_history = false,
            // Nonsense is refused rather than guessed: a typo leaves the
            // trader's own setting alone instead of silently flipping it.
            _ => {}
        }
    }
}

/// The tape and everything drawn on it: the drawing chrome's own five, the
/// aggression layer, the lanes, the window, the footprint and the budgets.
#[cfg(any(feature = "scenario-harness", test))]
fn tape(app: &mut QuantickApp, env: &ScenarioInputs) {
    // Same convenience for the aggression layer (bubbles + the live
    // column's footprint). Same code path as the toolbar toggle.
    if env
        .var("QUANTICK_BUBBLES_AUTOSTART")
        .is_some_and(|value| value == "1")
    {
        app.active_tab_mut().tape_mut().set_bubbles_enabled(true);
    }
    // The chart upside down, through the very setter the axis menu's
    // checkbox calls. The inverted frame is otherwise only reachable by
    // a long axis drag no scripted run can perform. Both panes of a
    // split layout: the hook exists so one capture audits every
    // price-mapped surface at once, and a half-inverted frame would
    // silently audit the time pane the right way up.
    if env
        .var("QUANTICK_INVERTED")
        .is_some_and(|value| value.trim() == "1")
    {
        let tab = app.active_tab_mut();
        for pane in tab.panes_mut() {
            pane.price_view.set_inverted(true);
        }
    }
    // The tape switch in the canvas's top-right corner — the one control
    // that decides whether there is a band at all. Same setter the chip
    // calls, so a capture shows what a click shows. Anything but `on`/`off`
    // leaves the tape alone rather than guessing.
    if let Some(value) = env.var("QUANTICK_TAPE") {
        match value.trim() {
            "on" => app.active_tab_mut().tape_mut().set_lane_enabled(true),
            "off" => app.active_tab_mut().tape_mut().set_lane_enabled(false),
            _ => {}
        }
    }
    // The tape's own layer switches. The two panes are configured apart and
    // the tape's menu is a right-click a scripted run cannot perform, so the
    // state behind it needs a door of its own — the state, not a second
    // way of drawing it: each entry calls the very setter the menu's
    // checkbox calls. Unlisted layers stay as they were, which is what
    // keeps this hook from being a second opinion about the whole tape.
    if let Some(value) = env.var("QUANTICK_TAPE_LAYERS") {
        let wanted: Vec<&str> = value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect();
        let tape = app.active_tab_mut().tape_mut();
        if wanted.contains(&"none") {
            tape.set_lane_depth_visible(false);
            tape.set_lane_bubbles_enabled(false);
        } else {
            for entry in wanted {
                match entry {
                    "heatmap" => tape.set_lane_depth_visible(true),
                    "bubbles" => tape.set_lane_bubbles_enabled(true),
                    "no-heatmap" => tape.set_lane_depth_visible(false),
                    "no-bubbles" => tape.set_lane_bubbles_enabled(false),
                    // A typo leaves the tape alone rather than guessing at
                    // a layer: a capture of the wrong state is worse than
                    // a capture of the default one.
                    _ => {}
                }
            }
        }
    }
    // How much market time the tape shows: `auto` follows the bars, a
    // duration pins it (`90s`, `2min`, `120000ms`, or bare milliseconds).
    // Nonsense is refused rather than guessed at, so a typo photographs
    // the default instead of an invented window.
    if let Some(value) = env.var("QUANTICK_TAPE_WINDOW")
        && let Some(window) = parse_tape_window(value.trim())
    {
        app.active_tab_mut().tape_mut().set_live_lane_window(window);
    }
    // Same convenience for the candle footprint — the same field the
    // pane's layer menu writes, so a validation run sees exactly what a
    // click would show.
    if app.chrome.harness.footprint() {
        app.active_tab_mut().flow_pane.footprint.visible = true;
    }
    // Every style by its own id, resolved through the same registry the
    // panel's selector and the TOML read. A style reachable by click but
    // not by name is a style the second operator cannot pick, and one
    // more list to keep in step by hand.
    if let Some(value) = env.var("QUANTICK_FOOTPRINT_STYLE") {
        match crate::footprint_config::FootprintStyle::from_id(value.trim()) {
            Some(style) => app.footprint_config.style = style,
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
    if let Some(px) = app.chrome.harness.candle_width() {
        app.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
    }
    // The bubble budget, scriptable. The fold is the one bubble state a
    // capture cannot otherwise reach: it needs a tape dense enough to
    // exhaust a budget of seven hundred, which is a market condition and
    // not a setting. `QUANTICK_BUBBLE_BUDGET=8` squeezes the same budget
    // the frame always spends, through the same field the projection
    // reads, so what a screenshot shows is what a busy session shows —
    // folded marks wearing their ring and their count.
    if let Some(value) = env.var("QUANTICK_BUBBLE_BUDGET")
        && let Ok(budget) = value.trim().parse::<usize>()
        && budget > 0
    {
        for tab in app.tabs.iter_mut() {
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
    if let Some(value) = env.var("QUANTICK_TAPE_STARVE_AFTER_MS")
        && let Ok(after_ms) = value.trim().parse::<i64>()
        && after_ms >= 0
    {
        for tab in app.tabs.iter_mut() {
            tab.tape_mut().set_starve_tape_after_ms(after_ms);
        }
    }
}

/// The indicators a launch opens with, and whether their legend is folded.
#[cfg(any(feature = "scenario-harness", test))]
fn indicator(app: &mut QuantickApp, env: &ScenarioInputs) {
    // Same convenience for indicators: open with the two M1 natives on
    // (EMA overlay + CVD pane), through the same code path the toolbar
    // menu takes, so a scripted validation run needs no clicks.
    if env
        .var("QUANTICK_INDICATORS_AUTOSTART")
        .is_some_and(|value| value == "1")
    {
        let pane = &mut app.active_tab_mut().flow_pane;
        for id in AUTOSTART_NATIVES {
            pane.add_indicator(IndicatorSource::Native {
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
    if env
        .var("QUANTICK_LEGEND_COLLAPSED")
        .is_some_and(|value| value == "1")
    {
        super::indicator_manager::IndicatorState::set_legend_collapsed(
            app.focused_pane_mut(),
            true,
        );
    }
}

/// The layout strip and the pane layouts. Runs after `seed_new_panes` -- see
/// the module's order rule.
#[cfg(any(feature = "scenario-harness", test))]
fn layout(app: &mut QuantickApp, env: &ScenarioInputs) {
    // The layout strip's hooks (`ui-harness`): open on a named layout,
    // creating it when the file has none by that name, and open the
    // rename box on the active one.
    if let Some(name) = env.var("QUANTICK_LAYOUT_TAB")
        && let Some(name) = crate::layouts::clean_name(&name)
    {
        let wanted = app
            .layout_state()
            .layouts()
            .by_name(&name)
            .map(|layout| layout.id);
        let outcome = match wanted {
            Some(id) => app.layout_adapter().switch_layout(id).map(|_| id),
            None => app.layout_adapter().create_layout(Some(&name)),
        };
        if let Err(error) = outcome {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "LAYOUT_TAB_HOOK_REFUSED",
                layout = %name,
                %error,
                action = "hook_ignored",
                "QUANTICK_LAYOUT_TAB could not open the layout"
            );
        }
    }
    // One layout per pane, by name, in pane-address order (`flow,top,bottom`):
    // a capture of two charts on two layouts side by side. Names the book
    // lacks are created empty; an empty entry leaves that pane alone.
    if let Some(names) = env.var("QUANTICK_PANE_LAYOUTS") {
        app.layout_adapter().apply_pane_layouts_hook(&names);
    }
    if env
        .var("QUANTICK_LAYOUT_RENAME")
        .is_some_and(|value| value == "1")
    {
        let active = app.layout_state().focused_pane_layout();
        app.layout_adapter().begin_layout_rename(active);
    }
    if env
        .var("QUANTICK_LAYOUT_DELETE")
        .is_some_and(|value| value == "1")
    {
        let active = app.layout_state().focused_pane_layout();
        app.layout_adapter()
            .apply_strip_action(crate::layout_strip::StripAction::Delete(active));
    }
    // Scripted validation runs can open with library scripts loaded:
    // a comma-separated list of script names, each through the same
    // code path the INDICATORS menu takes.
    if let Some(names) = env.var("QUANTICK_INDICATOR_SCRIPTS_AUTOSTART") {
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            match app
                .indicators
                .script_library
                .entries()
                .iter()
                .position(|entry| entry.name == name)
            {
                Some(_) => {
                    // Straight onto the focused pane, with no mirror: an
                    // env var is not a user edit. Without this, a scripted
                    // validation run appended its own scripts to the
                    // layout and they opened by themselves on the next
                    // plain launch — config presence activating
                    // something, which the rules forbid. The natives hook
                    // above never registers a kind, so it is already inert.
                    let (tab, side) = {
                        let tab_id = app.tabs.active_id();
                        let tab = app.active_tab();
                        (tab_id, tab.focused_side())
                    };
                    app.layout_adapter().add_indicator_at(
                        tab,
                        side,
                        &SavedKind::Script {
                            name: name.to_owned(),
                        },
                    );
                    app.indicators.forget_last_indicator_state_change();
                }
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_SCRIPT_UNKNOWN",
                    script = %name,
                    action = "autostart_entry_skipped",
                    "autostart names a script the library does not have"
                ),
            }
        }
    }
}

/// The replay transport. `QUANTICK_REPLAY_DAY_BEFORE` is read first, before
/// anything loads a session -- see the module's order rule.
#[cfg(any(feature = "scenario-harness", test))]
fn replay(app: &mut QuantickApp, env: &ScenarioInputs) {
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
        let staged = match value.trim() {
            "1" | "true" | "on" => Some(true),
            "0" | "false" | "off" => Some(false),
            _ => None,
        };
        match staged {
            Some(enabled) => {
                app.replay_view.stage_day_before(enabled);
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
    let autostart_play = match env
        .var("QUANTICK_REPLAY_AUTOSTART")
        .unwrap_or_default()
        .trim()
    {
        "1" => Some(true),
        "paused" => Some(false),
        _ => None,
    };
    if let Some(play) = autostart_play {
        let speed = env
            .var("QUANTICK_REPLAY_SPEED")
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|speed| *speed > 0.0)
            .unwrap_or(1.0);
        // Which recording, when the folder holds more than one. The
        // scan lists them oldest first, so without this a folder of days
        // always opens the one that can have nothing joined in front of
        // it — the single state this hook family exists to avoid.
        let day = env
            .var("QUANTICK_REPLAY_SESSION")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let started = app.replay_view.autostart(speed, day.as_deref(), play);
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_AUTOSTART",
            // The folder actually scanned, not the environment variable:
            // once a stored pick can supply it, reading the hook back
            // would report an empty folder for a run that scanned a full
            // one — a log that lies about the input it acted on.
            folder = app.replay_view.folder_in_use(),
            speed,
            day = day.as_deref().unwrap_or("(first)"),
            day_before = app.replay_view.day_before(),
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
    if env
        .var("QUANTICK_REPLAY_BROWSER")
        .is_some_and(|value| value == "1")
    {
        app.replay_view.open_browser();
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_BROWSER_AUTOSTART",
            folder = app.replay_view.folder_in_use(),
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
            "1" | "" => Some(app.active_tab().symbol.clone()),
            symbol => Some(symbol.to_string()),
        };
        app.replay_view.open_get_data(symbol.as_deref());
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "REPLAY_GET_DATA_AUTOSTART",
            symbol = symbol.as_deref().unwrap_or(""),
            "opened the replay download tab"
        );
    }
}

/// The dock and the paper report: the named tab, the calendar, the ledger
/// and the report list.
#[cfg(any(feature = "scenario-harness", test))]
fn dock_and_report(app: &mut QuantickApp, env: &ScenarioInputs) {
    // Same convenience for the dock: open a named tab, so a scripted
    // validation run shows a panel without a click.
    if let Some(name) = env.var("QUANTICK_DOCK_TAB") {
        let tab = match name.trim() {
            "l2" => Some(DockTab::L2),
            "bubbles" => Some(DockTab::Bubbles),
            "session" => Some(DockTab::Session),
            "trading" => Some(DockTab::Trading),
            "trades" => Some(DockTab::Trades),
            _ => None,
        };
        match tab {
            Some(tab) => app.dock.open_tab(tab),
            None => tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "DOCK_TAB_AUTOSTART_UNKNOWN",
                tab = %name,
                action = "dock_left_as_is",
                "QUANTICK_DOCK_TAB names no dock tab"
            ),
        }
    }
    // And for the performance report window — the Report… button's own
    // path, so a scripted run can show it.
    if env
        .var("QUANTICK_PAPER_REPORT_AUTOSTART")
        .is_some_and(|value| value == "1")
    {
        app.active_tab_mut().paper.account_mut().autostart_report();
    }
    // The calendar the report grew: reachable open, on a chosen day or
    // a chosen span, with no clicks at all.
    if let Some(spec) = env.var("QUANTICK_PAPER_CALENDAR") {
        match crate::paper_calendar::parse_selection(&spec) {
            Some(selection) => app
                .active_tab_mut()
                .paper
                .account_mut()
                .autostart_calendar(selection),
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
        let scope = match spec.trim() {
            "chart" => Some(crate::paper_trading::LedgerScope::Chart),
            "all" => Some(crate::paper_trading::LedgerScope::All),
            "" => None,
            symbol => Some(crate::paper_trading::LedgerScope::Symbol(symbol.to_owned())),
        };
        match scope {
            Some(scope) => app
                .active_tab_mut()
                .paper
                .account_mut()
                .set_ledger_scope(scope),
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
        match text.trim().parse::<usize>() {
            Ok(pages) if pages >= 1 => {
                app.active_tab_mut()
                    .paper
                    .account_mut()
                    .autostart_ledger_pages(pages);
            }
            _ => tracing::warn!(
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
    if env
        .var("QUANTICK_LEDGER_FOLD")
        .is_some_and(|value| value == "1")
    {
        let tz = app.tz;
        app.active_tab_mut()
            .paper
            .account_mut()
            .autostart_folded_days(tz);
    }
    // The report's trade list is open by default, so the hook is how a
    // capture reaches it collapsed.
    if let Some(value) = env.var("QUANTICK_PAPER_REPORT_LIST") {
        app.active_tab_mut()
            .paper
            .account_mut()
            .set_report_list_open(value.trim() != "0");
    }
}

/// The canvas layout, the workspace file, and the toast that says a launch
/// rescued something.
#[cfg(any(feature = "scenario-harness", test))]
fn workspace(app: &mut QuantickApp, env: &ScenarioInputs) {
    // Open on a named canvas layout, through the same path the View menu
    // takes. An env var is an explicit request for this run, so it wins
    // over a feed's declared `default_layout`.
    if let Some(name) = env.var("QUANTICK_LAYOUT") {
        let layout = crate::config::DeclaredLayout::parse(&name).map(CanvasLayout::from);
        match layout {
            Some(layout) => app.active_tab_mut().set_layout(layout),
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
    // The Workspace menu's own path, so a validation run can see the save
    // confirmation without a click. A menu entry cannot be reached by an
    // env var, but the state it produces has to be
    // (`.claude/skills/ui-harness`). This writes the file for real,
    // exactly as the entry does — a hook that fakes its surface proves
    // nothing — so point `QUANTICK_UI_STATE` at a scratchpad first.
    if env
        .var("QUANTICK_WORKSPACE_SAVE")
        .is_some_and(|value| value == "1")
    {
        app.workspace_save_adapter().save_workspace("autostart");
    }
    // The three file entries, reachable with no click for the same reason
    // (`.claude/skills/ui-harness`). Each runs the menu entry's own code
    // past the OS dialog — the dialog is the one thing a scripted run
    // cannot drive, so the path is given instead of picked. They really
    // write and really replace the cockpit, so point `QUANTICK_UI_STATE`
    // and its sibling stores at scratchpad files first.
    if let Some(path) = env.var("QUANTICK_WORKSPACE_EXPORT") {
        app.workspace_bundle_adapter()
            .export_workspace_to(std::path::Path::new(&path));
    }
    if let Some(path) = env.var("QUANTICK_WORKSPACE_IMPORT") {
        app.workspace_bundle_adapter()
            .import_workspace_from(std::path::Path::new(&path));
    }
}

/// The toast a launch raises through the simulator's own route.
#[cfg(any(feature = "scenario-harness", test))]
fn toast(app: &mut QuantickApp, env: &ScenarioInputs) {
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
    if env
        .var("QUANTICK_TOAST")
        .is_some_and(|value| value == "paper")
    {
        app.tabs
            .runtime_mut(0)
            .paper
            .show_toast("SIM: stop filled at 169 790 — flat.".to_owned());
    }
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks![
    "QUANTICK_BOOK_AUTOSTART",
    "QUANTICK_BUBBLES_AUTOSTART",
    "QUANTICK_BUBBLE_BUDGET",
    "QUANTICK_DOCK_TAB",
    "QUANTICK_FOOTPRINT_STYLE",
    "QUANTICK_HISTORY_REACH",
    "QUANTICK_HISTORY_REACH_SPAN_MINUTES",
    "QUANTICK_INDICATORS_AUTOSTART",
    "QUANTICK_INDICATOR_SCRIPTS_AUTOSTART",
    "QUANTICK_INVERTED",
    "QUANTICK_LAYOUT",
    "QUANTICK_LAYOUT_DELETE",
    "QUANTICK_LAYOUT_RENAME",
    "QUANTICK_LAYOUT_TAB",
    "QUANTICK_LEDGER_FOLD",
    "QUANTICK_LEDGER_PAGES",
    "QUANTICK_LEDGER_SCOPE",
    "QUANTICK_LEGEND_COLLAPSED",
    "QUANTICK_LIVE_STRIP_AUTOSTART",
    "QUANTICK_PANE_LAYOUTS",
    "QUANTICK_PAPER_CALENDAR",
    "QUANTICK_PAPER_REPORT_AUTOSTART",
    "QUANTICK_PAPER_REPORT_LIST",
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
    "QUANTICK_WORKSPACE_SAVE"
];
