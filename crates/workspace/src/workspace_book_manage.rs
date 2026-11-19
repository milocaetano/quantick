//! What Manage workspaces and the two create dialogs ask of the book: a new
//! workspace in a chosen colour (no swap on create), the name each dialog
//! suggests, and a delete that can be undone for [`UNDO_WINDOW_MS`],
//! restoring the recipe, the colour, the old names a desk may hold and the
//! tabs the delete moved. The time is told, never read.

use super::{
    CreateOutcome, DEFAULT_WORKSPACE, DeleteOutcome, MAX_WORKSPACE_NAME, SaveOutcome, TabWorkspace,
    Unsynced, WorkspaceBook, WorkspaceRefusal, clean,
};
use crate::workspace_colour::{ColourTaken, WorkspaceColour};
use crate::workspace_recipe::WorkspaceRecipe;

/// How long a delete can be undone, in milliseconds.
pub const UNDO_WINDOW_MS: i64 = 10_000;

/// One tab a delete moved to `Default`: as it stood, and the copy the
/// delete left it, so an Undo restores it exactly unless it changed since.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct MovedTab {
    tab: u64,
    before: TabWorkspace,
    unsynced: Option<Unsynced>,
    left: WorkspaceRecipe,
}

/// A delete that an Undo can take back until `until_ms`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Deleted {
    recipe: WorkspaceRecipe,
    colour: WorkspaceColour,
    /// The old names that led to it, which a desk may still hold.
    renamed: Vec<(String, String)>,
    tabs: Vec<MovedTab>,
    until_ms: i64,
    undone: bool,
}

/// What an Undo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    pub workspace: String,
    /// `false` for a repeat after the Undo landed.
    pub changed: bool,
    /// The colour it holds now.
    pub colour: WorkspaceColour,
    /// Its own colour was taken meanwhile, so it took the next free one.
    pub recoloured: bool,
    /// The tabs moved back: still open and still on `Default`.
    pub tabs: Vec<u64>,
}

impl WorkspaceBook {
    /// Whether a new workspace called `name` may take `colour`: on create a
    /// colour another workspace holds is refused, never swapped.
    ///
    /// # Errors
    /// Another workspace holds an identical colour.
    pub fn colour_for_new(
        &self,
        name: &str,
        colour: WorkspaceColour,
    ) -> Result<(), WorkspaceRefusal> {
        let owners: Vec<String> = (self.colours.iter())
            .filter(|(held, other)| held.as_str() != name && other.identical(colour))
            .map(|(held, _)| held.clone())
            .collect();
        if owners.is_empty() {
            return Ok(());
        }
        Err(WorkspaceRefusal::ColourTaken(ColourTaken::NewWorkspace {
            owners,
        }))
    }

    /// The colour checked before anything changes, so a refusal leaves the
    /// book as it was.
    fn checked(&self, name: &str, colour: Option<WorkspaceColour>) -> Result<(), WorkspaceRefusal> {
        match colour {
            Some(colour) => self.colour_for_new(&clean(name)?, colour),
            None => Ok(()),
        }
    }

    /// [`Self::save_as`] in `colour`, else the next free one.
    ///
    /// # Errors
    /// As [`Self::save_as`], or the colour is another workspace's.
    pub fn save_as_coloured(
        &mut self,
        tab: u64,
        name: &str,
        colour: Option<WorkspaceColour>,
    ) -> Result<SaveOutcome, WorkspaceRefusal> {
        self.checked(name, colour)?;
        let mut saved = self.save_as(tab, name)?;
        if let Some(colour) = colour {
            saved.changed |= self.set_colour(&saved.workspace, colour, false)?.changed;
        }
        Ok(saved)
    }

    /// [`Self::create`] in `colour`, else the next free one.
    ///
    /// # Errors
    /// As [`Self::create`], or the colour is another workspace's.
    pub fn create_coloured(
        &mut self,
        tab: u64,
        name: &str,
        colour: Option<WorkspaceColour>,
    ) -> Result<CreateOutcome, WorkspaceRefusal> {
        self.checked(name, colour)?;
        let made = self.create(tab, name)?;
        if let Some(colour) = colour {
            self.set_colour(&made.workspace, colour, false)?;
        }
        Ok(made)
    }

    /// The first free `<stem> <n>` from `n = from`, cut to fit a name.
    #[must_use]
    pub fn suggested_name(&self, stem: &str, from: usize) -> String {
        (from..)
            .map(|n| {
                let tail = format!(" {n}");
                let room = MAX_WORKSPACE_NAME.saturating_sub(tail.chars().count());
                let stem: String = stem.chars().take(room).collect();
                format!("{}{tail}", stem.trim_end())
            })
            .find(|name| self.taken(name).is_none())
            .unwrap_or_default()
    }

