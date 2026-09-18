//! The menu bar's menus (§10), one view per top-level menu.
//!
//! Each view borrows exactly the state it shows and records what the trader
//! chose as a [`MenuCommand`]; none of them changes the window. The window
//! carries the commands out after the bar is drawn, so a menu never acts on
//! state a later menu in the same bar is still reading.

use eframe::egui;

use crate::dock::DockTab;
use crate::layout_strip::StripAction;
use crate::tab::Tab;
use crate::tabstrip::TabAction;
use crate::timezone::TzOffset;
use crate::toolrail::{ToolRail, ToolboxDock};

use super::super::layout_wiring::LayoutRead;
use super::super::tabs::HistorySettings;
use super::super::workspace_bundle_adapter::WorkspaceBundleAdapter;
use super::commands::MenuCommand;
use super::{
    CLOSE_TAB_SHORTCUT, COLLAPSE_CONTEXT_SHORTCUT, DOCK_SHORTCUT, LEGEND_SHORTCUT,
    NEW_TAB_SHORTCUT, REPLAY_SHORTCUT, SAVE_WORKSPACE_SHORTCUT, layout_preset_shortcut,
    layout_tab_shortcut,
};

/// File: tabs, replay and the window itself.
pub(super) struct FileMenu {
    pub(super) tab_count: usize,
    pub(super) active_index: usize,
    /// Whether the active tab is playing a recording back.
    pub(super) replaying: bool,
}

