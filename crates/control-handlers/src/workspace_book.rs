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
use quantick_control_schema::workspace::{MODULE_ID, WorkspaceColourDto, WorkspaceColourRow};
use quantick_control_schema::workspace_book::{
    LAST_TAB_CODE, TAB_CLOSED_EVENT_KIND, TAB_NOT_READY_CODE, TAB_WORKSPACE_SET_EVENT_KIND,
    TabCloseInput, TabCloseResult, TabWorkspaceSetInput, TabWorkspaceSetResult, UNSAVED_EDITS_CODE,
    WORKSPACE_AUTOSAVE_SET_EVENT_KIND, WORKSPACE_COLOUR_SET_EVENT_KIND,
    WORKSPACE_CREATED_EVENT_KIND, WORKSPACE_DELETED_EVENT_KIND, WORKSPACE_RENAMED_EVENT_KIND,
    WORKSPACE_SAVED_EVENT_KIND, WRITE_FAILED_CODE, WorkspaceAutosaveInput, WorkspaceAutosaveResult,
    WorkspaceColourSetInput, WorkspaceColourSetResult, WorkspaceDeleteInput, WorkspaceDeleteResult,
    WorkspaceEntry, WorkspaceListInput, WorkspaceListResult, WorkspaceNewInput, WorkspaceNewResult,
    WorkspaceRenameInput, WorkspaceRenameResult, WorkspaceSaveAsInput, WorkspaceSaveResult,
    WorkspaceTabInput, tab_close_descriptor, tab_workspace_set_descriptor,
    workspace_autosave_set_descriptor, workspace_colour_set_descriptor,
    workspace_delete_descriptor, workspace_list_descriptor, workspace_new_descriptor,
    workspace_rename_descriptor, workspace_revert_descriptor, workspace_save_as_descriptor,
    workspace_save_to_descriptor,
};
use quantick_session::close_queue::{
    CloseAnswer, CloseQuestion, CloseQueue, CloseScope, CloseStep,
};
use quantick_session::prompt::Prompt;
use quantick_session::workspace_menu::{NameDialogMode, WorkspaceCommand, WorkspaceMenuState};
use quantick_stores::market_settings::{self, DrawingKey, MarketSettingsStore};
use quantick_stores::workspaces::{
    ColourOutcome, CreateOutcome, DEFAULT_WORKSPACE, DeleteOutcome, RenameOutcome, SaveOutcome,
    SetOutcome, UnsavedEdits, Unsynced, WorkspaceBook, WorkspaceBookStore, WorkspaceColour,
    WorkspaceRecipe, WorkspaceRefusal,
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
    /// Write what [`Self::workspace_market`] holds back for `keys` now, every
    /// other set left as the file has it; `None` writes every set. `Ok(true)`
    /// when the file was written, `Ok(false)` when it already held them.
    ///
    /// # Errors
    /// Why nothing reached the disk; the held sets stay counted.
    fn commit_workspace_market(&mut self, keys: Option<&[DrawingKey]>) -> Result<bool, String> {
        let Some(market) = self.workspace_market() else {
            return Ok(false);
        };
        match keys {
            Some(keys) => market_settings::commit(market, keys),
            None => market_settings::commit_all(market),
        }
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
    /// The colour of the tab's workspace, `Default`'s for a tab the book has
    /// not adopted yet: [`WorkspaceBook::tab_colour`], which the strip and the
    /// capsule paint too.
    fn tab_workspace_colour(&self, tab: u64) -> Option<WorkspaceColourDto> {
        self.workspace_book_ref()?.tab_colour(tab).map(Into::into)
    }
    /// Every workspace's colour, `Default` first, as the snapshot reads them.
    fn workspace_colours(&self) -> Vec<WorkspaceColourRow> {
        let rows = self.workspace_book_ref().map(WorkspaceBook::list);
        let row = |row: quantick_stores::workspaces::WorkspaceListing| WorkspaceColourRow {
            colour: row.colour.map(Into::into),
            name: row.name,
        };
        rows.unwrap_or_default().into_iter().map(row).collect()
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
    /// Close a market tab; `false` when it is its window's last. With `close`
    /// false nothing closes and the answer says whether it would: asked
    /// before anything is saved or flushed, so the refusal writes nothing.
    fn close_market_tab(&mut self, _tab: u64, _close: bool) -> bool {
        false
    }
}

/// Who journals a change: the call's access and actor, or the trader's hand
/// at the window. `None` where the window has no control identity; the change
/// still lands, and nothing is journaled.
pub type Journal<'a, A> = Option<(&'a mut A, &'a ActorContext)>;

/// A workspace row's command, its colour the palette's own slot.
pub type RowCommand = WorkspaceCommand<WorkspaceColour>;

/// Carry out what a workspace row asked for, through the same steps the
/// control call takes, so a row leaves the call's journal row. Answers what
/// to tell the trader, naming the tabs.
pub fn apply_row_command<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    host: &mut H,
    mut to: Journal<'_, A>,
    menu: &mut WorkspaceMenuState,
    tab: u64,
    symbol: &str,
    command: RowCommand,
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
        WorkspaceCommand::BeginManage => {
            menu.open_manage(tab);
            return None;
        }
        WorkspaceCommand::SubmitDialog => {
            return submit_name_dialog(host, &mut to, menu, symbol);
        }
        WorkspaceCommand::Close(scope, tabs) => {
            if menu.close.request(scope, tabs)
                && let Some(book) = host.workspace_book()
            {
                book.outvote_rivals();
            }
            return run_close(host, &mut to, &mut menu.close);
        }
        WorkspaceCommand::AnswerClose(answer) => {
            menu.close.answer(answer);
            return run_close(host, &mut to, &mut menu.close);
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
            autosave_journaled(host, &mut to, on, tab).map(|(_, saved)| saved_notes(host, &saved))
        }
        WorkspaceCommand::SetColour(name, colour) => {
            colour_journaled(host, &mut to, &name, colour).map(|_| None)
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
    registry.register_outside_replay(workspace_colour_set_descriptor(), colour_set)?;
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

fn last_tab() -> ControlError {
    let mut error = ControlError::invalid_request("the window's last tab stays open");
    error.context.details = Some(json!({ "code": LAST_TAB_CODE }));
    error
}

fn code_of(error: &ControlError) -> Option<&Value> {
    error.context.details.as_ref()?.get("code")
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
/// The recipe is written first, then the tab's held drawings: a recipe write
/// that fails writes no drawing, and a market write that fails puts the
/// recipe back, so a Save lands whole or not at all and the mark stays. A
/// recipe write that fails with autosave on marks the tab, which the frame
/// retries.
///
/// # Errors
/// No such tab, no book, or a write failed and the book was restored.
pub fn save_tab_to<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
) -> Result<SaveOutcome, ControlError> {
    let before = book(app)?.clone();
    let saved = written(app, |book| book.save_to(tab), |saved| saved.changed);
    if saved.as_ref().is_err_and(failed_write) {
        note_failed(app, tab);
    }
    let mut outcome = saved?;
    let wrote = commit_market_whole(app, tab, before, outcome.changed)?;
    if outcome.changed {
        show(app, &outcome.tabs_changed)?;
    }
    outcome.changed |= wrote;
    Ok(outcome)
}

/// A Save's second half, once its recipe landed: the tab's held drawings. A
/// market write that fails puts the book back as it stood `before` the save,
/// in memory and, when the recipe was written, on disk.
///
/// # Errors
/// The market file was not written.
fn commit_market_whole<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
    before: WorkspaceBook,
    recipe_written: bool,
) -> Result<bool, ControlError> {
    let mut error = match commit_market(app, tab) {
        Ok(wrote) => return Ok(wrote),
        Err(error) => error,
    };
    if recipe_written {
        *book(app)? = before;
        if let Err(reason) = app.persist_workspace_book() {
            error.message = format!(
                "{}; the workspaces file could not be put back either: {reason}",
                error.message
            );
        }
    }
    Err(error)
}

/// A save of `tab` did not reach the disk: with autosave on it reads modified
/// until one does.
fn note_failed<H: WorkspaceBookHost + ?Sized>(app: &mut H, tab: u64) {
    if let Some(book) = app.workspace_book().filter(|book| book.autosave()) {
        book.note_write_failed(tab);
    }
}

/// Save's other half: the drawings and anchored studies autosave off held
/// back on `tab`'s panes reach their file, and only those: another tab's held
/// set stays its own to save or drop. With autosave on nothing is held, and
/// the market's own debounce writes. Answers whether the file was written.
///
/// # Errors
/// The market file was not written; every held set stays counted.
fn commit_market<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
) -> Result<bool, ControlError> {
    if !app
        .workspace_market()
        .is_some_and(|market| market.is_held())
    {
        return Ok(false);
    }
    let keys = book(app)?.drawing_keys(tab).to_vec();
    let wrote = app
        .commit_workspace_market(Some(&keys))
        .map_err(write_failed)?;
    recount_held(app);
    Ok(wrote)
}

/// The book counts again what each tab's sets hold back, as the store reads.
fn recount_held<H: WorkspaceBookHost + ?Sized>(app: &mut H) {
    let sets = app.workspace_book().map(|book| book.drawing_sets());
    let (Some(sets), Some(market)) = (sets, app.workspace_market()) else {
        return;
    };
    let counted: Vec<_> = (sets.into_iter())
        .map(|(tab, keys)| (tab, market.held_changes(&keys), keys))
        .collect();
    if let Some(book) = app.workspace_book() {
        for (tab, held, keys) in counted {
            book.note_drawings(tab, keys, held);
        }
    }
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

/// Make a new workspace of a tab's copy, then write its held drawings, whole
/// or not at all as [`save_tab_to`] does.
///
/// # Errors
/// The name is refused, no such tab, no book, or a write failed.
pub fn save_tab_as<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
    name: &str,
) -> Result<SaveOutcome, ControlError> {
    // Asked of a copy first: a refused name writes no drawing either.
    let before = book(app)?.clone();
    before
        .clone()
        .save_as(tab, name)
        .map_err(|refusal| refused(&refusal))?;
    let mut outcome = written(app, |book| book.save_as(tab, name), |saved| saved.changed)?;
    outcome.changed |= commit_market_whole(app, tab, before, outcome.changed)?;
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
/// it: a switch or a close never drops what autosave was about to save. Only
/// the copy autosave would write ([`WorkspaceBook::flushes`]): an outvoted or
/// failed copy is held edits, so a close asks and a switch drops it.
fn flush<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
) -> Result<(), ControlError> {
    if book(app)?.flushes(tab) {
        save_to_journaled(app, to, tab)?;
    }
    Ok(())
}

/// Autosave's write-through for the tabs the frame found quiet (one per
/// workspace, as `quantick_workspace::book_upkeep::maintain` chose them),
/// journaled as the hand that edited. Answers the toasts: one only where
/// other tabs follow, or why a write failed, once: a failed tab reads
/// modified and the frame retries it each quiet second.
pub fn write_through<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    mut to: Journal<'_, A>,
    tabs: &[u64],
) -> Vec<String> {
    let mut notes = Vec::new();
    for tab in tabs {
        let known = app.workspace_book().and_then(|book| book.unsynced(*tab));
        match save_to_journaled(app, &mut to, *tab) {
            Ok(saved) => notes.extend(saved_notes(app, &[saved])),
            Err(error) if known == Some(Unsynced::Failed) && failed_write(&error) => {}
            Err(error) => notes.push(error.message),
        }
    }
    notes
}

