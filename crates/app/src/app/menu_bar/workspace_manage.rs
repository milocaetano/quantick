//! Paints the Manage workspaces dialog: every workspace with its tab count, a
//! check on the tab's own, and per row Rename and Delete. Delete lives only
//! here and asks inside the row first. The words are `window_menu`'s, the state
//! `workspace_menu`'s, and what a button does is [`WorkspaceCommand`]'s, the
//! engine the menu and `workspace.delete` share.

use eframe::egui;
use quantick_control_schema::window_menu as text;
use quantick_session::workspace_menu::{DeleteClick, WorkspaceCommand};

use super::MenuCommand;
use crate::app::QuantickApp;

const NAME_COLUMN_WIDTH_PT: f32 = 160.0;

/// The dialog, in the window presenting the tab whose menu opened it.
pub(super) fn manage_dialog(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    commands: &mut Vec<MenuCommand>,
) {
    let tab = app.runtime.tabs.active_id();
    let book = app.runtime.workspace.book.book();
    let current = book.tab_state(tab).map(|state| state.workspace.to_owned());
    let rows = book.list();
    let tabs = &app.runtime.tabs;
    let state = &mut app.chrome.window_menu.tab_workspace;
    let Some(dialog) = state.manage.as_mut().filter(|dialog| dialog.tab == tab) else {
        return;
    };
    let mut push = |command| commands.push(MenuCommand::Workspace(command));
    let mut open = true;
    egui::Window::new(text::MANAGE_DIALOG_TITLE)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("manage_workspaces")
                .striped(true)
                .show(ui, |ui| {
                    for row in rows {
                        let is_current = current.as_deref() == Some(row.name.as_str());
                        let check = if is_current { "\u{2713}" } else { "" };
                        ui.label(check).on_hover_text(text::MANAGE_CURRENT_HOVER);
                        ui.add_sized(
                            [NAME_COLUMN_WIDTH_PT, 0.0],
                            egui::Label::new(egui::RichText::new(&row.name).strong()).truncate(),
                        );
                        ui.label(crate::theme::supporting_text(
                            ui,
                            text::tab_count_label(row.tabs.len()),
                        ));
                        let protected = book.delete_refusal(&row.name).is_some();
                        let why = text::DEFAULT_PROTECTED_HOVER;
                        if dialog.armed.as_deref() == Some(row.name.as_str()) {
                            let moved = crate::app::tab_workspaces::symbols_of(tabs, &row.tabs);
                            ui.label(text::delete_armed_label(&row.name, &moved));
                            if ui.button(text::MANAGE_DELETE).clicked()
                                && dialog.click_delete(&row.name) == DeleteClick::Confirmed
                            {
                                push(WorkspaceCommand::Delete(row.name.clone()));
                            }
                            if ui.button(text::DIALOG_CANCEL).clicked() {
                                dialog.armed = None;
                            }
                        } else {
                            let rename =
                                ui.add_enabled(!protected, egui::Button::new(text::MANAGE_RENAME));
                            if rename.on_disabled_hover_text(why).clicked() {
                                push(WorkspaceCommand::BeginRename(row.name.clone()));
                            }
                            let delete =
                                ui.add_enabled(!protected, egui::Button::new(text::MANAGE_DELETE));
                            if delete.on_disabled_hover_text(why).clicked() {
                                dialog.click_delete(&row.name);
                            }
                        }
                        ui.end_row();
                    }
                });
            ui.separator();
            open &= !ui.button(text::MANAGE_CLOSE).clicked();
            open &=
                !ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        });
    if !open {
        state.close_manage();
    }
}
