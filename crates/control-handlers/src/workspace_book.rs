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
    LAST_TAB_CODE, TAB_CLOSED_EVENT_KIND, TAB_NOT_READY_CODE, TAB_WORKSPACE_SET_EVENT_KIND,
    TabCloseInput, TabCloseResult, TabWorkspaceSetInput, TabWorkspaceSetResult, UNSAVED_EDITS_CODE,
    WORKSPACE_AUTOSAVE_SET_EVENT_KIND, WORKSPACE_CREATED_EVENT_KIND, WORKSPACE_DELETED_EVENT_KIND,
    WORKSPACE_RENAMED_EVENT_KIND, WORKSPACE_SAVED_EVENT_KIND, WRITE_FAILED_CODE,
    WorkspaceAutosaveInput, WorkspaceAutosaveResult, WorkspaceDeleteInput, WorkspaceDeleteResult,
    WorkspaceEntry, WorkspaceListInput, WorkspaceListResult, WorkspaceNewInput, WorkspaceNewResult,
    WorkspaceRenameInput, WorkspaceRenameResult, WorkspaceSaveAsInput, WorkspaceSaveResult,
    WorkspaceTabInput, tab_close_descriptor, tab_workspace_set_descriptor,
    workspace_autosave_set_descriptor, workspace_delete_descriptor, workspace_list_descriptor,
    workspace_new_descriptor, workspace_rename_descriptor, workspace_revert_descriptor,
    workspace_save_as_descriptor, workspace_save_to_descriptor,
};
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::market_settings::{self, DrawingKey, MarketSettingsStore};
use quantick_stores::workspaces::{
    CreateOutcome, DEFAULT_WORKSPACE, DeleteOutcome, RenameOutcome, SaveOutcome, SetOutcome,
    UnsavedEdits, WorkspaceBook, WorkspaceBookStore, WorkspaceRecipe, WorkspaceRefusal,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::dock::ActionDock;
use crate::notify::NotifyAccess;
use crate::tabs::TabDirectory;

fn unavailable() -> ControlError {
    ControlError::invalid_request("the workspaces are unavailable")
}

/// What the window lends these calls. The defaults answer for a host without a
/// book.
pub trait WorkspaceBookHost {
    /// The book and its file, which the three defaults below reach.
    fn workspace_store(&mut self) -> Option<&mut WorkspaceBookStore> {
        None
    }
    fn workspace_store_ref(&self) -> Option<&WorkspaceBookStore> {
        None
    }
    /// The book the open tabs live on.
    fn workspace_book(&mut self) -> Option<&mut WorkspaceBook> {
        self.workspace_store().map(WorkspaceBookStore::book_mut)
    }
    /// Write the recipes; `Err` is why nothing reached the disk.
    fn persist_workspace_book(&mut self) -> Result<(), String> {
        (self.workspace_store_ref()).map_or_else(
            || Err("the workspaces are unavailable".to_owned()),
            WorkspaceBookStore::persist,
        )
    }
    /// Put a recipe on a tab's charts.
    fn show_tab_recipe(&mut self, _tab: u64, _recipe: &WorkspaceRecipe) {}
    /// The book, for reading.
    fn workspace_book_ref(&self) -> Option<&WorkspaceBook> {
        self.workspace_store_ref().map(WorkspaceBookStore::book)
    }
    /// The drawings and anchored studies a Save writes with the recipe.
    fn workspace_market(&mut self) -> Option<&mut MarketSettingsStore> {
        None
    }
    /// Write what [`Self::workspace_market`] holds back now; answers whether
    /// it reached the disk.
    fn commit_workspace_market(&mut self) -> bool {
        self.workspace_market().is_some_and(market_settings::commit)
    }
    /// The tab's workspace and whether it holds edits, as the snapshot reads it.
    fn tab_workspace_row(&self, tab: u64) -> (String, bool) {
        let state = self
            .workspace_book_ref()
            .and_then(|book| book.tab_state(tab));
        state.map_or_else(
            || (DEFAULT_WORKSPACE.to_owned(), false),
            |state| (state.workspace.to_owned(), state.modified),
        )
    }
    /// Whether layout edits are written through, as the snapshot reads it.
    fn workspace_autosave(&self) -> bool {
        self.workspace_book_ref()
            .is_none_or(WorkspaceBook::autosave)
    }
    /// The symbols of these tabs, in the order asked.
    fn tab_symbols(&self, _tabs: &[u64]) -> Vec<String> {
        Vec::new()
    }
    /// Write the desk's `auto_save`; `Err` is why nothing reached the disk.
    fn persist_workspace_autosave(&mut self, _on: bool) -> Result<(), String> {
        Err("the desk is unavailable".to_owned())
    }
    /// Close a market tab; `false` when it is its window's last.
    fn close_market_tab(&mut self, _tab: u64) -> bool {
        false
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
            menu.open_save_as(tab);
            return None;
        }
        WorkspaceCommand::BeginNew => {
            menu.open_new(tab);
            return None;
        }
        WorkspaceCommand::BeginRename(from) => {
            menu.open_rename(tab, &from);
            return None;
        }
        WorkspaceCommand::SubmitDialog => {
            return submit_name_dialog(host, &mut to, menu, symbol);
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
        WorkspaceCommand::SetAutosave(on) => {
            autosave_journaled(host, &mut to, on).map(|(_, saved)| saved_notes(host, &saved))
        }
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
    symbol: &str,
) -> Option<String> {
    let dialog = menu.dialog.clone()?;
    let tab = dialog.tab;
    let outcome = match &dialog.mode {
        NameDialogMode::SaveAsNew => save_as_journaled(host, to, tab, &dialog.name)
            .map(|saved| Some(text::saved_as_toast(symbol, &saved.workspace))),
        NameDialogMode::New => new_journaled(host, to, tab, &dialog.name)
            .map(|made| Some(text::created_toast(symbol, &made.workspace))),
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
    registry.register_outside_replay(workspace_new_descriptor(), new)?;
    registry.register_outside_replay(workspace_revert_descriptor(), revert)?;
    registry.register_outside_replay(workspace_autosave_set_descriptor(), autosave_set)?;
    registry.register_outside_replay(tab_close_descriptor(), close)?;
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

/// The reason, and the code a caller branches on. A tab still opening is
/// worth a retry, as [`tab_on_book`] answers it.
fn refused(refusal: &WorkspaceRefusal) -> ControlError {
    if let WorkspaceRefusal::TabOpening(tab) = refusal {
        return not_ready(*tab);
    }
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

/// The tab a call names, on the book. An id no open tab has is refused with
/// `workspace.unknown_tab`. A tab the book has not adopted yet is
/// refused before anything changes: its panes are still being built, and a
/// workspace put on it now would be undone when the desk's word lands.
fn tab_on_book<H: WorkspaceBookHost + TabDirectory + ?Sized>(
    app: &mut H,
    tab_id: Option<WireU64>,
) -> Result<u64, ControlError> {
    let index = match tab_id {
        None => app.active_tab_index(),
        Some(id) => app
            .tab_position(id.get())
            .ok_or_else(|| refused(&WorkspaceRefusal::UnknownTab(id.get())))?,
    };
    let tab = app.tab_id_at(index);
    if book(app)?.tab_state(tab).is_some() {
        return Ok(tab);
    }
    Err(not_ready(tab))
}

fn not_ready(tab: u64) -> ControlError {
    let mut error = ControlError::new(
        ErrorCode::new(codes::CAPABILITY_UNAVAILABLE).expect("static error code is valid"),
        WorkspaceRefusal::TabOpening(tab).to_string(),
        true,
    );
    error.context.details = Some(json!({ "code": TAB_NOT_READY_CODE }));
    error
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
    let mut outcome = written(app, |book| book.save_to(tab), |saved| saved.changed)?;
    if outcome.changed {
        show(app, &outcome.tabs_changed)?;
    }
    outcome.changed |= commit_market(app);
    Ok(outcome)
}

/// Save's other half: the drawings and anchored studies autosave off held back
/// reach their file right after the recipe. Answers whether they were written.
fn commit_market<H: WorkspaceBookHost + ?Sized>(app: &mut H) -> bool {
    let wrote = app.commit_workspace_market();
    if wrote && let Some(book) = app.workspace_book() {
        book.clear_held_drawings();
    }
    wrote
}

/// Close without saving: the closed tab's held drawings go back to the file's
/// version, unless another open tab still shows that set.
fn discard_market<H: WorkspaceBookHost + ?Sized>(app: &mut H, closed: &[DrawingKey], tab: u64) {
    let open = book(app)
        .map(|book| book.drawing_keys_except(tab))
        .unwrap_or_default();
    if let Some(market) = app.workspace_market() {
        market.discard(closed, &open);
    }
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
    let mut outcome = written(app, |book| book.save_as(tab, name), |saved| saved.changed)?;
    outcome.changed |= commit_market(app);
    Ok(outcome)
}

/// Make a new, empty workspace, put the tab on it and redraw the tab.
///
/// # Errors
/// The name is refused, no such tab, no book, or the write failed.
pub fn new_tab_workspace<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
    name: &str,
) -> Result<CreateOutcome, ControlError> {
    let outcome = written(app, |book| book.create(tab, name), |_| true)?;
    show(app, &[tab])?;
    Ok(outcome)
}

/// With autosave on, write a tab's pending edit through before anything moves
/// it: a switch or a close never drops what autosave was about to save.
fn flush<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
) -> Result<(), ControlError> {
    let held = book(app)?;
    if held.autosave() && held.differs(tab) {
        save_to_journaled(app, to, tab)?;
    }
    Ok(())
}

/// Autosave's write-through for the tabs the frame found quiet, journaled as
/// the hand that edited. Answers the toasts: one only where other tabs follow,
/// or why a write failed.
pub fn write_through<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    mut to: Journal<'_, A>,
    tabs: &[u64],
) -> Vec<String> {
    let mut notes = Vec::new();
    for tab in tabs {
        match save_to_journaled(app, &mut to, *tab) {
            Ok(saved) => notes.extend(saved_notes(app, &[saved])),
            Err(error) => notes.push(error.message),
        }
    }
    notes
}

fn saved_notes<H: WorkspaceBookHost + ?Sized>(app: &H, saved: &[SaveOutcome]) -> Option<String> {
    let notes: Vec<String> = saved
        .iter()
        .filter_map(|one| {
            text::autosaved_toast(&one.workspace, &app.tab_symbols(&one.tabs_changed))
        })
        .collect();
    (!notes.is_empty()).then(|| notes.join("; "))
}

/// Turn autosave on or off and write the desk; turning it on saves every tab
/// holding edits at once. A desk write that fails takes the choice back.
///
/// # Errors
/// No book, the desk write failed, or a save failed.
pub fn set_autosave<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    on: bool,
) -> Result<(bool, Vec<SaveOutcome>), ControlError> {
    if !book(app)?.set_autosave(on) {
        return Ok((false, Vec::new()));
    }
    if let Err(reason) = app.persist_workspace_autosave(on) {
        book(app)?.set_autosave(!on);
        return Err(write_failed(reason));
    }
    if let Some(market) = app.workspace_market() {
        market.set_held(!on);
    }
    let mut saved = Vec::new();
    if on {
        for tab in book(app)?.differing_tabs() {
            saved.push(save_tab_to(app, tab)?);
        }
        commit_market(app);
    }
    Ok((true, saved))
}

/// What closing a tab does with its held edits: `None` asks, and refuses with
/// [`UNSAVED_EDITS_CODE`] while it holds any; autosave on writes them first.
///
/// # Errors
/// Edits held and no decision, no book, a failed save, or the last tab.
pub fn close_tab<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    save: Option<bool>,
) -> Result<Option<SaveOutcome>, ControlError> {
    flush(app, to, tab)?;
    let held = book(app)?.unsaved_edits(tab);
    let saved = match (held, save) {
        (Some(edits), None) => return Err(unsaved(app, &edits)),
        (Some(_), Some(true)) => Some(save_to_journaled(app, to, tab)?),
        _ => None,
    };
    let (drop_held, keys) = (
        !book(app)?.autosave() && saved.is_none(),
        book(app)?.drawing_keys(tab).to_vec(),
    );
    if !app.close_market_tab(tab) {
        let mut error = ControlError::invalid_request("the window's last tab stays open");
        error.context.details = Some(json!({ "code": LAST_TAB_CODE }));
        return Err(error);
    }
    if drop_held {
        discard_market(app, &keys, tab);
    }
    journal(
        to,
        TAB_CLOSED_EVENT_KIND,
        json!({ "tab_id": WireU64::new(tab) }),
    );
    Ok(saved)
}

