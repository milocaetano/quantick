//! The tab strip and the `⋮` window menu (§10), and the shortcuts they mirror.
//!
//! A shallow menu for discoverability, never the only path to anything: every
//! entry that has a shortcut shows it, and the constants below are the one
//! place a key is bound. They live here rather than beside the struct
//! because the window menu is their only production reader — the paper
//! shortcuts are re-exported to `super` for the tests that name them.

use super::GatewayPort;
use super::arrangement_adapter::StripAsk;
use crate::orderflow_panel as panel;
use crate::surfaces::MarketPaletteSurface;
use crate::surfaces::replay_browser::ReplayBrowserExt as _;
use crate::surfaces::toast;
use crate::toolrail::ToolRailExt as _;
use crate::{chart_layers::shortcuts, tabstrip::TabAction, timezone::TzOffset};
use eframe::egui;
use quantick_control_schema::settings_sections::SettingsSection;
use quantick_session::{close_queue::CloseScope, workspace_menu::WorkspaceCommand};
use quantick_ui_model::market_palette::Intent as PaletteIntent;
use std::time::Instant;

use super::indicator_manager::IndicatorState;
use super::{ControlWindow, QuantickApp};
pub(super) use commands::{MenuCall, MenuCommand};
pub(in crate::app) use menus::{MenuDoor, WindowMenuState};
use quantick_control_handlers::desk::DeskAct;

mod commands;
mod help_window;
mod menus;
mod workspace_dialogs;
mod workspace_rows;

/// Opens the Market Replay browser (§10).
const REPLAY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);
/// Shows/hides the panels dock (§10).
/// `Ctrl+Shift+1` … `Ctrl+Shift+9` apply the layout registry's presets, in
/// table order.
///
/// The keys are listed; which preset each one reaches is not. A preset added
/// to `LAYOUT_PRESETS` gets its shortcut from its position without this array
/// or its dispatch being edited — the same rule the picker and the View menu
/// follow. Nine is what a number row has; `MAX_CANVAS_PANES` keeps the
/// registry far below that.
const LAYOUT_PRESET_KEYS: [egui::Key; 9] = [
    egui::Key::Num1,
    egui::Key::Num2,
    egui::Key::Num3,
    egui::Key::Num4,
    egui::Key::Num5,
    egui::Key::Num6,
    egui::Key::Num7,
    egui::Key::Num8,
    egui::Key::Num9,
];

/// The shortcut that reaches the preset at `index` in the registry, if a
/// number key still reaches that far.
fn layout_preset_shortcut(index: usize) -> Option<egui::KeyboardShortcut> {
    LAYOUT_PRESET_KEYS.get(index).map(|key| {
        egui::KeyboardShortcut::new(egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT), *key)
    })
}

/// The shortcut that reaches the layout tab at strip position `index`, if a
/// number key still reaches that far.
fn layout_tab_shortcut(index: usize) -> Option<egui::KeyboardShortcut> {
    LAYOUT_PRESET_KEYS
        .get(index)
        .map(|key| egui::KeyboardShortcut::new(egui::Modifiers::ALT, *key))
}

/// `Ctrl+0` puts the context charts away, or brings them back.
///
/// The number row's own zero, near `Ctrl+Shift+1..9` for the presets: nine
/// keys choose an arrangement and the tenth dismisses the column that arrangement
/// put beside the heatmap. Without it the only way to collapse was a drag,
/// which a trader working by keyboard cannot make and which WCAG 2.2's
/// dragging rule wants an alternative to besides.
const COLLAPSE_CONTEXT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Num0);

