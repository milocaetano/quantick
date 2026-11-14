//! The workspace menus' state, apart from their pixels: the status bar cell's
//! menu, the Manage workspaces dialog whose delete asks twice, the dialog that
//! asks for a name and the close that asks per tab holding edits.

use std::collections::BTreeMap;

use crate::close_queue::{CloseAnswer, CloseQueue, CloseScope};

/// What a workspace row or the dialog asked the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCommand {
    /// Put the active tab on this workspace.
    Set(String),
    /// Write the active tab's edits into its workspace.
    SaveTo,
    /// Turn "auto save layout changes" on or off.
    SetAutosave(bool),
    BeginSaveAs,
    /// Ask the name of a new, empty workspace. Nothing changes before Enter.
    BeginNew,
    /// Ask a new name for this workspace.
    BeginRename(String),
    /// Open the Manage workspaces dialog.
    BeginManage,
    Delete(String),
    /// The dialog's Enter.
    SubmitDialog,
    /// Close these tabs, or exit, asking first per tab holding edits.
    Close(CloseScope, Vec<u64>),
    /// The close prompt's answer.
    AnswerClose(CloseAnswer),
}

/// What the name dialog is asking for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameDialogMode {
    SaveAsNew,
    /// An empty recipe, made and applied only on Enter.
    New,
    Rename {
        from: String,
    },
}

/// The dialog that asks for a workspace name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameDialog {
    /// The tab whose menu asked: the dialog shows in that tab's window and
    /// Enter acts on that tab, whichever window has the focus.
    pub tab: u64,
    pub mode: NameDialogMode,
    pub name: String,
    /// Why the last Enter was refused, shown under the field.
    pub refusal: Option<String>,
    /// The field takes the keyboard on the dialog's first frame, and again
    /// after a refusal, so the trader retypes without reaching for the mouse.
    pub wants_focus: bool,
}

/// What a click on a row's Delete meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteClick {
    /// The first click: the row now states the outcome and asks.
    Armed,
    /// The second click on the same workspace.
    Confirmed,
}

/// The Manage workspaces dialog: every workspace, with its Rename and Delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManageDialog {
    /// The tab whose menu opened it: the dialog shows in that tab's window.
    pub tab: u64,
    /// The workspace whose Delete asks for its confirmation.
    pub armed: Option<String>,
}

impl ManageDialog {
    /// A click on `name`'s Delete. Arming another workspace disarms the first,
    /// so no confirmation ever lands on a name it did not read.
    pub fn click_delete(&mut self, name: &str) -> DeleteClick {
        if self.armed.as_deref() == Some(name) {
            self.armed = None;
            return DeleteClick::Confirmed;
        }
        self.armed = Some(name.to_owned());
        DeleteClick::Armed
    }
}

/// The menus' state across frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceMenuState {
    /// The tab whose cell opened the menu, while it is open. The menu shows
    /// and acts in the window presenting that tab, so a torn-off window's
    /// cell never reaches another window's tab.
    pub chip_menu: Option<u64>,
    /// How the open menu is measured, afresh on every opening.
    pub chip_sizing: ChipSizing,
    pub manage: Option<ManageDialog>,
    pub dialog: Option<NameDialog>,
    pub close: CloseQueue,
}

impl WorkspaceMenuState {
    /// The cell of `tab` was clicked, or its shortcut pressed: open the menu
    /// on that tab, or close it when it was already open there.
    pub fn toggle_chip_menu(&mut self, tab: u64) {
        self.chip_menu = (self.chip_menu != Some(tab)).then_some(tab);
        self.chip_sizing = ChipSizing::default();
    }

    pub fn close_chip_menu(&mut self) {
        self.chip_menu = None;
    }

    /// Open the Manage workspaces dialog in the window presenting `tab`.
    pub fn open_manage(&mut self, tab: u64) {
        self.manage = Some(ManageDialog { tab, armed: None });
    }

    pub fn close_manage(&mut self) {
        self.manage = None;
    }

