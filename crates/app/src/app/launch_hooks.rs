//! Where every `QUANTICK_*` launch hook is applied.
//!
//! One place, so the ui-harness has an address: this module is the
//! application point for every launch hook `.claude/skills/ui-harness`
//! documents. A hook is *read* here and applied to the built window, after
//! the constructor has assembled it and before the first frame. Five of the
//! drawing chrome's own live with the fields they set and are called from
//! [`apply_tape_hooks`]; the rest are here in full.
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
//! So the appliers below are called in file order and each keeps its group in
//! the order it was read in. Reordering them changes what a launch opens
//! with, and no compiler catches it.
//!
//! A child of `app` rather than a sibling so it can reach the app's own
//! fields: this *is* app logic, split off only so the constructor above it is
//! the window's definition and nothing else.

use crate::dock::DockTab;
use crate::drawings;
use crate::indicator_worker::IndicatorSource;
use crate::indicators::state_file::SavedKind;
use crate::tab::CanvasLayout;
use crate::toolrail::{Tool, ToolboxDock};
use quantick_feed::history_reach;

use super::{AUTOSTART_NATIVES, QuantickApp, parse_tape_window};

impl QuantickApp {
    /// Apply every launch hook to the built window, in the order the module's
    /// doc comment fixes. Called from `new_with_workspace` once the window is
    /// assembled and the saved workspace restored.
    pub(super) fn apply_launch_hooks(&mut self) {
        self.apply_book_and_strip_hooks();
        self.apply_control_hooks();
        self.apply_rail_hooks();
        self.apply_history_hooks();
        self.apply_tape_hooks();
        self.apply_indicator_hooks();
        self.apply_layout_hooks();
        self.apply_replay_hooks();
        self.apply_dock_and_report_hooks();
        self.apply_workspace_hooks();
    }

    /// The order book and the live strip: the two the map opens with.
    fn apply_book_and_strip_hooks(&mut self) {
        // Dev/ops can open the map without a click.
        if std::env::var("QUANTICK_BOOK_AUTOSTART").is_ok_and(|value| value == "1") {
            self.active_tab_mut().tape_mut().set_depth_visible(true);
        }
        // Same convenience for the live strip; its pixels stay
        // capability-gated either way (see live_strip_width).
        if std::env::var("QUANTICK_LIVE_STRIP_AUTOSTART").is_ok_and(|value| value == "1") {
            self.active_tab_mut().flow_pane.live_strip_visible = true;
        }
    }

    /// The control plane: the panel, the grant, and the four staged acts an
    /// operator other than the trader performs.
    fn apply_control_hooks(&mut self) {
        // Local agent access, reachable without a click: the panel through the
        // Tools menu entry's own function, and the enable action through the
        // panel button's own function on the first frame — one path for the
        // human, the hook and any later operator. Enabling publishes a real
        // descriptor in the private runtime directory, removed on a clean exit.
        if std::env::var("QUANTICK_CONTROL_PANEL").is_ok_and(|value| value == "1")
            && let Some(access) = self.control_access.as_mut()
        {
            access.open_panel();
        }
        // Which scopes the next connection is granted, by ID — the panel's
        // own checkboxes without a hand on the mouse. `annotate` grants the
        // whole annotate tier (the profile follows the scopes), and any
        // comma-separated list of registered permission IDs is honoured, so a
        // scripted run can reproduce exactly the grant a trader would tick.
        if let Ok(scopes) = std::env::var("QUANTICK_CONTROL_SCOPES")
            && let Some(access) = self.control_access.as_mut()
            && let Err(error) = access.configure_scopes(&scopes)
        {
            {
                tracing::warn!(
                    target: "quantick::control",
                    event_code = "CONTROL_SCOPE_HOOK_REFUSED",
                    error = %error,
                    "QUANTICK_CONTROL_SCOPES named something this build does not register"
                );
            }
        }
        self.pending_control_access_enable =
            std::env::var("QUANTICK_CONTROL_ACCESS").is_ok_and(|value| value == "1");
        // A mark from a launch: `1` marks with no note, anything else is the
        // note. It goes through the same action the hotkey calls.
        self.pending_control_mark = std::env::var("QUANTICK_CONTROL_MARK")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| if value == "1" { String::new() } else { value });
        // An assistant's own object and an assistant's own interruption, from
        // a launch: the surfaces that say *who* acted cannot be photographed
        // without something an operator other than the trader put there.
        // One evidence bundle from a launch, through the same read a client
        // calls: the capture a validation run asserts against, and the
        // screenshot notice a capture run photographs.
        self.pending_control_evidence = std::env::var("QUANTICK_CONTROL_EVIDENCE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        self.pending_control_annotation = std::env::var("QUANTICK_CONTROL_ANNOTATE")
            .ok()
            .filter(|value| !value.trim().is_empty());
        self.pending_control_notification = std::env::var("QUANTICK_CONTROL_NOTIFY")
            .ok()
            .filter(|value| !value.trim().is_empty());
    }

