//! The workspace book: named recipes, and which one each open tab uses.
//!
//! Every tab is one market and uses one workspace, and holds its own copy of
//! that recipe. An edit changes only the copy, so it reaches no other tab until
//! [`WorkspaceBook::save_to`]. Whether a copy differs is derived by comparing it
//! to the recipe, never stored, so a flag and the truth cannot disagree.
//!
//! With [`WorkspaceBook::autosave`] on (the default) the window writes every
//! edit through after a quiet second, so no tab reads as holding edits; with it
//! off a tab holds them, marked, until Save to. There is no revert: a workspace
//! switch drops held edits, and closing the tab asks first.
//!
//! The recipes persist in their own file; the per-tab state persists on
//! [`SavedTab`] in the desk, so an edited tab survives a restart still marked.
//! Headless and deterministic: the tab ids are told, nothing here paints or
//! reads a clock.
use crate::arrangement_document::{LAYOUT_UNRECORDED, SavedTab};
use crate::indicator_document::SavedIndicator;
use crate::layout_document::{LayoutBook, LayoutId};
use crate::workspace_recipe::{ContextPaneRecipe, FACTORY_SUMMARY, WorkspaceRecipe};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The file beside the layouts file.
pub const WORKSPACES_FILE: &str = "workspaces.toml";
/// The workspace that always exists and cannot be deleted or renamed.
pub const DEFAULT_WORKSPACE: &str = "Default";
/// Longest a workspace name may be: it sits on a tab chip.
pub const MAX_WORKSPACE_NAME: usize = 32;
/// Bumped on breaking format changes; unknown versions are refused whole.
const FORMAT_VERSION: u32 = 1;

/// Why a workspace call was refused. `code` is stable and `Display` is the
/// sentence shown under the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceRefusal {
    EmptyName,
    NameTooLong,
    DuplicateName(String),
    UnknownWorkspace(String),
    DefaultProtected,
    UnknownTab(u64),
    /// The tab's panes are still being built, so there is nothing to read an
    /// empty recipe from yet.
    TabOpening(u64),
    /// `workspace.revert` is gone: edits are kept by Save to or by autosave.
    RevertRemoved,
}

impl WorkspaceRefusal {
    /// The code a caller branches on.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyName => "workspace.name_empty",
            Self::NameTooLong => "workspace.name_too_long",
            Self::DuplicateName(_) => "workspace.name_taken",
            Self::UnknownWorkspace(_) => "workspace.unknown",
            Self::DefaultProtected => "workspace.default_protected",
            Self::UnknownTab(_) => "workspace.unknown_tab",
            Self::TabOpening(_) => "workspace.tab_not_ready",
            Self::RevertRemoved => "workspace.revert_removed",
        }
    }
}

impl std::fmt::Display for WorkspaceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => f.write_str("a workspace needs a name"),
            Self::NameTooLong => write!(
                f,
                "a workspace name holds at most {MAX_WORKSPACE_NAME} characters"
            ),
            Self::DuplicateName(name) => write!(f, "a workspace is already called {name}"),
            Self::UnknownWorkspace(name) => write!(f, "no workspace is called {name}"),
            Self::DefaultProtected => {
                write!(f, "{DEFAULT_WORKSPACE} cannot be deleted or renamed")
            }
            Self::UnknownTab(tab) => write!(f, "no open tab has id {tab}"),
            Self::TabOpening(tab) => {
                write!(f, "tab {tab} is still opening; retry once its charts stand")
            }
            Self::RevertRemoved => f.write_str(
                "workspace.revert was removed: keep edits with workspace.save_to, or turn on \
                 workspace.autosave.set; switching the workspace drops held edits",
            ),
        }
    }
}

impl std::error::Error for WorkspaceRefusal {}

/// Why a workspaces file was refused; its `Display` is the sentence a reader sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookParseError {
    Malformed(String),
    Version(u32),
}

impl std::fmt::Display for BookParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(message) => f.write_str(message),
            Self::Version(found) => write!(
                f,
                "workspaces format version {found} (this build reads {FORMAT_VERSION})"
            ),
        }
    }
}

impl std::error::Error for BookParseError {}

