//! The app's handle on the cockpit's stores: where each one lives this run,
//! and whether what is on screen has reached it yet.
//!
//! **This module writes no file format.** That is the line that keeps it from
//! being a fourth description of everything quantick remembers, beside the
//! three that already overlap. [`crate::ui_state`] owns the shape of
//! `ui-state.toml`, [`crate::layouts`] owns `layouts.toml`,
//! [`crate::workspace_bundle`] owns the bundle spanning both, and
//! [`crate::store_home`] owns where each of them resolves to. Every write this
//! module authorises is a call into one of those; it serialises nothing
//! itself, and a `Serialize` derive appearing here would mean the split had
//! been lost.
//!
//! What it adds is the layer none of them has: the state *between* the file
//! and the frame. Those four are file modules — a format, a `load`, a `save`.
//! None of them holds anything the app carries from one frame to the next, so
//! before this module all of it sat as loose fields on `QuantickApp`: six
//! paths with no common owner, a dirty flag, a clock, a blocked flag, and the
//! Workspace menu's cached answers about disk.
//!
//! **The invariant this exists for.** `layouts_dirty`, `last_layout_change`
//! and `layouts_save_blocked` were three independent fields carrying one rule
//! between them. Any method on the trunk could set the first and forget to
//! stamp the second — and a change with no timestamp is a change the debounce
//! never releases, so the file simply stops being written and nothing says so.
//! The save condition itself was re-derived at each of the two call sites that
//! needed it. Here the three are private to [`LayoutStore`], the only way to
//! record a change also stamps the clock, and the decision is one function.
//!
//! **The clock is a parameter.** [`LayoutStore::take_save`] is told what time
//! it is rather than reading it, the way [`crate::window_scale::SurfaceEnv`]
//! takes its `now` and `quantick-replay` is told how much time passed. That is
//! what makes the debounce testable without a window: the tests at the foot of
//! this file drive it across the boundary with no filesystem and no egui.
//!
//! **Paths arrive resolved, and are never resolved here.** Each store decides
//! its own location — an explicit `QUANTICK_*` ask, then the durable home,
//! then the launch directory ([`crate::store_home::resolve`]) — and hands the
//! answer in. This module never reads an environment variable and never calls
//! `resolve`, so no path becomes implicit by moving: a test pointing a store
//! at a scratch file still gets its scratch file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::layouts::LayoutBook;
use crate::ui_state::NamedArrangement;

/// How long after the last layout change the file is written.
///
/// A layout edit is rarely alone — dragging a level, retuning an indicator and
/// renaming a tab arrive as a burst — so the write waits for the burst to
/// settle rather than firing per keystroke. It is not a *deadline*: the exit
/// path ([`LayoutStore::take_flush`]) ignores it entirely, so nothing is ever
/// lost to a window that had not elapsed when the window closed.
pub(crate) const LAYOUTS_SAVE_DEBOUNCE: Duration = Duration::from_millis(1_000);

/// What the caller owes the layouts file, decided in one place.
///
/// Three answers rather than a boolean, because the blocked case is not
/// "don't save" — it is "consume the change and say out loud that it went
/// nowhere". A `should_persist` returning `false` while blocked would leave
/// the change pending forever and silence the warning the trader needs to see,
/// which is why the decision and the consumption are the same call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutSave {
    /// Nothing to write, or the debounce window has not elapsed. The pending
    /// change, if there is one, is left pending.
    Wait,
    /// Write the book to the layouts path. The change is consumed.
    Write,
    /// There is a change, and this session may not write the file. The change
    /// is consumed and the caller reports it; see [`LayoutStore::set_blocked`].
    Blocked,
}

/// The layout book, and the one rule that decides when it reaches disk.
///
/// The three fields below are private and stay that way. `dirty` without
/// `last_change` is a change the debounce can never release; the only
/// constructor of that pair is [`Self::mark_changed`], so the pair cannot come
/// apart.
pub(crate) struct LayoutStore {
    /// The workspace's layouts: the strip's tabs, their indicator sets and
    /// their per-market drawings. See [`crate::layouts`].
    book: LayoutBook,
    /// Where the layouts persist. Handed in, never resolved here.
    path: PathBuf,
    /// Set by any layout edit — a switch, a rename, a drawing, a settled
    /// indicator change; drained by the debounced save.
    dirty: bool,
    /// When the last layout change happened (the debounce clock).
    last_change: Option<Instant>,
    /// Whether the layouts file may be written this session. `true` only when
    /// the file was there at launch, could not be read, and could not be set
    /// aside — the trader's only copy, which this session's empty book must
    /// never replace.
    blocked: bool,
}

