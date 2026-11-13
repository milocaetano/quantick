//! Paints the workspace rows of the `⋮` menu, the menu above the status bar's
//! workspace cell and the name dialog. The words are `window_menu`'s, the state is
//! `workspace_menu`'s and what a row does is [`WorkspaceCommand`]'s.

use eframe::egui;
use quantick_control_schema::window_menu::{self as text, RowId, row_of};
use quantick_control_schema::workspace_book as calls;
use quantick_session::workspace_menu::{
    CHIP_MENU_DROP_PT, CHIP_MENU_MIN_WIDTH_PT, MenuBounds, MenuMetrics, NameDialogMode,
    WorkspaceCommand, WorkspaceMenuState, chip_menu_budget,
};
use quantick_stores::workspaces::{DEFAULT_WORKSPACE, WorkspaceBook};

use super::MenuCommand;
use crate::app::QuantickApp;
use crate::app::arrangement_host::ArrangementHost;
use crate::toolbar::DrawnControl;

const NAME_FIELD_WIDTH_PT: f32 = 220.0;
/// The height egui gives a horizontal separator before the gap after it.
const SEPARATOR_PT: f32 = 6.0;
/// The unsaved dot's centre, in from the Save row's left edge: inside the row,
/// in the padding before its text.
const UNSAVED_DOT_INSET_PT: f32 = 5.0;
/// The scene id of the cell's menu; its rows are `<this>.<row>`.
const CHIP_MENU_CONTROL: &str = "toolbar.workspace_menu";

/// What the workspace rows read this frame.
#[derive(Clone, Copy)]
pub(super) struct WorkspaceRows<'a> {
    pub(super) book: &'a WorkspaceBook,
    pub(super) tabs: &'a ArrangementHost,
    pub(super) tab: u64,
    pub(super) symbol: &'a str,
}

impl<'a> WorkspaceRows<'a> {
    /// The rows of the window's presented tab.
    pub(super) fn of(app: &'a QuantickApp) -> Self {
        Self {
            book: app.runtime.workspace.book.book(),
            tabs: &app.runtime.tabs,
            tab: app.runtime.tabs.active_id(),
            symbol: app.active_tab().symbol.as_str(),
        }
    }

    fn state(&self) -> (&str, bool) {
        self.book
            .tab_state(self.tab)
            .map_or((DEFAULT_WORKSPACE, false), |state| {
                (state.workspace, state.modified)
            })
    }

    fn symbols(&self, tabs: &[u64]) -> Vec<String> {
        crate::app::tab_workspaces::symbols_of(self.tabs, tabs)
    }

    fn push(commands: &mut Vec<MenuCommand>, command: WorkspaceCommand) {
        commands.push(MenuCommand::Workspace(command));
    }

    /// `Save WINV26 to Scalp`, which names the tabs it will change before the click.
    pub(super) fn save_to_row(
        self,
        ui: &mut egui::Ui,
        keys: String,
        commands: &mut Vec<MenuCommand>,
    ) -> egui::Response {
        let (workspace, modified) = self.state();
        let row = row_of(RowId::SaveToWorkspace);
        let followers = self.book.save_to_preview(self.tab).unwrap_or_default();
        let response = ui
            .add_enabled(
                modified,
                egui::Button::new(text::save_to_label(self.symbol, workspace))
                    .shortcut_text(crate::theme::supporting_text(ui, keys)),
            )
            .on_hover_text(text::save_to_hover(&self.symbols(&followers)))
            .on_disabled_hover_text(row.disabled_hover.unwrap_or_default());
        if response.clicked() {
            Self::push(commands, WorkspaceCommand::SaveTo);
        }
        response
    }

