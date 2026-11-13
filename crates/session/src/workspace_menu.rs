//! The workspace menus' state, apart from their pixels: the status bar cell's
//! menu, the Manage workspaces dialog whose delete asks twice, the dialog that
//! asks for a name and the close that asks per tab holding edits.

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
    /// Counts the menu's openings: each one is laid out afresh, so it is as
    /// wide as what it shows now and not the widest it ever showed.
    pub chip_menu_opened: u64,
    pub manage: Option<ManageDialog>,
    pub dialog: Option<NameDialog>,
    pub close: CloseQueue,
}

impl WorkspaceMenuState {
    /// The cell of `tab` was clicked, or its shortcut pressed: open the menu
    /// on that tab, or close it when it was already open there.
    pub fn toggle_chip_menu(&mut self, tab: u64) {
        self.chip_menu = (self.chip_menu != Some(tab)).then_some(tab);
        if self.chip_menu.is_some() {
            self.chip_menu_opened = self.chip_menu_opened.wrapping_add(1);
        }
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
/// What the menu keeps clear of the window's edges, in points: room for the
/// small differences between the measured rows and the drawn ones.
pub const CHIP_MENU_EDGE_PT: f32 = 8.0;
/// The workspace rows the list keeps in view however short the window. Below
/// that the menu overruns the window and is pushed up rather than show none.
pub const CHIP_MENU_MIN_LIST_ROWS: usize = 3;
/// The rows that never scroll: auto save and its line, Save, Save as,
/// Rename, the WORKSPACES caption, New and Manage.
pub const CHIP_MENU_FIXED_ROWS: usize = 8;
/// The separators between the menu's groups.
pub const CHIP_MENU_SEPARATORS: usize = 3;

/// A rectangle in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MenuBounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
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

/// The size the cell's menu may take this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipMenuBudget {
    pub min_width: f32,
    pub max_width: f32,
    /// The scrolled workspace list's height; the rest of the menu never
    /// scrolls.
    pub list_height: f32,
    /// The whole menu's height: over the room below the capsule only when
    /// the window cannot hold [`CHIP_MENU_MIN_LIST_ROWS`] rows.
    pub max_height: f32,
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
    let list_height = (room - fixed).min(whole).max(floor);
    ChipMenuBudget {
        min_width: CHIP_MENU_MIN_WIDTH_PT.min(max_width),
        max_width,
        list_height,
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
        assert_eq!(budget.list_height, 9.0 * METRICS.row_pt, "nothing scrolls");
        assert_eq!(budget.max_height, fixed_pt() + budget.list_height);
        let top = capsule().bottom + CHIP_MENU_DROP_PT;
        assert!(top + budget.max_height <= screen.bottom - CHIP_MENU_EDGE_PT);
    }

    #[test]
    fn forty_workspaces_scroll_and_the_menu_ends_above_the_window_edge() {
        let screen = rect(0.0, 0.0, 1920.0, 1000.0);
        let budget = chip_menu_budget(screen, capsule(), 40, METRICS);
        let top = capsule().bottom + CHIP_MENU_DROP_PT;
        let room = screen.bottom - CHIP_MENU_EDGE_PT - top;
        assert!(
            budget.list_height < 40.0 * METRICS.row_pt,
            "the list scrolls"
        );
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
    fn every_opening_is_a_new_one_and_a_close_is_not() {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        let first = state.chip_menu_opened;
        state.toggle_chip_menu(7);
        assert_eq!(state.chip_menu_opened, first, "closing counts nothing");
        state.toggle_chip_menu(7);
        assert_ne!(state.chip_menu_opened, first, "a reopening is fresh");
        let second = state.chip_menu_opened;
        state.toggle_chip_menu(8);
        assert_ne!(state.chip_menu_opened, second, "another tab's opening too");
    }
}