impl LayoutStore {
    /// A store over a book already loaded, at a path already resolved.
    ///
    /// `blocked` comes from the load: it is the "could not read it and could
    /// not set it aside" answer, which only the loader can give.
    pub(crate) fn new(book: LayoutBook, path: PathBuf, blocked: bool) -> Self {
        Self {
            book,
            path,
            dirty: false,
            last_change: None,
            blocked,
        }
    }

    /// The book, for the strip, the menu and the control plane to read.
    pub(crate) fn book(&self) -> &LayoutBook {
        &self.book
    }

    /// The book, for the edits that change it.
    ///
    /// Handing out `&mut` does not weaken the invariant: the invariant is
    /// about the *flags*, and an edit that forgets [`Self::mark_changed`] is
    /// the same forgotten save it always was — visible in one place rather
    /// than derivable from three.
    pub(crate) fn book_mut(&mut self) -> &mut LayoutBook {
        &mut self.book
    }

    /// Where the layouts persist.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Note that the file could not be read at launch, so this session's book
    /// must not replace it.
    pub(crate) fn set_blocked(&mut self, blocked: bool) {
        self.blocked = blocked;
    }

    /// Replace the book wholesale — a workspace bundle landing, or a reset.
    ///
    /// Deliberately silent about the flags. The caller has work to do between
    /// putting the book in place and knowing what it owes the file (it seeds
    /// panes, which marks changes of its own), so the pending state is stated
    /// afterwards by [`Self::settle`] rather than guessed at here.
    pub(crate) fn set_book(&mut self, book: LayoutBook) {
        self.book = book;
    }

    /// State outright what the book owes the file, overriding anything marked
    /// while it was being put in place.
    ///
    /// The screen is the file's after an import; nothing has changed since —
    /// unless the book was made from an imported indicator set, which the file
    /// does not hold yet. Either way this is the last word, which is why it
    /// clears as readily as it sets.
    pub(crate) fn settle(&mut self, changed: bool, now: Instant) {
        self.dirty = changed;
        self.last_change = changed.then_some(now);
    }

    /// Record that the book changed, and when.
    ///
    /// The flag and the clock move together or not at all. That is the whole
    /// point of this type.
    pub(crate) fn mark_changed(&mut self, now: Instant) {
        self.dirty = true;
        self.last_change = Some(now);
    }

    /// Whether an edit is waiting for the debounce.
    ///
    /// Test-only. Nothing in the running app asks: the whole point of
    /// [`Self::take_save`] is that the question and the answer to it are one
    /// call, so a caller that could read the flag separately could also act on
    /// a stale reading of it.
    #[cfg(test)]
    pub(crate) fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The frame's question: is there a change, and has it settled?
    ///
    /// `Wait` leaves the change pending. `Write` and `Blocked` both consume
    /// it, which is what the pre-existing `save_layouts_now` did — it cleared
    /// both flags before it checked whether it was allowed to write.
    pub(crate) fn take_save(&mut self, now: Instant) -> LayoutSave {
        let settled = self
            .last_change
            .is_some_and(|changed| now.duration_since(changed) >= LAYOUTS_SAVE_DEBOUNCE);
        if !settled {
            return LayoutSave::Wait;
        }
        self.take()
    }

    /// The way out on exit, and the moment before a bundle export reads the
    /// file: write now, whatever the debounce says.
    pub(crate) fn take_flush(&mut self) -> LayoutSave {
        self.take()
    }

    /// The half both questions share: consume a pending change and say where
    /// it goes. Split out so `Blocked` can never disagree with `Write` about
    /// what was consumed.
    fn take(&mut self) -> LayoutSave {
        if !self.dirty {
            return LayoutSave::Wait;
        }
        self.dirty = false;
        self.last_change = None;
        if self.blocked {
            LayoutSave::Blocked
        } else {
            LayoutSave::Write
        }
    }
}

