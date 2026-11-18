//! What the window menu and its shortcuts ask the window to do.
//!
//! The menus and the keyboard are two doors onto one set of commands: each
//! view records a [`MenuCommand`] instead of reaching into the window, and
//! [`super::apply_menu_command_on`] carries them out in the order they were
//! recorded. A [`MenuCall`] goes through the port its named call's handler
//! takes, so a click and an agent reach the same owner method.

use eframe::egui;
use quantick_control_handlers::{
    alerts::AlertDefinitionsPort, desk::DeskAct, layout::LayoutHost,
    orderflow_settings::OrderflowSettingsPort, trade::PaperHost,
};
use quantick_control_schema::{settings_sections::SettingsSection, window_menu::HelpPage};
use quantick_session::{paper::TradeCommand, workspace_menu::WorkspaceCommand};

use crate::app::{ChromePort, GatewayPort, LayersPort, TabsPort};
use crate::layouts::LayoutError;
use crate::{canvas_layout::LayoutPreset, chart_layers::ChartLayer, dock::DockTab};
use crate::{layout_strip::StripAction, toolrail::ToolboxDock, ui_state::PanelSection};

use super::{
    COLLAPSE_CONTEXT_SHORTCUT, DOCK_SHORTCUT, LAYOUT_PRESET_KEYS, LEGEND_SHORTCUT,
    PAPER_BUY_SHORTCUT, PAPER_CANCEL_SHORTCUT, PAPER_FLATTEN_SHORTCUT, PAPER_REVERSE_SHORTCUT,
    PAPER_SELL_SHORTCUT, REPLAY_SHORTCUT, SAVE_TO_WORKSPACE_SHORTCUT, SAVE_WORKSPACE_SHORTCUT,
    SETTINGS_SHORTCUT, WINDOW_MENU_SHORTCUT, layout_preset_shortcut, layout_tab_shortcut,
};

/// One thing a menu entry or shortcut asked for. Variants after [`Self::OpenSettings`]
/// are legacy window arms: a new command is a named call and a [`MenuCall`].
#[derive(Debug)]
pub(in crate::app) enum MenuCommand {
    /// What a named call also does, through the door its handler takes.
    Call(MenuCall),
    /// A workspace menu row: `apply_row_command`, as the `workspace.*` calls.
    Workspace(WorkspaceCommand),
    /// `settings.open`'s desk act, on the window that asked.
    OpenSettings(SettingsSection),
    OpenReplayBrowser,
    CloseReplay,
    ToggleDock,
    /// Put the context charts away, or bring them back.
    ToggleContextCharts,
    /// Fold the focused pane's indicator legend, or open it back up.
    ToggleLegend,
    /// `desk.forget`'s owner, without the call's skip of a file with no tabs.
    ForgetWorkspace,
    OpenRecentDesk(std::path::PathBuf),
    RevealDesk,
    SetToolboxDock(ToolboxDock),
    ToggleToolbox,
    OpenDockTab(DockTab),
    OpenReplayFormatHelp,
    /// `F10`: open or close the `⋮` window menu.
    ToggleWindowMenu,
    OpenHelp(HelpPage),
}