    /// Open a surface on `tab` as a capture asks (`QUANTICK_WORKSPACE_MENU`):
    /// `cell` the cell's menu, `manage` the dialog, `manage:delete=<name>`
    /// with that row's Delete armed, `manage:rename=<name>` with its Rename
    /// asked over it. Answers false, opening nothing, for any other token.
    pub fn stage(&mut self, tab: u64, token: &str) -> bool {
        let (surface, row) = token.split_once(':').unwrap_or((token, ""));
        match (surface, row.split_once('=')) {
            ("cell", None) => self.toggle_chip_menu(tab),
            ("manage", None) => self.open_manage(tab),
            ("manage", Some(("delete", name))) => {
                self.open_manage(tab);
                if let Some(dialog) = &mut self.manage {
                    dialog.click_delete(name);
                }
            }
            ("manage", Some(("rename", name))) => {
                self.open_manage(tab);
                self.open_rename(tab, name);
            }
            _ => return false,
        }
        true
    }

    /// The window presenting `presented` (`None` on Home) holds `window`'s
    /// tabs. A menu open on one of them that is no longer the one presented
    /// closes, so it never pops back open when its tab returns. A menu on
    /// another window's tab is that window's business.
    pub fn follow_window(&mut self, presented: Option<u64>, window: &[u64]) {
        if let Some(open) = self.chip_menu
            && window.contains(&open)
            && presented != Some(open)
        {
            self.close_chip_menu();
        }
    }

    /// One frame of the cell in the window presenting `tab` among `window`'s
    /// tabs; `shown` is false on Home, where the cell is absent. A click or
    /// the shortcut (`toggled`) counts only where the cell stands. Answers
    /// whether the menu is open on `tab`.
    pub fn follow_cell(&mut self, tab: u64, shown: bool, window: &[u64], toggled: bool) -> bool {
        self.follow_window(shown.then_some(tab), window);
        if shown && toggled {
            self.toggle_chip_menu(tab);
        }
        self.chip_menu_on(tab)
    }

    /// Whether the menu is open on `tab`.
    #[must_use]
    pub fn chip_menu_on(&self, tab: u64) -> bool {
        self.chip_menu == Some(tab)
    }

    fn open_dialog(&mut self, tab: u64, mode: NameDialogMode, name: String) {
        self.dialog = Some(NameDialog {
            tab,
            mode,
            name,
            refusal: None,
            wants_focus: true,
        });
    }

    pub fn open_save_as(&mut self, tab: u64) {
        self.open_dialog(tab, NameDialogMode::SaveAsNew, String::new());
    }

    /// Asks the new workspace's name first; Esc closes it with nothing made.
    pub fn open_new(&mut self, tab: u64) {
        self.open_dialog(tab, NameDialogMode::New, String::new());
    }

    /// Opened on the current name, which is what a rename edits.
    pub fn open_rename(&mut self, tab: u64, from: &str) {
        self.open_dialog(
            tab,
            NameDialogMode::Rename {
                from: from.to_owned(),
            },
            from.to_owned(),
        );
    }

    /// Enter was refused: the dialog stays up, says why and keeps the keyboard.
    pub fn refuse(&mut self, reason: String) {
        if let Some(dialog) = &mut self.dialog {
            dialog.refusal = Some(reason);
            dialog.wants_focus = true;
        }
    }

    pub fn close_dialog(&mut self) {
        self.dialog = None;
    }
}

