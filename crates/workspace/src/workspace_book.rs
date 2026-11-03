//! The workspace book: named recipes, and which one each open tab uses.
//!
//! Every tab is one market and uses one workspace, and holds its own unsaved
//! copy of that recipe. An edit changes only the copy, so it reaches no other
//! tab until [`WorkspaceBook::save_to`]. `modified` is derived by comparing the
//! copy to the recipe, never stored, so a flag and the truth cannot disagree.
//!
//! The recipes persist in their own file; the per-tab state persists on
//! [`SavedTab`] in the desk, so an edited tab survives a restart still marked.
//! Headless and deterministic: the tab ids are told, nothing here paints or
//! reads a clock.
use crate::arrangement_document::{LAYOUT_UNRECORDED, SavedTab};
use crate::indicator_document::SavedIndicator;
use crate::layout_document::{LayoutBook, LayoutId};
use crate::workspace_recipe::{ContextPaneRecipe, WorkspaceRecipe};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The file beside the layouts file.
pub const WORKSPACES_FILE: &str = "workspaces.toml";
/// The workspace that always exists and cannot be deleted or renamed.
pub const DEFAULT_WORKSPACE: &str = "Default";
/// Longest a workspace name may be: it sits on a tab chip.
pub const MAX_WORKSPACE_NAME: usize = 32;
/// What a first run calls the workspace seeded from a tab, before the symbol.
const SEED_PREFIX: &str = "from ";
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
}

/// What a chip and the snapshot read for one tab. Both read this, so they
/// cannot disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabWorkspaceState<'a> {
    pub workspace: &'a str,
    pub modified: bool,
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
    /// Whether a workspace went; `false` for a repeat.
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

/// One open tab a first run seeds a workspace from.
#[derive(Debug, Clone, PartialEq)]
pub struct SeedTab {
    pub tab: u64,
    pub symbol: String,
    pub live: WorkspaceRecipe,
}

/// Named recipes, `Default` always among them, plus each open tab's state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceBook {
    version: u32,
    #[serde(default)]
    recipes: BTreeMap<String, WorkspaceRecipe>,
    /// Kept in the desk, on [`SavedTab`], not in this file.
    #[serde(skip)]
    tabs: BTreeMap<u64, TabWorkspace>,
}

impl Default for WorkspaceBook {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            recipes: BTreeMap::from([(DEFAULT_WORKSPACE.to_owned(), WorkspaceRecipe::default())]),
            tabs: BTreeMap::new(),
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

    fn tabs_on(&self, name: &str) -> Vec<u64> {
        self.tabs
            .iter()
            .filter(|(_, state)| state.workspace == name)
            .map(|(tab, _)| *tab)
            .collect()
    }