const DOCK_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::B,
);
/// Folds the focused pane's on-chart indicator legend to its count puck, or
/// opens it back up (see [`crate::indicator_legend`]).
///
/// Ctrl+letter like the dock's own switch above, not the bare `L` the drawing
/// tools answer to: bare letters are the toolbox's namespace, and a chrome
/// switch borrowing one would arm a tool on every trader who learned it there.
const LEGEND_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::L);
/// Saves the desk — the arrangement the next launch opens on.
///
/// Ctrl+Shift+S: Ctrl+S saves the tab's workspace (the capsule's floppy), so
/// a trader who reaches for it out of habit saves the layout they see and
/// never silently redefines what their platform opens on.
const SAVE_WORKSPACE_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::S,
);
/// Saves the active tab's edits to its workspace; with none it does nothing.
pub(crate) const SAVE_TO_WORKSPACE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::S);
/// Opens the status bar cell's workspace menu. Not Alt: Alt is the sell
/// modifier.
const WORKSPACE_MENU_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::O,
);
/// Opens the source picker for a new tab (§10).
pub(super) const NEW_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::T);
/// Shows Home.
pub(super) const HOME_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Num1);
/// The same, by letter.
pub(super) const HOME_ALT_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::H,
);
/// Closes the active tab (§10). Free of any other binding: the chart has no
/// text inputs and no document to "write".
pub(super) const CLOSE_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::W);
/// Cycles forward through the strip (§10).
pub(super) const NEXT_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Tab);
/// See [`NEXT_TAB_SHORTCUT`].
pub(super) const PREVIOUS_TAB_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::Tab,
);
/// Simulated buy at market (`docs/ux/paper-trading.md` §9). All the
/// trading hotkeys are Shift+letter and stand down while any text field
/// owns the keyboard — a capital letter typed into a symbol box must
/// never become an order.
pub(super) const PAPER_BUY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::B);
/// Simulated sell at market.
pub(super) const PAPER_SELL_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::S);
/// Reverse the simulated position.
pub(super) const PAPER_REVERSE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::R);
/// Flatten: close the position and cancel every working order.
pub(super) const PAPER_FLATTEN_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::F);
/// Cancel every working order without trading.
pub(super) const PAPER_CANCEL_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::X);
/// Opens the `⋮` window menu. Not `Alt`, which is the sell modifier in CMD
/// trading and the layout switch on `Alt+1…9`.
pub(super) const WINDOW_MENU_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F10);
/// Opens the Settings window.
const SETTINGS_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Comma);

/// A live market a close took goes to Recent with its bar; `+` opens the palette.
pub(super) fn settle_strip(
    palette: &mut MarketPaletteSurface,
    tz: TzOffset,
    (open, closed): StripAsk,
) {
    if let Some(tab) = closed.filter(|tab| tab.link.replay.is_none()) {
        let now = crate::metrics::wall_clock_ms();
        let stamp = quantick_civil::fmt_offset_minute(now, tz);
        let closed = quantick_workspace::market_list::ClosedMarket {
            feed_id: tab.feed_id.clone(),
            symbol: tab.symbol.clone(),
            at: stamp.get(11..).unwrap_or_default().to_owned(),
            price: last_price(&tab),
            bar: tab.flow_pane.series.bar_config(),
            context_bars: super::arrangement_adapter::saved_tab_of(&tab).context_bars,
        };
        palette.update(PaletteIntent::Remember(closed));
    }
    if open {
        palette.update(PaletteIntent::open("", None));
    }
}

pub(super) fn last_price(tab: &crate::tab::Tab) -> Option<String> {
    let bar = tab.flow_pane.series.state.bars().last()?;
    Some(bar.close.to_string())
}

/// A shortcut as the chrome prints it, with the one key egui spells out.
pub(super) fn shortcut_text(ctx: &egui::Context, shortcut: &egui::KeyboardShortcut) -> String {
    ctx.format_shortcut(shortcut).replace("Comma", ",")
}

impl QuantickApp {
    /// The two view keys, on a focused flow pane: one side alone, and from
    /// there back beside the other. Consumed whatever the focus, before `Ctrl+T`
    /// and `Ctrl+R` are read: those would match them too. On Home, with no
    /// chart shown, they are consumed and do nothing.
    pub(super) fn read_view_keys(&mut self, ctx: &egui::Context) {
        let chart_shown = !self.chrome.title_bar.surface.is_home();
        if shortcuts::take_view_press(ctx, shortcuts::ORDERFLOW_SETTINGS_SHORTCUT) && chart_shown {
            panel::toggle(self.active_tab_mut());
        }
        let Some(key) = shortcuts::take_view_key(ctx).filter(|_| chart_shown) else {
            return;
        };
        let tab = self.active_tab_mut();
        if tab.focused_side() == crate::pane::PaneSide::Flow
            && let Some(tape) = tab.flow_pane.orderflow.as_mut()
            && let Some(view) = tape.cached_config().live_lane.keyed_view(key)
        {
            tape.set_lane_view(view);
        }
    }

    /// Tab shortcuts (§10): `Ctrl+T` new, `Ctrl+W` close, `Ctrl+Tab` cycle; none under the palette.
    pub(super) fn handle_tab_keys(&mut self, ctx: &egui::Context) {
        self.read_view_keys(ctx);
        let mut keys = super::home_wiring::read_tab_keys(ctx);
        if !self.surfaces.market_palette.model_state.is_open() {
            if std::mem::take(&mut keys.close_tab) && !self.chrome.title_bar.surface.is_home() {
                self.apply_tab_action(TabAction::Close(self.runtime.tabs.active_index()));
            }
            let asked = self.arrangement_adapter().apply_tab_keys(keys);
            settle_strip(&mut self.surfaces.market_palette, self.runtime.tz, asked);
        }
    }

