//! Paints the `⋮` window menu from its headless table.
//!
//! The groups, labels and tooltips live in
//! [`quantick_control_schema::window_menu`]; this view adds the live state and
//! the pixels, and records what the trader chose as a [`MenuCommand`]. egui's
//! own submenus open to the right, which from the window's right edge lands off
//! screen, so these panes are anchored by their top right corner.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_control_schema::window_menu::{
    HelpPage, Item, LiveRows, ROOT, RowId, SubmenuId, row_of, submenu,
};

use crate::dock::DockTab;
use crate::layout_strip::StripAction;
use crate::tab::Tab;
use crate::toolrail::{ToolRail, ToolboxDock};

use super::super::layout_wiring::LayoutRead;
use super::commands::MenuCommand;
use super::{
    COLLAPSE_CONTEXT_SHORTCUT, DOCK_SHORTCUT, LEGEND_SHORTCUT, REPLAY_SHORTCUT,
    SAVE_WORKSPACE_SHORTCUT, SETTINGS_SHORTCUT, layout_preset_shortcut, layout_tab_shortcut,
};

/// The `⋮` button's glyph.
pub(in crate::app) const KEBAB_GLYPH: &str = icons::DOTS_THREE_VERTICAL;

const PANE_MIN_WIDTH_PX: f32 = 240.0;
/// The gap between a pane and the pane that opened from it.
const SUBMENU_GAP_PX: f32 = 2.0;
/// How far a submenu rides above its row, so its first line sits level with it.
const SUBMENU_LIFT_PX: f32 = 6.0;
/// Submenu levels the table allows below the root.
const MAX_SUBMENU_LEVELS: usize = 2;

/// Whether the menu is up, and which submenus are open along the way.
#[derive(Default)]
pub(in crate::app) struct WindowMenuState {
    pub(in crate::app) open: bool,
    pub(in crate::app) viewport: Option<egui::ViewportId>,
    pub(in crate::app) settings_open: bool,
    pub(in crate::app) help: Option<HelpPage>,
    path: [Option<SubmenuId>; MAX_SUBMENU_LEVELS],
    /// The pane whose first row takes the keyboard next.
    focus_pane: Option<usize>,
    panes: Vec<egui::Rect>,
}

impl WindowMenuState {
    /// Open or close the menu on this window. Opened from the keyboard, its first
    /// row is ready for the keys; opened by the mouse, no row wears a focus ring.
    pub(in crate::app) fn toggle(&mut self, viewport: egui::ViewportId, by_keyboard: bool) {
        let open = !self.open;
        self.close();
        self.open = open;
        self.viewport = Some(viewport);
        self.focus_pane = (open && by_keyboard).then_some(0);
    }

    fn close(&mut self) {
        self.open = false;
        self.path = [None; MAX_SUBMENU_LEVELS];
        self.focus_pane = None;
        self.panes.clear();
    }
}

/// The live state one frame of the menu shows.
pub(super) struct WindowMenu<'a> {
    /// False while Home covers the canvas: the chart rows grey out.
    pub(super) chart_shown: bool,
    pub(super) replaying: bool,
    pub(super) layouts: LayoutRead<'a>,
    pub(super) tab: &'a Tab,
    pub(super) dock_visible: bool,
    pub(super) dock_tab: Option<DockTab>,
    pub(super) toolrail: &'a ToolRail,
    pub(super) startup_saved: bool,
    pub(super) save_on_exit: bool,
    pub(super) recent: &'a [std::path::PathBuf],
}

