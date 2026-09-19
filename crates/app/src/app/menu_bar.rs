//! The menu bar and the tab strip (§10), and the shortcuts they mirror.
//!
//! Shallow menus for discoverability, never the only path to anything: every
//! entry that has a shortcut shows it, and the constants below are the one
//! place a key is bound. They live here rather than beside the struct
//! because the menu bar is their only production reader — the paper
//! shortcuts are re-exported to `super` for the tests that name them.

use eframe::egui;

use crate::tabstrip::{self, TabAction};
use crate::theme;

use super::QuantickApp;
use commands::MenuCommand;

mod commands;
mod menus;

/// Opens the Market Replay browser (§10).
const REPLAY_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::R);
/// Shows/hides the panels dock (§10).
/// `Ctrl+1` … `Ctrl+9` apply the layout registry's presets, in table order.
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
    LAYOUT_PRESET_KEYS
        .get(index)
        .map(|key| egui::KeyboardShortcut::new(egui::Modifiers::CTRL, *key))
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
/// The number row's own zero, beside `Ctrl+1..9` for the presets: nine keys
/// choose an arrangement and the tenth dismisses the column that arrangement
/// put beside the heatmap. Without it the only way to collapse was a drag,
/// which a trader working by keyboard cannot make and which WCAG 2.2's
/// dragging rule wants an alternative to besides.
const COLLAPSE_CONTEXT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Num0);

const DOCK_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::B);
/// Folds the focused pane's on-chart indicator legend to its count puck, or
/// opens it back up (see [`crate::indicator_legend`]).
///
/// Ctrl+letter like the dock's own switch above, not the bare `L` the drawing
/// tools answer to: bare letters are the toolbox's namespace, and a chrome
/// switch borrowing one would arm a tool on every trader who learned it there.
const LEGEND_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::L);
/// Saves the workspace — the arrangement the next launch opens on.
///
/// Ctrl+Shift+S rather than the Ctrl+S every editor uses, deliberately: a
/// chart has no document, and a trader who reaches for Ctrl+S out of habit
/// mid-session should hit nothing rather than silently redefine what their
/// platform opens on.
const SAVE_WORKSPACE_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::CTRL.plus(egui::Modifiers::SHIFT),
    egui::Key::S,
);
/// Opens the source picker for a new tab (§10).
pub(super) const NEW_TAB_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::T);
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
/// Height of the menu bar, in pixels (§5 zone 1).
const MENU_BAR_HEIGHT: f32 = 28.0;

impl QuantickApp {
    /// The window's menu bar (§10): shallow menus for discoverability and
    /// shortcuts, never the only path to anything.
    ///
    /// Two doors, one set of commands: the shortcuts are carried out before
    /// the bar is drawn, so it shows what they did, and whatever the menus
    /// and the tab strip record is carried out once the bar is drawn.
    pub(super) fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        let mut commands = Vec::new();
        let context_column =
            self.active_tab().layout.shows_time() && self.active_tab().layout.shows_flow();
        commands::read_shortcuts(ctx, context_column, &mut commands);
        for command in commands.drain(..) {
            self.apply_menu_command(command);
        }