    /// Carry out what the tab strip asked for; a close asks first while the
    /// tab holds unsaved workspace edits.
    pub(super) fn apply_tab_action(&mut self, action: TabAction) {
        if let TabAction::Close(index) = action {
            let tab = (index < self.runtime.tabs.len()).then(|| self.runtime.tabs.id_at(index));
            let close = WorkspaceCommand::Close(CloseScope::Tabs, Vec::from_iter(tab));
            return self.workspace_calls().apply_workspace_command(close);
        }
        let asked = self.arrangement_adapter().apply_tab_action(action);
        settle_strip(&mut self.surfaces.market_palette, self.runtime.tz, asked);
    }

    /// The title strip and its `⋮` menu (§10): a shallow menu for discoverability
    /// and shortcuts, never the only path to anything.
    ///
    /// Two doors, one set of commands: the shortcuts are carried out before
    /// the strip is drawn, so it shows what they did, and whatever the menu
    /// and the tab strip record is carried out once it is drawn.
    pub(super) fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        let mut commands = Vec::new();
        let layout = self.active_tab().canvas.layout;
        let context_column = layout.shows_time() && layout.shows_flow();
        let chart_shown = !self.chrome.title_bar.surface.is_home();
        commands::read_shortcuts(ctx, context_column, chart_shown, &mut commands);
        for command in commands.drain(..) {
            apply_menu_command_on(self, ctx, command);
        }

        self.chrome.title_bar.menu_open = self
            .chrome
            .window_menu
            .opened_from(ctx.viewport_id(), MenuDoor::Kebab);
        self.chrome.title_bar.agent_on = self
            .control
            .control_access
            .as_ref()
            .is_some_and(crate::control::ControlAccess::is_enabled);
        let action = super::title_bar::draw(
            ctx,
            (&self.runtime, &self.config),
            &mut self.chrome.title_bar,
            &mut self.surfaces,
        );
        super::native_windows::apply_tab_action(self, action);
        if std::mem::take(&mut self.chrome.title_bar.kebab_clicked) {
            self.chrome.window_menu.toggle(ctx.viewport_id(), false);
        }
        if std::mem::take(&mut self.chrome.title_bar.agent_clicked) {
            commands.push(MenuCommand::OpenSettings(SettingsSection::AgentAccess));
        }
        super::chrome::menu_bar::draw(self, ctx, &mut commands);
        help_window::show(self, ctx);
        self.chrome.window_menu.dialog_rows = workspace_dialogs::dialogs(self, ctx, &mut commands);
        // The close or switch prompt: an answer settles its tab, an exit's last answer closes the app.
        let focused = self.runtime.tabs.windows().focused();
        let menu = &self.chrome.window_menu.tab_workspace;
        if let Some(answer) = super::native_windows::prompt_answer(focused, menu, ctx) {
            self.workspace_calls().apply_workspace_command(answer);
            let queue = &mut self.chrome.window_menu.tab_workspace.close;
            self.presentation.native.exit_when_settled(queue, ctx);
        }
        for command in commands {
            apply_menu_command_on(self, ctx, command);
        }
    }
}

pub(in crate::app) fn menu_shortcuts(
    ctx: &egui::Context,
) -> Vec<(quantick_ui_model::chrome_menu_bar::Shortcut, String)> {
    use quantick_control_schema::window_menu::RowId;
    use quantick_ui_model::chrome_menu_bar::Shortcut;
    [
        RowId::SaveToWorkspace,
        RowId::SaveDesk,
        RowId::TogglePanels,
        RowId::MarketReplay,
        RowId::Settings,
    ]
    .into_iter()
    .map(Shortcut::Row)
    .chain(
        (0..LAYOUT_PRESET_KEYS.len())
            .flat_map(|index| [Shortcut::Layout(index), Shortcut::Preset(index)]),
    )
    .chain([Shortcut::Context, Shortcut::Legend])
    .filter_map(|key| menu_shortcut(ctx, key).map(|text| (key, text)))
    .collect()
}
pub(crate) fn menu_shortcut(
    ctx: &egui::Context,
    shortcut: quantick_ui_model::chrome_menu_bar::Shortcut,
) -> Option<String> {
    use quantick_control_schema::window_menu::RowId;
    use quantick_ui_model::chrome_menu_bar::Shortcut;
    let binding = match shortcut {
        Shortcut::Row(RowId::SaveToWorkspace) => SAVE_TO_WORKSPACE_SHORTCUT,
        Shortcut::Row(RowId::SaveDesk) => SAVE_WORKSPACE_SHORTCUT,
        Shortcut::Row(RowId::TogglePanels) => DOCK_SHORTCUT,
        Shortcut::Row(RowId::MarketReplay) => REPLAY_SHORTCUT,
        Shortcut::Row(RowId::Settings) => SETTINGS_SHORTCUT,
        Shortcut::Row(_) => return None,
        Shortcut::Layout(index) => layout_tab_shortcut(index)?,
        Shortcut::Preset(index) => layout_preset_shortcut(index)?,
        Shortcut::Context => COLLAPSE_CONTEXT_SHORTCUT,
        Shortcut::Legend => LEGEND_SHORTCUT,
    };
    Some(shortcut_text(ctx, &binding))
}

