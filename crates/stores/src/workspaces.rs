//! The workspaces file: validation and guarded disk persistence, beside the
//! layouts file. The book itself is [`quantick_workspace::workspace_book`].
use quantick_workspace::book_upkeep::UpkeepSave;
use quantick_workspace::write_refusal::guard_write;
use std::path::{Path, PathBuf};

pub use quantick_workspace::workspace_book::{
    BookParseError, CreateOutcome, DEFAULT_WORKSPACE, DeleteOutcome, MAX_WORKSPACE_NAME,
    RenameOutcome, SaveOutcome, SeedTab, SetOutcome, TabWorkspaceState, UnsavedEdits, Unsynced,
    WORKSPACES_FILE, WorkspaceBook, WorkspaceListing, WorkspaceRefusal, capture_recipe, parse_book,
};
pub use quantick_workspace::workspace_colour::{ColourOutcome, Swatch, WorkspaceColour};
pub use quantick_workspace::workspace_recipe::{ContextPaneRecipe, WorkspaceRecipe};
pub use quantick_workspace::workspace_sync::{TabAdoption, TabRecipeSync};

/// The file beside a layouts file.
#[must_use]
pub fn path_beside(layouts: &Path) -> PathBuf {
    layouts.with_file_name(WORKSPACES_FILE)
}

/// The gate a bundle section goes through.
///
/// # Errors
/// The parser's reason when the document is invalid.
pub fn validate(text: &str) -> Result<(), String> {
    parse_book(text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Read the file; the flag says it must not be written back. A file this build
/// refuses is set aside under a `.broken` name; when that fails too it is the
/// trader's only copy, and the session must never save over it.
#[must_use]
pub fn load(path: &Path) -> (WorkspaceBook, bool) {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (WorkspaceBook::default(), false);
        }
        Err(error) => Err(error.to_string()),
    };
    let reason = match text.and_then(|text| parse_book(&text).map_err(|e| e.to_string())) {
        Ok(book) => return (book, false),
        Err(reason) => reason,
    };
    let aside = path.with_extension("toml.broken");
    let set_aside = guard_write(path).is_ok() && std::fs::rename(path, &aside).is_ok();
    tracing::warn!(
        target: "quantick::app",
        schema_version = 1_u8,
        event_code = "WORKSPACES_UNREADABLE",
        path = %path.display(),
        set_aside_to = %aside.display(),
        set_aside,
        reason = %reason,
        action = if set_aside { "starting_on_default" } else { "starting_on_default_and_never_saving_over_the_file" },
        "the workspaces file could not be read"
    );
    (WorkspaceBook::default(), !set_aside)
}

/// Write the book through a temp sibling and a rename, like every other store.
///
/// # Errors
/// The reason nothing reached the disk: this session writes no store, or the
/// write failed. The caller shows it; a saved workspace is never claimed.
pub fn save(path: &Path, book: &WorkspaceBook) -> Result<(), String> {
    guard_write(path)?;
    let written = toml::to_string_pretty(book)
        .map_err(|error| error.to_string())
        .and_then(|text| {
            let temp = path.with_extension("toml.tmp");
            std::fs::write(&temp, text)
                .and_then(|()| std::fs::rename(&temp, path))
                .map_err(|error| {
                    let _ = std::fs::remove_file(&temp);
                    error.to_string()
                })
        });
    if let Err(error) = &written {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "WORKSPACES_WRITE_FAILED",
            path = %path.display(),
            %error,
            action = "workspaces_not_saved",
            "could not save the workspaces"
        );
    }
    written
}

/// The book at its resolved path, and whether this session may write it.
#[derive(Debug)]
pub struct WorkspaceBookStore {
    book: WorkspaceBook,
    path: PathBuf,
    blocked: bool,
    sync: TabRecipeSync,
}

impl WorkspaceBookStore {
    /// Read the book beside a layouts file.
    #[must_use]
    pub fn open_beside(layouts: &Path) -> Self {
        Self::open_at(path_beside(layouts))
    }