/// Where the cockpit's stores live this run.
///
/// Six paths, each resolved by its own module and handed in. This struct is a
/// carrier, deliberately without behaviour: the moment it grew a `resolve` it
/// would become a second answer to a question [`crate::store_home`] already
/// answers, and the two would drift.
pub(crate) struct StorePaths {
    /// Where the picker's added instruments persist. See
    /// [`crate::symbols_file`].
    pub(crate) symbols: PathBuf,
    /// Where layer visibility persists.
    pub(crate) chart_layers: PathBuf,
    /// Where the footprint's live edits persist.
    pub(crate) footprint_settings: PathBuf,
    /// Where the settings dialog's named input setups persist.
    pub(crate) indicator_presets: PathBuf,
    /// Where the workspace persists.
    pub(crate) ui_state: PathBuf,
}

/// What the chart-layer file already says, so a switch is written once.
///
/// The mask is compared with the live one every frame, so a switch is saved
/// whoever flipped it — the menu, the toolbar, the dock or the appearance
/// panel — rather than four pieces of chrome each remembering to save. The tab
/// is here so a tab *switch*, which changes the mask with nobody touching a
/// switch, is a re-baseline rather than an edit.
pub(crate) struct SavedLayers {
    mask: u32,
    tab: u64,
}

impl SavedLayers {
    /// The visibility already on disk, as a bitmask over the active tab's flow
    /// pane.
    pub(crate) fn mask(&self) -> u32 {
        self.mask
    }

    /// Which tab [`Self::mask`] was taken from.
    pub(crate) fn tab(&self) -> u64 {
        self.tab
    }

    /// A different chart is answering: take its mask as the new baseline
    /// without treating the difference as a switch the trader flipped.
    pub(crate) fn rebaseline(&mut self, tab: u64, mask: u32) {
        self.tab = tab;
        self.mask = mask;
    }

    /// The mask reached the file; it is the baseline from here.
    pub(crate) fn record(&mut self, mask: u32) {
        self.mask = mask;
    }
}

/// What the Workspace menu knows without asking the filesystem.
///
/// The menu body runs every frame it is open, so a `Path::exists` inside it is
/// a syscall at 60 Hz for an answer that changes only when this app saves,
/// forgets, exports or imports. These fields are that answer, refreshed at
/// those moments instead.
pub(crate) struct WorkspaceSession {
    /// Whether closing the window writes the workspace. Read from the file at
    /// startup and toggled from the Workspace menu.
    save_on_exit: bool,
    /// Whether the rail's pinned tools were staged by
    /// `QUANTICK_TOOL_FAVORITES` rather than chosen by the trader.
    ///
    /// A validation run dresses the rail through that hook to reach a state a
    /// screenshot needs; the stars in it are a costume. Since a star is
    /// written to the workspace the moment it is clicked, a run that toggles
    /// one would otherwise write the harness's list into the trader's real
    /// file. Set once at startup, never cleared: a session that began wearing
    /// a costume never takes it off.
    favorites_are_staged: bool,
    /// The arrangements the trader named and kept, in the order the file lists
    /// them.
    ///
    /// Held across the session because every write of the workspace file
    /// rewrites the whole file: capturing the live window and saving it would
    /// drop the bookmarks on the floor otherwise.
    bookmarks: Vec<NamedArrangement>,
    /// Whether a workspace is on disk, so the menu can disable Reset without
    /// asking the filesystem.
    saved: bool,
    /// Workspace files exported or imported recently, newest first, as the
    /// file remembers them. Carried across the session for the same reason
    /// `bookmarks` is.
    recent: Vec<String>,
    /// Which of them are actually on disk, resolved when the list changes
    /// rather than when the menu is drawn.
    recent_on_disk: Vec<PathBuf>,
}

impl WorkspaceSession {
    /// Whether closing the window writes the workspace.
    pub(crate) fn save_on_exit(&self) -> bool {
        self.save_on_exit
    }

    /// The Workspace menu's own checkbox writes through this.
    pub(crate) fn save_on_exit_mut(&mut self) -> &mut bool {
        &mut self.save_on_exit
    }

    /// Whether the rail's pinned tools are a harness costume rather than the
    /// trader's own choice.
    pub(crate) fn favorites_are_staged(&self) -> bool {
        self.favorites_are_staged
    }