/// One tab's workspace and its own copy of the recipe.
#[derive(Debug, Clone, PartialEq)]
struct TabWorkspace {
    workspace: String,
    copy: WorkspaceRecipe,
    /// On the factory `Default`, which names no arrangement: what the tab
    /// showed when its panes last settled clean, so an edit undone by hand
    /// reads clean again, as on any other workspace.
    opened: Option<WorkspaceRecipe>,
}

impl TabWorkspace {
    fn on(workspace: &str, copy: WorkspaceRecipe) -> Self {
        Self {
            workspace: workspace.to_owned(),
            copy,
            opened: None,
        }
    }
}

/// What a rename did: the name as cleaned and stored, and whether it moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameOutcome {
    pub workspace: String,
    pub changed: bool,
}

/// What the status cell and the snapshot read for one tab. Both read this, so
/// they cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabWorkspaceState<'a> {
    pub workspace: &'a str,
    /// The tab holds edits it has not saved: never while autosave is on.
    pub modified: bool,
}

/// What closing a tab would lose, while autosave is off: the prompt's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsavedEdits {
    pub workspace: String,
    /// The clean tabs on the same workspace a save would also change.
    pub followers: Vec<u64>,
    /// What changed, as [`WorkspaceRecipe::changes_from`] names it.
    pub changes: Vec<String>,
}

/// What `tab.workspace.set` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetOutcome {
    /// Whether the tab moved; `false` for a repeat, which keeps any edits.
    pub changed: bool,
    /// Whether unsaved edits were dropped by the switch: the toast says so.
    pub dropped_edits: bool,
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    /// The workspace saved into.
    pub workspace: String,
    /// Whether the recipe changed; `false` for a repeat.
    pub changed: bool,
    /// The other tabs whose charts follow the new recipe. A tab with edits of
    /// its own keeps them and is not listed.
    pub tabs_changed: Vec<u64>,
}

/// What a delete did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeleteOutcome {
    /// Whether a workspace went: always, as a name no workspace has is refused.
    pub changed: bool,
    /// Every tab moved to `Default`.
    pub moved: Vec<u64>,
    /// The moved tabs that adopted `Default`'s recipe and must be redrawn.
    pub adopted: Vec<u64>,
    /// The moved tabs that kept their edits, now marked `Default •`.
    pub kept_edits: Vec<u64>,
}

/// One row of the list: a name and the tabs using it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceListing {
    pub name: String,
    pub tabs: Vec<u64>,
}

/// What making a new, empty workspace did. It always changes the book: a
/// repeat finds the name taken and is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateOutcome {
    /// The name as cleaned and stored.
    pub workspace: String,
    /// Whether the tab held unsaved edits, dropped by the move.
    pub dropped_edits: bool,
}

/// One open tab a first run seeds a workspace from.
#[derive(Debug, Clone, PartialEq)]
pub struct SeedTab {
    pub tab: u64,
    /// The name of the layout the tab shows (`Fluxo`), when it has one.
    pub layout: Option<String>,
    pub symbol: String,
    pub live: WorkspaceRecipe,
}

/// Named recipes, `Default` always among them, plus each open tab's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceBook {
    version: u32,
    #[serde(default)]
    recipes: BTreeMap<String, WorkspaceRecipe>,
    /// Old name to current name. The desk names a tab's workspace and is
    /// written only on exit or save, while a rename reaches this file at once:
    /// a desk read after a rename still finds its workspace.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    renamed: BTreeMap<String, String>,
    /// Kept in the desk, on [`SavedTab`], not in this file.
    #[serde(skip)]
    tabs: BTreeMap<u64, TabWorkspace>,
    /// The desk's `auto_save`, told at launch: not in this file.
    #[serde(skip, default = "autosave_default")]
    autosave: bool,
}

/// Autosave is the opening default.
const fn autosave_default() -> bool {
    true
}

impl Default for WorkspaceBook {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            recipes: BTreeMap::from([(DEFAULT_WORKSPACE.to_owned(), WorkspaceRecipe::default())]),
            renamed: BTreeMap::new(),
            tabs: BTreeMap::new(),
            autosave: autosave_default(),
        }
    }
}