fn unsaved<H: WorkspaceBookHost + ?Sized>(app: &H, edits: &UnsavedEdits) -> ControlError {
    let followers = app.tab_symbols(&edits.followers);
    let mut error = ControlError::invalid_request(format!(
        "{} Pass save true to save to {}, or save false to close without saving.",
        text::close_prompt_body(&edits.workspace, &edits.changes, &followers),
        edits.workspace
    ));
    error.context.details = Some(json!({ "code": UNSAVED_EDITS_CODE,
        "workspace": edits.workspace, "followers": followers, "changes": edits.changes }));
    error
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
/// `Default`, no workspace has the name, no book, or the write failed.
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
    flush(app, to, tab)?;
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

fn new_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    name: &str,
) -> Result<CreateOutcome, ControlError> {
    flush(app, to, tab)?;
    let outcome = new_tab_workspace(app, tab, name)?;
    let payload = json!({ "tab_id": WireU64::new(tab), "workspace": outcome.workspace,
        "dropped_edits": outcome.dropped_edits });
    journal(to, WORKSPACE_CREATED_EVENT_KIND, payload);
    Ok(outcome)
}

fn autosave_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    on: bool,
) -> Result<(bool, Vec<SaveOutcome>), ControlError> {
    let (changed, saved) = set_autosave(app, on)?;
    if changed {
        journal(
            to,
            WORKSPACE_AUTOSAVE_SET_EVENT_KIND,
            json!({ "auto_save": on }),
        );
    }
    for one in saved.iter().filter(|one| one.changed) {
        let payload =
            json!({ "workspace": one.workspace, "tabs_changed": wires(&one.tabs_changed) });
        journal(to, WORKSPACE_SAVED_EVENT_KIND, payload);
    }
    Ok((changed, saved))
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

fn new<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceNewInput { tab_id, name } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let outcome = new_journaled(app, &mut Some((access, actor)), tab, &name)?;
    answer(&WorkspaceNewResult {
        tab_id: WireU64::new(tab),
        workspace: outcome.workspace,
        dropped_edits: outcome.dropped_edits,
    })
}