impl FileMenu {
    pub(super) fn show(self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        ui.menu_button("File", |ui| {
            if ui
                .add(
                    egui::Button::new("New Tab…")
                        .shortcut_text(ui.ctx().format_shortcut(&NEW_TAB_SHORTCUT)),
                )
                .clicked()
            {
                commands.push(MenuCommand::Tab(TabAction::New));
                ui.close_menu();
            }
            if ui
                .add_enabled(
                    self.tab_count > 1,
                    egui::Button::new("Close Tab")
                        .shortcut_text(ui.ctx().format_shortcut(&CLOSE_TAB_SHORTCUT)),
                )
                .on_disabled_hover_text(
                    "The last tab stays open — a window with no market has nothing to show",
                )
                .clicked()
            {
                commands.push(MenuCommand::Tab(TabAction::Close(self.active_index)));
                ui.close_menu();
            }
            ui.separator();
            if ui
                .add(
                    egui::Button::new("Market Replay…")
                        .shortcut_text(ui.ctx().format_shortcut(&REPLAY_SHORTCUT)),
                )
                .clicked()
            {
                commands.push(MenuCommand::OpenReplayBrowser);
                ui.close_menu();
            }
            if self.replaying && ui.button("Close Replay").clicked() {
                commands.push(MenuCommand::CloseReplay);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Exit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

/// View: what the canvas shows and which chrome is around it.
///
/// What the canvas shows is a view concern, so the switch lives here rather
/// than under File, and each entry names the charts it shows — "Timeframe",
/// not layout jargon (audit §3).
pub(super) struct ViewMenu<'a> {
    pub(super) layouts: LayoutRead<'a>,
    pub(super) tab: &'a Tab,
    pub(super) dock_visible: bool,
    pub(super) toolrail: &'a ToolRail,
    pub(super) show_perf: bool,
    pub(super) history: &'a HistorySettings,
    pub(super) tz: TzOffset,
}

impl ViewMenu<'_> {
    pub(super) fn show(self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        ui.menu_button("View", |ui| {
            self.layouts_menu(ui, commands);
            self.preset_menu(ui, commands);
            self.context_entries(ui, commands);
            ui.separator();
            self.chrome_entries(ui, commands);
            ui.separator();
            self.settings_entries(ui, commands);
            ui.separator();
            self.timezone_menu(ui, commands);
        });
    }

    /// The strip's tabs, from the book: switch the focused pane by name,
    /// and the three edits the strip's own menu holds.
    fn layouts_menu(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        ui.menu_button("Layouts", |ui| {
            let active = self.layouts.focused_pane_layout();
            let layouts = self.layouts.layouts().layouts();
            for (index, layout) in layouts.iter().enumerate() {
                let mut button = egui::Button::new(layout.name.as_str());
                if let Some(shortcut) = layout_tab_shortcut(index) {
                    button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                }
                if ui.add(button.selected(layout.id == active)).clicked() {
                    commands.push(MenuCommand::Strip(StripAction::Switch(layout.id)));
                    ui.close_menu();
                }
            }
            ui.separator();
            let can_add = layouts.len() < crate::layouts::MAX_LAYOUTS;
            if ui
                .add_enabled(can_add, egui::Button::new("New layout"))
                .clicked()
            {
                commands.push(MenuCommand::Strip(StripAction::Create));
                ui.close_menu();
            }
            if ui.button("Rename layout…").clicked() {
                commands.push(MenuCommand::Strip(StripAction::BeginRename(active)));
                ui.close_menu();
            }
            if ui
                .add_enabled(layouts.len() > 1, egui::Button::new("Delete layout"))
                .clicked()
            {
                commands.push(MenuCommand::Strip(StripAction::Delete(active)));
                ui.close_menu();
            }
        });
    }

    /// Read from the registry, like the picker: a menu holding its own list
    /// of layouts is the second opinion that goes stale the day one is added.
    fn preset_menu(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        ui.menu_button("Layout", |ui| {
            let current = self.tab.layout.preset();
            for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
                // The menu is where a shortcut is learned, so it carries the
                // binding beside the name.
                let label = match layout_preset_shortcut(index) {
                    Some(shortcut) => {
                        format!("{}\t{}", preset.label, ui.ctx().format_shortcut(&shortcut))
                    }
                    None => preset.label.to_owned(),
                };
                if ui
                    .selectable_label(current.id == preset.id, label)
                    .clicked()
                {
                    commands.push(MenuCommand::ApplyLayoutPreset(preset));
                    ui.close_menu();
                }
            }
        });
    }

    /// Collapse and reorder the context charts without a drag.
    fn context_entries(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        // Collapsing was drag-only, which a trader working by keyboard could
        // not do at all — while the assistant had `layout.pane.collapse`.
        // Same call, three doors.
        if self.tab.layout.shows_time() && self.tab.layout.shows_flow() {
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
                ui.close_menu();
            }
        }
        // Reposition without a drag. WCAG 2.2's dragging rule wants a
        // single-pointer alternative to every drag, and TradingView — the
        // reference the trader named — moves charts by a menu command rather
        // than by dragging at all. Both go through `Tab::move_context_pane`.
        let context_panes = self.tab.pane_count().saturating_sub(1);
        if context_panes > 1 {
            ui.menu_button("Move chart", |ui| {
                for slot in 1..=context_panes {
                    let up = ui
                        .add_enabled(slot > 1, egui::Button::new(format!("Chart {slot} up")))
                        .on_disabled_hover_text("already the top chart");
                    if up.clicked() {
                        commands.push(MenuCommand::MoveContextChart {
                            from: slot,
                            to: slot - 1,
                        });
                        ui.close_menu();
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
                        ui.close_menu();
                    }
                }
            });
        }
    }

    /// The panels, the legend, the drawing toolbar and the dock's tabs.
    fn chrome_entries(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let panels_label = if self.dock_visible {
            "Hide panels"
        } else {
            "Show panels"
        };
        if ui
            .add(
                egui::Button::new(panels_label)
                    .shortcut_text(ui.ctx().format_shortcut(&DOCK_SHORTCUT)),
            )
            .clicked()
        {
            commands.push(MenuCommand::ToggleDock);
            ui.close_menu();
        }
        self.legend_entry(ui, commands);
        ui.menu_button("Drawing toolbar", |ui| {
            for (dock, label) in [
                (ToolboxDock::Left, "Left"),
                (ToolboxDock::Top, "Top"),
                (ToolboxDock::Bottom, "Bottom"),
            ] {
                if ui
                    .selectable_label(self.toolrail.dock() == dock, label)
                    .clicked()
                {
                    commands.push(MenuCommand::SetToolboxDock(dock));
                    ui.close_menu();
                }
            }
        });
        let toolbox_label = if self.toolrail.visible() {
            "Hide drawing toolbar"
        } else {
            "Show drawing toolbar"
        };
        if ui.button(toolbox_label).clicked() {
            commands.push(MenuCommand::ToggleToolbox);
            ui.close_menu();
        }
        for (tab, label) in [
            (DockTab::L2, "L2 settings"),
            (DockTab::Bubbles, "Bubble settings"),
            (DockTab::Session, "Session"),
            (DockTab::Trading, "Paper trading"),
            (DockTab::Trades, "Trades"),
        ] {
            if ui.button(label).clicked() {
                commands.push(MenuCommand::OpenDockTab(tab));
                ui.close_menu();
            }
        }
    }

    /// The legend belongs to a pane, so this entry names the focused one's
    /// state — the same pane the chevron on screen would fold.
    fn legend_entry(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let collapsed = self.tab.focused_pane().legend_collapsed;
        // Split open: say *which* chart, the way the layout entries above
        // name the charts they show. The action follows the focus like every
        // other chrome control, and a trader reading "Collapse indicator
        // legend" over two charts has no way to know which corner is about
        // to change.
        let split = self.tab.shows_context_charts();
        let pane_name = self.tab.focused_side().title();
        let legend_label = match (collapsed, split) {
            (true, false) => "Show indicator legend".to_owned(),
            (false, false) => "Collapse indicator legend".to_owned(),
            (true, true) => format!("Show indicator legend ({pane_name})"),
            (false, true) => format!("Collapse indicator legend ({pane_name})"),
        };
        // A pane with no indicators has no legend to fold, and an entry that
        // is enabled and does nothing reads as a broken feature rather than
        // as an empty chart.
        let has_legend = !self
            .tab
            .pane(self.tab.focused_side())
            .indicators
            .all()
            .is_empty();
        if ui
            .add_enabled(
                has_legend,
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
            ui.close_menu();
        }
    }

    /// The three switches that change what the window measures or fetches.
    fn settings_entries(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        let mut show_perf = self.show_perf;
        if ui
            .checkbox(&mut show_perf, "Perf readings")
            .on_hover_text("fps, frame time and trade count on the status bar")
            .changed()
        {
            commands.push(MenuCommand::SetShowPerf(show_perf));
        }
        let mut progressive = self.history.progressive_history;
        if ui
            .checkbox(&mut progressive, "Progressive venue history")
            .on_hover_text(
                "Build the venue's candle history from now backwards, a week at \
                 a time, so the chart fills in while the rest arrives. Off asks \
                 for the whole span in one request: fewer calls, nothing on \
                 screen until all of it lands.",
            )
            .changed()
        {
            commands.push(MenuCommand::SetProgressiveHistory(progressive));
        }
        let mut lead_in = self.history.venue_lead_in;
        if ui
            .checkbox(&mut lead_in, "Venue candles on charts cut by trades")
            .on_hover_text(
                "A tick, volume, dollar or imbalance chart cannot fold venue \
                 candles into its own bars, so it opens holding only the prints \
                 this session saw. Switch this on to put the venue's 1-minute \
                 candles in front of them anyway — counted apart from built bars \
                 on the status bar — so yesterday is on screen to compare \
                 against. They stay candles: a minute never becomes a tick bar, \
                 and an indicator running across the seam is averaging both \
                 kinds.",
            )
            .changed()
        {
            commands.push(MenuCommand::SetVenueLeadIn(lead_in));
        }
    }

    fn timezone_menu(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        ui.menu_button("Timezone", |ui| {
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    for tz in TzOffset::ALL {
                        if ui.selectable_label(self.tz == tz, tz.label()).clicked() {
                            commands.push(MenuCommand::SetTimezone(tz));
                            ui.close_menu();
                        }
                    }
                });
        });
    }
}

