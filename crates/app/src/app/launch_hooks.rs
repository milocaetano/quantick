//! Binds the headless launch scenario to the window between restoration and its first frame.
use super::owners_desk::OwnersDesk;
use super::{GatewayPort, TabsPort};
#[cfg(any(feature = "drawing-harness", test))]
use crate::toolrail::ToolRailExt as _;
#[cfg(any(feature = "scenario-harness", test))]
mod native;

pub(super) fn apply_launch_phase<D: OwnersDesk + GatewayPort + TabsPort>(
    desk: &mut D,
    #[cfg(any(feature = "scenario-harness", test))] env: &crate::hooks::ScenarioInputs,
    #[cfg(any(feature = "control-harness", test))]
    control_launch: super::control_host::ControlLaunch,
    #[cfg(any(feature = "drawing-harness", test))] rail_launch: crate::toolrail::ToolRailLaunch,
) {
    #[cfg(any(feature = "scenario-harness", test))]
    let lookup = |name: &str| env.var(name);
    #[cfg(any(feature = "scenario-harness", test))]
    let scenario = quantick_ui_model::launch_scenario::Scenario(&lookup);
    #[cfg(any(feature = "scenario-harness", test))]
    scenario.book_and_strip(&mut native::Desk(desk));
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
    scenario.history(&mut desk.owners().runtime.history);
    // Captured drawing scenarios apply here rather than on the first
    // drawn frame: the demo appliers run earlier in that frame and ask
    // whether the inspector is open, so a hook another hook depends on has
    // to be in place before any of them. They live with the fields they
    // set — see `surfaces::drawing_chrome::apply_launch_hooks`.
    #[cfg(any(feature = "quick-range-harness", feature = "drawing-harness", test))]
    crate::surfaces::drawing_chrome::apply_launch_hooks(&mut desk.owners().surfaces.drawing_chrome);
    #[cfg(any(feature = "scenario-harness", test))]
    {
        let harness = &desk.owners().chrome.harness;
        let (footprint, width) = (harness.footprint(), harness.candle_width());
        scenario.before_seed(&mut native::Desk(desk), footprint, width);
    }
    // Put the active layout on the first tab's panes before any autostart
    // hook: the file is what the user actually had open.
    desk.layouts().seed_new_panes();
    #[cfg(any(feature = "scenario-harness", test))]
    {
        scenario.layout(&mut native::Desk(desk));
        let owners = desk.owners();
        scenario.replay(
            &mut crate::surfaces::replay_browser::LaunchReplay(&mut owners.surfaces.replay_browser),
            &owners.runtime.active_tab().symbol,
        );
        scenario.after_replay(&mut native::Desk(desk));
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
    scenario.toast(&mut native::Desk(desk));
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
