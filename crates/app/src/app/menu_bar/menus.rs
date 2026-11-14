//! Paints the `⋮` window menu from its headless table.
//!
//! The groups, labels and tooltips live in
//! [`quantick_control_schema::window_menu`]; this view adds the live state and
//! the pixels, and records what the trader chose as a [`MenuCommand`]. egui's
//! own submenus open to the right, which from the window's right edge lands off
//! screen, so these panes are anchored by their top right corner.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_control_handlers::workspace_cell_menu::CellRow;
use quantick_control_schema::settings_sections::SettingsSection;
use quantick_control_schema::window_menu::{
    self, HOME_SHOWN_HOVER, HelpPage, Item, LEGEND_HOVER, LiveRows, ROOT, RowId, SubmenuId,
    panel_row_blocked, panel_row_selected, panel_section_of, row_of, submenu,
};

use crate::dock::DockTab;
use crate::layout_strip::StripAction;
use crate::tab::Tab;
use crate::toolrail::{ToolRail, ToolboxDock};

use super::super::layout_wiring::LayoutRead;
use super::commands::MenuCommand;
use super::{
    COLLAPSE_CONTEXT_SHORTCUT, DOCK_SHORTCUT, LEGEND_SHORTCUT, REPLAY_SHORTCUT,
    SAVE_TO_WORKSPACE_SHORTCUT, SAVE_WORKSPACE_SHORTCUT, SETTINGS_SHORTCUT, layout_preset_shortcut,
    layout_tab_shortcut,
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

/// The button the menu hangs from. Both open the same menu; the door only
/// decides where it drops and which way its submenus open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum MenuDoor {
    /// The title bar's `⋮`, at the window's right edge: panes open leftwards.
    #[default]
    Kebab,
    /// The toolbar's `≡`, at the window's left edge: panes open rightwards.
    Toolbar,
}

/// Whether the menu is up, and which submenus are open along the way.
#[derive(Default)]
pub(in crate::app) struct WindowMenuState {
    pub(in crate::app) open: bool,
    pub(in crate::app) viewport: Option<egui::ViewportId>,
    /// The button the open menu hangs from.
    door: MenuDoor,
    /// Where the toolbar's `≡` was drawn last frame, on the window it is on.
    pub(in crate::app) toolbar_door: Option<egui::Rect>,
    /// The window Settings and Help opened on, apart from the menu's own.
    pub(in crate::app) windows_viewport: Option<egui::ViewportId>,
    pub(in crate::app) settings_open: bool,
    /// The section Settings shows; the section list and every door write it.
    pub(in crate::app) settings_section: SettingsSection,
    pub(in crate::app) help: Option<HelpPage>,
    /// The chip's menu, the armed delete and the name dialog.
    pub(in crate::app) tab_workspace: quantick_session::workspace_menu::WorkspaceMenuState,
    pub(in crate::app) cell_rows: Vec<(CellRow, egui::Rect)>,
    path: [Option<SubmenuId>; MAX_SUBMENU_LEVELS],
    /// The pane whose first row takes the keyboard next.
    focus_pane: Option<usize>,
    panes: Vec<egui::Rect>,
}

impl WindowMenuState {
    /// The section on screen, while Settings is up.
    pub(in crate::app) fn settings_shown(&self) -> Option<SettingsSection> {
        self.settings_open.then_some(self.settings_section)
    }

    /// Bring Settings up on `section`, on this window.
    pub(in crate::app) fn open_settings(
        &mut self,
        viewport: egui::ViewportId,
        section: SettingsSection,
    ) {
        self.windows_viewport = Some(viewport);
        self.settings_open = true;
        self.settings_section = section;
    }

    /// Open or close the menu on this window. Opened from the keyboard, its first
    /// row is ready for the keys; opened by the mouse, no row wears a focus ring.
    pub(in crate::app) fn toggle(&mut self, viewport: egui::ViewportId, by_keyboard: bool) {
        self.toggle_from(viewport, MenuDoor::Kebab, by_keyboard);
    }

    /// [`Self::toggle`], hanging the menu from `door`.
    pub(in crate::app) fn toggle_from(
        &mut self,
        viewport: egui::ViewportId,
        door: MenuDoor,
        by_keyboard: bool,
    ) {
        // Open on another window, this press still opens it here.
        let open = !self.is_open_on(viewport);
        self.close();
        self.open = open;
        self.viewport = Some(viewport);
        self.door = door;
        self.focus_pane = (open && by_keyboard).then_some(0);
    }

    /// Whether the menu is up on this window and hangs from `door`.
    pub(in crate::app) fn opened_from(&self, viewport: egui::ViewportId, door: MenuDoor) -> bool {
        self.is_open_on(viewport) && self.door == door
    }

