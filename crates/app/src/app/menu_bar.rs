//! The tab strip and the `⋮` window menu (§10), and the shortcuts they mirror.
//!
//! A shallow menu for discoverability, never the only path to anything: every
//! entry that has a shortcut shows it, and the constants below are the one
//! place a key is bound. They live here rather than beside the struct
//! because the window menu is their only production reader — the paper
//! shortcuts are re-exported to `super` for the tests that name them.

use super::arrangement_adapter::StripAsk;
use super::{GatewayPort, LayersPort, LayoutPort};
use crate::orderflow_panel as panel;
use crate::{chart_layers::shortcuts, tabstrip::TabAction, timezone::TzOffset};
use crate::{surfaces::ChartSettingsTab::Candles, surfaces::MarketPaletteSurface};
use eframe::egui;
use quantick_control_schema::settings_sections::SettingsSection;
use quantick_control_schema::window_menu::KEBAB_CONTROL;
use quantick_session::{close_queue::CloseScope, workspace_menu::WorkspaceCommand};

use super::QuantickApp;
pub(super) use commands::MenuCommand;
pub(in crate::app) use menus::{KEBAB_GLYPH, MenuDoor, WindowMenuState};

mod commands;
mod menus;
mod settings_window;
mod workspace_manage;
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
            bar: tab.flow_pane.state.spec().to_config_string(),
            context_bars: super::arrangement_adapter::saved_tab_of(&tab).context_bars,
        };
        palette.recent.closed(closed);
    }
    if open {
        palette.open("", None);
    }
}

pub(super) fn last_price(tab: &crate::tab::Tab) -> Option<String> {
    let bar = tab.flow_pane.state.bars().last()?;
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
        if !self.surfaces.market_palette.is_open() {
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
            return super::tab_workspaces::apply_workspace_command(self, close);
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
        let action = super::title_bar::draw(ctx, &self.runtime.tabs, &mut self.chrome.title_bar);
        super::native_windows::apply_tab_action(self, action);
        if std::mem::take(&mut self.chrome.title_bar.kebab_clicked) {
            self.chrome.window_menu.toggle(ctx.viewport_id(), false);
        }
        if std::mem::take(&mut self.chrome.title_bar.agent_clicked) {
            commands.push(MenuCommand::OpenSettings(SettingsSection::AgentAccess));
        }
        draw_window_menu(self, ctx, &mut commands);
        settings_window::show(self, ctx, &mut commands);
        workspace_rows::dialogs(self, ctx, &mut commands);
        // The close prompt: an answer settles its tab, an exit's last answer closes the app.
        let focused = self.runtime.tabs.windows().focused();
        let queue = &self.chrome.window_menu.tab_workspace.close;
        if let Some(answer) = super::native_windows::close_answer(focused, queue, ctx) {
            super::tab_workspaces::apply_workspace_command(self, answer);
            let queue = &mut self.chrome.window_menu.tab_workspace.close;
            self.presentation.native.exit_when_settled(queue, ctx);
        }
        for command in commands {
            apply_menu_command_on(self, ctx, command);
        }
    }

    /// Carry out one command a menu entry or a menu shortcut recorded.
    pub(super) fn apply_menu_command(&mut self, command: MenuCommand) {
        if command.needs_chart() && self.chrome.title_bar.surface.is_home() {
            return;
        }
        match command {
            MenuCommand::OpenAlerts => super::alerts_runtime::apply_alert_request(
                self,
                crate::surfaces::alerts::Request {
                    open: true,
                    ..Default::default()
                },
            ),
            MenuCommand::OpenReplayBrowser => self.replay_view.open_browser(),
            MenuCommand::CloseReplay => {
                let (tab, config) = self.active_with_config();
                tab.close_replay(config);
            }
            MenuCommand::ToggleDock => self.dock.toggle_visible(),
            MenuCommand::ToggleLayer(layer) => {
                let pane = self.focused_pane();
                let capabilities = self.active_tab().capabilities(&self.config);
                if pane.layer_blocked(layer, capabilities).is_none() {
                    let visible = !pane.layer_visible(layer, &self.runtime.style);
                    // The control plane's door, so a grid reaches the pane's style.
                    let side = self.active_tab().focused_side();
                    let tab = self.runtime.tabs.active_index();
                    self.layer_wiring().set_visible(tab, side, layer, visible);
                }
            }
            MenuCommand::ToggleContextCharts => {
                let canvas = &mut self.active_tab_mut().canvas;
                canvas.fold(false, !canvas.folds().0 && canvas.column_painted());
            }
            MenuCommand::ApplyLayoutPreset(preset) => self.apply_layout_preset(preset),
            MenuCommand::SwitchLayoutIndex(index) => {
                if let Err(error) = self.layout_adapter().switch_layout_index(index) {
                    self.surfaces
                        .toast
                        .note(error.to_string(), std::time::Instant::now());
                }
            }
            MenuCommand::Strip(action) => self.layout_adapter().apply_strip_action(action),
            MenuCommand::MoveContextChart { from, to } => {
                let tab_id = self.runtime.tabs.active_id();
                self.layout_adapter().move_context_pane_at(tab_id, from, to);
            }
            MenuCommand::ToggleLegend => {
                let collapsed = self.focused_pane().legend_collapsed;
                super::indicator_manager::IndicatorState::set_legend_collapsed(
                    self.focused_pane_mut(),
                    !collapsed,
                );
            }
            MenuCommand::TakeMark => self.take_mark(None),
            MenuCommand::SaveWorkspace(reason) => {
                self.workspace_save_adapter().save_workspace(reason);
            }
            MenuCommand::Workspace(command) => {
                super::tab_workspaces::apply_workspace_command(self, command);
            }
            MenuCommand::ForgetWorkspace => {
                self.workspace_save_adapter().forget_workspace();
            }
            MenuCommand::ExportDesk => super::desk_wiring::open_export(&mut self.runtime),
            MenuCommand::ImportDesk => self.runtime.workspace.picker_mut().open_import(),
            MenuCommand::OpenRecentDesk(path) => {
                self.workspace_bundle_adapter().import_workspace_from(&path);
            }
            MenuCommand::RevealDesk => self.workspace_bundle_adapter().reveal_cockpit_home(),
            MenuCommand::SetSaveOnExit(save_on_exit) => {
                super::desk_wiring::write_reopen(self.workspace_save_adapter(), save_on_exit);
            }
            MenuCommand::PaperMarket(side) => self.active_tab_mut().paper.market(side),
            MenuCommand::PaperReverse => self.active_tab_mut().paper.reverse_position(),
            MenuCommand::PaperFlatten => self.active_tab_mut().paper.flatten(),
            MenuCommand::PaperCancelAll => {
                self.active_tab_mut()
                    .paper
                    .account_mut()
                    .cancel_all_orders();
            }
            MenuCommand::SetToolboxDock(dock) => self.toolrail.set_dock(dock),
            MenuCommand::ToggleToolbox => self.toolrail.toggle_visible(),
            MenuCommand::OpenDockTab(tab) => self.dock.open_tab(tab),
            MenuCommand::OpenOrderFlowPanel(s) => panel::open_on(self.active_tab_mut(), s),
            MenuCommand::SetShowPerf(show) => self.health.show_perf = show,
            MenuCommand::SetProgressiveHistory(on) => self.runtime.history.progressive_history = on,
            MenuCommand::SetVenueLeadIn(on) => self.runtime.history.venue_lead_in = on,
            MenuCommand::SetTimezone(tz) => self.runtime.tz = tz,
            MenuCommand::OpenAppearance => self.layer_wiring().open_settings(Candles),
            MenuCommand::OpenAgentAccess => {
                if let Some(access) = self.control.control_access.as_mut() {
                    access.open_panel();
                }
            }
            MenuCommand::OpenReplayFormatHelp => self.replay_view.open_format_help(),
            // Carried out by `apply_menu_command_on`, which knows the window.
            MenuCommand::ToggleWindowMenu
            | MenuCommand::OpenSettings(_)
            | MenuCommand::OpenHelp(_) => {}
        }
    }
}

