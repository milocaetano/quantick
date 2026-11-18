//! Applies one launch's captured scenario in phases around native initialization.
//! Commands are transient. In particular, scripted indicators are excluded from
//! the persisted layout and layer changes precede the saved-layer snapshot.
use crate::remove_chart::Intent as ChartMenu;
use quantick_control_schema::{
    desk,
    launch_calls::{self, LaunchCall},
    launch_values as values, layout,
};
use quantick_orderflow::LaneWindow;
use quantick_paper::report::{
    calendar::{self, DaySelection},
    ledger_rows::LedgerScope,
};
use quantick_sources::history_reach;
use quantick_stores::footprint_config::FootprintStyle;
use quantick_workspace::{
    canvas_layout::{self, CanvasLayout},
    dock::DockTab,
    orderflow_panel::PanelSection,
    workspace_recipe::DeclaredLayout,
};

/// A lookup over the immutable values captured by the launch root.
pub struct Scenario<'a>(pub &'a dyn Fn(&str) -> Option<String>);

/// Operations on native owners that a scenario can stage for this run.
pub enum Op {
    Book,
    LiveStrip,
    Bubbles,
    Invert,
    LaneDepth(bool),
    LaneBubbles(bool),
    Window(LaneWindow),
    Footprint,
    Style(FootprintStyle),
    CandleWidth(f32),
    Budget(usize),
    Starve(i64),
    Native(&'static str),
    CollapseLegend,
    Call(LaunchCall),
    PaneLayouts(String),
    RenameLayout,
    DeleteLayout,
    Script(String),
    Panel(PanelSection),
    Dock(DockTab),
    Report,
    ReportScroll(String),
    Calendar(DaySelection),
    LedgerScope(LedgerScope),
    LedgerPages(usize),
    FoldLedger,
    ReportList(bool),
    Layout(CanvasLayout),
    FoldContext(bool),
    PaneMenu(ChartMenu),
    Export(String),
    Import(String),
    Toast(&'static str),
}

/// Facts are read when the hook runs: earlier calls may have changed the workspace.
pub trait Desk {
    fn apply(&mut self, op: Op);
    fn flow_address(&mut self) -> (u64, u64);
    fn has_layout(&mut self, name: &str) -> bool;
    fn has_script(&mut self, name: &str) -> bool;
    fn workspace_menu(&mut self, value: &str) -> bool;
    fn canvas_layout(&mut self) -> CanvasLayout;
}

pub trait Replay {
    fn state(&self) -> &crate::replay_browser::State;
    fn dispatch(&mut self, intent: crate::replay_browser::Intent)
    -> crate::replay_browser::Effects;

    fn autostart(&mut self, speed: f32, day: Option<&str>, play: bool) -> bool {
        use crate::replay_browser::Intent;
        self.dispatch(Intent::Rescan);
        self.dispatch(Intent::AutostartSelection {
            speed,
            day: day.map(str::to_owned),
            play,
        })
        .load_started
    }
    fn open_get_data(&mut self, symbol: &str) {
        use crate::replay_browser::Intent;
        self.dispatch(Intent::OpenBrowser);
        self.dispatch(Intent::EnterDownload {
            symbol: Some(symbol.to_owned()),
            explicit: true,
        });
    }
}
impl Scenario<'_> {
    fn var(&self, name: &str) -> Option<String> {
        (self.0)(name)
    }
    fn one(&self, name: &str) -> bool {
        values::one(self.var(name).as_deref())
    }

    pub fn book_and_strip(&self, desk: &mut impl Desk) {
        if self.one("QUANTICK_BOOK_AUTOSTART") {
            desk.apply(Op::Book);
        }
        if self.one("QUANTICK_LIVE_STRIP_AUTOSTART") {
            desk.apply(Op::LiveStrip);
        }
    }
    /// Set the legacy span before resolving a reach token that refers to it.
    pub fn history(&self, history: &mut quantick_session::HistorySettings) {
        if let Some(raw) = self.var("QUANTICK_HISTORY_REACH_SPAN_MINUTES") {
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
        if let Some(token) = self.var("QUANTICK_HISTORY_REACH") {
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
        if let Some(value) = self.var("QUANTICK_VENUE_LEAD_IN") {
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
        if let Some(on) = self
            .var("QUANTICK_PROGRESSIVE_HISTORY")
            .and_then(|v| values::binary(&v))
        {
            history.progressive_history = on;
        }
    }
    fn pane_call(&self, desk: &mut impl Desk, hook: &str, read: launch_calls::PaneCall) {
        let (tab, pane) = desk.flow_address();
        if let Some(call) = self.var(hook).and_then(|v| read(&v, tab, pane)) {
            desk.apply(Op::Call(call));
        }
    }
    pub fn before_seed(&self, desk: &mut impl Desk, footprint: bool, candle_width: Option<f32>) {
        if self.one("QUANTICK_BUBBLES_AUTOSTART") {
            desk.apply(Op::Bubbles);
        }
        if values::one(self.var("QUANTICK_INVERTED").as_deref().map(str::trim)) {
            desk.apply(Op::Invert);
        }
        self.pane_call(desk, "QUANTICK_TAPE", launch_calls::tape);
        if let Some(value) = self.var("QUANTICK_TAPE_LAYERS") {
            let layers = values::lane_layers(&value);
            if let Some(on) = layers.depth {
                desk.apply(Op::LaneDepth(on));
            }
            if let Some(on) = layers.bubbles {
                desk.apply(Op::LaneBubbles(on));
            }
        }
        if let Some(window) = self
            .var("QUANTICK_TAPE_WINDOW")
            .and_then(|v| LaneWindow::parse(&v))
        {
            desk.apply(Op::Window(window));
        }
        if footprint {
            desk.apply(Op::Footprint);
        }
        if let Some(value) = self.var("QUANTICK_FOOTPRINT_STYLE") {
            match FootprintStyle::from_id(value.trim()) {
                Some(style) => desk.apply(Op::Style(style)),
                None => {
                    tracing::warn!(
                        target: "quantick_app::app::launch_hooks",
                        requested = %value,
                        known = ?FootprintStyle::ALL.map(FootprintStyle::id),
                        "QUANTICK_FOOTPRINT_STYLE names no known style; keeping the current one"
                    )
                }
            }
        }
        if let Some(px) = candle_width {
            desk.apply(Op::CandleWidth(px));
        }
        if let Some(n) = self
            .var("QUANTICK_BUBBLE_BUDGET")
            .and_then(|v| values::positive(&v))
        {
            desk.apply(Op::Budget(n));
        }
        if let Some(ms) = self
            .var("QUANTICK_TAPE_STARVE_AFTER_MS")
            .and_then(|v| values::non_negative_ms(&v))
        {
            desk.apply(Op::Starve(ms));
        }
        if self.one("QUANTICK_INDICATORS_AUTOSTART") {
            // A new registered native must not change existing captures.
            for id in ["native.ema", "native.cvd"] {
                desk.apply(Op::Native(id));
            }
        }
        if self.one("QUANTICK_LEGEND_COLLAPSED") {
            desk.apply(Op::CollapseLegend);
        }
    }
    pub fn layout(&self, desk: &mut impl Desk) {
        if let Some(name) = self
            .var("QUANTICK_LAYOUT_TAB")
            .and_then(|v| quantick_workspace::layout_document::clean_name(&v))
        {
            let known = desk.has_layout(&name);
            desk.apply(Op::Call(launch_calls::layout_tab(&name, known)));
        }
        if let Some(names) = self.var("QUANTICK_PANE_LAYOUTS") {
            desk.apply(Op::PaneLayouts(names));
        }
        if self.one("QUANTICK_LAYOUT_RENAME") {
            desk.apply(Op::RenameLayout);
        }
        if self.one("QUANTICK_LAYOUT_DELETE") {
            desk.apply(Op::DeleteLayout);
        }
        if let Some(names) = self.var("QUANTICK_INDICATOR_SCRIPTS_AUTOSTART") {
            for name in values::names(&names) {
                if desk.has_script(name) {
                    desk.apply(Op::Script(name.to_owned()));
                } else {
                    tracing::warn!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "INDICATOR_SCRIPT_UNKNOWN",
                        script = %name,
                        action = "autostart_entry_skipped",
                        "autostart names a script the library does not have"
                    );
                }
            }
        }
    }
    /// Stage the day-before preference before loading the selected recording.
    pub fn replay(&self, browser: &mut impl Replay, chart_symbol: &str) {
        if let Some(value) = self.var("QUANTICK_REPLAY_DAY_BEFORE") {
            match values::flag(&value) {
                Some(enabled) => {
                    browser.dispatch(crate::replay_browser::Intent::StageDayBefore(enabled));
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
        if let Some(play) =
            values::replay_autostart(self.var("QUANTICK_REPLAY_AUTOSTART").as_deref())
        {
            let speed = values::replay_speed(self.var("QUANTICK_REPLAY_SPEED").as_deref());
            let day = values::non_blank(self.var("QUANTICK_REPLAY_SESSION").as_deref());
            let started = browser.autostart(speed, day.as_deref(), play);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_AUTOSTART",
                folder = browser.state().folder.as_str(),
                speed,
                day = day.as_deref().unwrap_or("(first)"),
                day_before = browser.state().day_before,
                play,
                started,
                action = if started { "load_first_session" } else { "open_browser" },
                "market replay autostart"
            );
        }
        if values::one(self.var("QUANTICK_REPLAY_BROWSER").as_deref()) {
            browser.dispatch(crate::replay_browser::Intent::OpenBrowser);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_BROWSER_AUTOSTART",
                folder = browser.state().folder.as_str(),
                "opened the session browser"
            );
        }
        if let Some(value) = self.var("QUANTICK_REPLAY_GET_DATA") {
            let symbol = match value.trim() {
                "1" | "" => chart_symbol.to_owned(),
                symbol => symbol.to_string(),
            };
            browser.open_get_data(&symbol);
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "REPLAY_GET_DATA_AUTOSTART",
                symbol = symbol.as_str(),
                "opened the replay download tab"
            );
        }
    }
    pub fn after_replay(&self, desk: &mut impl Desk) {
        if let Some(name) = self.var("QUANTICK_DOCK_TAB") {
            let id = name.trim();
            if let Some(section) = PanelSection::ALL.into_iter().find(|at| at.id() == id) {
                desk.apply(Op::Panel(section));
            } else if let Some(tab) = DockTab::ALL.into_iter().find(|at| at.id() == id) {
                desk.apply(Op::Dock(tab));
            } else {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "DOCK_TAB_AUTOSTART_UNKNOWN",
                    tab = %name,
                    action = "dock_left_as_is",
                    "QUANTICK_DOCK_TAB names no dock tab"
                );
            }
        }
        self.paper_report(|op| desk.apply(op));
        self.pane_call(desk, "QUANTICK_PRICE_RANGE", launch_calls::price_range);
        self.workspace(desk);
    }
    fn paper_report(&self, mut apply: impl FnMut(Op)) {
        if values::one(self.var("QUANTICK_PAPER_REPORT_AUTOSTART").as_deref()) {
            apply(Op::Report);
        }
        if let Some(value) = self.var("QUANTICK_PAPER_REPORT_SCROLL") {
            apply(Op::ReportScroll(value));
        }
        if let Some(spec) = self.var("QUANTICK_PAPER_CALENDAR") {
            match calendar::parse_selection(&spec) {
                Some(selection) => apply(Op::Calendar(selection)),
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
        if let Some(spec) = self.var("QUANTICK_LEDGER_SCOPE") {
            match LedgerScope::from_hook(&spec) {
                Some(scope) => apply(Op::LedgerScope(scope)),
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
        if let Some(text) = self.var("QUANTICK_LEDGER_PAGES") {
            match values::positive(&text) {
                Some(pages) => apply(Op::LedgerPages(pages)),
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
        if values::one(self.var("QUANTICK_LEDGER_FOLD").as_deref()) {
            apply(Op::FoldLedger);
        }
        if let Some(value) = self.var("QUANTICK_PAPER_REPORT_LIST") {
            apply(Op::ReportList(value.trim() != "0"));
        }
    }
    fn when_one(&self, desk: &mut impl Desk, hook: &str, id: &'static str) {
        if let Some(call) = launch_calls::when_one(self.var(hook).as_deref(), id) {
            desk.apply(Op::Call(call));
        }
    }
    fn workspace(&self, desk: &mut impl Desk) {
        if let Some(value) = self.var("QUANTICK_WORKSPACE_MENU")
            && !desk.workspace_menu(&value)
        {
            tracing::warn!(target: "quantick::app", value = %value, "QUANTICK_WORKSPACE_MENU names no workspace surface");
        }
        if let Some(name) = self.var("QUANTICK_LAYOUT") {
            let layout = DeclaredLayout::parse(&name)
                .map(CanvasLayout::from)
                .or_else(|| canvas_layout::preset(&name).map(CanvasLayout::from_preset));
            match layout {
                Some(layout) => desk.apply(Op::Layout(layout)),
                None => tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "LAYOUT_AUTOSTART_UNKNOWN",
                    layout = %name,
                    action = "layout_left_as_is",
                    accepted = %canvas_layout::LAYOUT_PRESETS
                        .iter()
                        .map(|preset| preset.id)
                        .collect::<Vec<_>>()
                        .join(", "),
                    "QUANTICK_LAYOUT names no canvas layout"
                ),
            }
        }
        self.when_one(
            desk,
            "QUANTICK_PANE_COLLAPSED",
            layout::COLLAPSE_CAPABILITY_ID,
        );
        self.when_one(
            desk,
            "QUANTICK_FLOW_PANE_COLLAPSED",
            layout::FLOW_COLLAPSE_CAPABILITY_ID,
        );
        let upper = self.var("QUANTICK_CONTEXT_UPPER_COLLAPSED");
        let lower = self.var("QUANTICK_CONTEXT_LOWER_COLLAPSED");
        if desk.canvas_layout() == CanvasLayout::TimeTimeAndFlow
            && let Some(upper) = values::context_fold(upper.as_deref(), lower.as_deref())
        {
            desk.apply(Op::FoldContext(upper));
        }
        self.when_one(
            desk,
            "QUANTICK_WORKSPACE_SAVE",
            desk::DESK_SAVE_CAPABILITY_ID,
        );
        if let Some((slot, row)) = self
            .var("QUANTICK_PANE_MENU")
            .as_deref()
            .and_then(quantick_workspace::canvas::menu_hook)
        {
            desk.apply(Op::PaneMenu(
                row.map_or(ChartMenu::SetMenuOpen(Some(slot)), |row| {
                    ChartMenu::Pick(slot, row)
                }),
            ));
        }
        if let Some(path) = self.var("QUANTICK_WORKSPACE_EXPORT") {
            desk.apply(Op::Export(path));
        }
        if let Some(path) = self.var("QUANTICK_WORKSPACE_IMPORT") {
            desk.apply(Op::Import(path));
        }
    }
    pub fn toast(&self, desk: &mut impl Desk) {
        if self.var("QUANTICK_TOAST").as_deref() == Some("paper") {
            desk.apply(Op::Toast("SIM: stop filled at 169 790 — flat."));
        }
    }
}

#[cfg(test)]
mod tests;
