//! The switch that asks first: a row, the window menu and `tab.workspace.set`
//! put a tab on another workspace only once its unsaved edits are decided.
//!
//! The book says whether a switch drops edits ([`WorkspaceBook::switch_drops`],
//! the rule the switch drops by); [`SwitchPrompt`] holds the window's
//! question; this module asks it and carries out the answer. A row asks; the
//! call refuses with `workspace.unsaved_edits` unless told to discard: it
//! never writes the book, which `workspace.save_to` does behind the desk's
//! consent.
//!
//! [`WorkspaceBook::switch_drops`]: quantick_stores::workspaces::WorkspaceBook::switch_drops

use quantick_control::error::ControlError;
use quantick_control::wire::ActorContext;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::workspace_book::{SwitchEdits, UNSAVED_EDITS_CODE};
use quantick_session::close_queue::CloseAnswer;
use quantick_session::prompt::Prompt;
use quantick_session::switch_prompt::{SwitchAnswer, SwitchPrompt, SwitchQuestion};
use quantick_session::workspace_menu::{WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::workspaces::SetOutcome;
use serde_json::json;

use super::{
    Journal, WorkspaceBookHost, book, close_prompt, flush, refused, save_to_journaled,
    set_journaled, text,
};
use crate::notify::NotifyAccess;

/// What a switch of `tab` onto `name` would drop, as its question names it:
/// `None` when it drops nothing, so it goes ahead unasked. A pending autosave
/// write goes through first, so the edit it carries is never asked about.
///
/// # Errors
/// No such tab or workspace, no book, or the autosave write failed.
pub fn ask<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    name: &str,
) -> Result<Option<SwitchQuestion>, ControlError> {
    flush(app, to, tab)?;
    let book = book(app)?;
    if !book
        .switch_drops(tab, name)
        .map_err(|refusal| refused(&refusal))?
    {
        return Ok(None);
    }
    // What the switch drops: drawings are the market's, and stay.
    let (workspace, changes) = (book.tab_workspace(tab).to_owned(), book.layout_changes(tab));
    let followers = book.save_to_preview(tab).unwrap_or_default();
    Ok(Some(SwitchQuestion {
        tab,
        symbol: app.tab_symbols(&[tab]).pop().unwrap_or_default(),
        followers: app.tab_symbols(&followers),
        workspace,
        target: name.to_owned(),
        changes,
    }))
}

/// [`ask`] journaling nothing: the market palette's hand journals nothing,
/// as the window's other source changes do.
///
/// # Errors
/// As [`ask`].
pub(crate) fn ask_unjournaled<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
    name: &str,
) -> Result<Option<SwitchQuestion>, ControlError> {
    ask::<H, Unjournaled>(app, &mut None, tab, name)
}

/// The access a write with no journal names: there is none.
enum Unjournaled {}

impl NotifyAccess for Unjournaled {
    fn allow_notification(&mut self, _: &ActorContext) -> Result<(), std::time::Duration> {
        match *self {}
    }
    fn record_event(&mut self, _: NewEvent) {
        match *self {}
    }
}

/// Put `tab` on `name`, writing its edits into its workspace first when
/// `save`.
///
/// # Errors
/// The save or the switch was refused, or a write failed; a failed save
/// leaves the tab where it was.
fn carry_out<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    name: &str,
    save: bool,
) -> Result<SetOutcome, ControlError> {
    if save {
        save_to_journaled(app, to, tab)?;
    }
    set_journaled(app, to, tab, name)
}

/// A row's switch: asked when it would drop edits, else carried out. Answers
/// the toast.
pub(super) fn row<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    prompt: &mut SwitchPrompt,
    (tab, symbol): (u64, &str),
    name: &str,
) -> Result<Option<String>, ControlError> {
    if let Some(question) = ask(app, to, tab, name)? {
        prompt.ask(question);
        return Ok(None);
    }
    let set = carry_out(app, to, tab, name, false)?;
    Ok(set
        .changed
        .then(|| text::switched_toast(symbol, name, set.dropped_edits)))
}