/// The menu's panes, each handed exactly the state it shows.
fn draw_window_menu(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    let door = match app.chrome.window_menu.door() {
        MenuDoor::Kebab => app.chrome.title_bar.control_rect(KEBAB_CONTROL),
        MenuDoor::Toolbar => app.chrome.window_menu.toolbar_door,
    };
    let Some(door) = door else {
        return;
    };
    let mut state = std::mem::take(&mut app.chrome.window_menu);
    let workspace = workspace_rows::WorkspaceRows::of(app);
    menus::WindowMenu {
        workspace,
        chart_shown: !app.chrome.title_bar.surface.is_home(),
        replaying: app.active_tab().link.replay.is_some(),
        layouts: app.layout_state(),
        tab: app.active_tab(),
        dock_visible: app.dock.visible(),
        dock_tab: app.dock.tab(),
        toolrail: &app.toolrail,
        panel_folds: app.runtime.tabs.panel_folds(),
        startup_saved: app.runtime.workspace.session().saved(),
        save_on_exit: app.runtime.workspace.session().save_on_exit(),
        recent: app.runtime.workspace.recent_on_disk(),
        focus_live_row: std::cell::Cell::new(false),
    }
    .show(ctx, &mut state, door, commands);
    app.chrome.window_menu = state;
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
    let open = menu.follow_cell(tab, cell.is_some(), &window, clicked || pressed);
    let mut commands = Vec::new();
    let drawn = cell.filter(|_| open).map(|cell| {
        let keys = shortcut_text(ctx, &SAVE_TO_WORKSPACE_SHORTCUT);
        let rows = workspace_rows::WorkspaceRows::of(app);
        rows.chip_menu(ctx, cell, &mut menu, keys, &mut commands)
    });
    app.chrome.window_menu.tab_workspace = menu;
    for command in commands {
        apply_menu_command_on(app, ctx, command);
    }
    app.chrome.window_menu.cell_rows = drawn.unwrap_or_default();
}

/// The commands that need to know which window asked, then all the others.
fn apply_menu_command_on(app: &mut QuantickApp, ctx: &egui::Context, command: MenuCommand) {
    let viewport = ctx.viewport_id();
    match command {
        MenuCommand::ToggleWindowMenu => app.chrome.window_menu.toggle(viewport, true),
        MenuCommand::OpenSettings(section) => {
            app.chrome.window_menu.open_settings(viewport, section);
        }
        MenuCommand::OpenHelp(page) => {
            app.chrome.window_menu.windows_viewport = Some(viewport);
            app.chrome.window_menu.help = Some(page);
        }
        command => app.apply_menu_command(command),
    }
}