fn failed_write(error: &ControlError) -> bool {
    code_of(error) == Some(&json!(WRITE_FAILED_CODE))
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

/// What turning autosave on or off did, the saves included, even when one
/// failed: the journal and the answer name what landed.
#[derive(Debug)]
pub struct AutosaveOutcome {
    /// Whether the setting moved.
    pub changed: bool,
    /// The saves that reached the disk.
    pub saved: Vec<SaveOutcome>,
    /// The first save that did not; its tab reads modified and is retried.
    pub failed: Option<ControlError>,
}

/// Turn autosave on or off and write the desk. A desk write that fails takes
/// the choice back and changes nothing. Turning it on (or asking for on
/// again) saves every tab holding edits at once, one per workspace
/// ([`WorkspaceBook::autosave_winners`], `active` first): a tab outvoted
/// stays marked, and a save that fails marks its tab, which the frame
/// retries; the setting stays on and the saves that landed stand.
///
/// # Errors
/// No book, or the desk write failed: then nothing changed.
pub fn set_autosave<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    on: bool,
    active: u64,
) -> Result<AutosaveOutcome, ControlError> {
    let changed = book(app)?.set_autosave(on);
    if changed && let Err(reason) = app.persist_workspace_autosave(on) {
        book(app)?.set_autosave(!on);
        return Err(write_failed(reason));
    }
    if let Some(market) = app.workspace_market() {
        market.set_held(!on);
    }
    let mut outcome = AutosaveOutcome {
        changed,
        saved: Vec::new(),
        failed: None,
    };
    if !on {
        return Ok(outcome);
    }
    let held = book(app)?;
    for tab in held.autosave_winners(&held.differing_tabs(), active) {
        match save_tab_to(app, tab) {
            Ok(saved) => outcome.saved.push(saved),
            Err(error) => {
                outcome.failed.get_or_insert(error);
            }
        }
    }
    let market = app.commit_workspace_market(None).map(drop);
    if let Err(reason) = market {
        outcome.failed.get_or_insert(write_failed(reason));
    }
    recount_held(app);
    Ok(outcome)
}