/// Workspace: what quantick opens on, the named bookmarks and the files.
///
/// The workspace is its own menu, not a File entry: "what does quantick open
/// on" is a question a trader asks about their cockpit, not about a
/// document, and burying it under File is how a platform ends up with
/// traders who rebuild their screen every morning without knowing they
/// never had to (audit §6).
///
/// The file actions are the bundle adapter's own rendering, which carries
/// out its effects itself; this view hands it the menu's `Ui`.
pub(super) struct WorkspaceMenu<'a> {
    pub(super) bundle: WorkspaceBundleAdapter<'a>,
}

impl WorkspaceMenu<'_> {
    /// Returns the menu button's own rect, published for the capture hook —
    /// read, never acted on, so the menu behaves identically whether or not a
    /// scripted run is watching.
    pub(super) fn show(mut self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) -> egui::Rect {
        ui.menu_button("Workspace", |ui| {
            self.startup_entries(ui, commands);
            ui.separator();
            self.bookmark_entries(ui, commands);
            ui.separator();
            self.bundle.show_file_actions(ui);
            ui.separator();
            let mut save_on_exit = self.bundle.workspace.session().save_on_exit();
            if ui
                .checkbox(&mut save_on_exit, "Save on exit")
                .on_hover_text(
                    "Keep the arrangement automatically when the window closes. Off, \
                     only Save workspace changes what quantick opens on.",
                )
                .changed()
            {
                // The setting lives in the file it governs, so switching it
                // has to reach the disk now — not at the next exit, which is
                // exactly the exit it may have just switched off.
                commands.push(MenuCommand::SetSaveOnExit(save_on_exit));
            }
        })
        .response
        .rect
    }

    /// Save, and forget, what the next launch opens on.
    fn startup_entries(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        if ui
            .add(
                egui::Button::new("Save workspace")
                    .shortcut_text(ui.ctx().format_shortcut(&SAVE_WORKSPACE_SHORTCUT)),
            )
            .on_hover_text(
                "Remember this arrangement — the tabs, the charts on each, the \
                 panels, the timezone and the window — as what quantick opens on",
            )
            .clicked()
        {
            commands.push(MenuCommand::SaveWorkspace("menu"));
            ui.close_menu();
        }
        // Enabled only when there is something on disk to go back to: an
        // entry that would forget nothing is a question the trader should not
        // have to answer by clicking it.
        if ui
            .add_enabled(
                self.bundle.workspace.session().saved(),
                egui::Button::new("Reset startup layout"),
            )
            .on_hover_text(
                "Forget the saved workspace; the next launch opens on the \
                 configured default. The charts on screen are left alone.",
            )
            .on_disabled_hover_text(
                "Nothing saved yet — quantick already opens on the configured \
                 default",
            )
            .clicked()
        {
            commands.push(MenuCommand::ForgetWorkspace);
            ui.close_menu();
        }
    }

    /// Bookmarks. Named apart from the startup entries on purpose: those
    /// govern what the app *opens on*, these are places to come back to. The
    /// wording carries the distinction so the menu does not need a paragraph.
    fn bookmark_entries(&self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        if ui
            .button("Save as…")
            // Says what a bookmark keeps *and what it does not*: it is the
            // tabs and the panels, not the indicators or the colours. Two
            // entries in one menu that both "save a workspace" but restore
            // different amounts is exactly how a trader comes to believe the
            // app forgets things — use Export to file for the whole cockpit.
            .on_hover_text(
                "Keep these tabs and panels under a name you can reopen later. \
                 It does not change what quantick opens on, and it does not \
                 keep indicators or colours — use Export to file for those.",
            )
            .clicked()
        {
            commands.push(MenuCommand::NameWorkspace);
            ui.close_menu();
        }
        let bookmarks = self.bundle.workspace.session().bookmarks();
        ui.add_enabled_ui(!bookmarks.is_empty(), |ui| {
            ui.menu_button("Open", |ui| {
                for entry in bookmarks {
                    let tabs = entry.tabs.len();
                    if ui
                        .button(&entry.name)
                        .on_hover_text(format!(
                            "{tabs} chart {} — replaces what is on screen",
                            if tabs == 1 { "tab" } else { "tabs" }
                        ))
                        .clicked()
                    {
                        commands.push(MenuCommand::OpenNamedWorkspace(entry.name.clone()));
                        ui.close_menu();
                    }
                }
            })
            .response
            .on_disabled_hover_text("Nothing saved under a name yet");
            ui.menu_button("Delete", |ui| {
                for entry in bookmarks {
                    if ui.button(&entry.name).clicked() {
                        commands.push(MenuCommand::DeleteNamedWorkspace(entry.name.clone()));
                        ui.close_menu();
                    }
                }
            });
        });
    }
}