    /// Note that a harness hook dressed the rail this run.
    pub(crate) fn stage_favorites(&mut self) {
        self.favorites_are_staged = true;
    }

    /// The arrangements the trader named and kept.
    pub(crate) fn bookmarks(&self) -> &[NamedArrangement] {
        &self.bookmarks
    }

    /// The same list, for the menu entries that add, replace and forget one.
    pub(crate) fn bookmarks_mut(&mut self) -> &mut Vec<NamedArrangement> {
        &mut self.bookmarks
    }

    /// Whether a workspace is on disk.
    pub(crate) fn saved(&self) -> bool {
        self.saved
    }

    /// A write landed (or did not): a workspace exists from here on if one
    /// already did or this write made one.
    pub(crate) fn note_write(&mut self, written: bool) {
        self.saved |= written;
    }

    /// Set the answer outright — the load, which asks the filesystem once, and
    /// the reset, which knows what it left behind.
    pub(crate) fn set_saved(&mut self, saved: bool) {
        self.saved = saved;
    }

    /// Workspace files exported or imported recently, newest first.
    pub(crate) fn recent(&self) -> &[String] {
        &self.recent
    }

    /// The same list, for `workspace_bundle::remember_recent` to push onto.
    pub(crate) fn recent_mut(&mut self) -> &mut Vec<String> {
        &mut self.recent
    }

    /// Which of the recent files are actually on disk.
    pub(crate) fn recent_on_disk(&self) -> &[PathBuf] {
        &self.recent_on_disk
    }

    /// Re-resolve which recent files exist. Called when the list changes, not
    /// when the menu is drawn.
    pub(crate) fn set_recent_on_disk(&mut self, existing: Vec<PathBuf>) {
        self.recent_on_disk = existing;
    }

    /// Take the workspace-level keys the file just gave up.
    pub(crate) fn adopt(
        &mut self,
        save_on_exit: bool,
        bookmarks: Vec<NamedArrangement>,
        recent: Vec<String>,
    ) {
        self.save_on_exit = save_on_exit;
        self.bookmarks = bookmarks;
        self.recent = recent;
    }
}

/// The app's one handle on where the workspace lives and whether it is saved.
///
/// One field on `QuantickApp` where there were twenty-one. The four parts stay
/// separate inside because they answer different questions and have different
/// lifetimes — a path is fixed for the run, the layout rule changes every
/// edit, the layer baseline changes per tab, the session state changes per
/// menu action — and folding them into one flat bag would lose exactly the
/// structure that makes the layout rule guardable.
pub(crate) struct WorkspaceStore {
    paths: StorePaths,
    layouts: LayoutStore,
    layers: SavedLayers,
    session: WorkspaceSession,
    /// The native file dialog, while one is open, and what it is for. One at a
    /// time, and off the UI thread — the OS dialog never blocks a frame.
    picker: Option<(WorkspacePick, std::sync::mpsc::Receiver<Option<PathBuf>>)>,
    /// Where trades save this run — resolved once at boot (environment > the
    /// user's stored pick > config) and updated by the panel's folder picker;
    /// new tabs journal here too.
    trades_dir: PathBuf,
    /// The in-flight trades-folder dialog, if any. One at a time.
    trades_dir_picker: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
}

/// What the open workspace file dialog is for, so the one poll can land either
/// answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspacePick {
    /// Choosing where to write a workspace file.
    Export,
    /// Choosing a workspace file to open.
    Import,
}

impl WorkspaceStore {
    /// Build the handle from paths each store has already resolved for itself.
    ///
    /// Every argument arrives resolved. Nothing in this module reads an
    /// environment variable or calls [`crate::store_home::resolve`], so a
    /// store pointed at a scratch file by `QUANTICK_*` still writes there.
    pub(crate) fn new(paths: StorePaths, layouts: LayoutStore, trades_dir: PathBuf) -> Self {
        Self {
            paths,
            layouts,
            layers: SavedLayers { mask: 0, tab: 0 },
            session: WorkspaceSession {
                save_on_exit: true,
                favorites_are_staged: false,
                bookmarks: Vec::new(),
                saved: false,
                recent: Vec::new(),
                recent_on_disk: Vec::new(),
            },
            picker: None,
            trades_dir,
            trades_dir_picker: None,
        }
    }