        egui::TopBottomPanel::top("menu_bar")
            .exact_height(MENU_BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(6.0, 4.0)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| self.draw_menus(ui, &mut commands));
            });
        for command in commands {
            self.apply_menu_command(command);
        }
    }

    /// The menus left to right, then the tab strip, each handed exactly the
    /// state it shows.
    fn draw_menus(&mut self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        menus::FileMenu {
            tab_count: self.tabs.len(),
            active_index: self.tabs.active_index(),
            replaying: self.active_tab().replay.is_some(),
        }
        .show(ui, commands);
        menus::ViewMenu {
            layouts: self.layout_state(),
            tab: self.active_tab(),
            dock_visible: self.dock.visible(),
            toolrail: &self.toolrail,
            show_perf: self.health.show_perf,
            history: &self.history,
            tz: self.tz,
        }
        .show(ui, commands);
        let workspace_menu = menus::WorkspaceMenu {
            bundle: self.workspace_bundle_adapter(),
        }
        .show(ui, commands);
        self.chrome.workspace_menu_rect = Some(workspace_menu);
        let access_label = self
            .control
            .control_access
            .as_ref()
            .map_or("Local agent access…", |access| access.menu_label());
        menus::show_tools_menu(
            ui,
            access_label,
            |ui| super::deal_recording_wiring::draw_toggle(self, ui),
            commands,
        );
        menus::show_help_menu(ui, commands);
        let agent_access_on = self
            .control
            .control_access
            .as_ref()
            .is_some_and(crate::control::ControlAccess::is_enabled);
        menus::show_agent_access_indicator(ui, agent_access_on, commands);
        ui.separator();
        // The tab strip shares the menu row: zone 1 already had the
        // horizontal room, so tabs cost no chrome budget. Only a chip the
        // trader clicked records anything, so the strip's usual "nothing"
        // can no longer erase the File menu's New Tab… or Close Tab.
        if let Some(action) = self.draw_tab_strip(ui) {
            commands.push(MenuCommand::Tab(action));
        }
    }

    /// Carry out one command a menu entry or a menu shortcut recorded.
    fn apply_menu_command(&mut self, command: MenuCommand) {
        match command {
            MenuCommand::Tab(action) => self.apply_tab_action(action),
            MenuCommand::OpenReplayBrowser => self.replay_view.open_browser(),
            MenuCommand::CloseReplay => {
                let (tab, config) = self.active_with_config();
                tab.close_replay(config);
            }
            MenuCommand::ToggleDock => self.dock.toggle_visible(),
            MenuCommand::ToggleContextCharts => {
                let collapsed = self.active_tab().context_collapsed;
                self.active_tab_mut().set_context_collapsed(!collapsed);
            }
            MenuCommand::ApplyLayoutPreset(preset) => self.apply_layout_preset(preset),
            MenuCommand::SwitchLayoutIndex(index) => {
                if let Err(error) = self.layout_adapter().switch_layout_index(index) {
                    self.note_workspace(error.to_string());
                }
            }
            MenuCommand::Strip(action) => self.layout_adapter().apply_strip_action(action),
            MenuCommand::MoveContextChart { from, to } => {
                let tab_id = self.tabs.active_id();
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
            MenuCommand::ForgetWorkspace => self.workspace_save_adapter().forget_workspace(),
            MenuCommand::NameWorkspace => self.surfaces.workspace_name.open(),
            MenuCommand::OpenNamedWorkspace(name) => {
                self.arrangement_adapter().open_named_workspace(&name);
            }
            MenuCommand::DeleteNamedWorkspace(name) => {
                self.workspace_save_adapter().delete_named_workspace(&name);
            }
            MenuCommand::SetSaveOnExit(save_on_exit) => {
                self.workspace.session_mut().set_save_on_exit(save_on_exit);
                self.workspace_save_adapter()
                    .save_workspace("save_on_exit_toggled");
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
            MenuCommand::SetShowPerf(show) => self.health.show_perf = show,
            MenuCommand::SetProgressiveHistory(on) => self.history.progressive_history = on,
            MenuCommand::SetVenueLeadIn(on) => self.history.venue_lead_in = on,
            MenuCommand::SetTimezone(tz) => self.tz = tz,
            MenuCommand::OpenAppearance => self.surfaces.style_panel.open(),
            MenuCommand::OpenAgentAccess => {
                if let Some(access) = self.control.control_access.as_mut() {
                    access.open_panel();
                }
            }
            MenuCommand::OpenReplayFormatHelp => self.replay_view.open_format_help(),
        }
    }

    /// The chips, built from what each tab actually is right now.
    pub(super) fn draw_tab_strip(&self, ui: &mut egui::Ui) -> Option<TabAction> {
        let chips: Vec<tabstrip::TabChip<'_>> = self
            .tabs
            .iter()
            .map(|tab| tabstrip::TabChip {
                label: tab.chip_label(),
                replaying: tab.replay.is_some(),
                needs_attention: tab.needs_attention(),
            })
            .collect();
        tabstrip::draw(ui, &chips, self.tabs.active_index())
    }
}