impl WindowMenu<'_> {
    /// Draw the open panes, anchored under the `⋮` button at `kebab`.
    pub(super) fn show(
        &self,
        ctx: &egui::Context,
        state: &mut WindowMenuState,
        kebab: egui::Rect,
        commands: &mut Vec<MenuCommand>,
    ) {
        if !state.open || state.viewport != Some(ctx.viewport_id()) {
            return;
        }
        let recorded = commands.len();
        state.panes.clear();
        self.pane(ctx, state, 0, ROOT, kebab.right_bottom(), commands);
        // A switch stays up so the trader sees it flip; anything else is done.
        let acted = commands[recorded..]
            .iter()
            .any(|command| !matches!(command, MenuCommand::SetSaveOnExit(_)));
        let dismissed = ctx.input_mut(|input| {
            let escape = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            let outside = input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|at| {
                    !kebab.contains(at) && !state.panes.iter().any(|pane| pane.contains(at))
                });
            escape || outside
        });
        if acted || dismissed {
            state.close();
        }
    }

    fn pane(
        &self,
        ctx: &egui::Context,
        state: &mut WindowMenuState,
        level: usize,
        items: &[Item],
        top_right: egui::Pos2,
        commands: &mut Vec<MenuCommand>,
    ) {
        let mut child = None;
        let mut over_submenu = false;
        let area = egui::Area::new(egui::Id::new(("window_menu_pane", level)))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(top_right)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(PANE_MIN_WIDTH_PX);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
                        // Rows read as lines of a list until the pointer is on one.
                        let idle = &mut ui.visuals_mut().widgets.inactive;
                        idle.weak_bg_fill = egui::Color32::TRANSPARENT;
                        idle.bg_stroke = egui::Stroke::NONE;
                        for item in items {
                            match *item {
                                Item::Separator => {
                                    ui.separator();
                                }
                                Item::Caption(text) => {
                                    ui.weak(text);
                                }
                                Item::Row(id) => self.row(ui, state, level, id, commands),
                                Item::Live(rows) => self.live_rows(ui, rows, commands),
                                Item::Submenu(id) => {
                                    let Some(row) = self.submenu_row(ui, state, level, id) else {
                                        continue;
                                    };
                                    over_submenu |= row.hovered();
                                    if state.path[level] == Some(id) {
                                        child = Some((
                                            id,
                                            row.rect.left_top()
                                                + egui::vec2(-SUBMENU_GAP_PX, -SUBMENU_LIFT_PX),
                                        ));
                                    }
                                }
                            }
                        }
                    });
                });
            });
        state.panes.push(area.response.rect);
        // Moving onto a plain row puts away the submenu a sibling opened.
        let moved = ctx.input(|input| input.pointer.delta() != egui::Vec2::ZERO);
        if moved && !over_submenu && area.response.contains_pointer() && level < state.path.len() {
            state.path[level] = None;
            child = None;
        }
        if let Some((id, at)) = child {
            self.pane(ctx, state, level + 1, submenu(id).1, at, commands);
        }
    }

    /// A submenu's own row, or nothing when it has nothing to hold.
    fn submenu_row(
        &self,
        ui: &mut egui::Ui,
        state: &mut WindowMenuState,
        level: usize,
        id: SubmenuId,
    ) -> Option<egui::Response> {
        if id == SubmenuId::MoveChart && self.tab.pane_count().saturating_sub(1) < 2 {
            return None;
        }
        let empty = id == SubmenuId::OpenRecent && self.recent.is_empty();
        let chart_row = matches!(
            id,
            SubmenuId::Layouts | SubmenuId::Layout | SubmenuId::MoveChart
        );
        let response = ui
            .add_enabled(
                !empty && (self.chart_shown || !chart_row),
                egui::Button::new(submenu(id).0)
                    .selected(state.path[level] == Some(id))
                    .shortcut_text(icons::CARET_LEFT),
            )
            .on_disabled_hover_text(if empty {
                "No workspace files opened yet"
            } else {
                "Home is on screen: pick a market first"
            });
        take_focus(state, level, &response);
        if response.hovered() || response.clicked() {
            if state.path[level] != Some(id) && level + 1 < state.path.len() {
                state.path[level + 1] = None;
            }
            state.path[level] = Some(id);
            if response.clicked() {
                state.focus_pane = Some(level + 1);
            }
        }
        Some(response)
    }

    fn row(
        &self,
        ui: &mut egui::Ui,
        state: &mut WindowMenuState,
        level: usize,
        id: RowId,
        commands: &mut Vec<MenuCommand>,
    ) {
        if id == RowId::CloseReplay && !self.replaying {
            return;
        }
        let row = row_of(id);
        if id == RowId::ReopenLastTabs {
            let mut save_on_exit = self.save_on_exit;
            let response = ui
                .add(egui::Checkbox::new(&mut save_on_exit, row.label))
                .on_hover_text(row.hover.unwrap_or_default());
            take_focus(state, level, &response);
            if response.changed() {
                // The setting lives in the file it governs, so switching it
                // has to reach the disk now, not at the exit it may switch off.
                commands.push(MenuCommand::SetSaveOnExit(save_on_exit));
            }
            return;
        }
        let label = match row.label_when_on {
            Some(on) if id == RowId::TogglePanels && self.dock_visible => on,
            _ => row.label,
        };
        let mut button = egui::Button::new(label).selected(self.selected(id));
        if let Some(shortcut) = shortcut_of(id) {
            let keys = ui.ctx().format_shortcut(&shortcut).replace("Comma", ",");
            button = button.shortcut_text(keys);
        }
        let mut response = ui.add_enabled(self.enabled(id), button);
        if let Some(hover) = row.hover {
            response = response.on_hover_text(hover);
        }
        if let Some(hover) = row.disabled_hover {
            response = response.on_disabled_hover_text(hover);
        }
        take_focus(state, level, &response);
        if response.clicked() {
            self.push_row_commands(id, commands);
        }
    }

    const fn enabled(&self, id: RowId) -> bool {
        match id {
            RowId::CommandPalette => false,
            RowId::ResetStartupDesk => self.startup_saved,
            _ => true,
        }
    }

    fn selected(&self, id: RowId) -> bool {
        if let Some(tab) = dock_tab_of(id) {
            return self.dock_visible && self.dock_tab == Some(tab);
        }
        if let Some(dock) = toolbox_dock_of(id) {
            return self.toolrail.visible() && self.toolrail.dock() == dock;
        }
        id == RowId::ToolbarHidden && !self.toolrail.visible()
    }

    fn push_row_commands(&self, id: RowId, commands: &mut Vec<MenuCommand>) {
        if let Some(tab) = dock_tab_of(id) {
            commands.push(MenuCommand::OpenDockTab(tab));
            return;
        }
        if let Some(dock) = toolbox_dock_of(id) {
            commands.push(MenuCommand::SetToolboxDock(dock));
            if !self.toolrail.visible() {
                commands.push(MenuCommand::ToggleToolbox);
            }
            return;
        }
        commands.push(match id {
            RowId::SaveDesk => MenuCommand::SaveWorkspace("menu"),
            RowId::TogglePanels => MenuCommand::ToggleDock,
            RowId::MarketReplay => MenuCommand::OpenReplayBrowser,
            RowId::CloseReplay => MenuCommand::CloseReplay,
            RowId::Settings => MenuCommand::OpenSettings,
            RowId::ToolbarHidden if self.toolrail.visible() => MenuCommand::ToggleToolbox,
            RowId::ExportDesk => MenuCommand::ExportDesk,
            RowId::ImportDesk => MenuCommand::ImportDesk,
            RowId::RevealDesk => MenuCommand::RevealDesk,
            RowId::ResetStartupDesk => MenuCommand::ForgetWorkspace,
            RowId::KeyboardShortcuts => MenuCommand::OpenHelp(HelpPage::KeyboardShortcuts),
            RowId::ReplayFileFormat => MenuCommand::OpenReplayFormatHelp,
            RowId::AboutWorkspaces => MenuCommand::OpenHelp(HelpPage::AboutWorkspaces),
            RowId::AboutQuantick => MenuCommand::OpenHelp(HelpPage::AboutQuantick),
            // Hidden while already hidden asks for nothing; the disabled palette
            // and the switch drawn apart above never reach here.
            _ => return,
        });
    }

    fn live_rows(&self, ui: &mut egui::Ui, rows: LiveRows, commands: &mut Vec<MenuCommand>) {
        match rows {
            LiveRows::LayoutBook => self.layout_book(ui, commands),
            LiveRows::LayoutPresets => self.layout_presets(ui, commands),
            LiveRows::ContextCharts => {
                ui.add_enabled_ui(self.chart_shown, |ui| self.context_charts(ui, commands));
            }
            LiveRows::Legend => self.legend_entry(ui, commands),
            LiveRows::ChartMoves => self.chart_moves(ui, commands),
            LiveRows::RecentFiles => self.recent_files(ui, commands),
        }
    }

    /// The strip's tabs, from the book: switch the focused pane by name,
    /// and the three edits the strip's own menu holds.
    fn layout_book(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let active = self.layouts.focused_pane_layout();
        let layouts = self.layouts.layouts().layouts();
        for (index, layout) in layouts.iter().enumerate() {
            let mut button = egui::Button::new(layout.name.as_str());
            if let Some(shortcut) = layout_tab_shortcut(index) {
                button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
            }
            if ui.add(button.selected(layout.id == active)).clicked() {
                commands.push(MenuCommand::Strip(StripAction::Switch(layout.id)));
            }
        }
        ui.separator();
        let can_add = layouts.len() < crate::layouts::MAX_LAYOUTS;
        if ui
            .add_enabled(can_add, egui::Button::new("New layout"))
            .clicked()
        {
            commands.push(MenuCommand::Strip(StripAction::Create));
        }
        if ui.button("Rename layout…").clicked() {
            commands.push(MenuCommand::Strip(StripAction::BeginRename(active)));
        }
        if ui
            .add_enabled(layouts.len() > 1, egui::Button::new("Delete layout"))
            .clicked()
        {
            commands.push(MenuCommand::Strip(StripAction::Delete(active)));
        }
    }

    /// Read from the registry, like the picker: a menu holding its own list
    /// of layouts is the second opinion that goes stale the day one is added.
    fn layout_presets(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let current = self.tab.layout.preset();
        for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
            // The menu is where a shortcut is learned, so it carries the
            // binding beside the name.
            let mut button = egui::Button::new(preset.label).selected(current.id == preset.id);
            if let Some(shortcut) = layout_preset_shortcut(index) {
                button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
            }
            if ui.add(button).clicked() {
                commands.push(MenuCommand::ApplyLayoutPreset(preset));
            }
        }
    }

    /// Collapsing was drag-only, which a trader working by keyboard could not
    /// do at all, while the assistant had `layout.pane.collapse`.
    fn context_charts(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        if !(self.tab.layout.shows_time() && self.tab.layout.shows_flow()) {
            return;
        }
        let label = if self.tab.context_collapsed {
            "Show context charts"
        } else {
            "Hide context charts"
        };
        if ui
            .add(
                egui::Button::new(label)
                    .shortcut_text(ui.ctx().format_shortcut(&COLLAPSE_CONTEXT_SHORTCUT)),
            )
            .clicked()
        {
            commands.push(MenuCommand::ToggleContextCharts);
        }
    }

    /// Reposition without a drag. WCAG 2.2's dragging rule wants a
    /// single-pointer alternative to every drag, and both doors go through
    /// `Tab::move_context_pane`.
    fn chart_moves(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let context_panes = self.tab.pane_count().saturating_sub(1);
        for slot in 1..=context_panes {
            let up = ui
                .add_enabled(slot > 1, egui::Button::new(format!("Chart {slot} up")))
                .on_disabled_hover_text("already the top chart");
            if up.clicked() {
                commands.push(MenuCommand::MoveContextChart {
                    from: slot,
                    to: slot - 1,
                });
            }
            let down = ui
                .add_enabled(
                    slot < context_panes,
                    egui::Button::new(format!("Chart {slot} down")),
                )
                .on_disabled_hover_text("already the bottom chart");
            if down.clicked() {
                commands.push(MenuCommand::MoveContextChart {
                    from: slot,
                    to: slot + 1,
                });
            }
        }
    }

    /// The legend belongs to a pane, so this entry names the focused one's
    /// state — the same pane the chevron on screen would fold.
    fn legend_entry(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let collapsed = self.tab.focused_pane().legend_collapsed;
        // Split open: say *which* chart. A trader reading "Collapse indicator
        // legend" over two charts cannot know which corner is about to change.
        let split = self.tab.shows_context_charts();
        let pane_name = self.tab.focused_side().title();
        let legend_label = match (collapsed, split) {
            (true, false) => "Show indicator legend".to_owned(),
            (false, false) => "Collapse indicator legend".to_owned(),
            (true, true) => format!("Show indicator legend ({pane_name})"),
            (false, true) => format!("Collapse indicator legend ({pane_name})"),
        };
        // A pane with no indicators has no legend to fold, and an entry that
        // is enabled and does nothing reads as a broken feature.
        let has_legend = !self
            .tab
            .pane(self.tab.focused_side())
            .indicators
            .all()
            .is_empty();
        if ui
            .add_enabled(
                has_legend && self.chart_shown,
                egui::Button::new(legend_label)
                    .shortcut_text(ui.ctx().format_shortcut(&LEGEND_SHORTCUT)),
            )
            .on_hover_text(
                "Folds the healthy rows to a count on the focused chart. Errored and stale \
                 indicators stay on it.",
            )
            .on_disabled_hover_text("This chart has no indicators, so there is no legend to fold")
            .clicked()
        {
            commands.push(MenuCommand::ToggleLegend);
        }
    }

    /// Read off the field, not the filesystem: this body runs every frame the
    /// menu is open.
    fn recent_files(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        for path in self.recent {
            if ui
                .button(crate::workspace_bundle::recent_label(path))
                // A trader mid-tape has to read this before the click.
                .on_hover_text(format!(
                    "Replaces the cockpit on screen\n{}",
                    path.display()
                ))
                .clicked()
            {
                commands.push(MenuCommand::OpenRecentDesk(path.clone()));
            }
        }
    }
}