    /// The button the menu hangs from.
    pub(in crate::app) const fn door(&self) -> MenuDoor {
        self.door
    }

    pub(in crate::app) fn is_open_on(&self, viewport: egui::ViewportId) -> bool {
        self.open && self.viewport == Some(viewport)
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
    pub(super) workspace: super::workspace_rows::WorkspaceRows<'a>,
    pub(super) replaying: bool,
    pub(super) layouts: LayoutRead<'a>,
    pub(super) tab: &'a Tab,
    pub(super) dock_visible: bool,
    pub(super) dock_tab: Option<DockTab>,
    pub(super) toolrail: &'a ToolRail,
    pub(super) panel_folds: &'a crate::ui_state::AssetFolds,
    pub(super) startup_saved: bool,
    pub(super) save_on_exit: bool,
    pub(super) recent: &'a [std::path::PathBuf],
    /// Whether the next computed row takes the keyboard.
    pub(super) focus_live_row: std::cell::Cell<bool>,
}

impl WindowMenu<'_> {
    /// Draw the open panes, anchored under the button at `door`: the `⋮`'s
    /// right edge, or the toolbar `≡`'s left edge.
    pub(super) fn show(
        &self,
        ctx: &egui::Context,
        state: &mut WindowMenuState,
        door: egui::Rect,
        commands: &mut Vec<MenuCommand>,
    ) {
        if !state.is_open_on(ctx.viewport_id()) {
            return;
        }
        let recorded = commands.len();
        state.panes.clear();
        let at = match state.door {
            MenuDoor::Kebab => door.right_bottom(),
            MenuDoor::Toolbar => door.left_bottom(),
        };
        self.pane(ctx, state, 0, ROOT, at, commands);
        // A switch stays up so the trader sees it flip; anything else is done.
        let acted = commands[recorded..]
            .iter()
            .any(|command| !matches!(command, MenuCommand::SetSaveOnExit(_)));
        let dismissed = ctx.input_mut(|input| {
            let escape = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            let outside = input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|at| {
                    !door.contains(at) && !state.panes.iter().any(|pane| pane.contains(at))
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
        corner: egui::Pos2,
        commands: &mut Vec<MenuCommand>,
    ) -> egui::LayerId {
        let mut child = None;
        let mut over_submenu = false;
        let rightwards = state.door == MenuDoor::Toolbar;
        let pivot = if rightwards {
            egui::Align2::LEFT_TOP
        } else {
            egui::Align2::RIGHT_TOP
        };
        let area = egui::Area::new(egui::Id::new(("window_menu_pane", level)))
            .order(egui::Order::Foreground)
            .pivot(pivot)
            .fixed_pos(corner)
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(PANE_MIN_WIDTH_PX);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        flat_rows(ui);
                        for item in items {
                            match *item {
                                Item::Separator => {
                                    ui.separator();
                                }
                                Item::Caption(text) => {
                                    ui.label(crate::theme::supporting_text(ui, text));
                                }
                                Item::Row(id) => self.row(ui, state, level, id, commands),
                                Item::Live(rows) => {
                                    self.live_rows(ui, state, level, rows, commands)
                                }
                                Item::Submenu(id) => {
                                    let Some(row) = self.submenu_row(ui, state, level, id) else {
                                        continue;
                                    };
                                    over_submenu |= row.hovered();
                                    if state.path[level] == Some(id) {
                                        let corner = if rightwards {
                                            row.rect.right_top()
                                                + egui::vec2(SUBMENU_GAP_PX, -SUBMENU_LIFT_PX)
                                        } else {
                                            row.rect.left_top()
                                                + egui::vec2(-SUBMENU_GAP_PX, -SUBMENU_LIFT_PX)
                                        };
                                        child = Some((id, corner));
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
            let below = self.pane(ctx, state, level + 1, submenu(id).1, at, commands);
            // The menu stays over the edge its first submenu tucks under.
            if level == 0 {
                ctx.set_sublayer(below, area.response.layer_id);
            }
        }
        area.response.layer_id
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
        // The caret points the way the submenu opens.
        let caret = match state.door {
            MenuDoor::Kebab => icons::CARET_LEFT,
            MenuDoor::Toolbar => icons::CARET_RIGHT,
        };
        let response = ui
            .add_enabled(
                !empty && (self.chart_shown || !chart_row),
                egui::Button::new(submenu(id).0)
                    .selected(state.path[level] == Some(id))
                    .shortcut_text(crate::theme::supporting_text(ui, caret)),
            )
            .on_disabled_hover_text(if empty {
                "No workspace files opened yet"
            } else {
                HOME_SHOWN_HOVER
            });
        take_focus(state, level, &response);
        if response.hovered() || response.clicked() {
            if state.path[level] != Some(id) && level + 1 < state.path.len() {
                state.path[level + 1] = None;
            }
            state.path[level] = Some(id);
            if response.clicked() && !response.clicked_by(egui::PointerButton::Primary) {
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
        if id == RowId::SaveToWorkspace {
            let keys = super::shortcut_text(ui.ctx(), &SAVE_TO_WORKSPACE_SHORTCUT);
            let response = self.workspace.save_to_row(ui, keys, commands);
            take_focus(state, level, &response);
            return;
        }
        if self.workspace.action_row(ui, id, commands) {
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
            button = button.shortcut_text(crate::theme::supporting_text(
                ui,
                super::shortcut_text(ui.ctx(), &shortcut),
            ));
        }
        let blocked = self.panel_blocked(id);
        let mut response = ui.add_enabled(self.enabled(id) && blocked.is_none(), button);
        if let Some(hover) = row.hover {
            response = response.on_hover_text(hover);
        }
        if let Some(hover) = blocked.or(row.disabled_hover) {
            response = response.on_disabled_hover_text(hover);
        }
        take_focus(state, level, &response);
        if id == RowId::Settings {
            // The scripted theme path presses this row on its way to the choice.
            crate::theme::choice::record(&response, "Settings");
        }
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

    /// Why an L2 or Bubbles row cannot open its section now: the panel key's gate.
    fn panel_blocked(&self, id: RowId) -> Option<&'static str> {
        let shown = crate::orderflow_panel::flow_pane_shown(self.tab);
        panel_section_of(id).and(panel_row_blocked(self.chart_shown, shown))
    }

    fn selected(&self, id: RowId) -> bool {
        if let Some(section) = panel_section_of(id) {
            let (panel, symbol) = (&self.tab.tape().settings_panel, &self.tab.symbol);
            return self.panel_blocked(id).is_none()
                && panel_row_selected(panel, self.panel_folds, symbol, section);
        }
        if let Some(tab) = dock_tab_of(id) {
            return self.dock_visible && self.dock_tab == Some(tab);
        }
        if let Some(dock) = toolbox_dock_of(id) {
            return self.toolrail.visible() && self.toolrail.dock() == dock;
        }
        id == RowId::ToolbarHidden && !self.toolrail.visible()
    }

    fn push_row_commands(&self, id: RowId, commands: &mut Vec<MenuCommand>) {
        if let Some(section) = panel_section_of(id) {
            commands.push(MenuCommand::OpenOrderFlowPanel(section));
            return;
        }
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
            RowId::Alerts => MenuCommand::OpenAlerts,
            RowId::MarketReplay => MenuCommand::OpenReplayBrowser,
            RowId::CloseReplay => MenuCommand::CloseReplay,
            RowId::Settings => MenuCommand::OpenSettings(SettingsSection::Display),
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

    fn live_rows(
        &self,
        ui: &mut egui::Ui,
        state: &mut WindowMenuState,
        level: usize,
        rows: LiveRows,
        commands: &mut Vec<MenuCommand>,
    ) {
        let wants_focus = state.focus_pane == Some(level);
        self.focus_live_row.set(wants_focus);
        match rows {
            LiveRows::LayoutBook => self.layout_book(ui, commands),
            LiveRows::LayoutPresets => self.layout_presets(ui, commands),
            LiveRows::ContextCharts => {
                ui.add_enabled_ui(self.chart_shown, |ui| self.context_charts(ui, commands));
            }
            LiveRows::Legend => self.legend_entry(ui, commands),
            LiveRows::ChartMoves => self.chart_moves(ui, commands),
            LiveRows::RecentFiles => self.recent_files(ui, commands),
            LiveRows::WorkspaceList => self.workspace.list(ui, commands),
        }
        if wants_focus && !self.focus_live_row.get() {
            state.focus_pane = None;
        }
    }

    /// A computed row, which takes the keyboard when its pane is being entered.
    fn live_row(
        &self,
        ui: &mut egui::Ui,
        enabled: bool,
        button: egui::Button<'_>,
    ) -> egui::Response {
        let response = ui.add_enabled(enabled, button);
        if enabled && self.focus_live_row.get() {
            response.request_focus();
            self.focus_live_row.set(false);
        }
        response
    }

    /// The strip's tabs, from the book: switch the focused pane by name,
    /// and the three edits the strip's own menu holds.
    fn layout_book(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let active = self.layouts.focused_pane_layout();
        let layouts = self.layouts.layouts().layouts();
        for (index, layout) in layouts.iter().enumerate() {
            let mut button = egui::Button::new(layout.name.as_str());
            if let Some(shortcut) = layout_tab_shortcut(index) {
                button = button.shortcut_text(crate::theme::supporting_text(
                    ui,
                    super::shortcut_text(ui.ctx(), &shortcut),
                ));
            }
            if self
                .live_row(ui, true, button.selected(layout.id == active))
                .clicked()
            {
                commands.push(MenuCommand::Strip(StripAction::Switch(layout.id)));
            }
        }
        ui.separator();
        let can_add = layouts.len() < crate::layouts::MAX_LAYOUTS;
        if self
            .live_row(ui, can_add, egui::Button::new("New layout"))
            .clicked()
        {
            commands.push(MenuCommand::Strip(StripAction::Create));
        }
        if self
            .live_row(ui, true, egui::Button::new("Rename layout…"))
            .clicked()
        {
            commands.push(MenuCommand::Strip(StripAction::BeginRename(active)));
        }
        if self
            .live_row(ui, layouts.len() > 1, egui::Button::new("Delete layout"))
            .clicked()
        {
            commands.push(MenuCommand::Strip(StripAction::Delete(active)));
        }
    }

    /// Read from the registry, like the picker: a menu holding its own list
    /// of layouts is the second opinion that goes stale the day one is added.
    fn layout_presets(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let current = self.tab.canvas.layout.preset();
        for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
            // The menu is where a shortcut is learned, so it carries the
            // binding beside the name.
            let mut button = egui::Button::new(preset.label).selected(current.id == preset.id);
            if let Some(shortcut) = layout_preset_shortcut(index) {
                button = button.shortcut_text(crate::theme::supporting_text(
                    ui,
                    super::shortcut_text(ui.ctx(), &shortcut),
                ));
            }
            if self.live_row(ui, true, button).clicked() {
                commands.push(MenuCommand::ApplyLayoutPreset(preset));
            }
        }
    }

    /// Collapsing was drag-only, which a trader working by keyboard could not
    /// do at all, while the assistant had `layout.pane.collapse`.
    fn context_charts(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        if !(self.tab.canvas.layout.shows_time() && self.tab.canvas.layout.shows_flow()) {
            return;
        }
        let label = if self.tab.canvas.context_collapsed {
            "Show context charts"
        } else {
            "Hide context charts"
        };
        let button = egui::Button::new(label).shortcut_text(crate::theme::supporting_text(
            ui,
            super::shortcut_text(ui.ctx(), &COLLAPSE_CONTEXT_SHORTCUT),
        ));
        if self.live_row(ui, ui.is_enabled(), button).clicked() {
            commands.push(MenuCommand::ToggleContextCharts);
        }
    }

    /// Reposition without a drag. WCAG 2.2's dragging rule wants a
    /// single-pointer alternative to every drag, and both doors go through
    /// `Tab::move_context_pane`.
    fn chart_moves(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let context_panes = self.tab.pane_count().saturating_sub(1);
        for slot in 1..=context_panes {
            let up = self
                .live_row(ui, slot > 1, egui::Button::new(format!("Chart {slot} up")))
                .on_disabled_hover_text("already the top chart");
            if up.clicked() {
                commands.push(MenuCommand::MoveContextChart {
                    from: slot,
                    to: slot - 1,
                });
            }
            let down = self
                .live_row(
                    ui,
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
        let split = self
            .tab
            .shows_context_charts()
            .then(|| self.tab.focused_side().title());
        let legend_label =
            window_menu::legend_label(self.tab.focused_pane().legend_collapsed, split.as_deref());
        // A pane with no indicators has no legend to fold, and an entry that
        // is enabled and does nothing reads as a broken feature.
        let has_legend = !self
            .tab
            .pane(self.tab.focused_side())
            .indicators
            .all()
            .is_empty();
        let keys = super::shortcut_text(ui.ctx(), &LEGEND_SHORTCUT);
        if self
            .live_row(
                ui,
                has_legend && self.chart_shown,
                egui::Button::new(legend_label)
                    .shortcut_text(crate::theme::supporting_text(ui, keys)),
            )
            .on_hover_text(LEGEND_HOVER)
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
            if self
                .live_row(
                    ui,
                    true,
                    egui::Button::new(crate::workspace_bundle::recent_label(path)),
                )
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

/// Rows read as lines of a list until the pointer is on one, in `⋮` and the chip menu.
pub(super) fn flat_rows(ui: &mut egui::Ui) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    ui.spacing_mut().button_padding = egui::vec2(10.0, 4.0);
    ui.visuals_mut().widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    ui.visuals_mut().widgets.inactive.bg_stroke = egui::Stroke::NONE;
    let fill = crate::theme::get(ui.ctx()).bare_hover();
    let hover = &mut ui.visuals_mut().widgets.hovered;
    (hover.weak_bg_fill, hover.bg_stroke) = (fill, egui::Stroke::NONE);
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
