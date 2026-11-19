//! What a press or a key in Manage workspaces and in the two create dialogs
//! does, and the two calls that open those dialogs and take a delete back
//! without a hand. Recolour, rename, delete and Undo reach the handlers the
//! `workspace.*` calls use, journaled alike; the dialog's state is
//! `quantick_session::manage_workspaces`'s, the time arrives in the intent
//! (the window's frame) or with the call (`requested_at_unix_ms`).

use quantick_control::error::ControlError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_schema::window_menu as text;
use quantick_control_schema::workspace_manage::{
    WORKSPACE_DIALOG_OPENED_EVENT_KIND, WORKSPACE_RESTORED_EVENT_KIND, WorkspaceDeleteUndoInput,
    WorkspaceDeleteUndoResult, WorkspaceDialog, WorkspaceDialogOpenInput,
    WorkspaceDialogOpenResult,
};
use quantick_session::colour_popover::ColourIntent;
use quantick_session::manage_workspaces::{ManageIntent, UndoRow};
use quantick_session::workspace_menu::{NameDialog, NameDialogMode, WorkspaceMenuState};
use quantick_stores::workspaces::palette::{Pick, choosing, said};
use quantick_stores::workspaces::{
    DeleteOutcome, UndoOutcome, WorkspaceBook, WorkspaceColour, WorkspaceRefusal, hex,
    parse_typed_hex, rgb_hsv,
};
use serde_json::{Value, json};

use super::cell_menu::state_of;
use super::colour_popover::{act_on, pressed};
use super::{
    Journal, WorkspaceOwner, answer, delete_journaled, input, journal, refused, rename_journaled,
    unavailable, wires,
};
use crate::notify::NotifyAccess;
use crate::tabs::TabDirectory;

/// Carry out a press or key in Manage workspaces; answers what to tell the
/// trader.
pub fn act<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    host: &mut H,
    to: &mut Journal<'_, A>,
    menu: &mut WorkspaceMenuState,
    intent: ManageIntent,
) -> Option<String> {
    let dialog = menu.manage.as_mut()?;
    let closes = match intent {
        ManageIntent::Close => true,
        ManageIntent::Escape => !dialog.escape(),
        _ => false,
    };
    if closes {
        menu.close_manage();
        return None;
    }
    if intent == ManageIntent::Escape || dialog.local(&intent) {
        return None;
    }
    match intent {
        ManageIntent::Colour(press) => {
            let row = dialog.palette.take()?;
            let mut open = Some(row.popover);
            let note = act_on(host, to, &mut open, &row.workspace, press);
            dialog.palette = open.map(|popover| quantick_session::manage_workspaces::RowPalette {
                workspace: row.workspace,
                popover,
            });
            dialog.note.clone_from(&note);
            note
        }
        ManageIntent::CommitRename => {
            let rename = dialog.rename.clone()?;
            match rename_journaled(host, to, &rename.from, &rename.name) {
                Ok(moved) => {
                    dialog.rename = None;
                    moved
                        .changed
                        .then(|| text::renamed_toast(&rename.from, &moved.workspace))
                }
                Err(error) => {
                    dialog.refuse_rename(error.message);
                    None
                }
            }
        }
        ManageIntent::Delete { now_ms } => {
            let name = dialog.armed.clone()?;
            deleted(host, to, menu, &name, now_ms)
        }
        ManageIntent::Undo { now_ms } => {
            let name = dialog.undo_row(now_ms)?.workspace.clone();
            match undo_journaled(host, to, &name, now_ms) {
                Ok(undone) => {
                    let note = undone_note(&undone);
                    if let Some(dialog) = &mut menu.manage {
                        dialog.undone(&name);
                        dialog.note = Some(note.clone());
                    }
                    Some(note)
                }
                Err(error) => Some(error.message),
            }
        }
        _ => None,
    }
}

/// Delete `name` at `now_ms`, journaled as `workspace.delete` is; Manage, when
/// open, puts the Undo row in its place. Answers the toast.
fn deleted<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    host: &mut H,
    to: &mut Journal<'_, A>,
    menu: &mut WorkspaceMenuState,
    name: &str,
    now_ms: i64,
) -> Option<String> {
    let gone = match delete_journaled(host, to, name, Some(now_ms)) {
        Ok(gone) => gone,
        Err(error) => return Some(error.message),
    };
    note_deleted(host, menu, name, &gone);
    Some(text::deleted_toast(
        name,
        &host.tab_symbols(&gone.adopted),
        &host.tab_symbols(&gone.kept_edits),
    ))
}