/// Revert is gone; the refusal names the calls that keep edits instead.
fn revert<H: WorkspaceBookHost + ?Sized, A>(
    _: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceTabInput { .. } = input(value)?;
    Err(refused(&WorkspaceRefusal::RevertRemoved))
}

fn save_result(saved: SaveOutcome) -> WorkspaceSaveResult {
    WorkspaceSaveResult {
        workspace: saved.workspace,
        changed: saved.changed,
        tabs_changed: wires(&saved.tabs_changed),
    }
}

fn autosave_set<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceAutosaveInput { on } = input(value)?;
    let (changed, saved) = autosave_journaled(app, &mut Some((access, actor)), on)?;
    answer(&WorkspaceAutosaveResult {
        auto_save: on,
        changed,
        saved: saved.into_iter().map(save_result).collect(),
    })
}

fn close<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let TabCloseInput { tab_id, save } = input(value)?;
    let tab = tab_on_book(app, tab_id)?;
    let saved = close_tab(app, &mut Some((access, actor)), tab, save)?;
    answer(&TabCloseResult {
        tab_id: WireU64::new(tab),
        saved: saved.map(save_result),
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
    let book_ref = book(app)?;
    let workspaces = book_ref
        .list()
        .into_iter()
        .map(|row| WorkspaceEntry {
            tabs: wires(&row.tabs),
            summary: book_ref.summary(&row.name),
            name: row.name,
        })
        .collect();
    answer(&WorkspaceListResult { workspaces })
}

#[cfg(test)]
#[path = "workspace_book_tests.rs"]
mod workspace_book_tests;