/// Tools: appearance, deal recording and agent access.
///
/// `record_deals` draws the deal-recording toggle, which owns its own
/// setting and applies it itself.
pub(super) fn show_tools_menu(
    ui: &mut egui::Ui,
    access_label: &str,
    record_deals: impl FnOnce(&mut egui::Ui),
    commands: &mut Vec<MenuCommand>,
) {
    ui.menu_button("Tools", |ui| {
        if ui.button("Appearance…").clicked() {
            commands.push(MenuCommand::OpenAppearance);
            ui.close_menu();
        }
        record_deals(ui);
        if ui.button(access_label).clicked() {
            commands.push(MenuCommand::OpenAgentAccess);
            ui.close_menu();
        }
    });
}

/// Help: the one reference a trader needs away from the chart.
pub(super) fn show_help_menu(ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
    ui.menu_button("Help", |ui| {
        if ui.button("Replay file format…").clicked() {
            commands.push(MenuCommand::OpenReplayFormatHelp);
            ui.close_menu();
        }
    });
}

/// The bar's own "agent access is on" indicator, shown only while it is.
pub(super) fn show_agent_access_indicator(
    ui: &mut egui::Ui,
    enabled: bool,
    commands: &mut Vec<MenuCommand>,
) {
    if enabled && ui.button("Agent access: on").clicked() {
        commands.push(MenuCommand::OpenAgentAccess);
    }
}