    /// The tab's workspace and whether it holds unsaved edits.
    #[must_use]
    pub fn tab_state(&self, tab: u64) -> Option<TabWorkspaceState<'_>> {
        let state = self.tabs.get(&tab)?;
        Some(TabWorkspaceState {
            workspace: &state.workspace,
            modified: self.is_modified(state),
        })
    }

    /// The arrangement the tab holds now.
    #[must_use]
    pub fn tab_copy(&self, tab: u64) -> Option<&WorkspaceRecipe> {
        self.tabs.get(&tab).map(|state| &state.copy)
    }

    fn is_modified(&self, state: &TabWorkspace) -> bool {
        self.recipes
            .get(&state.workspace)
            .is_none_or(|recipe| *recipe != state.copy)
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
            .or_insert_with(|| TabWorkspace {
                workspace: DEFAULT_WORKSPACE.to_owned(),
                copy,
            })
            .copy
    }

    /// A tab closed: nothing of it stays. Its edits go with it.
    pub fn close_tab(&mut self, tab: u64) {
        self.tabs.remove(&tab);
    }

    /// A tab the desk brought back. A workspace the book no longer has hands it
    /// to `Default`, and whatever copy the desk kept stays, marked.
    pub fn restore_tab(&mut self, tab: u64, workspace: &str, copy: Option<WorkspaceRecipe>) {
        let known = self.recipes.contains_key(workspace);
        let name = if known { workspace } else { DEFAULT_WORKSPACE };
        let copy = copy.unwrap_or_else(|| {
            self.recipes
                .get(name)
                .cloned()
                .unwrap_or_else(|| self.default_recipe().clone())
        });
        self.tabs.insert(
            tab,
            TabWorkspace {
                workspace: name.to_owned(),
                copy,
            },
        );
    }

    /// The first run: one workspace per open tab, named `from <SYMBOL>` and
    /// deduplicated, with the recipe equal to what the tab shows, so no pixel
    /// changes and no tab reads as modified. A tab the book holds is left alone.
    /// Returns each seeded tab and its workspace.
    pub fn seed_first_run(&mut self, tabs: Vec<SeedTab>) -> Vec<(u64, String)> {
        let mut seeded = Vec::new();
        for SeedTab { tab, symbol, live } in tabs {
            if self.tabs.contains_key(&tab) {
                continue;
            }
            let name = self.free_seed_name(&symbol);
            self.recipes.insert(name.clone(), live.clone());
            self.tabs.insert(
                tab,
                TabWorkspace {
                    workspace: name.clone(),
                    copy: live,
                },
            );
            seeded.push((tab, name));
        }
        seeded
    }

    fn free_seed_name(&self, symbol: &str) -> String {
        let base = format!("{SEED_PREFIX}{symbol}");
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

    /// Put the tab on `name`. A repeat is no change and keeps any edits;
    /// [`Self::revert`] is the call that drops them.
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
        self.tabs.insert(
            tab,
            TabWorkspace {
                workspace: name.to_owned(),
                copy: recipe,
            },
        );
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

    /// Drop the tab's edits. Answers whether there were any.
    ///
    /// # Errors
    /// No such tab.
    pub fn revert(&mut self, tab: u64) -> Result<bool, WorkspaceRefusal> {
        let saver = self.state(tab)?;
        if !self.is_modified(saver) {
            return Ok(false);
        }
        let recipe = self
            .recipes
            .get(&saver.workspace)
            .cloned()
            .unwrap_or_default();
        if let Some(state) = self.tabs.get_mut(&tab) {
            state.copy = recipe;
        }
        Ok(true)
    }

    /// Rename a workspace; its tabs follow. Answers whether the name moved.
    ///
    /// # Errors
    /// `Default` on either side, no such workspace, or a bad or taken new name.
    pub fn rename(&mut self, name: &str, to: &str) -> Result<bool, WorkspaceRefusal> {
        if name == DEFAULT_WORKSPACE {
            return Err(WorkspaceRefusal::DefaultProtected);
        }
        if !self.recipes.contains_key(name) {
            return Err(WorkspaceRefusal::UnknownWorkspace(name.to_owned()));
        }
        let cleaned = clean(to)?;
        if cleaned == name {
            return Ok(false);
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
        Ok(true)
    }

    /// Delete a workspace; its tabs move to `Default`. A clean one adopts
    /// `Default`'s recipe; one with edits keeps them, marked `Default •`, so
    /// nothing is lost. A repeat finds nothing and answers no change.
    ///
    /// # Errors
    /// `name` is `Default`.
    pub fn delete(&mut self, name: &str) -> Result<DeleteOutcome, WorkspaceRefusal> {
        if name == DEFAULT_WORKSPACE {
            return Err(WorkspaceRefusal::DefaultProtected);
        }
        let Some(recipe) = self.recipes.remove(name) else {
            return Ok(DeleteOutcome::default());
        };
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
    let context_bars: Vec<String> = if tab.context_bars.is_empty() {
        tab.time_bars.iter().cloned().collect()
    } else {
        tab.context_bars.clone()
    };
    let panes = tab.context_layouts.len().max(context_bars.len());
    WorkspaceRecipe {
        layout: tab.layout,
        split_fraction: tab.split_fraction,
        context_collapsed: tab.context_collapsed,
        flow_collapsed: tab.flow_collapsed,
        context_heights: tab.context_heights.clone(),
        context_panes_collapsed: tab.context_panes_collapsed.clone(),
        context_bars,
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

/// Lay a recipe's canvas onto a saved tab. The market's own fields stay: feed,
/// symbol and the flow bar size are not the workspace's.
pub fn apply_canvas(recipe: &WorkspaceRecipe, tab: &mut SavedTab) {
    tab.layout = recipe.layout;
    tab.split_fraction = recipe.split_fraction;
    tab.context_collapsed = recipe.context_collapsed;
    tab.flow_collapsed = recipe.flow_collapsed;
    tab.context_heights.clone_from(&recipe.context_heights);
    tab.context_panes_collapsed
        .clone_from(&recipe.context_panes_collapsed);
    tab.time_bars = recipe.context_bars.first().cloned();
    tab.context_bars.clone_from(&recipe.context_bars);
}

#[cfg(test)]
#[path = "workspace_book_tests.rs"]
mod workspace_book_tests;