    /// Every workspace, the active tab's lit, beside the tabs on it or, in the
    /// cell's menu, its one-line summary with the name cut to fit and whole on
    /// hover. Answers the rows in view, for the scene.
    pub(super) fn list(
        self,
        ui: &mut egui::Ui,
        commands: &mut Vec<MenuCommand>,
        summary: bool,
    ) -> Vec<(String, egui::Rect)> {
        let (current, _) = self.state();
        let mut shown = Vec::new();
        for row in self.book.list() {
            let beside = if summary {
                self.book.summary(&row.name)
            } else {
                self.symbols(&row.tabs).join(", ")
            };
            let beside = egui::WidgetText::from(crate::theme::supporting_text(ui, beside));
            let (name, cut) = fitted(ui, &row.name, &beside, summary);
            let response = ui.add(
                egui::Button::new(name)
                    .selected(row.name == current)
                    .shortcut_text(beside),
            );
            let response = if cut {
                response.on_hover_text(row.name.as_str())
            } else {
                response
            };
            if ui.is_rect_visible(response.rect) {
                shown.push((row.name.clone(), response.rect));
            }
            if response.clicked() {
                Self::push(commands, WorkspaceCommand::Set(row.name));
            }
        }
        shown
    }

    /// The rows that act on the tab's workspace. Handles the rows `menus` routes
    /// here and answers `None` for any other.
    pub(super) fn action_row(
        self,
        ui: &mut egui::Ui,
        id: RowId,
        commands: &mut Vec<MenuCommand>,
    ) -> Option<egui::Response> {
        let (workspace, _) = self.state();
        let on_default = workspace == DEFAULT_WORKSPACE;
        let row = row_of(id);
        let (hover, mut on) = (row.hover.unwrap_or_default(), self.book.autosave());
        let response = match id {
            RowId::WorkspaceSaveAsNew => ui.button(row.label),
            RowId::WorkspaceNew | RowId::ManageWorkspaces => {
                ui.button(row.label).on_hover_text(hover)
            }
            RowId::WorkspaceRename => ui
                .add_enabled(!on_default, egui::Button::new(row.label))
                .on_disabled_hover_text(row.disabled_hover.unwrap_or_default()),
            RowId::WorkspaceAutosave => ui.checkbox(&mut on, row.label),
            _ => return None,
        };
        let command = match id {
            RowId::WorkspaceAutosave if response.changed() => WorkspaceCommand::SetAutosave(on),
            _ if !response.clicked() => return Some(response),
            RowId::WorkspaceSaveAsNew => WorkspaceCommand::BeginSaveAs,
            RowId::WorkspaceNew => WorkspaceCommand::BeginNew,
            RowId::WorkspaceRename => WorkspaceCommand::BeginRename(workspace.to_owned()),
            RowId::ManageWorkspaces => WorkspaceCommand::BeginManage,
            _ => return Some(response),
        };
        Self::push(commands, command);
        Some(response)
    }