/// The prompt's answer: Cancel changes nothing; Save writes the edits into
/// the tab's workspace, then switches; Switch without saving drops them. A
/// question gone stale (its tab closed or moved off the workspace it named,
/// or its target deleted or renamed) carries out nothing and says so: a Save
/// never writes a workspace it did not name, nor one a switch cannot follow.
pub(super) fn answer<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    prompt: &mut SwitchPrompt,
    answer: SwitchAnswer,
) -> Result<Option<String>, ControlError> {
    let Some(step) = prompt.answer(answer) else {
        return Ok(None);
    };
    let book = book(app)?;
    let on = book.tab_state(step.tab).map(|state| state.workspace);
    if on != Some(step.workspace.as_str()) || book.recipe(&step.target).is_none() {
        return Ok(Some(text::SWITCH_QUESTION_STALE.to_owned()));
    }
    let set = carry_out(app, to, step.tab, &step.target, step.save)?;
    let symbol = app.tab_symbols(&[step.tab]).pop().unwrap_or_default();
    Ok(set
        .changed
        .then(|| text::switched_toast(&symbol, &step.target, set.dropped_edits)))
}

/// `tab.workspace.set`'s switch: one that would drop edits goes ahead only
/// when `edits` says discard. The call never saves: that is
/// `workspace.save_to`'s, behind the desk's consent.
///
/// # Errors
/// Edits the switch would drop and no `edits`: `workspace.unsaved_edits`,
/// nothing changed. Otherwise as [`ask`] and [`carry_out`].
pub fn by_call<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    name: &str,
    edits: Option<SwitchEdits>,
) -> Result<SetOutcome, ControlError> {
    if let (Some(asked), None) = (ask(app, to, tab, name)?, edits) {
        return Err(unsaved(&asked));
    }
    carry_out(app, to, tab, name, false)
}

fn unsaved(asked: &SwitchQuestion) -> ControlError {
    let SwitchQuestion {
        workspace,
        target,
        changes,
        followers,
        ..
    } = asked;
    let mut error = ControlError::invalid_request(format!(
        "{} Call workspace.save_to first to keep them, or pass edits discard to switch to {target} without saving.",
        text::close_prompt_body(workspace, changes, followers),
    ));
    error.context.details = Some(json!({ "code": UNSAVED_EDITS_CODE, "workspace": workspace,
        "target": target, "followers": followers, "changes": changes }));
    error
}

/// The window's words for the switch up: `Switch WINZ26 to Swing`, what
/// changed and who follows a save, then Cancel / Switch without saving /
/// Save to Scalp, with Enter on the save and Esc on Cancel.
#[must_use]
pub fn prompt(prompt: &SwitchPrompt) -> Option<Prompt> {
    let asked = prompt.question()?;
    let label = |answer| match answer {
        SwitchAnswer::Cancel => text::CLOSE_CANCEL.to_owned(),
        SwitchAnswer::SwitchWithoutSaving => text::SWITCH_WITHOUT_SAVING.to_owned(),
        SwitchAnswer::Save => text::save_to_short_label(&asked.workspace),
    };
    Some(Prompt {
        id: format!("workspace_switch_prompt_{}", asked.tab),
        title: text::switch_prompt_title(&asked.symbol, &asked.target),
        body: text::close_prompt_body(&asked.workspace, &asked.changes, &asked.followers),
        note: None,
        buttons: SwitchAnswer::BUTTONS.map(label).to_vec(),
        enter: SwitchAnswer::BUTTONS.len() - 1,
        escape: 0,
    })
}

/// The question up in the workspace menu, in the window's words: a close's,
/// else a switch's.
#[must_use]
pub fn menu_prompt(menu: &WorkspaceMenuState) -> Option<Prompt> {
    close_prompt(&menu.close).or_else(|| prompt(&menu.switch))
}

/// What the button at `at` of [`menu_prompt`]'s question answers.
#[must_use]
pub fn prompt_answer(menu: &WorkspaceMenuState, at: usize) -> Option<WorkspaceCommand> {
    if menu.close.question().is_some() {
        return CloseAnswer::BUTTONS
            .get(at)
            .copied()
            .map(WorkspaceCommand::AnswerClose);
    }
    let switch = menu.switch.question().and(SwitchAnswer::BUTTONS.get(at));
    switch.copied().map(WorkspaceCommand::AnswerSwitch)
}