/// A menu command sent through the port the named calls' handlers take.
#[derive(Debug)]
pub(in crate::app) enum MenuCall {
    /// `alerts.open`.
    OpenAlerts,
    /// `layers.visibility.set`: flip it on the focused pane, unless blocked.
    ToggleLayer(ChartLayer),
    /// `layout.preset.apply` on the active tab.
    ApplyLayoutPreset(&'static LayoutPreset),
    /// `layout.tab.switch` to the layout at this strip position.
    SwitchLayoutIndex(usize),
    /// `layout.tab.switch`, `.create`; rename box and delete have no call yet.
    Strip(StripAction),
    /// `layout.pane.move`: the active tab's context chart at `from` to `to`.
    MoveContextChart { from: usize, to: usize },
    /// `attention.mark.create`.
    TakeMark,
    /// `desk.*`.
    Desk(DeskAct),
    /// `trade.*`: the active desk's `TradeDesk::execute`.
    Trade(TradeCommand),
    /// `orderflow.settings.open` on the active tab, while its flow pane shows.
    OpenOrderFlowPanel(PanelSection),
}

impl MenuCommand {
    /// Whether this changes or trades the chart on screen, so means nothing
    /// while Home hides it. Tabs, replay browsing and the window's own chrome
    /// are not the chart's.
    pub(in crate::app) const fn needs_chart(&self) -> bool {
        matches!(
            self,
            Self::CloseReplay
                | Self::ToggleContextCharts
                | Self::ToggleLegend
                | Self::Call(
                    MenuCall::ToggleLayer(_)
                        | MenuCall::ApplyLayoutPreset(_)
                        | MenuCall::SwitchLayoutIndex(_)
                        | MenuCall::Strip(_)
                        | MenuCall::MoveContextChart { .. }
                        | MenuCall::OpenOrderFlowPanel(_)
                        | MenuCall::Trade(_)
                )
        )
    }
}

impl MenuCall {
    /// Carry the command out through the port its named call's handler
    /// takes. A refusal the trader should read comes back as the error.
    pub(in crate::app) fn run<W>(self, window: &mut W) -> Result<(), LayoutError>
    where
        W: TabsPort
            + ChromePort
            + LayersPort
            + LayoutHost
            + GatewayPort
            + AlertDefinitionsPort
            + PaperHost
            + OrderflowSettingsPort
            + ?Sized,
    {
        match self {
            Self::OpenAlerts => _ = window.alert_open(),
            Self::ToggleLayer(layer) => {
                let (reads, style) = (window.tab_reads(), window.chrome_reads().style());
                let index = reads.active_tab_index();
                let tab = &reads.tabs()[index];
                let (pane, side) = (tab.focused_pane(), tab.focused_side());
                let blocked = pane.layer_blocked(layer, tab.capabilities(reads.config()));
                let visible = !pane.layer_visible(layer, style);
                if blocked.is_none() {
                    window
                        .layer_wiring()
                        .set_visible(index, side, layer, visible);
                }
            }
            Self::ApplyLayoutPreset(preset) => {
                let index = window.active_tab_index();
                _ = window.layout_tab(index).map(|tab| tab.pick_preset(preset));
            }
            Self::SwitchLayoutIndex(index) => {
                let switched = window.workspace_calls().layout().switch_layout_index(index);
                return switched.map(|_| ());
            }
            Self::Strip(action) => window.workspace_calls().layout().apply_strip_action(action),
            Self::MoveContextChart { from, to } => {
                let tab = window.tab_reads().tabs().active_id();
                window.move_context_pane(tab, from, to);
            }
            Self::TakeMark => window.take_mark(None),
            Self::Desk(act) => _ = window.desk().map(|mut desk| desk.act(act)),
            Self::Trade(command) => {
                _ = window
                    .paper()
                    .active_desk_mut()
                    .map(|desk| desk.execute(command));
            }
            Self::OpenOrderFlowPanel(section) => {
                let tab = window.tab_reads().tabs().active_id();
                _ = window
                    .panel_seat(tab)
                    .and_then(|mut seat| seat.open(Some(section)));
            }
        }
        Ok(())
    }
}

/// The menu shortcuts pressed this frame, as commands, in the order they
/// are consumed.
///
/// `context_column` is whether the active layout carves a context column
/// *beside* the flow pane — the same condition the View menu's entry is
/// gated on. Ungated, `Ctrl+0` on the Flow or Timeframe layout set a flag
/// nothing drew — and swallowed the key besides, so egui's own "reset zoom"
/// never ran.
///
/// `chart_on_screen` is false while Home covers the canvas: what would change a
/// chart or trade from it stands down, because a trader cannot see what those
/// keys did. Nothing else in the window depends on them.
pub(super) fn read_shortcuts(
    ctx: &egui::Context,
    context_column: bool,
    chart_on_screen: bool,
    commands: &mut Vec<MenuCommand>,
) {
    let pressed =
        |shortcut: &egui::KeyboardShortcut| ctx.input_mut(|input| input.consume_shortcut(shortcut));
    if pressed(&WINDOW_MENU_SHORTCUT) {
        commands.push(MenuCommand::ToggleWindowMenu);
    }
    if pressed(&SETTINGS_SHORTCUT) {
        commands.push(MenuCommand::OpenSettings(SettingsSection::Display));
    }
    if pressed(&REPLAY_SHORTCUT) {
        commands.push(MenuCommand::OpenReplayBrowser);
    }
    if pressed(&DOCK_SHORTCUT) {
        commands.push(MenuCommand::ToggleDock);
    }
    if chart_on_screen && !ctx.wants_keyboard_input() {
        for layer in [
            crate::chart_layers::ChartLayer::Bubbles,
            crate::chart_layers::ChartLayer::Footprint,
        ] {
            if let Some(shortcut) = crate::chart_layers::shortcuts::binding(layer)
                && pressed(&shortcut)
            {
                commands.push(MenuCommand::Call(MenuCall::ToggleLayer(layer)));
            }
        }
    }
    if chart_on_screen && context_column && pressed(&COLLAPSE_CONTEXT_SHORTCUT) {
        commands.push(MenuCommand::ToggleContextCharts);
    }
    // Layout by number, straight off the registry. The same
    // `tab::layout::pick` the picker and the menu call — three doors,
    // one room.
    for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
        let Some(shortcut) = layout_preset_shortcut(index) else {
            break;
        };
        if chart_on_screen && pressed(&shortcut) {
            commands.push(MenuCommand::Call(MenuCall::ApplyLayoutPreset(preset)));
        }
    }
    // Layout tabs by number: `Alt+1..9`, beside `Ctrl+Shift+1..9` for the
    // presets — one row of keys, two things a trader switches by number.
    // Not while a text field has the keyboard — a rename box, a note,
    // the ticket — where Alt+1 is text, not a switch.
    let typing = ctx.memory(|memory| memory.focused().is_some());
    for index in 0..LAYOUT_PRESET_KEYS.len() {
        let Some(shortcut) = layout_tab_shortcut(index) else {
            break;
        };
        if chart_on_screen && !typing && pressed(&shortcut) {
            commands.push(MenuCommand::Call(MenuCall::SwitchLayoutIndex(index)));
        }
    }
    if chart_on_screen && pressed(&LEGEND_SHORTCUT) {
        commands.push(MenuCommand::ToggleLegend);
    }
    if pressed(&crate::control::MARK_SHORTCUT) {
        commands.push(MenuCommand::Call(MenuCall::TakeMark));
    }
    if pressed(&SAVE_WORKSPACE_SHORTCUT) {
        commands.push(MenuCommand::Call(MenuCall::Desk(DeskAct::Save("shortcut"))));
    }
    // After the desk's Ctrl+Shift+S: a logical match ignores an extra Shift.
    if chart_on_screen && pressed(&SAVE_TO_WORKSPACE_SHORTCUT) {
        commands.push(MenuCommand::Workspace(WorkspaceCommand::SaveTo));
    }
    // Trading hotkeys, swallowed only while no text field owns the
    // keyboard. Market entries use the ticket's quantity and offsets,
    // exactly like the toolbar buttons they twin.
    if chart_on_screen && !ctx.wants_keyboard_input() {
        let market = |side| TradeCommand::Market { side };
        for (shortcut, command) in [
            (PAPER_BUY_SHORTCUT, market(quantick_engine::Side::Buy)),
            (PAPER_SELL_SHORTCUT, market(quantick_engine::Side::Sell)),
            (PAPER_REVERSE_SHORTCUT, TradeCommand::Reverse),
            (PAPER_FLATTEN_SHORTCUT, TradeCommand::Flatten),
            (PAPER_CANCEL_SHORTCUT, TradeCommand::CancelAll),
        ] {
            if pressed(&shortcut) {
                commands.push(MenuCommand::Call(MenuCall::Trade(command)));
            }
        }
    }
}
