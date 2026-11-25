//! The workspaces file: validation and guarded disk persistence, beside the
//! layouts file. The book itself is [`quantick_workspace::workspace_book`].
use quantick_appearance::{
    PremultipliedRgba, ResolvedTheme, Token, contrast::contrast_ratio, tab_strip,
};
use quantick_workspace::book_upkeep::UpkeepSave;
use quantick_workspace::write_refusal::guard_write;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub use quantick_workspace::workspace_book::{
    BookParseError, CreateOutcome, DEFAULT_WORKSPACE, DeleteOutcome, MAX_WORKSPACE_NAME,
    RenameOutcome, SaveOutcome, SeedTab, SetOutcome, TabWorkspaceState, UNDO_WINDOW_MS,
    UndoOutcome, UnsavedEdits, Unsynced, WORKSPACES_FILE, WorkspaceBook, WorkspaceListing,
    WorkspaceRefusal, capture_recipe, parse_book,
};
pub use quantick_workspace::workspace_colour::{
    ColourOutcome, ColourTaken, Rgb, WorkspaceColour, hex, hsv_rgb, parse_typed_hex, rgb_hsv,
};

pub mod palette;
pub use quantick_workspace::workspace_recipe::{ContextPaneRecipe, WorkspaceRecipe};
pub use quantick_workspace::workspace_sync::{TabAdoption, TabRecipeSync};

/// A toolbar workspace button's fill on `theme`: its control fill tinted
/// toward the workspace's colour, deeper while `lit` (hovered or open). The
/// name keeps 4.5:1 on both for every preset; the border's 3:1 holds at rest
/// only, as a deeper tint closes on the colour.
#[must_use]
pub fn capsule_fill(colour: WorkspaceColour, theme: &ResolvedTheme, lit: bool) -> [u8; 4] {
    let base = theme.color(Token::Control).to_array();
    let share = if lit {
        tab_strip::WORKSPACE_HOVER_TINT
    } else {
        tab_strip::WORKSPACE_TINT
    };
    tab_strip::workspace_tint(base, colour.rgb(theme.dark), share)
}

