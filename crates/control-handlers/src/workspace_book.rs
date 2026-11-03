//! `tab.workspace.set` and the `workspace.*` calls, through a headless host port.
//!
//! The book decides; the window lends the book, writes it and redraws a tab. A
//! call that changed nothing answers `changed: false` and journals nothing. A
//! write that did not reach the disk restores the book and refuses with the
//! reason, so memory and file never disagree and a retry writes again.
//!
//! Rate class: a human or an agent acting on the window. Never per trade.

use quantick_control::{
    error::{ControlError, codes},
    id::ErrorCode,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::window_menu as text;
use quantick_control_schema::workspace::MODULE_ID;
use quantick_control_schema::workspace_book::{
    TAB_NOT_READY_CODE, TAB_WORKSPACE_SET_EVENT_KIND, TabWorkspaceSetInput, TabWorkspaceSetResult,
    WORKSPACE_DELETED_EVENT_KIND, WORKSPACE_RENAMED_EVENT_KIND, WORKSPACE_REVERTED_EVENT_KIND,
    WORKSPACE_SAVED_EVENT_KIND, WRITE_FAILED_CODE, WorkspaceDeleteInput, WorkspaceDeleteResult,
    WorkspaceEntry, WorkspaceListInput, WorkspaceListResult, WorkspaceRenameInput,
    WorkspaceRenameResult, WorkspaceRevertResult, WorkspaceSaveAsInput, WorkspaceSaveResult,
    WorkspaceTabInput, tab_workspace_set_descriptor, workspace_delete_descriptor,
    workspace_list_descriptor, workspace_rename_descriptor, workspace_revert_descriptor,
    workspace_save_as_descriptor, workspace_save_to_descriptor,
};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::{
    DEFAULT_WORKSPACE, DeleteOutcome, RenameOutcome, SaveOutcome, SetOutcome, WorkspaceBook,
    WorkspaceRecipe, WorkspaceRefusal,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::dock::ActionDock;
use crate::notify::NotifyAccess;
use crate::tabs::{TabDirectory, tab_index};

fn unavailable() -> ControlError {
    ControlError::invalid_request("the workspaces are unavailable")
}

/// What the window lends these calls. The defaults answer for a host without a
/// book.
pub trait WorkspaceBookHost {
    /// The book the open tabs live on.
    fn workspace_book(&mut self) -> Option<&mut WorkspaceBook> {
        None
    }
    /// Write the recipes; `Err` is why nothing reached the disk.
    fn persist_workspace_book(&mut self) -> Result<(), String> {
        Err("the workspaces are unavailable".to_owned())
    }
    /// Put a recipe on a tab's charts.
    fn show_tab_recipe(&mut self, _tab: u64, _recipe: &WorkspaceRecipe) {}
    /// The tab's workspace and whether it holds edits, as the snapshot reads it.
    fn tab_workspace_row(&self, _tab: u64) -> (String, bool) {
        (DEFAULT_WORKSPACE.to_owned(), false)
    }
    /// The symbols of these tabs, in the order asked.
    fn tab_symbols(&self, _tabs: &[u64]) -> Vec<String> {
        Vec::new()
    }
}

/// Who journals a change: the call's access and actor, or the trader's hand
/// at the window. `None` where the window has no control identity; the change
/// still lands, and nothing is journaled.
pub type Journal<'a, A> = Option<(&'a mut A, &'a ActorContext)>;

/// Carry out what a workspace row asked for, through the same steps the
/// control call takes, so a row leaves the call's journal row. Answers what
/// to tell the trader, naming the tabs.
pub fn apply_row_command<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    host: &mut H,
    mut to: Journal<'_, A>,
    menu: &mut WorkspaceMenuState,
    tab: u64,
    symbol: &str,
    command: WorkspaceCommand,
) -> Option<String> {
    let note = match command {
        WorkspaceCommand::BeginSaveAs => {
            menu.open_save_as();
            return None;
        }
        WorkspaceCommand::BeginRename(from) => {
            menu.open_rename(&from);
            return None;
        }
        WorkspaceCommand::SubmitDialog => {
            return submit_name_dialog(host, &mut to, menu, tab, symbol);
        }
        WorkspaceCommand::Set(name) => set_journaled(host, &mut to, tab, &name).map(|set| {
            set.changed
                .then(|| text::switched_toast(symbol, &name, set.dropped_edits))
        }),
        WorkspaceCommand::SaveTo => save_to_journaled(host, &mut to, tab).map(|saved| {
            saved.changed.then(|| {
                text::saved_toast(&saved.workspace, &host.tab_symbols(&saved.tabs_changed))
            })
        }),
        WorkspaceCommand::Revert => revert_journaled(host, &mut to, tab)
            .map(|(changed, _)| changed.then(|| text::reverted_toast(symbol))),
        WorkspaceCommand::Delete(name) => delete_journaled(host, &mut to, &name).map(|gone| {
            gone.changed.then(|| {
                text::deleted_toast(
                    &name,
                    &host.tab_symbols(&gone.adopted),
                    &host.tab_symbols(&gone.kept_edits),
                )
            })
        }),
    };
    note.unwrap_or_else(|error| Some(error.message))
}

/// Enter on the name dialog: done and closed, or refused with the reason
/// under the field and the name still there.
fn submit_name_dialog<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    host: &mut H,
    to: &mut Journal<'_, A>,
    menu: &mut WorkspaceMenuState,
    tab: u64,
    symbol: &str,
) -> Option<String> {
    let dialog = menu.dialog.clone()?;
    let outcome = match &dialog.mode {
        NameDialogMode::SaveAsNew => save_as_journaled(host, to, tab, &dialog.name)
            .map(|saved| Some(text::saved_as_toast(symbol, &saved.workspace))),
        NameDialogMode::Rename { from } => {
            rename_journaled(host, to, from, &dialog.name).map(|moved| {
                moved
                    .changed
                    .then(|| text::renamed_toast(from, &moved.workspace))
            })
        }
    };
    match outcome {
        Ok(message) => {
            menu.close_dialog();
            message
        }
        Err(error) => {
            menu.refuse(error.message);
            None
        }
    }
}

