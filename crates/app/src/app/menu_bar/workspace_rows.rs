//! Paints the workspace rows of the `⋮` menu, the menu above the status bar's
//! workspace cell and the name dialog. The words are `window_menu`'s, the state is
//! `workspace_menu`'s and what a row does is [`WorkspaceCommand`]'s.

use eframe::egui;
use quantick_control_handlers::workspace_cell_menu::{CellRow, RowKind, action_row, cell_menu};
use quantick_control_handlers::workspace_cell_menu::{state_of, window_list};
use quantick_control_schema::window_menu::{self as text, RowId, row_of};
use quantick_session::workspace_menu::{self as model, ChipSizing, MenuBounds, MenuMetrics};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::WorkspaceBook;

use super::MenuCommand;
use crate::app::QuantickApp;
use crate::app::arrangement_host::ArrangementHost;

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
        let (workspace, modified) = state_of(self.book, self.tab);
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

    /// Every workspace, the active tab's lit, beside the tabs on it.
    pub(super) fn list(self, ui: &mut egui::Ui, commands: &mut Vec<MenuCommand>) {
        for row in window_list(self.book, self.tab, |tabs| self.symbols(tabs)) {
            let asked = cell_row(ui, &row, &mut ChipSizing::default(), &mut Vec::new());
            commands.extend(asked.map(MenuCommand::Workspace));
        }
    }

    /// The rows that act on the tab's workspace. Handles the rows `menus` routes
    /// here and answers false for any other.
    pub(super) fn action_row(
        self,
        ui: &mut egui::Ui,
        id: RowId,
        commands: &mut Vec<MenuCommand>,
    ) -> bool {
        let Some(row) = action_row(self.book, self.tab, id) else {
            return false;
        };
        let asked = cell_row(ui, &row, &mut ChipSizing::default(), &mut Vec::new());
        commands.extend(asked.map(MenuCommand::Workspace));
        true
    }

    /// The menu below the capsule's workspace name: `cell_menu`'s rows within
    /// `chip_menu_budget`, the workspaces alone scrolling, and their scene.
    pub(super) fn chip_menu(
        self,
        ctx: &egui::Context,
        anchor: egui::Rect,
        menu: &mut WorkspaceMenuState,
        keys: String,
        commands: &mut Vec<MenuCommand>,
    ) -> Vec<(CellRow, egui::Rect)> {
        let mut drawn = Vec::new();
        if !menu.chip_menu_on(self.tab) {
            return drawn;
        }
        let rows = cell_menu(self.book, self.tab, &keys);
        let (recorded, (measuring, shown)) =
            (commands.len(), menu.chip_sizing.frame(rows.signature()));
        let id = egui::Id::new(("tab_workspace_menu", self.tab));
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(anchor.right_bottom() + egui::vec2(0.0, model::CHIP_MENU_DROP_PT))
            .constrain(true)
            .show(ctx, |ui| {
                super::menus::flat_rows(ui);
                let at = |r: egui::Rect| MenuBounds::new(r.left(), r.top(), r.right(), r.bottom());
                let (sp, text) = (ui.spacing(), ui.text_style_height(&egui::TextStyle::Button));
                let button = text + 2.0 * sp.button_padding.y;
                let m = MenuMetrics::of(button, sp.item_spacing.y, sp.menu_margin.sum().y);
                let n = rows.list.len();
                let budget = model::chip_menu_budget(at(ctx.screen_rect()), at(anchor), n, m);
                ui.set_max_width(ui.available_width().min(budget.width(measuring)));
                if !shown {
                    ui.set_invisible();
                }
                if !measuring {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                }
                let layout = egui::Layout::default().with_cross_justify(!measuring);
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(budget.min_width);
                    ui.with_layout(layout, |ui| {
                        let mut draw = |ui: &mut egui::Ui, rows: &[CellRow]| {
                            for row in rows {
                                let asked = cell_row(ui, row, &mut menu.chip_sizing, &mut drawn);
                                commands.extend(asked.map(MenuCommand::Workspace));
                            }
                        };
                        draw(ui, &rows.head);
                        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
                        egui::ScrollArea::vertical()
                            .max_height(budget.list_max())
                            .show(ui, |ui| draw(ui, &rows.list));
                        draw(ui, &rows.foot);
                    });
                });
            });
        let rect = area.response.rect;
        let outside = |at: egui::Pos2| !anchor.contains(at) && !rect.contains(at);
        let dismissed = ctx.input_mut(|input| {
            let pressed =
                input.pointer.any_pressed() && input.pointer.interact_pos().is_some_and(outside);
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) || pressed
        });
        if commands.len() > recorded || dismissed {
            menu.close_chip_menu();
        }
        drawn.insert(0, (CellRow::menu(), rect));
        drawn.retain(|_| shown); // An unseen frame shows nothing to press.
        drawn
    }
}

/// One cell-menu row as its kind draws it; answers what a press on it asked.
fn cell_row(
    ui: &mut egui::Ui,
    row: &CellRow,
    sizing: &mut ChipSizing,
    drawn: &mut Vec<(CellRow, egui::Rect)>,
) -> Option<WorkspaceCommand> {
    let (label, mut on) = (row.label.as_str(), row.on);
    let mut response = match row.kind {
        RowKind::Separator => ui.separator(),
        RowKind::Caption => ui.label(crate::theme::supporting_text(ui, label)),
        RowKind::Switch => ui.checkbox(&mut on, label),
        RowKind::Press => {
            let beside = crate::theme::supporting_text(ui, row.beside.as_str());
            let button = egui::Button::new(label).selected(on).shortcut_text(beside);
            ui.add_enabled(row.unavailable.is_none(), button)
        }
    };
    let asked = response.intrinsic_size.map_or(0.0, |size| size.x);
    let cut = row.kind == RowKind::Press && sizing.cut(&row.key, asked, ui.available_width());
    if let Some(text) = row.hover.or(cut.then_some(label)) {
        response = response.on_hover_text(text);
    }
    if let Some(text) = row.disabled_hover {
        response = response.on_disabled_hover_text(text);
    }
    if response.gained_focus() || (row.lit() && sizing.measuring()) {
        response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
    }
    if row.dot {
        let at = response.rect.left_center() + egui::vec2(model::CHIP_MENU_DOT_INSET_PT, 0.0);
        let accent = crate::theme::get(ui.ctx()).accent();
        ui.painter().circle_filled(at, 2.5, accent);
    }
    let seen = response.rect.intersect(ui.clip_rect());
    if !row.key.is_empty() && seen.height() * 2.0 >= response.rect.height() {
        drawn.push((row.clone(), seen));
    }
    response.clicked().then(|| row.command.clone()).flatten()
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