/// The capsule's workspace name: a click, or Ctrl+Shift+O from anywhere in
/// the window, opens its menu below the name on this window's tab. A torn-off
/// window's capsule reaches its own tab and no other.
pub(super) fn workspace_cell(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    cell: Option<egui::Rect>,
    clicked: bool,
) {
    let (tab, window) = (
        app.runtime.tabs.active_id(),
        app.runtime.tabs.presented_window_ids(),
    );
    let pressed = ctx.input_mut(|input| input.consume_shortcut(&WORKSPACE_MENU_SHORTCUT));
    let mut menu = std::mem::take(&mut app.chrome.window_menu.tab_workspace);
    let mut cache = std::mem::take(&mut app.chrome.window_menu.cell_cache);
    let open = menu.follow_cell(tab, cell.is_some(), &window, clicked || pressed);
    let mut commands = Vec::new();
    let drawn = cell.filter(|_| open).map(|cell| {
        let keys = shortcut_text(ctx, &SAVE_TO_WORKSPACE_SHORTCUT);
        let rows = (&mut menu, &mut cache);
        workspace_rows::chip_menu(app, ctx, cell, rows, keys, &mut commands)
    });
    app.chrome.window_menu.tab_workspace = menu;
    app.chrome.window_menu.cell_cache = cache;
    for command in commands {
        apply_menu_command_on(app, ctx, command);
    }
    app.chrome.window_menu.cell_rows = drawn.unwrap_or_default();
}

/// Carry out one command a menu entry or shortcut recorded: a [`MenuCall`] through
/// its named call's door. The window arms below are legacy; never add one.
pub(super) fn apply_menu_command_on(app: &mut QuantickApp, ctx: &egui::Context, ask: MenuCommand) {
    if ask.needs_chart() && app.chrome.title_bar.surface.is_home() {
        return;
    }
    let viewport = ctx.viewport_id();
    // Settings and Help open on the window that asked.
    if matches!(ask, MenuCommand::OpenSettings(_) | MenuCommand::OpenHelp(_)) {
        app.chrome.window_menu.windows_viewport = Some(viewport);
    }
    match ask {
        MenuCommand::Call(call) => {
            if let Err(error) = call.run(app as &mut ControlWindow) {
                toast::note(&mut app.surfaces.toast, error.to_string(), Instant::now());
            }
        }
        MenuCommand::Workspace(command) => app.workspace_calls().apply_workspace_command(command),
        MenuCommand::OpenSettings(section) => {
            _ = MenuCall::Desk(DeskAct::OpenSettings(section)).run(app as &mut ControlWindow);
        }
        MenuCommand::OpenReplayBrowser => app.surfaces.replay_browser.open_browser(),
        MenuCommand::CloseReplay => {
            let (tab, config) = app.active_with_config();
            tab.close_replay(config);
        }
        MenuCommand::ToggleDock => app.surfaces.dock.toggle_visible(),
        MenuCommand::ToggleContextCharts => {
            let canvas = &mut app.active_tab_mut().canvas;
            canvas.fold(false, !canvas.folds().0 && canvas.column_painted());
        }
        MenuCommand::ToggleLegend => {
            let collapsed = !app.focused_pane().legend_collapsed;
            IndicatorState::set_legend_collapsed(app.focused_pane_mut(), collapsed);
        }
        MenuCommand::ForgetWorkspace => _ = app.workspace_save_adapter().forget_workspace(),
        MenuCommand::OpenRecentDesk(path) => {
            app.workspace_bundle_adapter().import_workspace_from(&path)
        }
        MenuCommand::RevealDesk => app.workspace_bundle_adapter().reveal_cockpit_home(),
        MenuCommand::SetToolboxDock(dock) => app.surfaces.toolrail.set_dock(dock),
        MenuCommand::ToggleToolbox => app.surfaces.toolrail.toggle_visible(),
        MenuCommand::OpenDockTab(tab) => app.surfaces.dock.open_tab(tab),
        MenuCommand::OpenReplayFormatHelp => app.surfaces.replay_browser.open_format_help(),
        MenuCommand::ToggleWindowMenu => app.chrome.window_menu.toggle(viewport, true),
        MenuCommand::OpenHelp(page) => app.chrome.window_menu.help = Some(page),
    }
}