/// Outside the replay trace: none of these changes a market, and a recorded
/// call must not rewrite the trader's workspaces when a session re-runs.
pub fn register<H, A, D>(registry: &mut D) -> Result<(), RegistryError>
where
    H: WorkspaceBookHost + TabDirectory + ?Sized,
    A: NotifyAccess,
    D: ActionDock<H, A>,
{
    registry.register_outside_replay(tab_workspace_set_descriptor(), set)?;
    registry.register_outside_replay(workspace_save_to_descriptor(), save_to)?;
    registry.register_outside_replay(workspace_save_as_descriptor(), save_as)?;
    registry.register_outside_replay(workspace_revert_descriptor(), revert)?;
    registry.register_outside_replay(workspace_rename_descriptor(), rename)?;
    registry.register_outside_replay(workspace_delete_descriptor(), delete)?;
    registry.register_outside_replay(workspace_list_descriptor(), list)
}

fn input<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, ControlError> {
    serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}

fn answer<T: Serialize>(result: &T) -> Result<Value, ControlError> {
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// The reason, and the code a caller branches on.
fn refused(refusal: &WorkspaceRefusal) -> ControlError {
    let mut error = ControlError::invalid_request(refusal.to_string());
    error.context.details = Some(json!({ "code": refusal.code() }));
    error
}

fn write_failed(reason: String) -> ControlError {
    let mut error = ControlError::new(
        ErrorCode::new(codes::CAPABILITY_UNAVAILABLE).expect("static error code is valid"),
        format!("the workspaces were not saved: {reason}"),
        true,
    );
    error.context.details = Some(json!({ "code": WRITE_FAILED_CODE }));
    error
}

fn wires(tabs: &[u64]) -> Vec<WireU64> {
    tabs.iter().copied().map(WireU64::new).collect()
}

fn book<H: WorkspaceBookHost + ?Sized>(app: &mut H) -> Result<&mut WorkspaceBook, ControlError> {
    app.workspace_book().ok_or_else(unavailable)
}

/// The tab a call names, on the book. A tab the book has not adopted yet is
/// refused before anything changes: its panes are still being built, and a
/// workspace put on it now would be undone when the desk's word lands.
fn tab_on_book<H: WorkspaceBookHost + TabDirectory + ?Sized>(
    app: &mut H,
    tab_id: Option<WireU64>,
) -> Result<u64, ControlError> {
    let tab = app.tab_id_at(tab_index(app, tab_id)?);
    if book(app)?.tab_state(tab).is_some() {
        return Ok(tab);
    }
    let mut error = ControlError::new(
        ErrorCode::new(codes::CAPABILITY_UNAVAILABLE).expect("static error code is valid"),
        format!("tab {tab} is still opening; retry once its charts stand"),
        true,
    );
    error.context.details = Some(json!({ "code": TAB_NOT_READY_CODE }));
    Err(error)
}

/// Run `change` on the book and write the result when it says it changed;
/// a write that fails restores the book.
fn written<H, T>(
    app: &mut H,
    change: impl FnOnce(&mut WorkspaceBook) -> Result<T, WorkspaceRefusal>,
    changed: impl FnOnce(&T) -> bool,
) -> Result<T, ControlError>
where
    H: WorkspaceBookHost + ?Sized,
{
    let held = book(app)?;
    let before = held.clone();
    let outcome = change(held).map_err(|refusal| refused(&refusal))?;
    if changed(&outcome)
        && let Err(reason) = app.persist_workspace_book()
    {
        *book(app)? = before;
        return Err(write_failed(reason));
    }
    Ok(outcome)
}

/// Redraw each tab on the arrangement the book now holds for it.
fn show<H: WorkspaceBookHost + ?Sized>(app: &mut H, tabs: &[u64]) -> Result<(), ControlError> {
    for tab in tabs {
        let copy = book(app)?.tab_copy(*tab).cloned();
        if let Some(copy) = copy {
            app.show_tab_recipe(*tab, &copy);
        }
    }
    Ok(())
}

fn workspace_of<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
) -> Result<String, ControlError> {
    book(app)?
        .tab_state(tab)
        .map(|state| state.workspace.to_owned())
        .ok_or_else(|| refused(&WorkspaceRefusal::UnknownTab(tab)))
}