    /// Read the book at `path`.
    #[must_use]
    pub fn open_at(path: PathBuf) -> Self {
        let (book, blocked) = load(&path);
        Self {
            book,
            path,
            blocked,
            sync: TabRecipeSync::default(),
        }
    }

    #[must_use]
    pub fn book(&self) -> &WorkspaceBook {
        &self.book
    }

    pub fn book_mut(&mut self) -> &mut WorkspaceBook {
        &mut self.book
    }

    /// Whether layout edits are written through as they are made.
    #[must_use]
    pub fn autosave(&self) -> bool {
        self.book.autosave()
    }
    /// Turn autosave on or off, as the desk file says.
    pub fn set_autosave(&mut self, on: bool) {
        self.book.set_autosave(on);
    }

    /// What the snapshot and the chip read for a tab: a tab the book has not met
    /// is on `Default`, clean, as a new tab is.
    #[must_use]
    pub fn tab_row(&self, tab: u64) -> (String, bool) {
        self.book.tab_state(tab).map_or_else(
            || (DEFAULT_WORKSPACE.to_owned(), false),
            |state| (state.workspace.to_owned(), state.modified),
        )
    }

    /// The window's side of the book.
    #[must_use]
    pub fn sync(&self) -> &TabRecipeSync {
        &self.sync
    }

    pub fn sync_mut(&mut self) -> &mut TabRecipeSync {
        &mut self.sync
    }

    /// The book and the window's side of it, together, to read.
    #[must_use]
    pub fn parts(&self) -> (&WorkspaceBook, &TabRecipeSync) {
        (&self.book, &self.sync)
    }

    /// The book and the window's side of it, together, to change.
    pub fn parts_mut(&mut self) -> (&mut WorkspaceBook, &mut TabRecipeSync) {
        (&mut self.book, &mut self.sync)
    }

    /// A tab closed: nothing of it stays.
    pub fn forget_tab(&mut self, tab: u64) {
        self.book.close_tab(tab);
        self.sync.forget(tab);
    }

    /// Read the file again: an import wrote it under the running window.
    pub fn reload(&mut self) {
        let autosave = self.book.autosave();
        (self.book, self.blocked) = load(&self.path);
        self.book.set_autosave(autosave);
        self.sync.restart();
    }

    /// Follow a layouts file moved to another folder.
    pub fn move_beside(&mut self, layouts: &Path) {
        self.path = path_beside(layouts);
    }

    /// Write the recipes.
    ///
    /// # Errors
    /// Why nothing reached the disk, as [`save`] says, or that the file holds
    /// what this session could not read and must not be written over.
    pub fn persist(&self) -> Result<(), String> {
        if self.blocked {
            return Err(format!(
                "{} could not be read at launch and is left untouched",
                self.path.display()
            ));
        }
        save(&self.path, &self.book)
    }

    /// Write what the frame's upkeep asked for. A refused write leaves the
    /// seeded workspaces for this session only, and says so in the log.
    pub fn save_upkeep(&self, save: UpkeepSave) {
        match save {
            UpkeepSave::Nothing => {}
            UpkeepSave::SeededBook => self.save_seeds(),
        }
    }

