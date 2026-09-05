//! The menu bar and the tab strip (§10), and the shortcuts they mirror.
//!
//! Shallow menus for discoverability, never the only path to anything: every
//! entry that has a shortcut shows it, and the constants below are the one
//! place a key is bound. They live here rather than beside the struct
//! because the menu bar is their only production reader — the paper
//! shortcuts are re-exported to `super` for the tests that name them.

use eframe::egui;

use crate::dock::DockTab;
use crate::tabstrip::{self, TabAction};
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolrail::ToolboxDock;

use super::QuantickApp;

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
    pub(super) fn draw_menu_bar(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&REPLAY_SHORTCUT)) {
            self.replay_view.open_browser();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&DOCK_SHORTCUT)) {
            self.dock.toggle_visible();
        }
        // Gated on the same condition the View menu's entry is gated on: only
        // a layout that carves a context column *beside* the flow pane has a
        // column to put away. Ungated, `Ctrl+0` on the Flow or Timeframe
        // layout set a flag nothing drew — and swallowed the key besides, so
        // egui's own "reset zoom" never ran.
        if self.active_tab().layout.shows_time()
            && self.active_tab().layout.shows_flow()
            && ctx.input_mut(|i| i.consume_shortcut(&COLLAPSE_CONTEXT_SHORTCUT))
        {
            let collapsed = self.active_tab().context_collapsed;
            self.active_tab_mut().set_context_collapsed(!collapsed);
        }
        // Layout by number, straight off the registry. The same
        // `apply_layout_preset` the picker and the menu call — three doors,
        // one room.
        for (index, preset) in crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate() {
            let Some(shortcut) = layout_preset_shortcut(index) else {
                break;
            };
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.apply_layout_preset(preset);
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
            if !typing
                && ctx.input_mut(|i| i.consume_shortcut(&shortcut))
                && let Err(error) = self.switch_layout_index(index)
            {
                self.note_workspace(error.to_string());
            }
        }
        if ctx.input_mut(|i| i.consume_shortcut(&LEGEND_SHORTCUT)) {
            let collapsed = self.focused_legend_collapsed();
            self.set_focused_legend_collapsed(!collapsed);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&crate::control::MARK_SHORTCUT)) {
            self.take_mark(None);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SAVE_WORKSPACE_SHORTCUT)) {
            self.save_workspace("shortcut");
        }
        // Trading hotkeys, swallowed only while no text field owns the
        // keyboard. Market entries use the ticket's quantity and offsets,
        // exactly like the toolbar buttons they twin.
        if !ctx.wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_BUY_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .market(quantick_engine::Side::Buy);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_SELL_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .market(quantick_engine::Side::Sell);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_REVERSE_SHORTCUT)) {
                self.active_tab_mut().paper.reverse_position();
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_FLATTEN_SHORTCUT)) {
                self.active_tab_mut().paper.flatten();
            }
            if ctx.input_mut(|i| i.consume_shortcut(&PAPER_CANCEL_SHORTCUT)) {
                self.active_tab_mut()
                    .paper
                    .account_mut()
                    .cancel_all_orders();
            }
        }

        let mut tab_action = None;
        egui::TopBottomPanel::top("menu_bar")
            .exact_height(MENU_BAR_HEIGHT)
            .frame(
                egui::Frame::none()
                    .fill(theme::CHROME)
                    .inner_margin(egui::Margin::symmetric(6.0, 4.0)),
            )
            .show(ctx, |ui| {
                egui::menu::bar(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui
                            .add(
                                egui::Button::new("New Tab…")
                                    .shortcut_text(ui.ctx().format_shortcut(&NEW_TAB_SHORTCUT)),
                            )
                            .clicked()
                        {
                            tab_action = Some(TabAction::New);
                            ui.close_menu();
                        }
                        if ui
                            .add_enabled(
                                self.tabs.len() > 1,
                                egui::Button::new("Close Tab").shortcut_text(
                                    ui.ctx().format_shortcut(&CLOSE_TAB_SHORTCUT),
                                ),
                            )
                            .on_disabled_hover_text(
                                "The last tab stays open — a window with no market has nothing to show",
                            )
                            .clicked()
                        {
                            tab_action = Some(TabAction::Close(self.active_tab));
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
                            self.replay_view.open_browser();
                            ui.close_menu();
                        }
                        if self.active_tab().replay.is_some() && ui.button("Close Replay").clicked()
                        {
                            let (tab, config) = self.active_with_config();
                            tab.close_replay(config);
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("View", |ui| {
                        // What the canvas shows is a view concern, so the
                        // switch lives here rather than under File, and each
                        // entry names the charts it shows — "Timeframe", not
                        // layout jargon (audit §3).
                        ui.menu_button("Layouts", |ui| {
                            // The strip's tabs, from the book: switch the
                            // focused pane by name, and the three edits the
                            // strip's own menu holds.
                            let active = self.focused_pane_layout();
                            let names: Vec<(crate::layouts::LayoutId, String)> = self
                                .layouts()
                                .layouts()
                                .iter()
                                .map(|layout| (layout.id, layout.name.clone()))
                                .collect();
                            for (index, (id, name)) in names.iter().enumerate() {
                                let mut button = egui::Button::new(name.as_str());
                                if let Some(shortcut) = layout_tab_shortcut(index) {
                                    button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                                }
                                if ui.add(button.selected(*id == active)).clicked() {
                                    self.apply_strip_action(crate::layout_strip::StripAction::Switch(*id));
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            let can_add = names.len() < crate::layouts::MAX_LAYOUTS;
                            if ui
                                .add_enabled(can_add, egui::Button::new("New layout"))
                                .clicked()
                            {
                                self.apply_strip_action(crate::layout_strip::StripAction::Create);
                                ui.close_menu();
                            }
                            if ui.button("Rename layout…").clicked() {
                                self.apply_strip_action(crate::layout_strip::StripAction::BeginRename(active));
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(names.len() > 1, egui::Button::new("Delete layout"))
                                .clicked()
                            {
                                self.apply_strip_action(crate::layout_strip::StripAction::Delete(active));
                                ui.close_menu();
                            }
                        });
                        ui.menu_button("Layout", |ui| {
                            // Read from the registry, like the picker: a menu
                            // holding its own list of layouts is the second
                            // opinion that goes stale the day one is added.
                            let current = self.active_tab().layout.preset();
                            for (index, preset) in
                                crate::canvas_layout::LAYOUT_PRESETS.iter().enumerate()
                            {
                                // The menu is where a shortcut is learned, so
                                // it carries the binding beside the name.
                                let label = match layout_preset_shortcut(index) {
                                    Some(shortcut) => format!(
                                        "{}	{}",
                                        preset.label,
                                        ui.ctx().format_shortcut(&shortcut)
                                    ),
                                    None => preset.label.to_owned(),
                                };
                                if ui
                                    .selectable_label(current.id == preset.id, label)
                                    .clicked()
                                {
                                    self.apply_layout_preset(preset);
                                    ui.close_menu();
                                }
                            }
                        });
                        // Collapsing was drag-only, which a trader working
                        // by keyboard could not do at all — while the
                        // assistant had `layout.pane.collapse`. Same call,
                        // three doors.
                        if self.active_tab().layout.shows_time()
                            && self.active_tab().layout.shows_flow()
                        {
                            let collapsed = self.active_tab().context_collapsed;
                            let label = if collapsed {
                                "Show context charts"
                            } else {
                                "Hide context charts"
                            };
                            if ui
                                .add(
                                    egui::Button::new(label).shortcut_text(
                                        ui.ctx().format_shortcut(&COLLAPSE_CONTEXT_SHORTCUT),
                                    ),
                                )
                                .clicked()
                            {
                                self.active_tab_mut().set_context_collapsed(!collapsed);
                                ui.close_menu();
                            }
                        }
                        // Reposition without a drag. WCAG 2.2's dragging rule
                        // wants a single-pointer alternative to every drag, and
                        // TradingView — the reference the trader named — moves
                        // charts by a menu command rather than by dragging at
                        // all. Both go through `Tab::move_context_pane`.
                        let context_panes = self.active_tab().pane_count().saturating_sub(1);
                        if context_panes > 1 {
                            ui.menu_button("Move chart", |ui| {
                                for slot in 1..=context_panes {
                                    let up = ui
                                        .add_enabled(slot > 1, egui::Button::new(format!(
                                            "Chart {slot} up"
                                        )))
                                        .on_disabled_hover_text("already the top chart");
                                    if up.clicked() {
                                        let tab_id = self.active_tab().id;
                                        self.move_context_pane_at(tab_id, slot, slot - 1);
                                        ui.close_menu();
                                    }
                                    let down = ui
                                        .add_enabled(
                                            slot < context_panes,
                                            egui::Button::new(format!("Chart {slot} down")),
                                        )
                                        .on_disabled_hover_text("already the bottom chart");
                                    if down.clicked() {
                                        let tab_id = self.active_tab().id;
                                        self.move_context_pane_at(tab_id, slot, slot + 1);
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                        ui.separator();
                        let panels_label = if self.dock.visible() {
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
                            self.dock.toggle_visible();
                            ui.close_menu();
                        }
                        // The legend belongs to a pane, so this entry names
                        // the focused one's state — the same pane the chevron
                        // on screen would fold.
                        let collapsed = self.focused_legend_collapsed();
                        // Split open: say *which* chart, the way the layout
                        // entries above name the charts they show. The action
                        // follows the focus like every other chrome control,
                        // and a trader reading "Collapse indicator legend"
                        // over two charts has no way to know which corner is
                        // about to change.
                        let split = self.active_tab().shows_context_charts();
                        let pane_name = self.active_tab().focused_side().title();
                        let legend_label = match (collapsed, split) {
                            (true, false) => "Show indicator legend".to_owned(),
                            (false, false) => "Collapse indicator legend".to_owned(),
                            (true, true) => format!("Show indicator legend ({pane_name})"),
                            (false, true) => format!("Collapse indicator legend ({pane_name})"),
                        };
                        // A pane with no indicators has no legend to fold, and
                        // an entry that is enabled and does nothing reads as a
                        // broken feature rather than as an empty chart.
                        let has_legend = {
                            let tab = self.active_tab();
                            !tab.pane(tab.focused_side()).indicators.all().is_empty()
                        };
                        if ui
                            .add_enabled(
                                has_legend,
                                egui::Button::new(legend_label)
                                    .shortcut_text(ui.ctx().format_shortcut(&LEGEND_SHORTCUT)),
                            )
                            .on_hover_text(
                                "Folds the healthy rows to a count on the focused chart. Errored and stale indicators stay on it.",
                            )
                            .on_disabled_hover_text(
                                "This chart has no indicators, so there is no legend to fold",
                            )
                            .clicked()
                        {
                            self.set_focused_legend_collapsed(!collapsed);
                            ui.close_menu();
                        }
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
                                    self.toolrail.set_dock(dock);
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
                            self.toolrail.toggle_visible();
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
                                self.dock.open_tab(tab);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        ui.checkbox(&mut self.show_perf, "Perf readings")
                            .on_hover_text("fps, frame time and trade count on the status bar");
                        ui.checkbox(&mut self.progressive_history, "Progressive venue history")
                            .on_hover_text(
                                "Build the venue's candle history from now backwards, a week at \
                                 a time, so the chart fills in while the rest arrives. Off asks \
                                 for the whole span in one request: fewer calls, nothing on \
                                 screen until all of it lands.",
                            );
                        ui.checkbox(&mut self.venue_lead_in, "Venue candles on charts cut by trades")
                            .on_hover_text(
                                "A tick, volume, dollar or imbalance chart cannot fold venue \
                                 candles into its own bars, so it opens holding only the prints \
                                 this session saw. Switch this on to put the venue's 1-minute \
                                 candles in front of them anyway — counted apart from built bars \
                                 on the status bar — so yesterday is on screen to compare \
                                 against. They stay candles: a minute never becomes a tick bar, \
                                 and an indicator running across the seam is averaging both \
                                 kinds.",
                            );
                        ui.separator();
                        ui.menu_button("Timezone", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    for tz in TzOffset::ALL {
                                        if ui.selectable_label(self.tz == tz, tz.label()).clicked()
                                        {
                                            self.tz = tz;
                                            ui.close_menu();
                                        }
                                    }
                                });
                        });
                    });
                    // The workspace is its own menu, not a File entry: "what
                    // does quantick open on" is a question a trader asks about
                    // their cockpit, not about a document, and burying it
                    // under File is how a platform ends up with traders who
                    // rebuild their screen every morning without knowing they
                    // never had to (audit §6).
                    // The button's own rect, published for the capture hook —
                    // read, never acted on, so the menu behaves identically
                    // whether or not a scripted run is watching.
                    let workspace_menu = ui.menu_button("Workspace", |ui| {
                        if ui
                            .add(
                                egui::Button::new("Save workspace").shortcut_text(
                                    ui.ctx().format_shortcut(&SAVE_WORKSPACE_SHORTCUT),
                                ),
                            )
                            .on_hover_text(
                                "Remember this arrangement — the tabs, the charts on each, the \
                                 panels, the timezone and the window — as what quantick opens on",
                            )
                            .clicked()
                        {
                            self.save_workspace("menu");
                            ui.close_menu();
                        }
                        // Enabled only when there is something on disk to go
                        // back to: an entry that would forget nothing is a
                        // question the trader should not have to answer by
                        // clicking it.
                        if ui
                            .add_enabled(
                                self.workspace.session().saved(),
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
                            self.forget_workspace();
                            ui.close_menu();
                        }
                        ui.separator();
                        // Bookmarks. Named apart from the two entries above on
                        // purpose: those govern what the app *opens on*, these
                        // are places to come back to. The wording carries the
                        // distinction so the menu does not need a paragraph.
                        if ui
                            .button("Save as…")
                            // Says what a bookmark keeps *and what it does
                            // not*: it is the tabs and the panels, not the
                            // indicators or the colours. Two entries in one
                            // menu that both "save a workspace" but restore
                            // different amounts is exactly how a trader comes
                            // to believe the app forgets things — use Export
                            // to file for the whole cockpit.
                            .on_hover_text(
                                "Keep these tabs and panels under a name you can reopen later. \
                                 It does not change what quantick opens on, and it does not \
                                 keep indicators or colours — use Export to file for those.",
                            )
                            .clicked()
                        {
                            self.surfaces.workspace_name.open();
                            ui.close_menu();
                        }
                        let mut open: Option<String> = None;
                        let mut delete: Option<String> = None;
                        ui.add_enabled_ui(!self.workspace.session().bookmarks().is_empty(), |ui| {
                            ui.menu_button("Open", |ui| {
                                for entry in self.workspace.session().bookmarks() {
                                    let tabs = entry.tabs.len();
                                    if ui
                                        .button(&entry.name)
                                        .on_hover_text(format!(
                                            "{tabs} chart {} — replaces what is on screen",
                                            if tabs == 1 { "tab" } else { "tabs" }
                                        ))
                                        .clicked()
                                    {
                                        open = Some(entry.name.clone());
                                        ui.close_menu();
                                    }
                                }
                            })
                            .response
                            .on_disabled_hover_text("Nothing saved under a name yet");
                            ui.menu_button("Delete", |ui| {
                                for entry in self.workspace.session().bookmarks() {
                                    if ui.button(&entry.name).clicked() {
                                        delete = Some(entry.name.clone());
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                        if let Some(name) = open {
                            self.open_named_workspace(&name);
                        }
                        if let Some(name) = delete {
                            self.delete_named_workspace(&name);
                        }
                        ui.separator();
                        // Files, named apart from the two groups above again:
                        // those live inside quantick, these are documents the
                        // trader owns, can copy, back up and carry to another
                        // machine. That is the difference the wording carries.
                        if ui
                            .button("Export to file…")
                            .on_hover_text(
                                "Save the whole cockpit — tabs, indicators, layers, drawing \
                                 colours, footprint and added symbols — as one file in your \
                                 documents",
                            )
                            .clicked()
                        {
                            self.open_workspace_export_picker();
                            ui.close_menu();
                        }
                        if ui
                            .button("Open from file…")
                            .on_hover_text(
                                "Open a workspace file. It replaces the cockpit on screen; a \
                                 file that cannot be read changes nothing.",
                            )
                            .clicked()
                        {
                            self.open_workspace_import_picker();
                            ui.close_menu();
                        }
                        // Read off the field, not the filesystem: this body
                        // runs every frame the menu is open.
                        let mut reopen: Option<std::path::PathBuf> = None;
                        ui.add_enabled_ui(!self.workspace.session().recent_on_disk().is_empty(), |ui| {
                            ui.menu_button("Open recent", |ui| {
                                for path in self.workspace.session().recent_on_disk() {
                                    if ui
                                        .button(crate::workspace_bundle::recent_label(path))
                                        // The same warning the bookmark list
                                        // carries: this replaces the cockpit,
                                        // and a trader mid-tape has to read
                                        // that before the click, not after.
                                        .on_hover_text(format!(
                                            "Replaces the cockpit on screen\n{}",
                                            path.display()
                                        ))
                                        .clicked()
                                    {
                                        reopen = Some(path.clone());
                                        ui.close_menu();
                                    }
                                }
                            })
                            .response
                            .on_disabled_hover_text("No workspace files opened yet");
                        });
                        if let Some(path) = reopen {
                            self.import_workspace_from(&path);
                        }
                        if ui
                            .button("Show where it's saved")
                            .on_hover_text(
                                "Open the folder quantick keeps your cockpit in, so you can see \
                                 it and back it up",
                            )
                            .clicked()
                        {
                            self.reveal_cockpit_home();
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui
                            .checkbox(self.workspace.session_mut().save_on_exit_mut(), "Save on exit")
                            .on_hover_text(
                                "Keep the arrangement automatically when the window closes. Off, \
                                 only Save workspace changes what quantick opens on.",
                            )
                            .changed()
                        {
                            // The setting lives in the file it governs, so
                            // switching it has to reach the disk now — not at
                            // the next exit, which is exactly the exit it may
                            // have just switched off.
                            self.save_workspace("save_on_exit_toggled");
                        }
                    });
                    self.workspace_menu_rect = Some(workspace_menu.response.rect);
                    ui.menu_button("Tools", |ui| {
                        if ui.button("Appearance…").clicked() {
                            self.surfaces.style_panel.open();
                            ui.close_menu();
                        }
                        self.draw_record_deals_toggle(ui);
                        let access_label = self
                            .control_access
                            .as_ref()
                            .map_or("Local agent access…", |access| access.menu_label());
                        if ui.button(access_label).clicked() {
                            if let Some(access) = self.control_access.as_mut() {
                                access.open_panel();
                            }
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("Replay file format…").clicked() {
                            self.replay_view.open_format_help();
                            ui.close_menu();
                        }
                    });
                    if self
                        .control_access
                        .as_ref()
                        .is_some_and(crate::control::ControlAccess::is_enabled)
                        && ui.button("Agent access: on").clicked()
                        && let Some(access) = self.control_access.as_mut()
                    {
                        access.open_panel();
                    }
                    ui.separator();
                    // The tab strip shares the menu row: zone 1 already had
                    // the horizontal room, so tabs cost no chrome budget.
                    tab_action = self.draw_tab_strip(ui);
                });
            });
        if let Some(action) = tab_action {
            self.apply_tab_action(action);
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
        tabstrip::draw(ui, &chips, self.active_tab)
    }
}