fn take_focus(state: &mut WindowMenuState, level: usize, response: &egui::Response) {
    if state.focus_pane == Some(level) && response.enabled() {
        response.request_focus();
        state.focus_pane = None;
    }
}

fn shortcut_of(id: RowId) -> Option<egui::KeyboardShortcut> {
    match id {
        RowId::SaveDesk => Some(SAVE_WORKSPACE_SHORTCUT),
        RowId::TogglePanels => Some(DOCK_SHORTCUT),
        RowId::MarketReplay => Some(REPLAY_SHORTCUT),
        RowId::Settings => Some(SETTINGS_SHORTCUT),
        _ => None,
    }
}

const fn dock_tab_of(id: RowId) -> Option<DockTab> {
    match id {
        RowId::DockL2 => Some(DockTab::L2),
        RowId::DockBubbles => Some(DockTab::Bubbles),
        RowId::DockSession => Some(DockTab::Session),
        RowId::DockPaperTrading => Some(DockTab::Trading),
        RowId::DockTrades => Some(DockTab::Trades),
        _ => None,
    }
}

const fn toolbox_dock_of(id: RowId) -> Option<ToolboxDock> {
    match id {
        RowId::ToolbarLeft => Some(ToolboxDock::Left),
        RowId::ToolbarTop => Some(ToolboxDock::Top),
        RowId::ToolbarBottom => Some(ToolboxDock::Bottom),
        _ => None,
    }
}
