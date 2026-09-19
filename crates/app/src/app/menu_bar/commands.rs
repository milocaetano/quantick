//! What the menu bar and its shortcuts ask the window to do.
//!
//! The menus and the keyboard are two doors onto one set of commands: each
//! view records a [`MenuCommand`] instead of reaching into the window, and
//! [`super::QuantickApp::apply_menu_command`] is the single place a command
//! is carried out. Commands are applied in the order they were recorded.

use eframe::egui;

use crate::canvas_layout::LayoutPreset;
use crate::dock::DockTab;
use crate::layout_strip::StripAction;
use crate::tabstrip::TabAction;
use crate::timezone::TzOffset;
use crate::toolrail::ToolboxDock;

use super::{
    COLLAPSE_CONTEXT_SHORTCUT, DOCK_SHORTCUT, LAYOUT_PRESET_KEYS, LEGEND_SHORTCUT,
    PAPER_BUY_SHORTCUT, PAPER_CANCEL_SHORTCUT, PAPER_FLATTEN_SHORTCUT, PAPER_REVERSE_SHORTCUT,
    PAPER_SELL_SHORTCUT, REPLAY_SHORTCUT, SAVE_WORKSPACE_SHORTCUT, layout_preset_shortcut,
    layout_tab_shortcut,
};

/// One thing a menu entry or a menu shortcut asked for.
#[derive(Debug)]
pub(super) enum MenuCommand {
    /// What the tab strip, or the File menu's tab entries, asked for.
    Tab(TabAction),
    OpenReplayBrowser,
    CloseReplay,
    ToggleDock,
    /// Put the context charts away, or bring them back.
    ToggleContextCharts,
    ApplyLayoutPreset(&'static LayoutPreset),
    /// Switch to the layout tab at this strip position.
    SwitchLayoutIndex(usize),
    Strip(StripAction),
    /// Move the active tab's context chart at `from` to `to`.
    MoveContextChart {
        from: usize,
        to: usize,
    },
    /// Fold the focused pane's indicator legend, or open it back up.
    ToggleLegend,
    TakeMark,
    /// Save the workspace; the reason is what the save event records.
    SaveWorkspace(&'static str),
    ForgetWorkspace,
    /// Open the dialog that names a workspace bookmark.
    NameWorkspace,
    OpenNamedWorkspace(String),
    DeleteNamedWorkspace(String),
    SetSaveOnExit(bool),
    PaperMarket(quantick_engine::Side),
    PaperReverse,
    PaperFlatten,
    PaperCancelAll,
    SetToolboxDock(ToolboxDock),
    ToggleToolbox,
    OpenDockTab(DockTab),
    SetShowPerf(bool),
    SetProgressiveHistory(bool),
    SetVenueLeadIn(bool),
    SetTimezone(TzOffset),
    OpenAppearance,
    OpenAgentAccess,
    OpenReplayFormatHelp,
}

/// The menu shortcuts pressed this frame, as commands, in the order they
/// are consumed.
///
/// `context_column` is whether the active layout carves a context column
/// *beside* the flow pane — the same condition the View menu's entry is
/// gated on. Ungated, `Ctrl+0` on the Flow or Timeframe layout set a flag
/// nothing drew — and swallowed the key besides, so egui's own "reset zoom"
/// never ran.
pub(super) fn read_shortcuts(
    ctx: &egui::Context,
    context_column: bool,
    commands: &mut Vec<MenuCommand>,
) {
    let pressed =
        |shortcut: &egui::KeyboardShortcut| ctx.input_mut(|input| input.consume_shortcut(shortcut));
    if pressed(&REPLAY_SHORTCUT) {
        commands.push(MenuCommand::OpenReplayBrowser);
    }
    if pressed(&DOCK_SHORTCUT) {
        commands.push(MenuCommand::ToggleDock);
    }
    if context_column && pressed(&COLLAPSE_CONTEXT_SHORTCUT) {
        commands.push(MenuCommand::ToggleContextCharts);
    }
    // Layout by number, straight off the registry. The same
    // `apply_layout_preset` the picker and the menu call — three doors,
    // one room.
    for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
        let Some(shortcut) = layout_preset_shortcut(index) else {
            break;
        };
        if pressed(&shortcut) {
            commands.push(MenuCommand::ApplyLayoutPreset(preset));
        }
    }
    // Layout tabs by number: `Alt+1..9`, beside `Ctrl+1..9` for the
    // presets — one row of keys, two things a trader switches by number.
    // Not while a text field has the keyboard — a rename box, a note,
    // the ticket — where Alt+1 is text, not a switch.
    let typing = ctx.memory(|memory| memory.focused().is_some());
    for index in 0..LAYOUT_PRESET_KEYS.len() {
        let Some(shortcut) = layout_tab_shortcut(index) else {
            break;
        };
        if !typing && pressed(&shortcut) {
            commands.push(MenuCommand::SwitchLayoutIndex(index));
        }
    }
    if pressed(&LEGEND_SHORTCUT) {
        commands.push(MenuCommand::ToggleLegend);
    }
    if pressed(&crate::control::MARK_SHORTCUT) {
        commands.push(MenuCommand::TakeMark);
    }
    if pressed(&SAVE_WORKSPACE_SHORTCUT) {
        commands.push(MenuCommand::SaveWorkspace("shortcut"));
    }
    // Trading hotkeys, swallowed only while no text field owns the
    // keyboard. Market entries use the ticket's quantity and offsets,
    // exactly like the toolbar buttons they twin.
    if !ctx.wants_keyboard_input() {
        for (shortcut, command) in [
            (
                PAPER_BUY_SHORTCUT,
                MenuCommand::PaperMarket(quantick_engine::Side::Buy),
            ),
            (
                PAPER_SELL_SHORTCUT,
                MenuCommand::PaperMarket(quantick_engine::Side::Sell),
            ),
            (PAPER_REVERSE_SHORTCUT, MenuCommand::PaperReverse),
            (PAPER_FLATTEN_SHORTCUT, MenuCommand::PaperFlatten),
            (PAPER_CANCEL_SHORTCUT, MenuCommand::PaperCancelAll),
        ] {
            if pressed(&shortcut) {
                commands.push(command);
            }
        }
    }
}