    /// Where the picker's added instruments persist.
    pub(crate) fn symbols_path(&self) -> &Path {
        &self.paths.symbols
    }

    /// Where layer visibility persists.
    pub(crate) fn chart_layers_path(&self) -> &Path {
        &self.paths.chart_layers
    }

    /// Where the footprint's live edits persist.
    pub(crate) fn footprint_settings_path(&self) -> &Path {
        &self.paths.footprint_settings
    }

    /// Where the settings dialog's named input setups persist.
    pub(crate) fn indicator_presets_path(&self) -> &Path {
        &self.paths.indicator_presets
    }

    /// Where the workspace persists.
    pub(crate) fn ui_state_path(&self) -> &Path {
        &self.paths.ui_state
    }

    /// Where the layouts persist.
    pub(crate) fn layouts_path(&self) -> &Path {
        self.layouts.path()
    }

    /// The layout book and the rule that guards it.
    pub(crate) fn layouts(&self) -> &LayoutStore {
        &self.layouts
    }

    /// The same, for the edits and the debounced save.
    pub(crate) fn layouts_mut(&mut self) -> &mut LayoutStore {
        &mut self.layouts
    }

    /// What the chart-layer file already says.
    pub(crate) fn layers(&self) -> &SavedLayers {
        &self.layers
    }

    /// The same, for the per-frame compare that writes a switch down.
    pub(crate) fn layers_mut(&mut self) -> &mut SavedLayers {
        &mut self.layers
    }

    /// What the Workspace menu knows without asking the filesystem.
    pub(crate) fn session(&self) -> &WorkspaceSession {
        &self.session
    }

    /// The same, for the menu actions that change it.
    pub(crate) fn session_mut(&mut self) -> &mut WorkspaceSession {
        &mut self.session
    }

    /// Whether a workspace file dialog is already open. One at a time.
    pub(crate) fn picker_open(&self) -> bool {
        self.picker.is_some()
    }

    /// Hand the in-flight dialog over, so its poll can read the channel.
    pub(crate) fn picker(
        &self,
    ) -> Option<&(WorkspacePick, std::sync::mpsc::Receiver<Option<PathBuf>>)> {
        self.picker.as_ref()
    }

    /// A dialog just opened.
    pub(crate) fn open_picker(
        &mut self,
        intent: WorkspacePick,
        receiver: std::sync::mpsc::Receiver<Option<PathBuf>>,
    ) {
        self.picker = Some((intent, receiver));
    }

    /// The dialog answered, or the channel died.
    pub(crate) fn close_picker(&mut self) {
        self.picker = None;
    }

    /// Point a store at a scratch file for the length of one test.
    ///
    /// Test-only, and deliberately so: in a running app a store's path is
    /// resolved once at construction and never moves, which is what makes
    /// "where does this write?" answerable from the launch alone. A test needs
    /// the opposite — it builds an app, then redirects one store at the
    /// temporary file it is about to inspect — and that is the only reason a
    /// setter exists at all. It compiles out of the shipped binary.
    #[cfg(test)]
    pub(crate) fn set_ui_state_path(&mut self, path: PathBuf) {
        self.paths.ui_state = path;
    }

    /// See [`Self::set_ui_state_path`].
    #[cfg(test)]
    pub(crate) fn set_symbols_path(&mut self, path: PathBuf) {
        self.paths.symbols = path;
    }

    /// See [`Self::set_ui_state_path`].
    #[cfg(test)]
    pub(crate) fn set_chart_layers_path(&mut self, path: PathBuf) {
        self.paths.chart_layers = path;
    }

    /// See [`Self::set_ui_state_path`].
    #[cfg(test)]
    pub(crate) fn set_layouts_path(&mut self, path: PathBuf) {
        self.layouts.path = path;
    }

    /// Where trades save this run.
    pub(crate) fn trades_dir(&self) -> &Path {
        &self.trades_dir
    }

    /// The panel's folder picker chose a new one.
    pub(crate) fn set_trades_dir(&mut self, dir: PathBuf) {
        self.trades_dir = dir;
    }

    /// Whether a trades-folder dialog is already open. One at a time.
    pub(crate) fn trades_dir_picker_open(&self) -> bool {
        self.trades_dir_picker.is_some()
    }