/// The widest the cell's menu draws, in points: a workspace name of about
/// twenty characters beside its one-line summary. A longer name is cut short
/// in its row and reads whole on hover.
pub const CHIP_MENU_MAX_WIDTH_PT: f32 = 360.0;
/// The narrowest the cell's menu draws, in points.
pub const CHIP_MENU_MIN_WIDTH_PT: f32 = 200.0;
/// How far below the capsule the menu hangs, in points.
pub const CHIP_MENU_DROP_PT: f32 = 4.0;
/// The unsaved dot's centre in from the Save row's left edge, in points:
/// inside the row, in the padding before its text.
pub const CHIP_MENU_DOT_INSET_PT: f32 = 5.0;
/// What the menu keeps clear of the window's edges, in points: room for the
/// small differences between the measured rows and the drawn ones.
pub const CHIP_MENU_EDGE_PT: f32 = 8.0;
/// The workspace rows the list keeps in view however short the window. Below
/// that the menu overruns the window and is pushed up rather than show none.
pub const CHIP_MENU_MIN_LIST_ROWS: usize = 3;
/// The most workspace rows the list shows at once, fewer in a short window;
/// the rest scroll, so a long list never makes the menu a tall panel.
pub const CHIP_MENU_MAX_LIST_ROWS: usize = 12;
/// The rows that never scroll: auto save and its line, Save, Save as,
/// Rename, the WORKSPACES caption, New and Manage.
pub const CHIP_MENU_FIXED_ROWS: usize = 8;
/// The least of a row's caption worth showing cut short, in points; with
/// less room beside the whole name the caption is dropped.
pub const CHIP_MENU_MIN_CAPTION_PT: f32 = 24.0;
/// The separators between the menu's groups.
pub const CHIP_MENU_SEPARATORS: usize = 3;
/// The height egui gives a horizontal separator, before the gap after it.
pub const CHIP_MENU_SEPARATOR_PT: f32 = 6.0;

/// How the open cell menu is measured. A UI sizes the menu from its last
/// frame, so the frame after an opening, or after its rows' words change,
/// lays every row out whole and unseen, no wider than the floor but for what
/// a row asks, so a rule spanning the menu cannot carry an old width. The
/// next frame settles at the capped width, unseen too, so the first frame
/// shown is placed by the width it shows. The frames after it draw, and a
/// row whose whole width does not fit its room has its name cut short.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChipSizing {
    /// The signature of the rows last measured; `None` before any frame.
    measured: Option<u64>,
    pass: ChipPass,
    /// Each row's whole width, in points rounded up, by scene key.
    whole: BTreeMap<String, u32>,
}

/// Which frame of the open menu this is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum ChipPass {
    /// Unseen: every row laid out whole.
    Measure,
    /// Unseen: laid out at the capped width.
    Settle,
    #[default]
    Draw,
}

impl ChipSizing {
    /// A frame of the menu begins for rows whose signature is `rows`:
    /// answers whether it measures, and whether it is shown.
    pub fn frame(&mut self, rows: u64) -> (bool, bool) {
        self.pass = if self.measured != Some(rows) {
            self.measured = Some(rows);
            self.whole.clear();
            ChipPass::Measure
        } else if self.pass == ChipPass::Measure {
            ChipPass::Settle
        } else {
            ChipPass::Draw
        };
        (self.measuring(), self.pass == ChipPass::Draw)
    }

    /// Whether this frame lays the rows out whole.
    #[must_use]
    pub fn measuring(&self) -> bool {
        self.pass == ChipPass::Measure
    }

    /// A row under `key` that asked `desired_pt`: on the measuring frame its
    /// whole width, kept, answering false; after it, whether that whole
    /// width is more than `room_pt`, so its name is cut short.
    pub fn cut(&mut self, key: &str, desired_pt: f32, room_pt: f32) -> bool {
        if self.measuring() {
            self.whole.insert(key.to_owned(), desired_pt.ceil() as u32);
            return false;
        }
        self.whole
            .get(key)
            .is_some_and(|&whole| whole as f32 > room_pt.ceil())
    }
}
/// How a press row's caption shares the row with its name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaptionFit {
    /// Both fit whole: the row draws as it always has.
    Whole,
    /// The caption cut short to this many points beside the whole name.
    Cut(f32),
    /// No room for a caption: the name takes the row, cut short only when
    /// it alone is wider.
    Dropped,
}

/// Fit a row's `name_pt` wide name and `caption_pt` wide caption, `gap_pt`
/// apart, into `room_pt`, `None` for a row that extends to what it asks:
/// the caption gives way first, cut short to what the whole name leaves and
/// then dropped, so the name keeps its width and two names that differ only
/// at their end stay apart in the cell menu and the window menu's Workspace
/// submenu alike.
#[must_use]
pub fn fit_caption(room_pt: Option<f32>, name_pt: f32, gap_pt: f32, caption_pt: f32) -> CaptionFit {
    let left = room_pt.unwrap_or(f32::INFINITY) - name_pt - gap_pt;
    if caption_pt <= 0.0 || caption_pt <= left {
        CaptionFit::Whole
    } else if left >= CHIP_MENU_MIN_CAPTION_PT {
        CaptionFit::Cut(left)
    } else {
        CaptionFit::Dropped
    }
}

