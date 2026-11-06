//! Paints the workspace rows of the `⋮` menu, the menu above the status bar's
//! workspace cell and the name dialog. The words are `window_menu`'s, the state is
//! `workspace_menu`'s and what a row does is [`WorkspaceCommand`]'s.

use eframe::egui;
use quantick_control_schema::window_menu::{self as text, RowId, row_of};
use quantick_session::workspace_menu::{
    DeleteClick, NameDialogMode, WorkspaceCommand, WorkspaceMenuState,
};
use quantick_stores::workspaces::{DEFAULT_WORKSPACE, WorkspaceBook};

use super::MenuCommand;
use crate::app::QuantickApp;
use crate::app::arrangement_host::ArrangementHost;

const CHIP_MENU_MIN_WIDTH_PT: f32 = 200.0;
const NAME_FIELD_WIDTH_PT: f32 = 220.0;

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
        tabs.iter()
            .filter_map(|id| self.tabs.by_id(*id))
            .map(|tab| tab.symbol.clone())
            .collect()
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
                egui::Button::new(text::save_to_label(self.symbol, workspace)).shortcut_text(keys),
            )
            .on_hover_text(text::save_to_hover(&self.symbols(&followers)))
            .on_disabled_hover_text(row.disabled_hover.unwrap_or_default());
        if response.clicked() {
            Self::push(commands, WorkspaceCommand::SaveTo);
        }
        response
    }

    /// Every workspace, the active tab's lit, beside the tabs on it or, in the
    /// cell's menu, its one-line summary.
    pub(super) fn list(self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>, summary: bool) {
        let (current, _) = self.state();
        for row in self.book.list() {
            let beside = if summary {
                self.book.summary(&row.name)
            } else {
                self.symbols(&row.tabs).join(", ")
            };
            let response = ui.add(
                egui::Button::new(row.name.as_str())
                    .selected(row.name == current)
                    .shortcut_text(beside),
            );
            if response.clicked() {
                Self::push(commands, WorkspaceCommand::Set(row.name));
            }
        }
    }

    /// The rows that act on the tab's workspace. Handles the rows `menus` routes
    /// here and answers false for any other.
    pub(super) fn action_row(
        self,
        ui: &mut egui::Ui,
        id: RowId,
        menu: &mut WorkspaceMenuState,
        commands: &mut Vec<MenuCommand>,
    ) -> bool {
        let (workspace, _) = self.state();
        let on_default = workspace == DEFAULT_WORKSPACE;
        let row = row_of(id);
        match id {
            RowId::WorkspaceSaveAsNew => {
                if ui.button(row.label).clicked() {
                    Self::push(commands, WorkspaceCommand::BeginSaveAs);
                }
            }
            RowId::WorkspaceNew => {
                let hover = row.hover.unwrap_or_default();
                if ui.button(row.label).on_hover_text(hover).clicked() {
                    Self::push(commands, WorkspaceCommand::BeginNew);
                }
            }
            RowId::WorkspaceRename => {
                let response = ui
                    .add_enabled(!on_default, egui::Button::new(row.label))
                    .on_disabled_hover_text(row.disabled_hover.unwrap_or_default());
                if response.clicked() {
                    let from = workspace.to_owned();
                    Self::push(commands, WorkspaceCommand::BeginRename(from));
                }
            }
            RowId::WorkspaceAutosave => {
                let mut on = self.book.autosave();
                if ui.checkbox(&mut on, row.label).changed() {
                    Self::push(commands, WorkspaceCommand::SetAutosave(on));
                }
            }
            RowId::WorkspaceDelete => self.delete_row(ui, workspace, on_default, menu, commands),
            _ => return false,
        }
        true
    }

    /// Armed by the first click, when the row says what the second one will do.
    fn delete_row(
        self,
        ui: &mut egui::Ui,
        workspace: &str,
        on_default: bool,
        menu: &mut WorkspaceMenuState,
        commands: &mut Vec<MenuCommand>,
    ) {
        let row = row_of(RowId::WorkspaceDelete);
        let armed = menu.armed_delete.as_deref() == Some(workspace);
        let label = if armed {
            let moved = self
                .book
                .list()
                .into_iter()
                .find(|listing| listing.name == workspace)
                .map(|listing| self.symbols(&listing.tabs))
                .unwrap_or_default();
            text::delete_armed_label(workspace, &moved)
        } else {
            row.label.to_owned()
        };
        let response = ui
            .add_enabled(!on_default, egui::Button::new(label))
            .on_disabled_hover_text(row.disabled_hover.unwrap_or_default());
        if response.clicked() && menu.click_delete(workspace) == DeleteClick::Confirmed {
            Self::push(commands, WorkspaceCommand::Delete(workspace.to_owned()));
        }
    }

    /// The menu above the status bar's workspace cell, opening upward from its
    /// top-left. Opens with `WINV26 uses`.
    pub(super) fn chip_menu(
        self,
        ctx: &egui::Context,
        anchor: egui::Rect,
        menu: &mut WorkspaceMenuState,
        keys: String,
        commands: &mut Vec<MenuCommand>,
    ) {
        if !menu.chip_menu_on(self.tab) {
            return;
        }
        let recorded = commands.len();
        let area = egui::Area::new(egui::Id::new("tab_workspace_menu"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(anchor.left_top())
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(CHIP_MENU_MIN_WIDTH_PT);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        super::menus::flat_rows(ui);
                        ui.weak(text::uses_caption(self.symbol));
                        self.list(ui, commands, true);
                        ui.separator();
                        self.action_row(ui, RowId::WorkspaceAutosave, menu, commands);
                        self.save_to_row(ui, keys, commands);
                        for id in text::CELL_MENU_ROWS {
                            self.action_row(ui, *id, menu, commands);
                        }
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
    }
}

/// The dialog that asks for a name: Enter saves, Esc cancels, and a refusal stands
/// under the field with the name still in it.
pub(super) fn name_dialog(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    commands: &mut Vec<MenuCommand>,
) {
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