/// Parse a workspaces file. A file without `Default` gets the factory one back.
///
/// # Errors
/// [`BookParseError`] for text that is not a book or a version this build refuses.
pub fn parse_book(text: &str) -> Result<WorkspaceBook, BookParseError> {
    let mut book: WorkspaceBook =
        toml::from_str(text).map_err(|error| BookParseError::Malformed(error.to_string()))?;
    if book.version != FORMAT_VERSION {
        return Err(BookParseError::Version(book.version));
    }
    book.recipes
        .entry(DEFAULT_WORKSPACE.to_owned())
        .or_default();
    Ok(book)
}

/// Clean up a typed name: trimmed, whitespace collapsed.
fn clean(name: &str) -> Result<String, WorkspaceRefusal> {
    let cleaned = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        return Err(WorkspaceRefusal::EmptyName);
    }
    if cleaned.chars().count() > MAX_WORKSPACE_NAME {
        return Err(WorkspaceRefusal::NameTooLong);
    }
    Ok(cleaned)
}

impl WorkspaceBook {
    /// Whether nothing but `Default` was ever saved here.
    #[must_use]
    pub fn is_unused(&self) -> bool {
        self.recipes.len() == 1
    }

    /// The recipe called `name`.
    #[must_use]
    pub fn recipe(&self, name: &str) -> Option<&WorkspaceRecipe> {
        self.recipes.get(name)
    }