/// A delete landed while Manage may be open: its Undo row, with the tabs
/// that went back to Default, stands for as long as the book takes it back.
pub(super) fn note_deleted<H: WorkspaceOwner + ?Sized>(
    host: &H,
    menu: &mut WorkspaceMenuState,
    name: &str,
    gone: &DeleteOutcome,
) {
    let until = host
        .workspace_book_ref()
        .and_then(|book| book.undo_until(name));
    let (Some(dialog), Some(until_ms)) = (menu.manage.as_mut(), until) else {
        return;
    };
    dialog.deleted(UndoRow {
        workspace: name.to_owned(),
        words: text::deleted_row_label(
            name,
            &host.tab_symbols(&gone.adopted),
            &host.tab_symbols(&gone.kept_edits),
        ),
        until_ms,
    });
}

/// `Scalp is back`, or `Scalp is back as Orchid`.
fn undone_note(undone: &UndoOutcome) -> String {
    let recoloured = undone.recoloured.then(|| said(undone.colour));
    text::undone_label(&undone.workspace, recoloured.as_deref())
}

/// Take back the delete of `name`, journaled, redrawing the tabs moved back.
fn undo_journaled<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    host: &mut H,
    to: &mut Journal<'_, A>,
    name: &str,
    now_ms: i64,
) -> Result<UndoOutcome, ControlError> {
    let undo = |book: &mut WorkspaceBook| book.undo_delete(name, now_ms);
    let undone = super::written(host, undo, |undone| undone.changed)?;
    super::show(host, &undone.tabs)?;
    if undone.changed {
        let payload = json!({ "workspace": name, "colour": undone.colour.key(),
            "recoloured": undone.recoloured, "tabs": wires(&undone.tabs) });
        journal(to, WORKSPACE_RESTORED_EVENT_KIND, payload);
    }
    Ok(undone)
}

/// The create dialog just opened on `menu`, once: its suggested name
/// (`Flow 2`, `Workspace 9`) and the next free colour.
pub fn fill_create<H: WorkspaceOwner + ?Sized>(host: &H, menu: &mut WorkspaceMenuState) {
    let (Some(book), Some(dialog)) = (host.workspace_book_ref(), menu.dialog.as_mut()) else {
        return;
    };
    if dialog.colour.is_some() {
        return;
    }
    dialog.name = match dialog.mode {
        NameDialogMode::SaveAsNew => book.suggested_name(state_of(book, dialog.tab).0, 2),
        NameDialogMode::New => book.suggested_name("Workspace", book.list().len() + 1),
        NameDialogMode::Rename { .. } => return,
    };
    dialog.colour = Some(choosing(book, None).current.key());
    dialog.creation = initial_creation(host, dialog.tab);
}

pub fn initial_creation<H: WorkspaceOwner + ?Sized>(
    host: &H,
    tab: u64,
) -> quantick_workspace::workspace_book::WorkspaceCreation {
    let mut creation = quantick_workspace::workspace_book::WorkspaceCreation::default();
    if let Some(ms) = host.creation_timeframe_ms(tab) {
        creation.timeframe_ms = ms;
    }
    creation
}

pub fn validate_creation(
    creation: quantick_workspace::workspace_book::WorkspaceCreation,
) -> Result<(), ControlError> {
    creation.recipe().map_err(|error| refused(&error))?;
    let ms = i64::try_from(creation.timeframe_ms).ok();
    if !ms.is_some_and(quantick_engine::bar_registry::is_time_interval) {
        return Err(refused(&WorkspaceRefusal::CreationInvalid));
    }
    Ok(())
}

pub fn creation_colour(dialog: &NameDialog) -> Result<Option<WorkspaceColour>, ControlError> {
    chosen(dialog)
        .map(Some)
        .ok_or_else(|| refused(&WorkspaceRefusal::ColourInvalid(dialog.palette.hex.clone())))
}

/// New uses its live custom draft. Save as uses the last applied swatch.
#[must_use]
pub fn chosen(dialog: &NameDialog) -> Option<WorkspaceColour> {
    if dialog.mode == NameDialogMode::New && dialog.palette.custom {
        return parse_typed_hex(&dialog.palette.hex).map(WorkspaceColour::Custom);
    }
    WorkspaceColour::parse(dialog.colour.as_deref()?)
}

/// A press in the create dialog's palette: a free swatch is chosen, a held
/// one is locked and does nothing; Custom opens on the chosen colour and its
/// Apply chooses the typed one when it is free. Nothing reaches the book
/// before Save.
pub fn name_colour<H: WorkspaceOwner + ?Sized>(
    host: &H,
    menu: &mut WorkspaceMenuState,
    intent: ColourIntent,
) {
    let (Some(book), Some(dialog)) = (host.workspace_book_ref(), menu.dialog.as_mut()) else {
        return;
    };
    let current = WorkspaceColour::parse(dialog.colour.as_deref().unwrap_or_default());
    let model = choosing(book, current);
    let popover = &mut dialog.palette;
    let picked = match intent {
        ColourIntent::Pick(word) => model.swatch(&word).map(|swatch| swatch.colour),
        ColourIntent::Apply => parse_typed_hex(&popover.hex).map(WorkspaceColour::Custom),
        ColourIntent::Custom { dark } => {
            let rgb = model.current.rgb(dark);
            popover.open_custom(hex(rgb), rgb_hsv(rgb));
            None
        }
        ColourIntent::Square { x, y } => pressed(popover, None, Some((x, y))).and(None),
        ColourIntent::Hue(x) => pressed(popover, Some(x), None).and(None),
        ColourIntent::Close | ColourIntent::Toggle | ColourIntent::Answer(_) => {
            if dialog.mode == NameDialogMode::New
                && popover.custom
                && let Some(rgb) = parse_typed_hex(&popover.hex)
            {
                dialog.colour = Some(WorkspaceColour::Custom(rgb).key());
            }
            popover.custom = false;
            None
        }
    };
    if let Some(Pick::Apply(colour)) = picked.map(|colour| model.pick(colour, false)) {
        dialog.colour = Some(colour.key());
        popover.custom = false;
    }
}