    fn save_seeds(&self) {
        if let Err(reason) = self.persist() {
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "WORKSPACES_SEED_NOT_SAVED", reason = %reason,
                action = "seeded_for_this_session_only",
                "the workspaces seeded from the open tabs were not saved");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchDir;
    use quantick_workspace::write_refusal::{WritesRefused, refuse_writes_on_this_thread};

    fn saved_book() -> WorkspaceBook {
        let mut book = WorkspaceBook::default();
        book.open_tab(1);
        book.save_as(1, "Scalp").unwrap();
        book
    }

    #[test]
    fn the_file_sits_beside_the_layouts_and_round_trips_without_tab_state() {
        let dir = ScratchDir::new("workspaces-round-trip");
        let path = path_beside(&dir.join("layouts.toml"));
        assert_eq!(path, dir.join(WORKSPACES_FILE));
        assert_eq!(load(&path), (WorkspaceBook::default(), false));
        save(&path, &saved_book()).unwrap();
        let (book, blocked) = load(&path);
        assert!(!blocked);
        assert!(book.recipe("Scalp").is_some());
        assert_eq!(book.tab_state(1), None, "tab state lives in the desk");
        assert!(validate(&std::fs::read_to_string(&path).unwrap()).is_ok());
    }

    #[test]
    fn the_store_reads_a_stranger_as_clean_default_and_writes_beside_the_layouts() {
        let dir = ScratchDir::new("workspaces-store");
        let mut store = WorkspaceBookStore::open_beside(&dir.join("layouts.toml"));
        assert_eq!(store.tab_row(7), ("Default".to_owned(), false));
        store.book_mut().open_tab(7);
        store.book_mut().save_as(7, "Scalp").unwrap();
        assert_eq!(store.tab_row(7), ("Scalp".to_owned(), false));
        store.persist().unwrap();
        let reopened = WorkspaceBookStore::open_beside(&dir.join("layouts.toml"));
        assert!(reopened.book().recipe("Scalp").is_some());
    }

    /// The upkeep's word decides the write: nothing to save leaves the disk alone.
    #[test]
    fn the_store_writes_only_what_the_upkeep_asks_for() {
        let dir = ScratchDir::new("workspaces-upkeep-save");
        let mut store = WorkspaceBookStore::open_beside(&dir.join("layouts.toml"));
        store.book_mut().open_tab(7);
        store.book_mut().save_as(7, "Scalp").unwrap();
        store.save_upkeep(UpkeepSave::Nothing);
        assert!(!dir.join(WORKSPACES_FILE).exists());
        store.save_upkeep(UpkeepSave::SeededBook);
        let reopened = WorkspaceBookStore::open_beside(&dir.join("layouts.toml"));
        assert!(reopened.book().recipe("Scalp").is_some());
    }

    #[test]
    fn a_blocked_store_refuses_with_the_reason_and_writes_nothing() {
        let dir = ScratchDir::new("workspaces-store-blocked");
        let path = dir.join(WORKSPACES_FILE);
        let store = WorkspaceBookStore {
            book: WorkspaceBook::default(),
            path: path.clone(),
            blocked: true,
            sync: TabRecipeSync::default(),
        };
        assert!(store.persist().unwrap_err().contains("left untouched"));
        assert!(!path.exists());
    }

    #[test]
    fn a_refused_file_is_set_aside_and_never_written_over() {
        let dir = ScratchDir::new("workspaces-refused");
        let path = dir.join(WORKSPACES_FILE);
        std::fs::write(&path, "version = 99\n").unwrap();
        assert_eq!(load(&path), (WorkspaceBook::default(), false));
        assert_eq!(
            std::fs::read_to_string(path.with_extension("toml.broken")).unwrap(),
            "version = 99\n"
        );
        assert!(validate("not = [toml").is_err());
    }

    #[test]
    fn a_refused_session_writes_nothing_and_says_why() {
        let dir = ScratchDir::new("workspaces-write-refused");
        let path = dir.join(WORKSPACES_FILE);
        refuse_writes_on_this_thread(Some(WritesRefused {
            hooks: vec!["QUANTICK_UI_STATE".to_owned()],
        }));
        let refused = save(&path, &saved_book());
        refuse_writes_on_this_thread(None);
        assert!(refused.unwrap_err().contains("saving is off"));
        assert!(!path.exists());
    }

    #[test]
    fn a_failed_write_answers_the_reason_and_leaves_no_temp_file() {
        let dir = ScratchDir::new("workspaces-write-failed");
        let path = dir.join("no-such-folder").join(WORKSPACES_FILE);
        assert!(save(&path, &saved_book()).is_err());
        assert!(!path.with_extension("toml.tmp").exists());
    }
}