    /// The drawing toolbar: the armed tool, the magnet, the favourites, and
    /// where the toolbox sits.
    fn apply_rail_hooks(&mut self) {
        // Drawing-toolbar hooks, so a validation run reaches every new
        // surface without a click (`.claude/skills/ui-harness`).
        if let Ok(id) = std::env::var("QUANTICK_DRAWING_TOOL")
            && let Some(tool) = drawings::DRAWING_TOOLS
                .into_iter()
                .find(|tool| tool.id() == id.trim())
        {
            self.toolrail.arm(Tool::Drawing(tool));
        }
        if std::env::var("QUANTICK_DRAWING_MAGNET").is_ok_and(|value| value == "1") {
            self.toolrail.set_magnet(true);
        }
        // Pinned favorites by tool id, comma-separated — the same restore
        // path the workspace file takes, so the hook cannot drift from it.
        if let Ok(ids) = std::env::var("QUANTICK_TOOL_FAVORITES") {
            let ids: Vec<String> = ids
                .split(',')
                .map(|id| id.trim().to_owned())
                .filter(|id| !id.is_empty())
                .collect();
            self.toolrail.set_favorites(&ids);
            // A staged rail, so a star toggled during the run stays in the run.
            self.workspace.session_mut().stage_favorites();
        }
        // Dock the rail against a named edge, so a validation run can shoot
        // the horizontal band without editing the workspace file.
        if let Ok(edge) = std::env::var("QUANTICK_TOOLBOX_DOCK") {
            let dock = match edge.trim() {
                "left" => Some(ToolboxDock::Left),
                "top" => Some(ToolboxDock::Top),
                "bottom" => Some(ToolboxDock::Bottom),
                _ => None,
            };
            if let Some(dock) = dock {
                self.toolrail.set_dock(dock);
            }
        }
        // Park the scrolling tool band mid-travel. Only the middle of the
        // run shows both chevrons live at once, and a screenshot cannot
        // click an arrow to get there.
        // Nonsense is refused rather than guessed, like the dock above: a
        // typo that silently parked the band at zero would photograph the
        // wrong state and call it the right one.
        if let Ok(offset) = std::env::var("QUANTICK_TOOLBAR_SCROLL") {
            let parked = match offset.trim() {
                "end" => Some(f32::INFINITY),
                other => other.parse::<f32>().ok().filter(|at| at.is_finite()),
            };
            if let Some(parked) = parked {
                self.toolrail.set_band_offset(parked);
            }
        }
        // Open a family flyout on the first frame — the star column lives
        // there, and a screenshot cannot click a caret.
        if let Ok(family_id) = std::env::var("QUANTICK_TOOLBOX_FLYOUT") {
            self.toolrail.request_flyout(family_id.trim().to_owned());
        }
    }