/// A rectangle in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl MenuBounds {
    #[must_use]
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

/// The heights the menu's style draws, in points, separators and the frame
/// included: what turns row counts into a height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuMetrics {
    /// One row and the gap after it.
    pub row_pt: f32,
    /// One separator and the gap after it.
    pub separator_pt: f32,
    /// The frame's margins, top and bottom together.
    pub frame_pt: f32,
}

impl MenuMetrics {
    /// From a button's height, the gap between rows and the frame's margins.
    #[must_use]
    pub const fn of(button_pt: f32, gap_pt: f32, margins_pt: f32) -> Self {
        Self {
            row_pt: button_pt + gap_pt,
            separator_pt: CHIP_MENU_SEPARATOR_PT + gap_pt,
            frame_pt: margins_pt,
        }
    }
}

/// The size the cell's menu may take this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipMenuBudget {
    pub min_width: f32,
    pub max_width: f32,
    /// The workspace list's height, at most [`CHIP_MENU_MAX_LIST_ROWS`]
    /// rows; the rest of the menu never scrolls.
    pub list_height: f32,
    /// Whether the list holds more rows than that height shows.
    pub scrolls: bool,
    /// The whole menu's height: over the room below the capsule only when
    /// the window cannot hold [`CHIP_MENU_MIN_LIST_ROWS`] rows.
    pub max_height: f32,
}

impl ChipMenuBudget {
    /// The width the menu's frame may take: the floor on the frame that
    /// measures it, so a rule spanning the menu cannot carry an old width,
    /// and the cap on the frames that draw.
    #[must_use]
    pub const fn width(&self, measuring: bool) -> f32 {
        if measuring {
            self.min_width
        } else {
            self.max_width
        }
    }

    /// The height the list may take: [`Self::list_height`] while it scrolls,
    /// and unbounded while every row shows, so a row measured a little short
    /// never clips the list it ends.
    #[must_use]
    pub const fn list_max(&self) -> f32 {
        if self.scrolls {
            self.list_height
        } else {
            f32::INFINITY
        }
    }
}

/// The height a list of `rows` workspace rows may take in a menu with no
/// anchor to keep below, the `⋮` menu's Workspace submenu: unbounded while
/// [`CHIP_MENU_MAX_LIST_ROWS`] or fewer show whole, that many rows after.
#[must_use]
pub fn list_cap(rows: usize, metrics: MenuMetrics) -> f32 {
    if rows > CHIP_MENU_MAX_LIST_ROWS {
        CHIP_MENU_MAX_LIST_ROWS as f32 * metrics.row_pt
    } else {
        f32::INFINITY
    }
}