    /// The menu below the capsule's workspace name (designer 01): auto save,
    /// save, save as, rename, the workspaces, new and manage. It keeps to the
    /// budget [`chip_menu_budget`] sets, the workspaces alone scrolling, and
    /// answers the menu and its rows in view, for the scene.
    pub(super) fn chip_menu(
        self,
        ctx: &egui::Context,
        anchor: egui::Rect,
        menu: &mut WorkspaceMenuState,
        keys: String,
        commands: &mut Vec<MenuCommand>,
    ) -> Vec<DrawnControl> {
        let mut drawn = Vec::new();
        if !menu.chip_menu_on(self.tab) {
            return drawn;
        }
        let recorded = commands.len();
        let workspaces = self.book.list().len();
        let mut sizing = false;
        // An area is laid out inside its last frame's size, so one kept across
        // openings would never narrow: each opening and each tab gets its own.
        let id = ("tab_workspace_menu", self.tab, menu.chip_menu_opened);
        let area = egui::Area::new(egui::Id::new(id))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(anchor.right_bottom() + egui::vec2(0.0, CHIP_MENU_DROP_PT))
            .default_width(CHIP_MENU_MIN_WIDTH_PT)
            .constrain(true)
            .show(ctx, |ui| {
                super::menus::flat_rows(ui);
                let (screen, at) = (bounds(ctx.screen_rect()), bounds(anchor));
                let budget = chip_menu_budget(screen, at, workspaces, metrics(ui));
                // The first pass measures every row whole; the next cut to fit.
                sizing = ui.is_sizing_pass();
                if !sizing {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui.set_max_width(ui.available_width().min(budget.max_width));
                }
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(budget.min_width);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        let height = budget.list_height;
                        self.chip_rows(ui, id, height, keys, commands, &mut drawn);
                    });
                });
            });
        let dismissed = ctx.input_mut(|input| {
            let escape = input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
            let outside = input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| !anchor.contains(at) && !area.response.rect.contains(at));
            escape || outside
        });
        if commands.len() > recorded || dismissed {
            menu.close_chip_menu();
        }
        if sizing {
            // The measuring pass draws nothing a reader could press.
            drawn.clear();
        } else {
            let list = calls::WORKSPACE_LIST_CAPABILITY_ID;
            let rect = area.response.rect;
            drawn.insert(0, chip_control("", "Workspace menu", rect, true, list));
        }
        drawn
    }

    /// The cell menu's rows, top to bottom, each one in view written to
    /// `drawn` with the call that does what it does.
    fn chip_rows(
        self,
        ui: &mut egui::Ui,
        id: (&'static str, u64, u64),
        list_height: f32,
        keys: String,
        commands: &mut Vec<MenuCommand>,
        drawn: &mut Vec<DrawnControl>,
    ) {
        let (workspace, modified) = self.state();
        let auto = self.book.autosave();
        self.chip_action(ui, RowId::WorkspaceAutosave, commands, drawn);
        let line = if auto {
            "Every change is saved".to_owned()
        } else {
            format!("Save with {keys}")
        };
        ui.label(crate::theme::supporting_text(ui, line));
        ui.separator();
        let label = format!("Save \u{201c}{workspace}\u{201d}");
        let save = egui::Button::new(label.as_str())
            .shortcut_text(crate::theme::supporting_text(ui, keys));
        let save = ui.add_enabled(modified && !auto, save);
        if modified && !auto {
            let inset = egui::vec2(UNSAVED_DOT_INSET_PT, 0.0);
            let accent = crate::theme::get(ui.ctx()).accent();
            ui.painter()
                .circle_filled(save.rect.left_center() + inset, 2.5, accent);
        }
        let save_to = calls::WORKSPACE_SAVE_TO_CAPABILITY_ID;
        drawn.push(chip_control("save", &label, save.rect, false, save_to));
        if save.clicked() {
            Self::push(commands, WorkspaceCommand::SaveTo);
        }
        self.chip_action(ui, RowId::WorkspaceSaveAsNew, commands, drawn);
        self.chip_action(ui, RowId::WorkspaceRename, commands, drawn);
        ui.separator();
        ui.label(crate::theme::supporting_text(ui, "WORKSPACES"));
        let shown = egui::ScrollArea::vertical()
            .id_salt(id)
            .max_height(list_height)
            .min_scrolled_height(list_height)
            .auto_shrink([false, true])
            .show(ui, |ui| self.list(ui, commands, true))
            .inner;
        let set = calls::TAB_WORKSPACE_SET_CAPABILITY_ID;
        for (name, rect) in shown {
            let key = format!("workspace.{name}");
            drawn.push(chip_control(&key, &name, rect, name == workspace, set));
        }
        ui.separator();
        self.chip_action(ui, RowId::WorkspaceNew, commands, drawn);
        self.chip_action(ui, RowId::ManageWorkspaces, commands, drawn);
    }

    /// One action row of the cell menu, written to `drawn` with its call.
    fn chip_action(
        self,
        ui: &mut egui::Ui,
        id: RowId,
        commands: &mut Vec<MenuCommand>,
        drawn: &mut Vec<DrawnControl>,
    ) {
        let (key, call) = match id {
            RowId::WorkspaceAutosave => ("autosave", calls::WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID),
            RowId::WorkspaceSaveAsNew => ("save_as", calls::WORKSPACE_SAVE_AS_CAPABILITY_ID),
            RowId::WorkspaceRename => ("rename", calls::WORKSPACE_RENAME_CAPABILITY_ID),
            RowId::WorkspaceNew => ("new", calls::WORKSPACE_NEW_CAPABILITY_ID),
            _ => ("manage", calls::WORKSPACE_LIST_CAPABILITY_ID),
        };
        let on = id == RowId::WorkspaceAutosave && self.book.autosave();
        if let Some(it) = self.action_row(ui, id, commands) {
            drawn.push(chip_control(key, row_of(id).label, it.rect, on, call));
        }
    }
}

