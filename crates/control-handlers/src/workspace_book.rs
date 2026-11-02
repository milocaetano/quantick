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
use quantick_control_schema::workspace::MODULE_ID;
use quantick_control_schema::workspace_book::{
    TAB_WORKSPACE_SET_EVENT_KIND, TabWorkspaceSetInput, TabWorkspaceSetResult,
    WORKSPACE_DELETED_EVENT_KIND, WORKSPACE_RENAMED_EVENT_KIND, WORKSPACE_REVERTED_EVENT_KIND,
    WORKSPACE_SAVED_EVENT_KIND, WRITE_FAILED_CODE, WorkspaceDeleteInput, WorkspaceDeleteResult,
    WorkspaceEntry, WorkspaceListInput, WorkspaceListResult, WorkspaceRenameInput,
    WorkspaceRenameResult, WorkspaceRevertResult, WorkspaceSaveAsInput, WorkspaceSaveResult,
    WorkspaceTabInput, tab_workspace_set_descriptor, workspace_delete_descriptor,
    workspace_list_descriptor, workspace_rename_descriptor, workspace_revert_descriptor,
    workspace_save_as_descriptor, workspace_save_to_descriptor,
};
use quantick_stores::workspaces::{
    DEFAULT_WORKSPACE, DeleteOutcome, SaveOutcome, SetOutcome, WorkspaceBook, WorkspaceRecipe,
    WorkspaceRefusal,
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

/// The tab a call names, on the book: a tab the book has not met opens on
/// `Default`, as a new tab does.
fn tab_on_book<H: WorkspaceBookHost + TabDirectory + ?Sized>(
    app: &mut H,
    tab_id: Option<WireU64>,
) -> Result<u64, ControlError> {
    let tab = app.tab_id_at(tab_index(app, tab_id)?);
    book(app)?.open_tab(tab);
    Ok(tab)
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

/// Put a tab on a workspace and redraw it: the one path the call and the
/// window's rows share, so the two cannot disagree.
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

/// Rename a workspace. Answers whether the name moved.
///
/// # Errors
/// The name is refused, no book, or the write failed.
pub fn rename_workspace<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    name: &str,
    to: &str,
) -> Result<bool, ControlError> {
    written(app, |book| book.rename(name, to), |moved| *moved)
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

fn journal<A: NotifyAccess>(
    access: &mut A,
    kind: &'static str,
    actor: &ActorContext,
    payload: Value,
) {
    access.record_event(NewEvent::by(MODULE_ID, kind, actor, payload));
}

fn set<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let TabWorkspaceSetInput { tab_id, name } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let outcome = set_tab_workspace(app, tab, &name)?;
    if outcome.changed {
        let payload =
            json!({ "tab_id": tab, "workspace": name, "dropped_edits": outcome.dropped_edits });
        journal(access, TAB_WORKSPACE_SET_EVENT_KIND, actor, payload);
    }
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
    let outcome = save_tab_to(app, tab)?;
    if outcome.changed {
        let payload =
            json!({ "workspace": outcome.workspace, "tabs_changed": outcome.tabs_changed });
        journal(access, WORKSPACE_SAVED_EVENT_KIND, actor, payload);
    }
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
    let outcome = save_tab_as(app, tab, &name)?;
    if outcome.changed {
        let payload = json!({ "workspace": outcome.workspace, "tabs_changed": [] });
        journal(access, WORKSPACE_SAVED_EVENT_KIND, actor, payload);
    }
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
    let changed = revert_tab(app, tab)?;
    let workspace = workspace_of(app, tab)?;
    if changed {
        let payload = json!({ "tab_id": tab, "workspace": workspace });
        journal(access, WORKSPACE_REVERTED_EVENT_KIND, actor, payload);
    }
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
    let changed = rename_workspace(app, &name, &to)?;
    let workspace = to.split_whitespace().collect::<Vec<_>>().join(" ");
    if changed {
        let payload = json!({ "from": name, "workspace": workspace });
        journal(access, WORKSPACE_RENAMED_EVENT_KIND, actor, payload);
    }
    answer(&WorkspaceRenameResult { workspace, changed })
}

fn delete<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceDeleteInput { name } = input(value)?;
    let outcome = delete_workspace(app, &name)?;
    if outcome.changed {
        let payload =
            json!({ "workspace": name, "moved": outcome.moved, "kept_edits": outcome.kept_edits });
        journal(access, WORKSPACE_DELETED_EVENT_KIND, actor, payload);
    }
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