/// Put a tab on a workspace and redraw it, journaling nothing: the call and
/// the window's rows reach it through one journaled step, so they cannot
/// disagree.
///
/// # Errors
/// No such tab or workspace, or no book.
pub fn set_tab_workspace<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
    name: &str,
) -> Result<SetOutcome, ControlError> {
    let outcome = book(app)?
        .set(tab, name)
        .map_err(|refusal| refused(&refusal))?;
    if outcome.changed {
        show(app, &[tab])?;
    }
    Ok(outcome)
}

/// Write a tab's edits into its workspace and redraw the tabs that follow.
///
/// # Errors
/// No such tab, no book, or the write failed and the book was restored.
pub fn save_tab_to<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
) -> Result<SaveOutcome, ControlError> {
    let outcome = written(app, |book| book.save_to(tab), |saved| saved.changed)?;
    if outcome.changed {
        show(app, &outcome.tabs_changed)?;
    }
    Ok(outcome)
}

/// Make a new workspace of a tab's copy.
///
/// # Errors
/// The name is refused, no such tab, no book, or the write failed.
pub fn save_tab_as<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
    name: &str,
) -> Result<SaveOutcome, ControlError> {
    written(app, |book| book.save_as(tab, name), |saved| saved.changed)
}

/// Drop a tab's edits and redraw it. Answers whether there were any.
///
/// # Errors
/// No such tab, or no book.
pub fn revert_tab<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
) -> Result<bool, ControlError> {
    let changed = book(app)?
        .revert(tab)
        .map_err(|refusal| refused(&refusal))?;
    if changed {
        show(app, &[tab])?;
    }
    Ok(changed)
}

/// Rename a workspace. Answers the name as stored and whether it moved.
///
/// # Errors
/// The name is refused, no book, or the write failed.
pub fn rename_workspace<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    name: &str,
    to: &str,
) -> Result<RenameOutcome, ControlError> {
    written(app, |book| book.rename(name, to), |moved| moved.changed)
}

/// Delete a workspace, moving its tabs to `Default` and redrawing the clean ones.
///
/// # Errors
/// `Default`, no book, or the write failed.
pub fn delete_workspace<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    name: &str,
) -> Result<DeleteOutcome, ControlError> {
    let outcome = written(app, |book| book.delete(name), |gone| gone.changed)?;
    if outcome.changed {
        show(app, &outcome.adopted)?;
    }
    Ok(outcome)
}

fn journal<A: NotifyAccess>(to: &mut Journal<'_, A>, kind: &'static str, payload: Value) {
    if let Some((access, actor)) = to {
        access.record_event(NewEvent::by(MODULE_ID, kind, actor, payload));
    }
}

// The change and its journal row, once each: the call and the window's rows
// both land here, so a hand on the menu journals what a script would.

fn set_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    name: &str,
) -> Result<SetOutcome, ControlError> {
    let outcome = set_tab_workspace(app, tab, name)?;
    if outcome.changed {
        let payload = json!({ "tab_id": WireU64::new(tab), "workspace": name,
            "dropped_edits": outcome.dropped_edits });
        journal(to, TAB_WORKSPACE_SET_EVENT_KIND, payload);
    }
    Ok(outcome)
}