    /// The workspace a desk's `name` means now: itself, or what it was renamed
    /// to. `None` once it was deleted.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Option<&str> {
        let held = |name: &str| {
            self.recipes
                .get_key_value(name)
                .map(|(held, _)| held.as_str())
        };
        held(name).or_else(|| self.renamed.get(name).and_then(|current| held(current)))
    }

    /// The name already taken by `name` up to case: `scalp` and `Scalp` would
    /// read as one workspace on a chip.
    fn taken(&self, name: &str) -> Option<&str> {
        self.recipes
            .keys()
            .find(|held| held.to_lowercase() == name.to_lowercase())
            .map(String::as_str)
    }

    /// Check a name a new workspace would take. The dialog asks this before
    /// the click, so the reason stands under the field.
    ///
    /// # Errors
    /// The name is empty, too long or already taken.
    pub fn validate_new_name(&self, name: &str) -> Result<String, WorkspaceRefusal> {
        let cleaned = clean(name)?;
        match self.taken(&cleaned) {
            Some(held) => Err(WorkspaceRefusal::DuplicateName(held.to_owned())),
            None => Ok(cleaned),
        }
    }

    /// Every workspace with the tabs using it, `Default` first, then by name.
    #[must_use]
    pub fn list(&self) -> Vec<WorkspaceListing> {
        let row = |name: &str| WorkspaceListing {
            name: name.to_owned(),
            tabs: self.tabs_on(name),
        };
        std::iter::once(row(DEFAULT_WORKSPACE))
            .chain(
                self.recipes
                    .keys()
                    .filter(|name| *name != DEFAULT_WORKSPACE)
                    .map(|name| row(name)),
            )
            .collect()
    }

    /// What a workspace holds, in the one line the menu and `workspace.list`
    /// show beside its name: [`WorkspaceRecipe::summary`].
    #[must_use]
    pub fn summary(&self, name: &str) -> String {
        self.recipes
            .get(name)
            .map_or_else(|| FACTORY_SUMMARY.to_owned(), WorkspaceRecipe::summary)
    }

    fn tabs_on(&self, name: &str) -> Vec<u64> {
        self.tabs
            .iter()
            .filter(|(_, state)| state.workspace == name)
            .map(|(tab, _)| *tab)
            .collect()
    }

    /// The tab's workspace and whether it holds unsaved edits. With autosave
    /// on an edit is on its way to the workspace, so nothing reads as held.
    #[must_use]
    pub fn tab_state(&self, tab: u64) -> Option<TabWorkspaceState<'_>> {
        let state = self.tabs.get(&tab)?;
        Some(TabWorkspaceState {
            workspace: &state.workspace,
            modified: !self.autosave && self.is_modified(state),
        })
    }

    /// Whether edits are written through as they are made.
    #[must_use]
    pub fn autosave(&self) -> bool {
        self.autosave
    }

    /// Turn autosave on or off. Answers whether it moved; the caller saves
    /// [`Self::differing_tabs`] when it turns on.
    pub fn set_autosave(&mut self, on: bool) -> bool {
        std::mem::replace(&mut self.autosave, on) != on
    }

    /// Every tab whose copy differs from its workspace, in id order, whatever
    /// autosave says: what turning it on or a write-through saves.
    #[must_use]
    pub fn differing_tabs(&self) -> Vec<u64> {
        self.tabs
            .iter()
            .filter(|(_, state)| self.is_modified(state))
            .map(|(tab, _)| *tab)
            .collect()
    }

    /// Whether the tab's copy differs from its workspace, whatever autosave says.
    #[must_use]
    pub fn differs(&self, tab: u64) -> bool {
        self.tabs
            .get(&tab)
            .is_some_and(|state| self.is_modified(state))
    }

    /// What closing the tab would lose: `None` while autosave is on or the tab
    /// holds nothing, so closing never asks.
    #[must_use]
    pub fn unsaved_edits(&self, tab: u64) -> Option<UnsavedEdits> {
        let state = self.tabs.get(&tab)?;
        if self.autosave || !self.is_modified(state) {
            return None;
        }
        let changes = self
            .baseline(state)
            .map(|base| state.copy.changes_from(base))
            .unwrap_or_default();
        Some(UnsavedEdits {
            workspace: state.workspace.clone(),
            followers: self.save_to_preview(tab).unwrap_or_default(),
            changes,
        })
    }

    /// The arrangement the tab holds now.
    #[must_use]
    pub fn tab_copy(&self, tab: u64) -> Option<&WorkspaceRecipe> {
        self.tabs.get(&tab).map(|state| &state.copy)
    }

    /// The arrangement the tab's copy is told against: the recipe, or on the
    /// factory `Default` what the tab settled on.
    fn baseline<'a>(&'a self, state: &'a TabWorkspace) -> Option<&'a WorkspaceRecipe> {
        let recipe = self.recipes.get(&state.workspace)?;
        Some(
            state
                .opened
                .as_ref()
                .filter(|_| recipe.is_factory())
                .unwrap_or(recipe),
        )
    }

    fn is_modified(&self, state: &TabWorkspace) -> bool {
        self.baseline(state).is_none_or(|base| *base != state.copy)
    }

    /// A tab's panes stand, showing `live`. On the factory `Default`, a clean
    /// tab takes that as its copy and its baseline; a tab with edits keeps them.
    pub fn settle_tab(&mut self, tab: u64, live: &WorkspaceRecipe) {
        let Some(state) = self.tabs.get(&tab) else {
            return;
        };
        let factory = self
            .recipes
            .get(&state.workspace)
            .is_some_and(WorkspaceRecipe::is_factory);
        if factory
            && !self.is_modified(state)
            && let Some(state) = self.tabs.get_mut(&tab)
        {
            state.copy = live.clone();
            state.opened = Some(live.clone());
        }
    }

    /// What the desk keeps for a tab: the workspace, and the copy only while it
    /// differs from the recipe, so a clean tab follows later saves.
    #[must_use]
    pub fn saved_fields(&self, tab: u64) -> (Option<String>, Option<WorkspaceRecipe>) {
        match self.tabs.get(&tab) {
            Some(state) => (
                Some(state.workspace.clone()),
                self.is_modified(state).then(|| state.copy.clone()),
            ),
            None => (None, None),
        }
    }

    /// A new tab opens on `Default`. A tab the book already holds stays as it is.
    /// While `Default` [`is_factory`](WorkspaceRecipe::is_factory) the window
    /// leaves what the tab opened with where it is.
    pub fn open_tab(&mut self, tab: u64) -> &WorkspaceRecipe {
        let copy = self.default_recipe().clone();
        &self
            .tabs
            .entry(tab)
            .or_insert_with(|| TabWorkspace::on(DEFAULT_WORKSPACE, copy))
            .copy
    }

    /// A tab closed: nothing of it stays. Its edits go with it.
    pub fn close_tab(&mut self, tab: u64) {
        self.tabs.remove(&tab);
    }

    /// A tab the desk brought back, following a rename since. A workspace the
    /// book no longer has hands it to `Default`, keeping the copy it is given,
    /// marked; the window gives what the tab shows when the desk kept none, so
    /// a lost workspace never lays `Default` over the tab.
    pub fn restore_tab(&mut self, tab: u64, workspace: &str, copy: Option<WorkspaceRecipe>) {
        let name = self
            .resolve(workspace)
            .unwrap_or(DEFAULT_WORKSPACE)
            .to_owned();
        let copy = copy.unwrap_or_else(|| self.recipes.get(&name).cloned().unwrap_or_default());
        self.tabs.insert(tab, TabWorkspace::on(&name, copy));
    }

    /// The first run: one workspace per open tab, named after the layout the
    /// tab shows (`Fluxo`) or else its bare symbol, deduplicated with a number,
    /// with the recipe equal to what the tab shows, so no pixel changes and no
    /// tab reads as modified. A tab the book holds is left alone. Returns each
    /// seeded tab and its workspace.
    pub fn seed_first_run(&mut self, tabs: Vec<SeedTab>) -> Vec<(u64, String)> {
        let mut seeded = Vec::new();
        for SeedTab {
            tab,
            layout,
            symbol,
            live,
        } in tabs
        {
            if self.tabs.contains_key(&tab) {
                continue;
            }
            let named = layout
                .and_then(|layout| clean(&layout).ok())
                .filter(|layout| !layout.eq_ignore_ascii_case(DEFAULT_WORKSPACE));
            let name = self.free_seed_name(&named.unwrap_or(symbol));
            self.recipes.insert(name.clone(), live.clone());
            self.tabs.insert(tab, TabWorkspace::on(&name, live));
            seeded.push((tab, name));
        }
        seeded
    }

    fn free_seed_name(&self, base: &str) -> String {
        let base: String = base.chars().take(MAX_WORKSPACE_NAME - 4).collect();
        std::iter::once(base.clone())
            .chain((2_u32..).map(|n| format!("{base} {n}")))
            .find(|candidate| self.taken(candidate).is_none())
            .expect("the naturals are not exhausted")
    }

    /// What the tab shows now, as the window reads it after an edit. Answers
    /// whether the copy changed.
    ///
    /// # Errors
    /// The book holds no such tab.
    pub fn observe(&mut self, tab: u64, live: &WorkspaceRecipe) -> Result<bool, WorkspaceRefusal> {
        let state = self
            .tabs
            .get_mut(&tab)
            .ok_or(WorkspaceRefusal::UnknownTab(tab))?;
        if state.copy == *live {
            return Ok(false);
        }
        state.copy = live.clone();
        Ok(true)
    }

    /// Put the tab on `name`. A repeat is no change and keeps any edits; a
    /// switch drops them, and says so.
    ///
    /// # Errors
    /// No such tab or workspace.
    pub fn set(&mut self, tab: u64, name: &str) -> Result<SetOutcome, WorkspaceRefusal> {
        let recipe = self
            .recipes
            .get(name)
            .ok_or_else(|| WorkspaceRefusal::UnknownWorkspace(name.to_owned()))?
            .clone();
        let state = self.state(tab)?;
        if state.workspace == name {
            return Ok(SetOutcome {
                changed: false,
                dropped_edits: false,
            });
        }
        let dropped_edits = self.is_modified(state);
        self.tabs.insert(tab, TabWorkspace::on(name, recipe));
        Ok(SetOutcome {
            changed: true,
            dropped_edits,
        })
    }

    fn state(&self, tab: u64) -> Result<&TabWorkspace, WorkspaceRefusal> {
        self.tabs.get(&tab).ok_or(WorkspaceRefusal::UnknownTab(tab))
    }

    /// The other tabs a [`Self::save_to`] from `tab` would change, named before
    /// the click: clean tabs on the same workspace.
    ///
    /// # Errors
    /// No such tab.
    pub fn save_to_preview(&self, tab: u64) -> Result<Vec<u64>, WorkspaceRefusal> {
        let saver = self.state(tab)?;
        if !self.is_modified(saver) {
            return Ok(Vec::new());
        }
        Ok(self
            .tabs
            .iter()
            .filter(|(other, state)| {
                **other != tab && state.workspace == saver.workspace && !self.is_modified(state)
            })
            .map(|(other, _)| *other)
            .collect())
    }

    /// Write the tab's copy into its workspace. Clean tabs on it follow; a tab
    /// with edits of its own keeps them, compared now to the new recipe.
    ///
    /// # Errors
    /// No such tab.
    pub fn save_to(&mut self, tab: u64) -> Result<SaveOutcome, WorkspaceRefusal> {
        let tabs_changed = self.save_to_preview(tab)?;
        let saver = self.state(tab)?;
        let workspace = saver.workspace.clone();
        if !self.is_modified(saver) {
            return Ok(SaveOutcome {
                workspace,
                changed: false,
                tabs_changed,
            });
        }
        let recipe = saver.copy.clone();
        for other in &tabs_changed {
            if let Some(state) = self.tabs.get_mut(other) {
                state.copy = recipe.clone();
            }
        }
        self.recipes.insert(workspace.clone(), recipe);
        Ok(SaveOutcome {
            workspace,
            changed: true,
            tabs_changed,
        })
    }

    /// Make a new workspace of the tab's copy and put the tab on it. No other
    /// tab changes. A repeat from the tab that made it is no change.
    ///
    /// # Errors
    /// No such tab; the name is empty, too long or taken.
    pub fn save_as(&mut self, tab: u64, name: &str) -> Result<SaveOutcome, WorkspaceRefusal> {
        let saver = self.state(tab)?;
        let cleaned = clean(name)?;
        if saver.workspace == cleaned && !self.is_modified(saver) {
            return Ok(SaveOutcome {
                workspace: cleaned,
                changed: false,
                tabs_changed: Vec::new(),
            });
        }
        let cleaned = self.validate_new_name(&cleaned)?;
        let copy = saver.copy.clone();
        self.recipes.insert(cleaned.clone(), copy);
        if let Some(state) = self.tabs.get_mut(&tab) {
            state.workspace.clone_from(&cleaned);
        }
        Ok(SaveOutcome {
            workspace: cleaned,
            changed: true,
            tabs_changed: Vec::new(),
        })
    }

    /// Make a new workspace of an empty recipe ([`WorkspaceRecipe::emptied`]
    /// of what the tab holds) and put the tab on it. No other tab changes.
    ///
    /// # Errors
    /// No such tab; a tab still opening, whose copy is the factory `Default`
    /// that names no arrangement; the name is empty, too long or taken.
    pub fn create(&mut self, tab: u64, name: &str) -> Result<CreateOutcome, WorkspaceRefusal> {
        let state = self.state(tab)?;
        if state.copy.is_factory() {
            return Err(WorkspaceRefusal::TabOpening(tab));
        }
        let cleaned = self.validate_new_name(name)?;
        let dropped_edits = self.is_modified(state);
        let recipe = state.copy.emptied();
        self.recipes.insert(cleaned.clone(), recipe.clone());
        self.tabs.insert(tab, TabWorkspace::on(&cleaned, recipe));
        Ok(CreateOutcome {
            workspace: cleaned,
            dropped_edits,
        })
    }

    /// Rename a workspace; its tabs follow, and so does a desk still naming it
    /// the old way. Answers the name as stored and whether it moved.
    ///
    /// # Errors
    /// `Default` on either side, no such workspace, or a bad or taken new name.
    pub fn rename(&mut self, name: &str, to: &str) -> Result<RenameOutcome, WorkspaceRefusal> {
        if name == DEFAULT_WORKSPACE {
            return Err(WorkspaceRefusal::DefaultProtected);
        }
        if !self.recipes.contains_key(name) {
            return Err(WorkspaceRefusal::UnknownWorkspace(name.to_owned()));
        }
        let cleaned = clean(to)?;
        if cleaned == name {
            return Ok(RenameOutcome {
                workspace: cleaned,
                changed: false,
            });
        }
        if let Some(held) = self.taken(&cleaned)
            && held != name
        {
            return Err(WorkspaceRefusal::DuplicateName(held.to_owned()));
        }
        if let Some(recipe) = self.recipes.remove(name) {
            self.recipes.insert(cleaned.clone(), recipe);
        }
        for state in self.tabs.values_mut() {
            if state.workspace == name {
                state.workspace.clone_from(&cleaned);
            }
        }
        self.renamed.remove(&cleaned);
        for current in self.renamed.values_mut() {
            if current == name {
                current.clone_from(&cleaned);
            }
        }
        self.renamed.insert(name.to_owned(), cleaned.clone());
        Ok(RenameOutcome {
            workspace: cleaned,
            changed: true,
        })
    }

    /// Delete a workspace; its tabs move to `Default`. A clean one adopts
    /// `Default`'s recipe; one with edits keeps them, marked `Default •`, so
    /// nothing is lost.
    ///
    /// # Errors
    /// `name` is `Default`, or no workspace has it, as on a repeat.
    pub fn delete(&mut self, name: &str) -> Result<DeleteOutcome, WorkspaceRefusal> {
        if name == DEFAULT_WORKSPACE {
            return Err(WorkspaceRefusal::DefaultProtected);
        }
        let Some(recipe) = self.recipes.remove(name) else {
            return Err(WorkspaceRefusal::UnknownWorkspace(name.to_owned()));
        };
        self.renamed.retain(|_, current| current != name);
        let fallback = self.default_recipe().clone();
        let mut outcome = DeleteOutcome {
            changed: true,
            ..DeleteOutcome::default()
        };
        for (tab, state) in &mut self.tabs {
            if state.workspace != name {
                continue;
            }
            state.workspace = DEFAULT_WORKSPACE.to_owned();
            state.opened = None;
            outcome.moved.push(*tab);
            if state.copy == recipe {
                state.copy.clone_from(&fallback);
                outcome.adopted.push(*tab);
            } else {
                outcome.kept_edits.push(*tab);
            }
        }
        Ok(outcome)
    }

    fn default_recipe(&self) -> &WorkspaceRecipe {
        self.recipes
            .get(DEFAULT_WORKSPACE)
            .expect("parse and the constructor keep Default in the book")
    }
}