/// The open tab's symbol ink on `theme`: the workspace's colour while it keeps
/// [`tab_strip::WORKSPACE_SYMBOL_CONTRAST`] on the tab's sheet, else `None`,
/// the tab's normal ink.
#[must_use]
pub fn symbol_ink(colour: WorkspaceColour, theme: &ResolvedTheme) -> Option<Rgb> {
    let [r, g, b] = colour.rgb(theme.dark);
    let sheet = theme.color(tab_strip::tab_fill(true));
    let ratio = contrast_ratio(PremultipliedRgba::from_rgb(r, g, b), sheet);
    (ratio >= tab_strip::WORKSPACE_SYMBOL_CONTRAST).then_some([r, g, b])
}

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
    drawing_reservation: Cell<(u64, Option<Instant>)>,
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
            drawing_reservation: Cell::new((0, None)),
        }
    }

    #[must_use]
    pub fn book(&self) -> &WorkspaceBook {
        &self.book
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn book_mut(&mut self) -> &mut WorkspaceBook {
        &mut self.book
    }

    /// Upgrade stable identities before moving legacy drawings into those scopes.
    pub fn bind_drawings(
        &self,
        layouts: &mut quantick_workspace::layout_store::LayoutStore,
        now: Instant,
    ) {
        if self.ensure_drawing_identities(now).is_ok() {
            crate::market_settings::migrate_workspace_scopes(layouts, &self.book, now);
        }
    }

    fn ensure_drawing_identities(&self, now: Instant) -> Result<bool, String> {
        if self.drawing_reservation.get().0 >= self.book.workspace_id_highwater() {
            return Ok(false);
        }
        self.persist().inspect_err(|_| {
            self.drawing_reservation.set((0, Some(now)));
        })?;
        Ok(true)
    }

    /// Reserve workspace identities and layout IDs before publishing a scoped drawing identity.
    ///
    /// # Errors
    /// A workspace or layout identity could not be made durable.
    pub fn reserve_drawing_identities(
        &self,
        layouts: &mut quantick_workspace::layout_store::LayoutStore,
        now: Instant,
    ) -> Result<(), String> {
        if self.ensure_drawing_identities(now)? {
            crate::market_settings::migrate_workspace_scopes(layouts, &self.book, now);
        }
        crate::market_settings::reserve_layout_identities(layouts, now)
    }

    /// Read or change a scoped drawing preference after reserving its owners.
    ///
    /// # Errors
    /// A required identity or drawing preference write failed.
    pub fn drawing_autosave(
        &self,
        layouts: &mut quantick_workspace::layout_store::LayoutStore,
        key: &crate::market_settings::DrawingKey,
        on: Option<bool>,
        now: Instant,
    ) -> Result<(bool, bool), String> {
        if on.is_some() {
            self.reserve_drawing_identities(layouts, now)?;
        }
        crate::market_settings::drawing_autosave(layouts, key, on, now)
    }

    /// Publish an alert target only after its complete owner chain is durable.
    ///
    /// # Errors
    /// An owner reservation, drawing write or exact identity verification failed.
    pub fn commit_drawing_identity(
        &self,
        layouts: &mut quantick_workspace::layout_store::LayoutStore,
        key: &crate::market_settings::DrawingKey,
        identity: &quantick_workspace::drawing_identity::DurableDrawingRef,
        now: Instant,
    ) -> Result<(), crate::market_settings::TargetResolveError> {
        self.reserve_drawing_identities(layouts, now)
            .map_err(crate::market_settings::TargetResolveError::WriteFailed)?;
        crate::market_settings::commit_identity(layouts, key, identity, now)
    }

    /// Write drawings under the independent configuration and identity gates.
    pub fn persist_drawings(
        &self,
        layouts: &mut quantick_workspace::layout_store::LayoutStore,
        now: Instant,
        immediate: bool,
    ) {
        if layouts.set_configuration_autosave(self.autosave()) && layouts.configuration_changed() {
            layouts.mark_changed(now);
        }
        let retry_waits = self.drawing_reservation.get().1.is_some_and(|failed| {
            now.saturating_duration_since(failed)
                < quantick_workspace::layout_commit::LAYOUTS_SAVE_DEBOUNCE
        });
        if !immediate && retry_waits {
            return;
        }
        match self.ensure_drawing_identities(now) {
            Ok(true) => crate::market_settings::migrate_workspace_scopes(layouts, &self.book, now),
            Ok(false) => {}
            Err(_) => return,
        }
        if immediate {
            crate::market_settings::flush(layouts, now);
        } else {
            crate::market_settings::save_settled(layouts, now);
        }
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

    /// The workspace this tab belongs to, as last saved.
    pub fn saved_recipe(&self, tab: u64) -> Option<&WorkspaceRecipe> {
        self.book.recipe(&self.tab_row(tab).0)
    }

    /// Live comparison used before the periodic workspace upkeep has run.
    pub fn unsaved(&self, tab: u64, live: Option<WorkspaceRecipe>) -> bool {
        let differs = self
            .saved_recipe(tab)
            .zip(live)
            .is_some_and(|(saved, live)| !saved.holds(&live));
        differs && !self.autosave()
    }

    pub fn canvas_layout(
        &self,
        tab: u64,
        state: &quantick_workspace::canvas::CanvasState,
    ) -> (String, bool, quantick_workspace::canvas::LayoutMarks) {
        let (workspace, unsaved) = self.tab_row(tab);
        let saved = self
            .book
            .recipe(&workspace)
            .map(quantick_workspace::canvas_layout::recipe_canvas);
        (
            workspace,
            unsaved,
            quantick_workspace::canvas::LayoutMarks::of(state, saved.as_ref(), unsaved),
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

    /// Each open tab's workspace, for the layout session to share copies by:
    /// `quantick_workspace::book_upkeep::tab_holdings`.
    pub fn tab_holdings(&self) -> impl Iterator<Item = (u64, &str, bool)> + Clone {
        quantick_workspace::book_upkeep::tab_holdings(self.parts())
    }

    /// Before a save: `quantick_workspace::book_upkeep::take_shown_sets`.
    pub fn take_shown_sets(&mut self, tab: u64) {
        quantick_workspace::book_upkeep::take_shown_sets(&mut self.book, &self.sync, tab);
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
        self.drawing_reservation.set((0, None));
    }

    /// Follow a layouts file moved to another folder.
    pub fn move_beside(&mut self, layouts: &Path) {
        self.path = path_beside(layouts);
        self.drawing_reservation.set((0, None));
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
        save(&self.path, &self.book)?;
        self.drawing_reservation
            .set((self.book.workspace_id_highwater(), None));
        Ok(())
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
    fn live_canvas_dirty_state_uses_the_save_and_autosave_policy() {
        let dir = ScratchDir::new("canvas-dirty-owner");
        let mut store = WorkspaceBookStore::open_beside(&dir.join("layouts.toml"));
        store.book_mut().open_tab(7);
        store.book_mut().save_as(7, "Scalp").unwrap();
        let saved = store.saved_recipe(7).unwrap().clone();
        assert_eq!(store.saved_recipe(7), store.book().recipe("Scalp"));
        store.set_autosave(false);
        assert!(!store.unsaved(7, None));
        assert!(!store.unsaved(7, Some(saved.clone())));
        let mut changed = saved;
        changed.canvas = Some(quantick_workspace::canvas::CanvasState::from_preset(
            quantick_workspace::canvas::preset("charts-4").unwrap(),
        ));
        assert!(store.unsaved(7, Some(changed.clone())));
        store.set_autosave(true);
        assert!(!store.unsaved(7, Some(changed)));
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
    fn drawing_writes_wait_for_workspace_identity_reservation_and_retry() {
        use quantick_workspace::layout_document::LayoutBook;
        use quantick_workspace::layout_store::LayoutStore;
        use quantick_workspace::market_settings::DrawingKey;
        let dir = ScratchDir::new("workspace-drawing-identity-retry");
        let path = dir.join("layouts.toml");
        let mut layouts = LayoutStore::new(LayoutBook::default(), path.clone(), false);
        let book = WorkspaceBookStore::open_beside(&path);
        let obstacle = book.path().with_extension("toml.tmp");
        std::fs::create_dir(&obstacle).unwrap();
        let now = Instant::now();
        book.bind_drawings(&mut layouts, now);
        let scope = book
            .book()
            .drawing_scope(1, layouts.book().active_id())
            .unwrap();
        let key = DrawingKey::of_pane("binance", "BTCUSDT".to_owned(), None).in_scope(scope);
        layouts.market_mut().set_drawing_autosave(&key, false, now);
        book.persist_drawings(&mut layouts, now, true);
        assert!(!layouts.market().path().exists());
        assert!(layouts.market().is_dirty());
        assert!(book.reserve_drawing_identities(&mut layouts, now).is_err());
        std::fs::remove_dir(obstacle).unwrap();
        book.persist_drawings(
            &mut layouts,
            now + quantick_workspace::layout_commit::LAYOUTS_SAVE_DEBOUNCE,
            false,
        );
        let saved = crate::market_settings::parse_settings(
            &std::fs::read_to_string(layouts.market().path()).unwrap(),
        )
        .unwrap();
        assert!(!saved.drawing_autosave(&key));
        assert_eq!(
            load(book.path()).0.workspace_id("Default"),
            Some(scope.workspace)
        );
        assert!(!layouts.market().is_dirty());
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
            drawing_reservation: Cell::new((0, None)),
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

#[cfg(test)]
mod colour_ink_tests;