    /// How far back a launch reaches, and how it gets there.
    fn apply_history_hooks(&mut self) {
        // The switch itself, so both sides of it are reachable without a
        // click. Set explicitly, it also overrides what the workspace saved:
        // a validation run must be able to pin the state it is photographing.
        // The same registry the menu lists from, so a hook can reach every
        // reach the trader can — and an unknown token is refused out loud
        // rather than silently leaving the default in place, which would look
        // like a press that ignored the run it was told to make.
        if let Ok(token) = std::env::var("QUANTICK_HISTORY_REACH") {
            match history_reach::HistoryReach::from_token(&token) {
                Some(reach) => self.set_history_reach(reach),
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
        if let Ok(raw) = std::env::var("QUANTICK_HISTORY_REACH_SPAN_MINUTES") {
            // Beside `QUANTICK_HISTORY_REACH`, because the reach and how far it
            // goes are one choice: a hook that could pick `by time` but not say
            // how much time would leave the operator setting half of it.
            match raw.trim().parse::<u32>() {
                Ok(minutes) => self.set_history_reach_span_minutes(minutes),
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
        if let Ok(value) = std::env::var("QUANTICK_VENUE_LEAD_IN") {
            // `1` and `0`, and nothing else understood. A typo must not decide
            // a switch the trader set: read as a bare truthiness test, `true`
            // or `on` would silently turn the lead-in *off* and overwrite what
            // the workspace saved, and a capture run would photograph the off
            // state while reporting it as on.
            match value.trim() {
                "1" => self.venue_lead_in = true,
                "0" => self.venue_lead_in = false,
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
        if let Ok(value) = std::env::var("QUANTICK_PROGRESSIVE_HISTORY") {
            match value.trim() {
                "1" => self.progressive_history = true,
                "0" => self.progressive_history = false,
                // Nonsense is refused rather than guessed: a typo leaves the
                // trader's own setting alone instead of silently flipping it.
                _ => {}
            }
        }
    }

    /// The tape and everything drawn on it: the drawing chrome's own five, the
    /// aggression layer, the lanes, the window, the footprint and the budgets.
    fn apply_tape_hooks(&mut self) {
        // The drawing chrome's five hooks, read here rather than on the first
        // drawn frame: the demo appliers run earlier in that frame and ask
        // whether the inspector is open, so a hook another hook depends on has
        // to be in place before any of them. They live with the fields they
        // set — see `surfaces::drawing_chrome::apply_launch_hooks`.
        crate::surfaces::drawing_chrome::apply_launch_hooks(&mut self.surfaces.drawing_chrome);

        // Same convenience for the aggression layer (bubbles + the live
        // column's footprint). Same code path as the toolbar toggle.
        if std::env::var("QUANTICK_BUBBLES_AUTOSTART").is_ok_and(|value| value == "1") {
            self.active_tab_mut().tape_mut().set_bubbles_enabled(true);
        }
        // The chart upside down, through the very setter the axis menu's
        // checkbox calls. The inverted frame is otherwise only reachable by
        // a long axis drag no scripted run can perform. Both panes of a
        // split layout: the hook exists so one capture audits every
        // price-mapped surface at once, and a half-inverted frame would
        // silently audit the time pane the right way up.
        if std::env::var("QUANTICK_INVERTED").is_ok_and(|value| value.trim() == "1") {
            let tab = self.active_tab_mut();
            for pane in tab.panes_mut() {
                pane.price_view.set_inverted(true);
            }
        }
        // The tape switch in the canvas's top-right corner — the one control
        // that decides whether there is a band at all. Same setter the chip
        // calls, so a capture shows what a click shows. Anything but `on`/`off`
        // leaves the tape alone rather than guessing.
        if let Ok(value) = std::env::var("QUANTICK_TAPE") {
            match value.trim() {
                "on" => self.active_tab_mut().tape_mut().set_lane_enabled(true),
                "off" => self.active_tab_mut().tape_mut().set_lane_enabled(false),
                _ => {}
            }
        }
        // The tape's own layer switches. The two panes are configured apart and
        // the tape's menu is a right-click a scripted run cannot perform, so the
        // state behind it needs a door of its own — the state, not a second
        // way of drawing it: each entry calls the very setter the menu's
        // checkbox calls. Unlisted layers stay as they were, which is what
        // keeps this hook from being a second opinion about the whole tape.
        if let Ok(value) = std::env::var("QUANTICK_TAPE_LAYERS") {
            let wanted: Vec<&str> = value
                .split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .collect();
            let tape = self.active_tab_mut().tape_mut();
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
        if let Ok(value) = std::env::var("QUANTICK_TAPE_WINDOW")
            && let Some(window) = parse_tape_window(value.trim())
        {
            self.active_tab_mut()
                .tape_mut()
                .set_live_lane_window(window);
        }
        // Same convenience for the candle footprint — the same field the
        // pane's layer menu writes, so a validation run sees exactly what a
        // click would show.
        if self.harness.footprint() {
            self.active_tab_mut().flow_pane.footprint.visible = true;
        }
        // Every style by its own id, resolved through the same registry the
        // panel's selector and the TOML read. A style reachable by click but
        // not by name is a style the second operator cannot pick, and one
        // more list to keep in step by hand.
        if let Ok(value) = std::env::var("QUANTICK_FOOTPRINT_STYLE") {
            match crate::footprint_config::FootprintStyle::from_id(value.trim()) {
                Some(style) => self.footprint_config.style = style,
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
        if let Some(px) = self.harness.candle_width() {
            self.active_tab_mut().flow_pane.viewport.set_px_per_bar(px);
        }
        // The bubble budget, scriptable. The fold is the one bubble state a
        // capture cannot otherwise reach: it needs a tape dense enough to
        // exhaust a budget of seven hundred, which is a market condition and
        // not a setting. `QUANTICK_BUBBLE_BUDGET=8` squeezes the same budget
        // the frame always spends, through the same field the projection
        // reads, so what a screenshot shows is what a busy session shows —
        // folded marks wearing their ring and their count.
        if let Ok(value) = std::env::var("QUANTICK_BUBBLE_BUDGET")
            && let Ok(budget) = value.trim().parse::<usize>()
            && budget > 0
        {
            for tab in &mut self.tabs {
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
        if let Ok(value) = std::env::var("QUANTICK_TAPE_STARVE_AFTER_MS")
            && let Ok(after_ms) = value.trim().parse::<i64>()
            && after_ms >= 0
        {
            for tab in &mut self.tabs {
                tab.tape_mut().set_starve_tape_after_ms(after_ms);
            }
        }
    }

    /// The indicators a launch opens with, and whether their legend is folded.
    fn apply_indicator_hooks(&mut self) {
        // Same convenience for indicators: open with the two M1 natives on
        // (EMA overlay + CVD pane), through the same code path the toolbar
        // menu takes, so a scripted validation run needs no clicks.
        if std::env::var("QUANTICK_INDICATORS_AUTOSTART").is_ok_and(|value| value == "1") {
            let pane = &mut self.active_tab_mut().flow_pane;
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
        // `set_focused_legend_collapsed`, the same call the chevron and the
        // menu entry make — never a field poked from the side.
        if std::env::var("QUANTICK_LEGEND_COLLAPSED").is_ok_and(|value| value == "1") {
            self.set_focused_legend_collapsed(true);
        }
    }

    /// The layout strip and the pane layouts. Opens with `seed_new_panes`, which
    /// must run before any autostart hook -- see the module's order rule.
    fn apply_layout_hooks(&mut self) {
        // Put the active layout on the first tab's panes before any autostart
        // hook: the file is what the user actually had open.
        self.seed_new_panes();
        // The layout strip's hooks (`ui-harness`): open on a named layout,
        // creating it when the file has none by that name, and open the
        // rename box on the active one.
        if let Ok(name) = std::env::var("QUANTICK_LAYOUT_TAB")
            && let Some(name) = crate::layouts::clean_name(&name)
        {
            let wanted = self.layouts().by_name(&name).map(|layout| layout.id);
            let outcome = match wanted {
                Some(id) => self.switch_layout(id).map(|_| id),
                None => self.create_layout(Some(&name)),
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
        if let Ok(names) = std::env::var("QUANTICK_PANE_LAYOUTS") {
            self.apply_pane_layouts_hook(&names);
        }
        if std::env::var("QUANTICK_LAYOUT_RENAME").is_ok_and(|value| value == "1") {
            let active = self.focused_pane_layout();
            self.begin_layout_rename(active);
        }
        if std::env::var("QUANTICK_LAYOUT_DELETE").is_ok_and(|value| value == "1") {
            let active = self.focused_pane_layout();
            self.apply_strip_action(crate::layout_strip::StripAction::Delete(active));
        }
        // Scripted validation runs can open with library scripts loaded:
        // a comma-separated list of script names, each through the same
        // code path the INDICATORS menu takes.
        if let Ok(names) = std::env::var("QUANTICK_INDICATOR_SCRIPTS_AUTOSTART") {
            for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                match self
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
                            let tab = self.active_tab();
                            (tab.id, tab.focused_side())
                        };
                        self.add_indicator_at(
                            tab,
                            side,
                            &SavedKind::Script {
                                name: name.to_owned(),
                            },
                        );
                        self.forget_last_indicator_state_change();
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
    fn apply_replay_hooks(&mut self) {
        // Whether a recording opens with the day before it joined in front.
        // Read before anything loads a session, because that is the frame the
        // setting is consulted on. Staged rather than chosen: a validation run
        // states the screen it wants to photograph, and must not write a QA
        // preference into the trader's workspace — the same rule the replay
        // folder follows.
        if let Ok(value) = std::env::var("QUANTICK_REPLAY_DAY_BEFORE") {
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
                    self.replay_view.stage_day_before(enabled);
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
        let autostart_play = match std::env::var("QUANTICK_REPLAY_AUTOSTART")
            .unwrap_or_default()
            .trim()
        {
            "1" => Some(true),
            "paused" => Some(false),
            _ => None,
        };
        if let Some(play) = autostart_play {
            let speed = std::env::var("QUANTICK_REPLAY_SPEED")
                .ok()
                .and_then(|value| value.trim().parse::<f32>().ok())
                .filter(|speed| *speed > 0.0)
                .unwrap_or(1.0);
            // Which recording, when the folder holds more than one. The
            // scan lists them oldest first, so without this a folder of days
            // always opens the one that can have nothing joined in front of
            // it — the single state this hook family exists to avoid.
            let day = std::env::var("QUANTICK_REPLAY_SESSION")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            let started = self.replay_view.autostart(speed, day.as_deref(), play);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_AUTOSTART",
                // The folder actually scanned, not the environment variable:
                // once a stored pick can supply it, reading the hook back
                // would report an empty folder for a run that scanned a full
                // one — a log that lies about the input it acted on.
                folder = self.replay_view.folder_in_use(),
                speed,
                day = day.as_deref().unwrap_or("(first)"),
                day_before = self.replay_view.day_before(),
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
        if std::env::var("QUANTICK_REPLAY_BROWSER").is_ok_and(|value| value == "1") {
            self.replay_view.open_browser();
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_BROWSER_AUTOSTART",
                folder = self.replay_view.folder_in_use(),
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
        if let Ok(value) = std::env::var(crate::replay_view::GET_DATA_ENV) {
            let symbol = match value.trim() {
                "1" | "" => Some(self.active_tab().symbol.clone()),
                symbol => Some(symbol.to_string()),
            };
            self.replay_view.open_get_data(symbol.as_deref());
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
    fn apply_dock_and_report_hooks(&mut self) {
        // Same convenience for the dock: open a named tab, so a scripted
        // validation run shows a panel without a click.
        if let Ok(name) = std::env::var("QUANTICK_DOCK_TAB") {
            let tab = match name.trim() {
                "l2" => Some(DockTab::L2),
                "bubbles" => Some(DockTab::Bubbles),
                "session" => Some(DockTab::Session),
                "trading" => Some(DockTab::Trading),
                "trades" => Some(DockTab::Trades),
                _ => None,
            };
            match tab {
                Some(tab) => self.dock.open_tab(tab),
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
        if std::env::var("QUANTICK_PAPER_REPORT_AUTOSTART").is_ok_and(|value| value == "1") {
            self.active_tab_mut().paper.account_mut().autostart_report();
        }
        // The calendar the report grew: reachable open, on a chosen day or
        // a chosen span, with no clicks at all.
        if let Ok(spec) = std::env::var("QUANTICK_PAPER_CALENDAR") {
            match crate::paper_calendar::parse_selection(&spec) {
                Some(selection) => self
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
        if let Ok(spec) = std::env::var("QUANTICK_LEDGER_SCOPE") {
            let scope = match spec.trim() {
                "chart" => Some(crate::paper_trading::LedgerScope::Chart),
                "all" => Some(crate::paper_trading::LedgerScope::All),
                "" => None,
                symbol => Some(crate::paper_trading::LedgerScope::Symbol(symbol.to_owned())),
            };
            match scope {
                Some(scope) => self
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
        if let Ok(text) = std::env::var("QUANTICK_LEDGER_PAGES") {
            match text.trim().parse::<usize>() {
                Ok(pages) if pages >= 1 => {
                    self.active_tab_mut()
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
        if std::env::var("QUANTICK_LEDGER_FOLD").is_ok_and(|value| value == "1") {
            let tz = self.tz;
            self.active_tab_mut()
                .paper
                .account_mut()
                .autostart_folded_days(tz);
        }
        // The report's trade list is open by default, so the hook is how a
        // capture reaches it collapsed.
        if let Ok(value) = std::env::var("QUANTICK_PAPER_REPORT_LIST") {
            self.active_tab_mut()
                .paper
                .account_mut()
                .set_report_list_open(value.trim() != "0");
        }
    }

    /// The canvas layout, the workspace file, and the toast that says a launch
    /// rescued something.
    fn apply_workspace_hooks(&mut self) {
        // Open on a named canvas layout, through the same path the View menu
        // takes. An env var is an explicit request for this run, so it wins
        // over a feed's declared `default_layout`.
        if let Ok(name) = std::env::var("QUANTICK_LAYOUT") {
            let layout = crate::config::DeclaredLayout::parse(&name).map(CanvasLayout::from);
            match layout {
                Some(layout) => self.active_tab_mut().set_layout(layout),
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
        if std::env::var("QUANTICK_WORKSPACE_SAVE").is_ok_and(|value| value == "1") {
            self.save_workspace("autostart");
        }
        // The three file entries, reachable with no click for the same reason
        // (`.claude/skills/ui-harness`). Each runs the menu entry's own code
        // past the OS dialog — the dialog is the one thing a scripted run
        // cannot drive, so the path is given instead of picked. They really
        // write and really replace the cockpit, so point `QUANTICK_UI_STATE`
        // and its sibling stores at scratchpad files first.
        if let Ok(path) = std::env::var("QUANTICK_WORKSPACE_EXPORT") {
            self.export_workspace_to(std::path::Path::new(&path));
        }
        if let Ok(path) = std::env::var("QUANTICK_WORKSPACE_IMPORT") {
            self.import_workspace_from(std::path::Path::new(&path));
        }
        // An env var is not a user edit: what the autostart hooks switched on
        // must not be written back as though the user had asked for it every
        // launch from now on. Same rule the indicator state follows.
        let staged_layers = self.layer_mask();
        self.workspace.layers_mut().record(staged_layers);
        // The cockpit rescue ran in `main`, before any store was read. A
        // silent one would look like the app relocated the trader's settings
        // behind their back — and leave them not knowing which folder to back
        // up. Same toast channel the journal's rescue uses.
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
        if std::env::var("QUANTICK_TOAST").is_ok_and(|value| value == "paper") {
            self.tabs[0]
                .paper
                .show_toast("SIM: stop filled at 169 790 — flat.".to_owned());
        }
    }
}

crate::hooks::declare_hooks![
    "QUANTICK_BOOK_AUTOSTART",
    "QUANTICK_BUBBLES_AUTOSTART",
    "QUANTICK_BUBBLE_BUDGET",
    "QUANTICK_CONTROL_ACCESS",
    "QUANTICK_CONTROL_ANNOTATE",
    "QUANTICK_CONTROL_EVIDENCE",
    "QUANTICK_CONTROL_MARK",
    "QUANTICK_CONTROL_NOTIFY",
    "QUANTICK_CONTROL_PANEL",
    "QUANTICK_CONTROL_SCOPES",
    "QUANTICK_DOCK_TAB",
    "QUANTICK_DRAWING_MAGNET",
    "QUANTICK_DRAWING_TOOL",
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
    "QUANTICK_TOOLBAR_SCROLL",
    "QUANTICK_TOOLBOX_DOCK",
    "QUANTICK_TOOLBOX_FLYOUT",
    "QUANTICK_TOOL_FAVORITES",
    "QUANTICK_VENUE_LEAD_IN",
    "QUANTICK_WORKSPACE_EXPORT",
    "QUANTICK_WORKSPACE_IMPORT",
    "QUANTICK_WORKSPACE_SAVE"
];