    /// Hand the in-flight folder dialog over, so its poll can read the
    /// channel.
    pub(crate) fn trades_dir_picker(&self) -> Option<&std::sync::mpsc::Receiver<Option<PathBuf>>> {
        self.trades_dir_picker.as_ref()
    }

    /// A folder dialog just opened.
    pub(crate) fn open_trades_dir_picker(
        &mut self,
        receiver: std::sync::mpsc::Receiver<Option<PathBuf>>,
    ) {
        self.trades_dir_picker = Some(receiver);
    }

    /// The folder dialog answered, or the channel died.
    pub(crate) fn close_trades_dir_picker(&mut self) {
        self.trades_dir_picker = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store with a book, at a path no test ever writes to, blocked or not.
    fn store(blocked: bool) -> LayoutStore {
        LayoutStore::new(
            LayoutBook::default(),
            PathBuf::from("layouts.toml"),
            blocked,
        )
    }

    #[test]
    fn a_change_inside_the_debounce_window_is_not_yet_asked_for() {
        let mut layouts = store(false);
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE - Duration::from_millis(1)),
            LayoutSave::Wait,
            "a change one millisecond short of the window must not reach the file"
        );
        assert!(
            layouts.is_dirty(),
            "a change the debounce held back is still pending, not consumed"
        );
    }

    #[test]
    fn a_change_that_has_settled_is_asked_for_once() {
        let mut layouts = store(false);
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Write,
            "the window's own edge releases the change"
        );
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Wait,
            "a change that reached the file is not written a second time"
        );
    }

    #[test]
    fn a_blocked_store_never_asks_to_write() {
        let mut layouts = store(true);
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Blocked,
            "a session that could not read the file must not write over it"
        );
        layouts.mark_changed(changed);
        assert_eq!(
            layouts.take_flush(),
            LayoutSave::Blocked,
            "not even the exit flush, which ignores the debounce, may write it"
        );
    }

    #[test]
    fn the_exit_flush_ignores_the_debounce_but_not_the_absence_of_a_change() {
        let mut layouts = store(false);
        assert_eq!(
            layouts.take_flush(),
            LayoutSave::Wait,
            "nothing changed, so exiting writes nothing"
        );
        layouts.mark_changed(Instant::now());
        assert_eq!(
            layouts.take_flush(),
            LayoutSave::Write,
            "a change still inside the window is written on the way out, not lost"
        );
    }

    #[test]
    fn marking_a_change_moves_the_flag_and_the_clock_together() {
        let mut layouts = store(false);
        assert!(!layouts.is_dirty());
        let changed = Instant::now();
        layouts.mark_changed(changed);
        assert!(layouts.is_dirty());
        // The clock is not readable from outside; that it was stamped is
        // proven by the debounce releasing on time rather than never.
        assert_eq!(
            layouts.take_save(changed + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Write,
            "a change recorded without its timestamp would never settle"
        );
    }

    #[test]
    fn settling_a_replaced_book_overrides_what_landing_it_marked() {
        let mut layouts = store(false);
        let now = Instant::now();
        layouts.set_book(LayoutBook::default());
        // Putting the book in place seeds panes, and seeding marks changes.
        layouts.mark_changed(now);
        layouts.settle(false, now);
        assert_eq!(
            layouts.take_save(now + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Wait,
            "an import that changed nothing must not write the file back, whatever              seeding its panes marked on the way"
        );
        layouts.settle(true, now);
        assert_eq!(
            layouts.take_save(now + LAYOUTS_SAVE_DEBOUNCE),
            LayoutSave::Write,
            "an import that migrated something owes the file a write"
        );
    }

    #[test]
    fn a_tab_switch_rebaselines_the_layers_rather_than_recording_a_switch() {
        let mut layers = SavedLayers {
            mask: 0b101,
            tab: 1,
        };
        layers.rebaseline(2, 0b010);
        assert_eq!(layers.tab(), 2);
        assert_eq!(
            layers.mask(),
            0b010,
            "the other tab's opinion becomes the baseline, not an edit to write"
        );
        layers.record(0b011);
        assert_eq!(layers.mask(), 0b011);
        assert_eq!(
            layers.tab(),
            2,
            "recording a mask does not change whose it is"
        );
    }
}