fn save_to_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
) -> Result<SaveOutcome, ControlError> {
    let outcome = save_tab_to(app, tab)?;
    if outcome.changed {
        let payload =
            json!({ "workspace": outcome.workspace, "tabs_changed": wires(&outcome.tabs_changed) });
        journal(to, WORKSPACE_SAVED_EVENT_KIND, payload);
    }
    Ok(outcome)
}

fn save_as_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    name: &str,
) -> Result<SaveOutcome, ControlError> {
    let outcome = save_tab_as(app, tab, name)?;
    if outcome.changed {
        let payload = json!({ "workspace": outcome.workspace, "tabs_changed": [] });
        journal(to, WORKSPACE_SAVED_EVENT_KIND, payload);
    }
    Ok(outcome)
}

/// Whether there were edits to drop, and the workspace the tab is on.
fn revert_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
) -> Result<(bool, String), ControlError> {
    let changed = revert_tab(app, tab)?;
    let workspace = workspace_of(app, tab)?;
    if changed {
        let payload = json!({ "tab_id": WireU64::new(tab), "workspace": workspace });
        journal(to, WORKSPACE_REVERTED_EVENT_KIND, payload);
    }
    Ok((changed, workspace))
}

fn rename_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    name: &str,
    new_name: &str,
) -> Result<RenameOutcome, ControlError> {
    let outcome = rename_workspace(app, name, new_name)?;
    if outcome.changed {
        let payload = json!({ "from": name, "workspace": outcome.workspace });
        journal(to, WORKSPACE_RENAMED_EVENT_KIND, payload);
    }
    Ok(outcome)
}

fn delete_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    name: &str,
) -> Result<DeleteOutcome, ControlError> {
    let outcome = delete_workspace(app, name)?;
    if outcome.changed {
        let payload = json!({ "workspace": name, "moved": wires(&outcome.moved),
            "kept_edits": wires(&outcome.kept_edits) });
        journal(to, WORKSPACE_DELETED_EVENT_KIND, payload);
    }
    Ok(outcome)
}

fn set<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let TabWorkspaceSetInput { tab_id, name } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let outcome = set_journaled(app, &mut Some((access, actor)), tab, &name)?;
    answer(&TabWorkspaceSetResult {
        tab_id: WireU64::new(tab),
        workspace: name,
        changed: outcome.changed,
        dropped_edits: outcome.dropped_edits,
    })
}

fn save_to<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceTabInput { tab_id } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let outcome = save_to_journaled(app, &mut Some((access, actor)), tab)?;
    answer(&WorkspaceSaveResult {
        workspace: outcome.workspace,
        changed: outcome.changed,
        tabs_changed: wires(&outcome.tabs_changed),
    })
}

fn save_as<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceSaveAsInput { tab_id, name } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let outcome = save_as_journaled(app, &mut Some((access, actor)), tab, &name)?;
    answer(&WorkspaceSaveResult {
        workspace: outcome.workspace,
        changed: outcome.changed,
        tabs_changed: Vec::new(),
    })
}

fn revert<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceTabInput { tab_id } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let (changed, workspace) = revert_journaled(app, &mut Some((access, actor)), tab)?;
    answer(&WorkspaceRevertResult {
        tab_id: WireU64::new(tab),
        workspace,
        changed,
    })
}

fn rename<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceRenameInput { name, to } = input(value)?;
    let RenameOutcome { workspace, changed } =
        rename_journaled(app, &mut Some((access, actor)), &name, &to)?;
    answer(&WorkspaceRenameResult { workspace, changed })
}

fn delete<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceDeleteInput { name } = input(value)?;
    let outcome = delete_journaled(app, &mut Some((access, actor)), &name)?;
    answer(&WorkspaceDeleteResult {
        changed: outcome.changed,
        moved: wires(&outcome.moved),
        kept_edits: wires(&outcome.kept_edits),
    })
}

fn list<H: WorkspaceBookHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceListInput {} = input(value)?;
    let workspaces = book(app)?
        .list()
        .into_iter()
        .map(|row| WorkspaceEntry {
            tabs: wires(&row.tabs),
            name: row.name,
        })
        .collect();
    answer(&WorkspaceListResult { workspaces })
}

#[cfg(test)]
#[path = "workspace_book_tests.rs"]
mod workspace_book_tests;