/// One part of the cell menu as the scene reads it; an empty `key` is the
/// menu itself.
fn chip_control(
    key: &str,
    label: &str,
    rect: egui::Rect,
    selected: bool,
    call: &'static str,
) -> DrawnControl {
    let id = if key.is_empty() {
        CHIP_MENU_CONTROL.to_owned()
    } else {
        format!("{CHIP_MENU_CONTROL}.{key}")
    };
    DrawnControl {
        id,
        label: label.to_owned(),
        rect,
        selected,
        capability: Some(call),
    }
}

/// A rect as the headless budget reads it.
fn bounds(rect: egui::Rect) -> MenuBounds {
    MenuBounds {
        left: rect.left(),
        top: rect.top(),
        right: rect.right(),
        bottom: rect.bottom(),
    }
}

/// The heights the menu's style draws a row, a separator and the frame at.
fn metrics(ui: &egui::Ui) -> MenuMetrics {
    let spacing = ui.spacing();
    let text = ui.text_style_height(&egui::TextStyle::Button);
    let button = (text + 2.0 * spacing.button_padding.y).max(spacing.interact_size.y);
    let stroke = ui.visuals().window_stroke().width;
    MenuMetrics {
        row_pt: button + spacing.item_spacing.y,
        separator_pt: SEPARATOR_PT + spacing.item_spacing.y,
        frame_pt: spacing.menu_margin.sum().y + 2.0 * stroke,
    }
}

/// `name` cut to the room its row leaves beside `beside`, and whether it was
/// cut. Only the cell menu cuts, and never in the pass that measures it.
fn fitted(
    ui: &egui::Ui,
    name: &str,
    beside: &egui::WidgetText,
    cut: bool,
) -> (egui::WidgetText, bool) {
    if !cut || ui.is_sizing_pass() {
        return (name.into(), false);
    }
    let spacing = ui.spacing();
    let style = egui::TextStyle::Button;
    let extend = Some(egui::TextWrapMode::Extend);
    let beside = (beside.clone()).into_galley(ui, extend, f32::INFINITY, style.clone());
    let room = ui.available_width()
        - 2.0 * spacing.button_padding.x
        - spacing.item_spacing.x
        - beside.size().x;
    let truncate = Some(egui::TextWrapMode::Truncate);
    let galley = egui::WidgetText::from(name).into_galley(ui, truncate, room, style);
    let elided = galley.elided;
    (galley.into(), elided)
}

/// The workspace dialogs: the one asking a name over the Manage one.
pub(super) fn dialogs(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    name_dialog(app, ctx, commands); // First, so its Esc is not the Manage one's.
    super::workspace_manage::manage_dialog(app, ctx, commands);
}

/// The dialog that asks for a name: Enter saves, Esc cancels, and a refusal stands
/// under the field with the name still in it.
fn name_dialog(app: &mut QuantickApp, ctx: &egui::Context, commands: &mut Vec<MenuCommand>) {
    let palette = crate::theme::get(ctx);
    let tab = app.runtime.tabs.active_id();
    let state = &mut app.chrome.window_menu.tab_workspace;
    let Some(dialog) = state.dialog.as_mut().filter(|dialog| dialog.tab == tab) else {
        return;
    };
    let title = match &dialog.mode {
        NameDialogMode::SaveAsNew => text::SAVE_AS_DIALOG_TITLE,
        NameDialogMode::New => text::NEW_DIALOG_TITLE,
        NameDialogMode::Rename { .. } => text::RENAME_DIALOG_TITLE,
    };
    let mut cancelled = false;
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut dialog.name).desired_width(NAME_FIELD_WIDTH_PT),
            );
            if std::mem::take(&mut dialog.wants_focus) {
                field.request_focus();
            }
            if field.changed() {
                dialog.refusal = None;
            }
            if let Some(reason) = &dialog.refusal {
                ui.colored_label(palette.warn(), reason);
            }
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            cancelled =
                ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            ui.horizontal(|ui| {
                if ui.button(text::DIALOG_SAVE).clicked() || (enter && !cancelled) {
                    commands.push(MenuCommand::Workspace(WorkspaceCommand::SubmitDialog));
                }
                cancelled |= ui.button(text::DIALOG_CANCEL).clicked();
            });
        });
    if cancelled {
        state.close_dialog();
    }
}