/// What closing a tab does with its held edits: `None` asks, and refuses with
/// [`UNSAVED_EDITS_CODE`] while it holds any; autosave on writes a pending
/// edit first, and asks for a copy it did not write.
///
/// # Errors
/// Edits held and no decision, no book, a failed save, or the last tab.
pub fn close_tab<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    save: Option<bool>,
) -> Result<Option<SaveOutcome>, ControlError> {
    // First: the tab that stays open writes and asks nothing.
    if !app.close_market_tab(tab, false) {
        return Err(last_tab());
    }
    flush(app, to, tab)?;
    let saved = match (close_question(app, tab), save) {
        (Some(asked), None) => return Err(unsaved(&asked)),
        (Some(_), Some(true)) => Some(save_to_journaled(app, to, tab)?),
        _ => None,
    };
    let (drop_held, keys) = (
        !book(app)?.autosave() && saved.is_none(),
        book(app)?.drawing_keys(tab).to_vec(),
    );
    if !app.close_market_tab(tab, true) {
        return Err(last_tab());
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

fn unsaved(asked: &CloseQuestion) -> ControlError {
    let CloseQuestion {
        workspace,
        changes,
        followers,
        ..
    } = asked;
    let mut error = ControlError::invalid_request(format!(
        "{} Pass save true to save to {workspace}, or save false to close without saving.",
        text::close_prompt_body(workspace, changes, followers),
    ));
    error.context.details = Some(json!({ "code": UNSAVED_EDITS_CODE,
        "workspace": workspace, "followers": followers, "changes": changes }));
    error
}

/// The question closing `tab` asks: `None` when the tab holds no edits (with
/// autosave on, unless its copy is one autosave did not write). `tab.close`
/// refuses with this same question, so the call and the window's prompt
/// cannot disagree on when to ask.
pub fn close_question<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    tab: u64,
) -> Option<CloseQuestion> {
    let UnsavedEdits {
        workspace,
        followers,
        changes,
    } = app.workspace_book()?.unsaved_edits(tab)?;
    Some(CloseQuestion {
        tab,
        symbol: app.tab_symbols(&[tab]).pop().unwrap_or_default(),
        followers: app.tab_symbols(&followers),
        workspace,
        changes,
    })
}

/// The window's words for the question up: `Close WINZ26`, what changed and
/// who follows, then Cancel / Close without saving / Save to Scalp, with
/// Enter on the save and Esc on Cancel.
#[must_use]
pub fn close_prompt(queue: &CloseQueue) -> Option<Prompt> {
    let asked = queue.question()?;
    let label = |answer| match answer {
        CloseAnswer::Cancel => text::CLOSE_CANCEL.to_owned(),
        CloseAnswer::CloseWithoutSaving => text::CLOSE_WITHOUT_SAVING.to_owned(),
        CloseAnswer::Save => text::save_to_short_label(&asked.workspace),
    };
    Some(Prompt {
        id: format!("workspace_close_prompt_{}", asked.tab),
        title: text::close_prompt_title(&asked.symbol),
        body: text::close_prompt_body(&asked.workspace, &asked.changes, &asked.followers),
        note: None,
        buttons: CloseAnswer::BUTTONS.map(label).to_vec(),
        enter: CloseAnswer::BUTTONS.len() - 1,
        escape: 0,
    })
}

/// Carry a close as far as it goes without the trader: settle each tab that
/// asks nothing and stop at the first question. A step that fails ends the
/// close and answers why; the window's last tab staying open is no news, and
/// it asks nothing first: the close would refuse it, so no answer, Save
/// included, would be carried out.
fn run_close<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    queue: &mut CloseQueue,
) -> Option<String> {
    let tabs = queue.scope() == Some(CloseScope::Tabs);
    let ask = |app: &mut H, tab| {
        let stays = tabs && !app.close_market_tab(tab, false);
        if stays {
            None
        } else {
            close_question(app, tab)
        }
    };
    while let CloseStep::Settle { tab, save, scope } = queue.next(|tab| ask(app, tab)) {
        let settled = match scope {
            CloseScope::Tabs => close_tab(app, to, tab, save).map(drop),
            CloseScope::App => settle_in_place(app, to, tab, save),
        };
        if let Err(error) = settled {
            queue.abort();
            return (code_of(&error) != Some(&json!(LAST_TAB_CODE))).then_some(error.message);
        }
    }
    None
}

