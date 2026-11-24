//! `workspace.revert` and `workspace.revert.undo`: a tab's unsaved layout
//! back to its workspace as saved, and back again. The book decides; the
//! window redraws the tab. The capsule's buttons land here too, so a hand
//! journals what a script would.

use quantick_control::error::ControlError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_schema::workspace_book::{
    WORKSPACE_REVERT_UNDONE_EVENT_KIND, WORKSPACE_REVERTED_EVENT_KIND, WorkspaceRevertResult,
    WorkspaceTabInput,
};
use quantick_workspace::workspace_book::RevertOutcome;
use serde_json::{Value, json};

use super::{Journal, WorkspaceOwner, answer, book, input, journal, refused, show, tab_on_book};
use crate::notify::NotifyAccess;

pub(super) fn revert_journaled<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
) -> Result<RevertOutcome, ControlError> {
    let outcome = book(app)?
        .revert(tab)
        .map_err(|refusal| refused(&refusal))?;
    landed(app, to, tab, &outcome, WORKSPACE_REVERTED_EVENT_KIND)?;
    Ok(outcome)
}

pub(super) fn undo_journaled<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
) -> Result<RevertOutcome, ControlError> {
    let outcome = (book(app)?.undo_revert(tab)).map_err(|refusal| refused(&refusal))?;
    landed(app, to, tab, &outcome, WORKSPACE_REVERT_UNDONE_EVENT_KIND)?;
    Ok(outcome)
}

/// Redraw and journal a change; a call that changed nothing does neither.
fn landed<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    outcome: &RevertOutcome,
    kind: &'static str,
) -> Result<(), ControlError> {
    if outcome.changed {
        show(app, &[tab])?;
        let payload = json!({ "tab_id": WireU64::new(tab), "workspace": outcome.workspace });
        journal(to, kind, payload);
    }
    Ok(())
}

pub(super) fn revert<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceTabInput { tab_id } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let outcome = revert_journaled(app, &mut Some((access, actor)), tab)?;
    answer(&result(tab, outcome))
}

pub(super) fn undo<H: WorkspaceOwner + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceTabInput { tab_id } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let outcome = undo_journaled(app, &mut Some((access, actor)), tab)?;
    answer(&result(tab, outcome))
}

fn result(tab: u64, outcome: RevertOutcome) -> WorkspaceRevertResult {
    WorkspaceRevertResult {
        tab_id: WireU64::new(tab),
        workspace: outcome.workspace,
        changed: outcome.changed,
    }
}
