//! Paints the workspace rows of the `⋮` menu, the menu above the status bar's
//! workspace cell and the name dialog. The words are `window_menu`'s, the state is
//! `workspace_menu`'s and what a row does is [`WorkspaceCommand`]'s.

use eframe::egui;
use quantick_appearance::tab_strip as sizes;
use quantick_control_schema::window_menu::{self as text, RowId, row_of};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::{DEFAULT_WORKSPACE, WorkspaceBook, WorkspaceColour};

use super::MenuCommand;
use crate::app::QuantickApp;
use crate::app::arrangement_host::ArrangementHost;

const CHIP_MENU_MIN_WIDTH_PT: f32 = 200.0;
const NAME_FIELD_WIDTH_PT: f32 = 220.0;
/// How far below the capsule the menu hangs.
const MENU_DROP_PT: f32 = 4.0;

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

    fn push(commands: &mut Vec<MenuCommand>, command: WorkspaceCommand<WorkspaceColour>) {
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
    /// cell's menu, its one-line summary.
    pub(super) fn list(self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>, summary: bool) {
        let (current, _) = self.state();
        let palette = crate::theme::get(ui.ctx());
        let edge = ui.max_rect().left() - ui.style().spacing.menu_margin.left;
        ui.spacing_mut().button_padding.x += sizes::WORKSPACE_ROW_INDENT_PT;
        for row in self.book.list() {
            let beside = if summary {
                self.book.summary(&row.name)
            } else {
                self.symbols(&row.tabs).join(", ")
            };
            let response = ui.add(
                egui::Button::new(row.name.as_str())
                    .selected(row.name == current)
                    .shortcut_text(crate::theme::supporting_text(ui, beside)),
            );
            // The row's bar at the menu's left edge, hollow without a colour.
            let y = response.rect.y_range().shrink(2.0);
            let bar = egui::Rect::from_x_y_ranges(edge..=edge + sizes::WORKSPACE_ROW_BAR_PT, y);
            match row.colour.and_then(|c| palette.workspace(c.index())) {
                Some(ink) => ui.painter().rect_filled(bar, 2.0, ink),
                None => ui
                    .painter()
                    .rect_stroke(bar, 2.0, (1.0, palette.text_faint())),
            };
            if response.clicked() {
                Self::push(commands, WorkspaceCommand::Set(row.name));
            }
        }
        ui.spacing_mut().button_padding.x -= sizes::WORKSPACE_ROW_INDENT_PT;
    }

    /// Below the list, the open workspace's eight colours: its own outlined,
    /// one another workspace holds dimmed under that owner's initials; a click
    /// gives it the colour, swapping with the owner, never sharing one.
    fn swatches(self, ui: &mut egui::Ui, workspace: &str, commands: &mut Vec<MenuCommand>) {
        let palette = crate::theme::get(ui.ctx());
        let caption = text::colour_caption(workspace);
        ui.label(crate::theme::supporting_text(ui, caption));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = sizes::WORKSPACE_SWATCH_GAP_PT;
            for swatch in self.book.swatches(workspace) {
                let ink = palette.workspace(swatch.colour.index()).unwrap_or_default();
                let held =
                    [1.0, sizes::WORKSPACE_SWATCH_USED_ALPHA][usize::from(swatch.owner.is_some())];
                let ring = (
                    if swatch.current { 2.0 } else { 0.0 },
                    palette.text_primary(),
                );
                let initials = egui::RichText::new(&swatch.initials).size(10.0).strong();
                let button = egui::Button::new(initials.color(palette.text_primary()))
                    .fill(ink.gamma_multiply(held))
                    .stroke(ring)
                    .min_size(egui::Vec2::splat(sizes::WORKSPACE_SWATCH_PT));
                let hover = text::swatch_hover(&swatch);
                if ui.add(button).on_hover_text(hover).clicked() && !swatch.current {
                    let pick = WorkspaceCommand::SetColour(workspace.to_owned(), swatch.colour);
                    Self::push(commands, pick);
                }
            }
        });
        ui.label(crate::theme::supporting_text(ui, text::COLOUR_SWAP_HINT));
    }

    /// The rows that act on the tab's workspace. Handles the rows `menus` routes
    /// here and answers false for any other.
    pub(super) fn action_row(
        self,
        ui: &mut egui::Ui,
        id: RowId,
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
            RowId::ManageWorkspaces => {
                if ui
                    .button(row.label)
                    .on_hover_text(row.hover.unwrap_or_default())
                    .clicked()
                {
                    Self::push(commands, WorkspaceCommand::BeginManage);
                }
            }
            _ => return false,
        }
        true
    }

    /// The menu below the capsule's workspace name (designer 01): auto save,
    /// save, save as, rename, the workspaces, new and manage.
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
        let (workspace, modified) = self.state();
        let auto = self.book.autosave();
        let area = egui::Area::new(egui::Id::new("tab_workspace_menu"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(anchor.right_bottom() + egui::vec2(0.0, MENU_DROP_PT))
            .constrain(true)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(CHIP_MENU_MIN_WIDTH_PT);
                    ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                        super::menus::flat_rows(ui);
                        self.action_row(ui, RowId::WorkspaceAutosave, commands);
                        let line = text::autosave_line(auto, &keys);
                        ui.label(crate::theme::supporting_text(ui, line));
                        ui.separator();
                        let save = egui::Button::new(format!("Save \u{201c}{workspace}\u{201d}"))
                            .shortcut_text(crate::theme::supporting_text(ui, keys));
                        let save = ui.add_enabled(modified && !auto, save);
                        if modified && !auto {
                            let dot = save.rect.left_center() + egui::vec2(-1.0, 0.0);
                            ui.painter().circle_filled(
                                dot,
                                2.5,
                                crate::theme::get(ui.ctx()).accent(),
                            );
                        }
                        if save.clicked() {
                            Self::push(commands, WorkspaceCommand::SaveTo);
                        }
                        self.action_row(ui, RowId::WorkspaceSaveAsNew, commands);
                        self.action_row(ui, RowId::WorkspaceRename, commands);
                        ui.separator();
                        ui.label(crate::theme::supporting_text(ui, "WORKSPACES"));
                        self.list(ui, commands, true);
                        ui.separator();
                        self.swatches(ui, workspace, commands);
                        ui.separator();
                        self.action_row(ui, RowId::WorkspaceNew, commands);
                        self.action_row(ui, RowId::ManageWorkspaces, commands);
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