    /// [`Self::delete`], recorded so [`Self::undo_delete`] can take it back
    /// until `now_ms` plus [`UNDO_WINDOW_MS`].
    ///
    /// # Errors
    /// As [`Self::delete`].
    pub fn delete_at(
        &mut self,
        name: &str,
        now_ms: i64,
    ) -> Result<DeleteOutcome, WorkspaceRefusal> {
        if let Some(refusal) = self.delete_refusal(name) {
            return Err(refusal);
        }
        let recipe = self.recipes.get(name).cloned().unwrap_or_default();
        let colour = self.colour(name).unwrap_or(WorkspaceColour::Preset(0));
        let renamed = (self.renamed.iter())
            .filter(|(_, current)| current.as_str() == name)
            .map(|(old, current)| (old.clone(), current.clone()))
            .collect();
        let before: Vec<(u64, TabWorkspace, Option<Unsynced>)> = (self.tabs.iter())
            .filter(|(_, state)| state.workspace == name)
            .map(|(tab, state)| (*tab, state.clone(), self.unsynced.get(tab).copied()))
            .collect();
        let outcome = self.delete(name)?;
        let tabs = (before.into_iter())
            .map(|(tab, before, unsynced)| MovedTab {
                left: self
                    .tabs
                    .get(&tab)
                    .map(|state| state.copy.clone())
                    .unwrap_or_default(),
                tab,
                before,
                unsynced,
            })
            .collect();
        self.deleted.retain(|_, record| record.until_ms > now_ms);
        let record = Deleted {
            recipe,
            colour,
            renamed,
            tabs,
            until_ms: now_ms.saturating_add(UNDO_WINDOW_MS),
            undone: false,
        };
        self.deleted.insert(name.to_owned(), record);
        Ok(outcome)
    }

    /// The tabs a delete of `name` would put on `Default`'s recipe, and those
    /// that would keep their edits there: the split [`Self::delete`] makes,
    /// named before the click.
    #[must_use]
    pub fn delete_split(&self, name: &str) -> (Vec<u64>, Vec<u64>) {
        let recipe = self.recipes.get(name);
        let on = self
            .tabs
            .iter()
            .filter(|(_, state)| state.workspace == name);
        let (adopted, kept): (Vec<_>, Vec<_>) =
            on.partition(|(_, state)| recipe.is_some_and(|recipe| recipe.holds(&state.copy)));
        let ids = |tabs: Vec<(&u64, _)>| tabs.into_iter().map(|(tab, _)| *tab).collect();
        (ids(adopted), ids(kept))
    }

    /// When the Undo of `name`'s delete ends, while one waits.
    #[must_use]
    pub fn undo_until(&self, name: &str) -> Option<i64> {
        (self.deleted.get(name))
            .filter(|record| !record.undone)
            .map(|record| record.until_ms)
    }

    /// Take back the delete of `name` at `now_ms`: its recipe and colour (the
    /// next free one when its own was taken meanwhile), the old names a desk
    /// may hold, and the tabs it moved that are still open and on `Default`.
    /// A repeat answers no change.
    ///
    /// # Errors
    /// No recent delete of the name; the ten seconds ended; the name was taken.
    pub fn undo_delete(
        &mut self,
        name: &str,
        now_ms: i64,
    ) -> Result<UndoOutcome, WorkspaceRefusal> {
        let unknown = || WorkspaceRefusal::UnknownWorkspace(name.to_owned());
        let record = self.deleted.get(name).ok_or_else(unknown)?.clone();
        if record.undone {
            return Ok(UndoOutcome {
                workspace: name.to_owned(),
                changed: false,
                colour: self.colour(name).unwrap_or(record.colour),
                recoloured: false,
                tabs: Vec::new(),
            });
        }
        if now_ms >= record.until_ms {
            return Err(WorkspaceRefusal::UndoExpired(name.to_owned()));
        }
        if let Some(held) = self.taken(name) {
            return Err(WorkspaceRefusal::DuplicateName(held.to_owned()));
        }
        self.recipes.insert(name.to_owned(), record.recipe.clone());
        self.renamed.extend(record.renamed.iter().cloned());
        if self
            .colours
            .values()
            .all(|held| !held.identical(record.colour))
        {
            self.colours.insert(name.to_owned(), record.colour);
        }
        self.settle_colours();
        let tabs = self.move_back(name, &record.tabs);
        if let Some(kept) = self.deleted.get_mut(name) {
            kept.undone = true;
        }
        let colour = self.colour(name).unwrap_or(record.colour);
        Ok(UndoOutcome {
            workspace: name.to_owned(),
            changed: true,
            colour,
            recoloured: colour != record.colour,
            tabs,
        })
    }

    /// The tabs a delete moved, back on `name` where still open and on
    /// `Default`: exactly as they stood when untouched since or still clean
    /// there, else with their copy, which then reads as edits.
    fn move_back(&mut self, name: &str, moved: &[MovedTab]) -> Vec<u64> {
        let mut back = Vec::new();
        for one in moved {
            let Some(state) = self.tabs.get(&one.tab) else {
                continue;
            };
            if state.workspace != DEFAULT_WORKSPACE {
                continue;
            }
            // Untouched, or settled clean on a factory Default: as it stood.
            let untouched = state.copy == one.left || !self.is_modified(one.tab, state);
            let Some(state) = self.tabs.get_mut(&one.tab) else {
                continue;
            };
            back.push(one.tab);
            if untouched {
                *state = one.before.clone();
                match one.unsynced {
                    Some(unsynced) => self.unsynced.insert(one.tab, unsynced),
                    None => self.unsynced.remove(&one.tab),
                };
                continue;
            }
            (state.workspace, state.opened) = (name.to_owned(), None);
            if self.autosave && self.differs(one.tab) {
                self.unsynced.insert(one.tab, Unsynced::Outvoted);
            }
        }
        back
    }
}

#[cfg(test)]
#[path = "workspace_book_manage_tests.rs"]
mod tests;