/// The tab a call names, among the open ones; refused when none has it.
fn tab_of<H: TabDirectory + ?Sized>(app: &H, tab_id: Option<WireU64>) -> Result<u64, ControlError> {
    let index = match tab_id {
        None => app.active_tab_index(),
        Some(id) => app
            .tab_position(id.get())
            .ok_or_else(|| refused(&WorkspaceRefusal::UnknownTab(id.get())))?,
    };
    Ok(app.tab_id_at(index))
}

/// `workspace.dialog.open`.
pub(super) fn dialog_open<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceDialogOpenInput {
        mode,
        tab_id,
        timeframe_ms,
        charts,
        custom_hex,
    } = input(value)?;
    let tab = tab_of(app, tab_id)?;
    if mode != WorkspaceDialog::New
        && (timeframe_ms.is_some() || charts.is_some() || custom_hex.is_some())
    {
        return Err(ControlError::invalid_request(
            "creation choices require the new workspace dialog",
        ));
    }
    let mut creation = initial_creation(app, tab);
    if let Some(ms) = timeframe_ms {
        creation.timeframe_ms = ms.get();
    }
    if let Some(charts) = charts {
        creation.charts = charts;
    }
    validate_creation(creation)?;
    if custom_hex.as_ref().is_some_and(|word| word.len() > 16) {
        return Err(ControlError::invalid_request(
            "a color draft holds at most 16 characters",
        ));
    }
    let mut menu = std::mem::take(app.workspace_menu().ok_or_else(unavailable)?.0);
    menu.close_chip_menu();
    match mode {
        WorkspaceDialog::SaveAs => menu.open_save_as(tab),
        WorkspaceDialog::New => menu.open_new(tab),
        WorkspaceDialog::Manage => menu.open_manage(tab),
    }
    fill_create(app, &mut menu);
    if let Some(dialog) = &mut menu.dialog {
        dialog.creation = creation;
        if let Some(hex) = custom_hex {
            let rgb = parse_typed_hex(&hex).unwrap_or([0, 0, 0]);
            dialog.palette.open_custom(hex, rgb_hsv(rgb));
        }
    }
    let dialog = menu
        .dialog
        .as_ref()
        .filter(|_| mode != WorkspaceDialog::Manage);
    let title = match mode {
        WorkspaceDialog::SaveAs => text::SAVE_AS_DIALOG_TITLE,
        WorkspaceDialog::New => text::NEW_DIALOG_TITLE,
        WorkspaceDialog::Manage => text::MANAGE_DIALOG_TITLE,
    };
    let result = WorkspaceDialogOpenResult {
        dialog: mode.word().to_owned(),
        title: title.to_owned(),
        tab_id: WireU64::new(tab),
        name: dialog.map(|dialog| dialog.name.clone()),
        colour: dialog.and_then(chosen).map(Into::into),
        timeframe_ms: (mode == WorkspaceDialog::New).then_some(WireU64::new(creation.timeframe_ms)),
        charts: (mode == WorkspaceDialog::New).then_some(creation.charts),
    };
    *app.workspace_menu().ok_or_else(unavailable)?.0 = menu;
    let payload = json!({ "dialog": mode.word(), "tab_id": WireU64::new(tab) });
    journal(
        &mut Some((access, actor)),
        WORKSPACE_DIALOG_OPENED_EVENT_KIND,
        payload,
    );
    answer(&result)
}

/// `workspace.delete.undo`, at the time the call was asked.
pub(super) fn undo_call<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceDeleteUndoInput { name } = input(value)?;
    let now_ms = actor.requested_at_unix_ms;
    let undone = undo_journaled(app, &mut Some((access, actor)), &name, now_ms)?;
    if let Some(dialog) = app.workspace_menu().and_then(|menu| menu.0.manage.as_mut()) {
        dialog.undone(&name);
    }
    answer(&WorkspaceDeleteUndoResult {
        workspace: undone.workspace,
        changed: undone.changed,
        colour: undone.colour.into(),
        recoloured: undone.recoloured,
        tabs: wires(&undone.tabs),
    })
}

#[cfg(test)]
#[path = "manage_tests.rs"]
mod tests;