/// Exit's settle, once every tab is answered (the queue defers them, so a
/// Cancel applies nothing): the tab stays in the desk for the next launch,
/// its edits written into its workspace or let go, and a pending autosave
/// written. Let go means the recipe copy too, so the desk keeps no edits
/// the trader dropped and the next launch opens the tab on its workspace.
fn settle_in_place<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    tab: u64,
    save: Option<bool>,
) -> Result<(), ControlError> {
    match save {
        Some(true) => save_to_journaled(app, to, tab).map(drop),
        Some(false) => {
            if book(app)?.discard_edits(tab) {
                show(app, &[tab])?;
            }
            // With autosave on nothing is held: the market's own debounce
            // writes what the panes show.
            if !book(app)?.autosave() {
                let keys = book(app)?.drawing_keys(tab).to_vec();
                discard_market(app, &keys, tab);
                recount_held(app);
            }
            Ok(())
        }
        None => flush(app, to, tab),
    }
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

/// The setting and every save that landed are journaled, a failed save
/// included: the answer is then that save's error, its details naming the
/// setting as it now stands and the workspaces saved, as the journal does.
fn autosave_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    on: bool,
    active: u64,
) -> Result<(bool, Vec<SaveOutcome>), ControlError> {
    let AutosaveOutcome {
        changed,
        saved,
        failed,
    } = set_autosave(app, on, active)?;
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
    match failed {
        None => Ok((changed, saved)),
        Some(mut error) => {
            let landed: Vec<&str> = saved.iter().map(|one| one.workspace.as_str()).collect();
            let code = code_of(&error).cloned().unwrap_or(json!(WRITE_FAILED_CODE));
            error.context.details = Some(json!({ "code": code, "auto_save": on,
                "changed": changed, "saved": landed }));
            Err(error)
        }
    }
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

/// Give a workspace a colour, swapping with its holder, and write the book.
///
/// # Errors
/// No workspace has the name, no book, or the write failed.
pub fn set_workspace_colour<H: WorkspaceBookHost + ?Sized>(
    app: &mut H,
    name: &str,
    colour: WorkspaceColour,
) -> Result<ColourOutcome, ControlError> {
    written(app, |book| book.set_colour(name, colour), |set| set.changed)
}

fn colour_journaled<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    to: &mut Journal<'_, A>,
    name: &str,
    colour: WorkspaceColour,
) -> Result<ColourOutcome, ControlError> {
    let outcome = set_workspace_colour(app, name, colour)?;
    if outcome.changed {
        let payload = json!({ "workspace": name, "colour": colour.key(),
            "swapped_with": outcome.swapped_with,
            "holder_colour": outcome.holder_colour.map(WorkspaceColour::key) });
        journal(to, WORKSPACE_COLOUR_SET_EVENT_KIND, payload);
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

fn autosave_set<H: WorkspaceBookHost + TabDirectory + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceAutosaveInput { on } = input(value)?;
    let active = app.tab_id_at(app.active_tab_index());
    let (changed, saved) = autosave_journaled(app, &mut Some((access, actor)), on, active)?;
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

fn colour_set<H: WorkspaceBookHost + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let WorkspaceColourSetInput { name, colour } = input(value)?;
    let outcome = colour_journaled(app, &mut Some((access, actor)), &name, colour.into())?;
    answer(&WorkspaceColourSetResult {
        workspace: outcome.workspace,
        colour,
        changed: outcome.changed,
        swapped_with: outcome.swapped_with,
        holder_colour: outcome.holder_colour.map(Into::into),
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
            colour: row.colour.map(Into::into),
            name: row.name,
        })
        .collect();
    answer(&WorkspaceListResult { workspaces })
}

#[cfg(test)]
#[path = "workspace_book_tests.rs"]
mod workspace_book_tests;