/// What the cell's menu may take on `screen`, hanging below `anchor` with
/// `workspaces` rows in its list: no wider than [`CHIP_MENU_MAX_WIDTH_PT`] or
/// the window, and no lower than the window's bottom edge, the list shrinking
/// to scroll first.
#[must_use]
pub fn chip_menu_budget(
    screen: MenuBounds,
    anchor: MenuBounds,
    workspaces: usize,
    metrics: MenuMetrics,
) -> ChipMenuBudget {
    let window_width = (screen.right - screen.left - 2.0 * CHIP_MENU_EDGE_PT).max(0.0);
    let max_width = CHIP_MENU_MAX_WIDTH_PT.min(window_width);
    let top = anchor.bottom + CHIP_MENU_DROP_PT;
    let room = (screen.bottom - CHIP_MENU_EDGE_PT - top).max(0.0);
    let fixed = CHIP_MENU_FIXED_ROWS as f32 * metrics.row_pt
        + CHIP_MENU_SEPARATORS as f32 * metrics.separator_pt
        + metrics.frame_pt;
    let whole = workspaces as f32 * metrics.row_pt;
    let floor = workspaces.min(CHIP_MENU_MIN_LIST_ROWS) as f32 * metrics.row_pt;
    let most = CHIP_MENU_MAX_LIST_ROWS as f32 * metrics.row_pt;
    let list_height = (room - fixed).min(most).min(whole).max(floor);
    ChipMenuBudget {
        min_width: CHIP_MENU_MIN_WIDTH_PT.min(max_width),
        max_width,
        list_height,
        scrolls: list_height < whole,
        max_height: fixed + list_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managing() -> ManageDialog {
        let mut state = WorkspaceMenuState::default();
        state.open_manage(7);
        state.manage.expect("opened")
    }

    #[test]
    fn delete_asks_twice_for_the_same_name() {
        let mut dialog = managing();
        assert_eq!(dialog.click_delete("Scalp"), DeleteClick::Armed);
        assert_eq!(dialog.click_delete("Scalp"), DeleteClick::Confirmed);
        assert_eq!(dialog.armed, None);
    }

    #[test]
    fn a_second_click_on_another_name_only_moves_the_arm() {
        let mut dialog = managing();
        dialog.click_delete("Scalp");
        assert_eq!(dialog.click_delete("Tape"), DeleteClick::Armed);
        assert_eq!(dialog.armed.as_deref(), Some("Tape"));
    }

    #[test]
    fn a_capture_stages_each_workspace_surface_and_refuses_the_rest() {
        let mut state = WorkspaceMenuState::default();
        assert!(state.stage(7, "cell"));
        assert!(state.chip_menu_on(7));
        assert!(state.stage(7, "manage:delete=Scalp"));
        assert_eq!(
            state.manage.as_ref().unwrap().armed.as_deref(),
            Some("Scalp")
        );
        assert!(state.stage(7, "manage:rename=Scalp"));
        assert!(
            state.dialog.is_some(),
            "the rename is asked over the dialog"
        );
        let mut fresh = WorkspaceMenuState::default();
        assert!(!fresh.stage(7, "manage:drop=Scalp"));
        assert!(!fresh.stage(7, "file"));
        assert_eq!(fresh, WorkspaceMenuState::default(), "nothing opened");
    }

    #[test]
    fn reopening_the_manage_dialog_disarms_the_delete() {
        let mut state = WorkspaceMenuState::default();
        state.open_manage(7);
        if let Some(dialog) = &mut state.manage {
            dialog.click_delete("Scalp");
        }
        state.close_manage();
        assert_eq!(state.manage, None);
        state.open_manage(7);
        assert_eq!(state.manage.expect("opened").armed, None);
    }

    #[test]
    fn the_menu_belongs_to_the_tab_whose_cell_opened_it() {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        assert!(state.chip_menu_on(7));
        assert!(!state.chip_menu_on(8), "another window's tab");
        state.toggle_chip_menu(8);
        assert!(state.chip_menu_on(8), "the other cell moves it");
        state.toggle_chip_menu(8);
        assert_eq!(state.chip_menu, None, "the same cell closes it");
    }

    #[test]
    fn a_menu_whose_tab_leaves_the_screen_closes_and_another_windows_stays() {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        state.follow_window(Some(9), &[9]);
        assert!(state.chip_menu_on(7), "tab 7 lives in another window");
        state.follow_window(Some(7), &[7, 8]);
        assert!(state.chip_menu_on(7), "still presented");
        state.follow_window(Some(8), &[7, 8]);
        assert_eq!(state.chip_menu, None, "its window moved to tab 8");
        state.toggle_chip_menu(7);
        state.follow_window(None, &[7, 8]);
        assert_eq!(state.chip_menu, None, "Home shows no cell");
    }

    #[test]
    fn the_cell_opens_only_where_it_stands() {
        let mut state = WorkspaceMenuState::default();
        assert!(!state.follow_cell(7, false, &[7], true), "Home");
        assert!(state.follow_cell(7, true, &[7], true));
        assert!(state.follow_cell(7, true, &[7], false), "stays open");
        assert!(!state.follow_cell(7, true, &[7], true), "a second press");
    }

    #[test]
    fn new_workspace_asks_an_empty_name_and_esc_leaves_no_dialog() {
        let mut state = WorkspaceMenuState::default();
        state.open_new(7);
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!(dialog.mode, NameDialogMode::New);
        assert!(dialog.name.is_empty());
        assert!(dialog.wants_focus);
        state.close_dialog();
        assert_eq!(state.dialog, None);
    }

    #[test]
    fn a_refusal_keeps_the_dialog_the_name_typed_and_the_focus() {
        let mut state = WorkspaceMenuState::default();
        state.open_save_as(7);
        let dialog = state.dialog.as_mut().unwrap();
        dialog.name = "Scalp".to_owned();
        dialog.wants_focus = false;
        state.refuse("a workspace is already called Scalp".to_owned());
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!(dialog.name, "Scalp");
        assert!(dialog.refusal.is_some());
        assert!(dialog.wants_focus, "the field takes the keyboard back");
    }

    #[test]
    fn rename_opens_on_the_current_name() {
        let mut state = WorkspaceMenuState::default();
        state.open_rename(7, "Scalp");
        let dialog = state.dialog.as_ref().unwrap();
        assert_eq!((dialog.tab, dialog.name.as_str()), (7, "Scalp"));
        assert_eq!(
            dialog.mode,
            NameDialogMode::Rename {
                from: "Scalp".to_owned()
            }
        );
    }

    /// Row, separator and frame heights close to what the menu's style draws.
    const METRICS: MenuMetrics = MenuMetrics {
        row_pt: 24.0,
        separator_pt: 9.0,
        frame_pt: 14.0,
    };

    fn rect(left: f32, top: f32, right: f32, bottom: f32) -> MenuBounds {
        MenuBounds {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The capsule's name on a 1920x1000 window's toolbar.
    fn capsule() -> MenuBounds {
        rect(900.0, 40.0, 1000.0, 66.0)
    }

    fn fixed_pt() -> f32 {
        CHIP_MENU_FIXED_ROWS as f32 * METRICS.row_pt
            + CHIP_MENU_SEPARATORS as f32 * METRICS.separator_pt
            + METRICS.frame_pt
    }

    #[test]
    fn nine_workspaces_on_a_full_hd_window_show_whole_below_the_capsule() {
        let screen = rect(0.0, 0.0, 1920.0, 1000.0);
        let budget = chip_menu_budget(screen, capsule(), 9, METRICS);
        assert_eq!(budget.max_width, CHIP_MENU_MAX_WIDTH_PT);
        assert_eq!(budget.min_width, CHIP_MENU_MIN_WIDTH_PT);
        assert_eq!(budget.list_height, 9.0 * METRICS.row_pt);
        assert!(!budget.scrolls, "nothing scrolls");
        assert_eq!(budget.list_max(), f32::INFINITY, "a short row never clips");
        assert_eq!(budget.max_height, fixed_pt() + budget.list_height);
        let top = capsule().bottom + CHIP_MENU_DROP_PT;
        assert!(top + budget.max_height <= screen.bottom - CHIP_MENU_EDGE_PT);
    }

    #[test]
    fn thirty_or_forty_workspaces_show_twelve_rows_and_scroll() {
        let screen = rect(0.0, 0.0, 1920.0, 1000.0);
        for workspaces in [13, 30, 40] {
            let budget = chip_menu_budget(screen, capsule(), workspaces, METRICS);
            let twelve = CHIP_MENU_MAX_LIST_ROWS as f32 * METRICS.row_pt;
            assert_eq!(budget.list_height, twelve, "{workspaces}");
            assert!(budget.scrolls);
            assert_eq!(budget.list_max(), twelve);
            assert_eq!(budget.max_height, fixed_pt() + twelve);
        }
        let budget = chip_menu_budget(screen, capsule(), 12, METRICS);
        assert!(!budget.scrolls, "twelve fit whole");
    }

    #[test]
    fn a_window_shorter_than_twelve_rows_scrolls_sooner_and_ends_above_its_edge() {
        let screen = rect(0.0, 0.0, 1280.0, 560.0);
        let budget = chip_menu_budget(screen, capsule(), 30, METRICS);
        let top = capsule().bottom + CHIP_MENU_DROP_PT;
        let room = screen.bottom - CHIP_MENU_EDGE_PT - top;
        assert!(budget.list_height < CHIP_MENU_MAX_LIST_ROWS as f32 * METRICS.row_pt);
        assert_eq!(budget.max_height, room, "the menu takes the room below");
        assert_eq!(budget.list_height, room - fixed_pt());
    }
    #[test]
    fn a_short_window_keeps_three_workspace_rows_and_says_it_overruns() {
        let screen = rect(0.0, 0.0, 640.0, 360.0);
        let budget = chip_menu_budget(screen, capsule(), 40, METRICS);
        let rows = CHIP_MENU_MIN_LIST_ROWS as f32 * METRICS.row_pt;
        assert_eq!(budget.list_height, rows);
        let room = screen.bottom - CHIP_MENU_EDGE_PT - capsule().bottom - CHIP_MENU_DROP_PT;
        assert!(budget.max_height > room, "the window pushes the menu up");
        assert_eq!(
            budget.max_width, CHIP_MENU_MAX_WIDTH_PT,
            "640 is wide enough"
        );
    }

    #[test]
    fn a_list_shorter_than_the_floor_is_never_padded() {
        let screen = rect(0.0, 0.0, 640.0, 300.0);
        let budget = chip_menu_budget(screen, capsule(), 2, METRICS);
        assert_eq!(budget.list_height, 2.0 * METRICS.row_pt);
        let budget = chip_menu_budget(screen, capsule(), 0, METRICS);
        assert_eq!(budget.list_height, 0.0);
    }

    #[test]
    fn a_narrow_window_narrows_the_menu_and_its_floor() {
        let budget = chip_menu_budget(rect(0.0, 0.0, 300.0, 1000.0), capsule(), 9, METRICS);
        assert_eq!(budget.max_width, 300.0 - 2.0 * CHIP_MENU_EDGE_PT);
        assert_eq!(budget.min_width, CHIP_MENU_MIN_WIDTH_PT);
        let budget = chip_menu_budget(rect(0.0, 0.0, 150.0, 1000.0), capsule(), 9, METRICS);
        assert_eq!(budget.max_width, 150.0 - 2.0 * CHIP_MENU_EDGE_PT);
        assert_eq!(
            budget.min_width, budget.max_width,
            "the floor never passes the cap"
        );
    }

    #[test]
    fn a_window_offset_on_the_desktop_measures_from_its_own_edges() {
        let screen = rect(100.0, 200.0, 2020.0, 1200.0);
        let anchor = rect(1000.0, 240.0, 1100.0, 266.0);
        let shifted = chip_menu_budget(screen, anchor, 40, METRICS);
        let home = chip_menu_budget(rect(0.0, 0.0, 1920.0, 1000.0), capsule(), 40, METRICS);
        assert_eq!(shifted, home);
    }

    #[test]
    fn every_opening_measures_then_settles_unseen_then_draws() {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        let sizing = &mut state.chip_sizing;
        assert_eq!(sizing.frame(1), (true, false), "the opening frame measures");
        assert_eq!(sizing.frame(1), (false, false), "then settles unseen");
        assert_eq!(sizing.frame(1), (false, true), "then draws");
        assert_eq!(sizing.frame(1), (false, true));
        state.toggle_chip_menu(7);
        state.toggle_chip_menu(8);
        assert_eq!(state.chip_sizing.frame(1), (true, false), "a reopening too");
    }

    #[test]
    fn rows_whose_words_change_are_measured_again() {
        let sizing = &mut ChipSizing::default();
        sizing.frame(1);
        sizing.cut("save", 220.0, 200.0);
        sizing.frame(1);
        assert_eq!(sizing.frame(1), (false, true));
        assert!(!sizing.cut("save", 300.0, 300.0), "measured whole at 220");
        assert_eq!(sizing.frame(2), (true, false), "a longer Save label");
        sizing.cut("save", 420.0, 200.0);
        assert!(
            !sizing.cut("workspace.New", 200.0, 200.0),
            "measuring cuts nothing"
        );
        sizing.frame(2);
        assert!(sizing.cut("save", 344.0, 344.0), "measured again at 420");
        assert!(!sizing.cut("workspace.New", 180.0, 344.0));
    }

    #[test]
    fn a_name_is_cut_only_when_its_whole_row_does_not_fit() {
        let sizing = &mut ChipSizing::default();
        assert!(sizing.frame(1).0);
        assert!(
            !sizing.cut("wide", 420.2, 200.0),
            "the measuring frame cuts nothing"
        );
        assert!(!sizing.cut("narrow", 180.0, 200.0));
        assert!(!sizing.frame(1).0);
        assert!(sizing.cut("wide", 344.0, 344.0), "asks the whole room, cut");
        assert!(!sizing.cut("narrow", 180.0, 344.0));
        assert!(!sizing.cut("wide", 421.0, 421.0), "fits exactly, whole");
        assert!(!sizing.cut("unmeasured", 344.0, 344.0));
    }
    #[test]
    fn the_style_s_heights_become_rows_separators_and_a_frame() {
        let metrics = MenuMetrics::of(20.0, 4.0, 12.0);
        assert_eq!(metrics.row_pt, 24.0);
        assert_eq!(metrics.separator_pt, CHIP_MENU_SEPARATOR_PT + 4.0);
        assert_eq!(metrics.frame_pt, 12.0);
        assert_eq!(
            MenuBounds::new(1.0, 2.0, 3.0, 4.0),
            rect(1.0, 2.0, 3.0, 4.0)
        );
    }

    #[test]
    fn the_window_menu_list_shows_twelve_rows_at_most() {
        assert_eq!(list_cap(9, METRICS), f32::INFINITY, "nine show whole");
        assert_eq!(list_cap(12, METRICS), f32::INFINITY);
        let twelve = CHIP_MENU_MAX_LIST_ROWS as f32 * METRICS.row_pt;
        assert_eq!(list_cap(30, METRICS), twelve);
    }
    /// The caption gives way before the name: whole while both fit, then cut
    /// to what the whole name leaves, then dropped; never bounded, whole.
    #[test]
    fn a_row_s_caption_gives_way_before_its_name() {
        assert_eq!(
            fit_caption(Some(300.0), 100.0, 8.0, 120.0),
            CaptionFit::Whole
        );
        assert_eq!(
            fit_caption(Some(228.0), 100.0, 8.0, 120.0),
            CaptionFit::Whole
        );
        assert_eq!(
            fit_caption(Some(200.0), 100.0, 8.0, 120.0),
            CaptionFit::Cut(92.0)
        );
        let least = 100.0 + 8.0 + CHIP_MENU_MIN_CAPTION_PT;
        assert_eq!(
            fit_caption(Some(least), 100.0, 8.0, 120.0),
            CaptionFit::Cut(CHIP_MENU_MIN_CAPTION_PT)
        );
        assert_eq!(
            fit_caption(Some(least - 1.0), 100.0, 8.0, 120.0),
            CaptionFit::Dropped
        );
        assert_eq!(
            fit_caption(Some(80.0), 100.0, 8.0, 120.0),
            CaptionFit::Dropped
        );
        assert_eq!(fit_caption(Some(80.0), 100.0, 8.0, 0.0), CaptionFit::Whole);
        let open = fit_caption(None, 900.0, 8.0, 900.0);
        assert_eq!(open, CaptionFit::Whole, "a row that extends");
    }

    #[test]
    fn the_measuring_frame_keeps_to_the_floor_and_the_others_to_the_cap() {
        let budget = chip_menu_budget(rect(0.0, 0.0, 1920.0, 1000.0), capsule(), 9, METRICS);
        assert_eq!(budget.width(true), CHIP_MENU_MIN_WIDTH_PT);
        assert_eq!(budget.width(false), CHIP_MENU_MAX_WIDTH_PT);
    }
}