/// A pane's layout id as a copy of what it holds, the book's active layout where
/// the desk did not record one.
fn indicators_of(layouts: &LayoutBook, id: Option<u64>) -> Vec<SavedIndicator> {
    id.filter(|id| *id != LAYOUT_UNRECORDED)
        .and_then(|id| layouts.get(LayoutId(id)))
        .unwrap_or_else(|| layouts.active())
        .indicators
        .clone()
}

/// The recipe a saved tab amounts to: the canvas it carries, and copies of the
/// indicators its panes' layout ids point at. The caller adds what the window
/// keeps elsewhere: each pane's layer switches, flow first and then the context
/// panes top to bottom, and the footprint style.
#[must_use]
pub fn capture_recipe(
    tab: &SavedTab,
    layouts: &LayoutBook,
    (layers, context_layers): (BTreeMap<String, bool>, Vec<BTreeMap<String, bool>>),
    footprint_style: Option<String>,
) -> WorkspaceRecipe {
    let intervals = if tab.context_bars.is_empty() {
        tab.time_bars.iter().count()
    } else {
        tab.context_bars.len()
    };
    let panes = tab.context_layouts.len().max(intervals);
    // A pane the stack has not sized yet is automatic and open, which is what
    // a missing entry means: one reading for both, or a fresh pane is an edit.
    let mut context_heights = tab.context_heights.clone();
    context_heights.resize(context_heights.len().max(panes), 0.0);
    let mut context_panes_collapsed = tab.context_panes_collapsed.clone();
    context_panes_collapsed.resize(context_panes_collapsed.len().max(panes), false);
    WorkspaceRecipe {
        layout: tab.layout,
        split_fraction: tab.split_fraction,
        context_collapsed: tab.context_collapsed,
        flow_collapsed: tab.flow_collapsed,
        context_heights,
        context_panes_collapsed,
        flow_indicators: indicators_of(layouts, tab.flow_layout),
        context_panes: (0..panes)
            .map(|pane| ContextPaneRecipe {
                indicators: indicators_of(layouts, tab.context_layouts.get(pane).copied()),
                layers: context_layers.get(pane).cloned().unwrap_or_default(),
            })
            .collect(),
        layers,
        footprint_style,
    }
}

#[cfg(test)]
#[path = "workspace_book_tests.rs"]
mod workspace_book_tests;
