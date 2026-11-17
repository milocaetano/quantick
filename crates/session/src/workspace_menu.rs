//! The workspace menus' state, apart from their pixels: the status bar cell's
//! menu, the Manage workspaces dialog whose delete asks twice, the dialog that
//! asks for a name, the close that asks per tab holding edits and the switch
//! that asks before it drops them.

use crate::close_queue::{CloseAnswer, CloseQueue, CloseScope};
use crate::colour_popover::{ColourIntent, ColourPopover};
pub use crate::manage_workspaces::{DeleteClick, ManageDialog, ManageIntent};
use crate::switch_prompt::{SwitchAnswer, SwitchPrompt};

/// What a workspace row or the dialog asked the window to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceCommand {
    /// Put the active tab on this workspace, asking first when the switch
    /// would drop edits.
    Set(String),
    /// The switch prompt's answer.
    AnswerSwitch(SwitchAnswer),
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
    /// The dialog's Enter.
    SubmitDialog,
    /// The name dialog's Cancel, its close button or Esc.
    CloseDialog,
    /// Close these tabs, or exit, asking first per tab holding edits.
    Close(CloseScope, Vec<u64>),
    /// The close prompt's answer.
    AnswerClose(CloseAnswer),
    /// A press in the header swatch's palette popover.
    Colour(ColourIntent),
    /// A press or key in Manage workspaces.
    Manage(ManageIntent),
    /// A press in the create dialog's palette.
    NameColour(ColourIntent),
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
#[derive(Debug, Clone, PartialEq)]
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
    /// The create dialogs' colour, as a call names it (`petrol`,
    /// `#3A7BD5`): the next free one until a pick; `None` for a rename.
    pub colour: Option<String>,
    /// The create dialogs' palette: its focus and Custom.
    pub palette: ColourPopover,
}

/// The menus' state across frames.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceMenuState {
    /// The tab whose cell opened the menu, while it is open. The menu shows
    /// and acts in the window presenting that tab, so a torn-off window's
    /// cell never reaches another window's tab.
    pub chip_menu: Option<u64>,
    /// The words typed in the open menu's filter field.
    pub filter: String,
    /// The list row, among those the filter shows, that Up and Down moved
    /// to and Enter takes.
    pub focus: Option<usize>,
    /// The list's scroll offset as last drawn, in points.
    pub scroll: f32,
    /// The next frame scrolls the list to the focus, else the current row.
    reveal: bool,
    /// A capture staged the menu before the window presented its tab: the
    /// first frame that shows the cell carries it there (a replay opening
    /// at launch hands the window a new tab).
    staged: bool,
    pub manage: Option<ManageDialog>,
    pub dialog: Option<NameDialog>,
    pub close: CloseQueue,
    /// The switch waiting for an answer before it drops edits.
    pub switch: SwitchPrompt,
    /// The palette popover under the header swatch, while it is open.
    pub colour: Option<ColourPopover>,
    /// A capture staged a dialog before the window presented its tab.
    staged_dialog: bool,
}

impl WorkspaceMenuState {
    /// The cell of `tab` was clicked, or its shortcut pressed: open the menu
    /// on that tab, or close it when it was already open there.
    pub fn toggle_chip_menu(&mut self, tab: u64) {
        self.chip_menu = (self.chip_menu != Some(tab)).then_some(tab);
        self.filter.clear();
        (self.focus, self.scroll, self.reveal) = (None, 0.0, true);
        self.colour = None;
    }

    /// The header swatch: open the palette popover over the menu, or close it.
    pub fn toggle_colour(&mut self) {
        self.colour = match self.colour {
            Some(_) => None,
            None => Some(ColourPopover::default()),
        };
    }

    /// The end of a frame on the open menu: `asked` the press, if any.
    /// While the popover is open, Esc or a click outside closes only it; a
    /// colour press never closes the menu; otherwise a press, Esc or a click
    /// outside closes the menu.
    pub fn settle(&mut self, asked: Option<&Option<WorkspaceCommand>>, escape: bool, out: bool) {
        let colour = matches!(asked, Some(Some(WorkspaceCommand::Colour(_))));
        if self.colour.is_some() && (escape || out) {
            self.colour = None;
        } else if !colour && (asked.is_some() || escape || out) {
            self.close_chip_menu();
        }
    }

    /// The filter's words changed: the focus goes back to the field, from
    /// which Enter takes the first match and Down steps onto it.
    pub fn filter_changed(&mut self) {
        (self.focus, self.scroll, self.reveal) = (None, 0.0, true);
    }

    /// Whether the filter narrows the list.
    fn filtering(&self) -> bool {
        !self.filter.trim().is_empty()
    }

    /// Up (`down` false) or Down among `rows` list rows, from the focus or
    /// else the tab's `current` row; it stops at either end.
    pub fn move_focus(&mut self, down: bool, rows: usize, current: Option<usize>) {
        let last = rows.checked_sub(1);
        let from = if self.filtering() {
            self.focus
        } else {
            self.focus.or(current)
        };
        self.focus = match (from, last) {
            (_, None) => None,
            (None, Some(last)) => Some(if down { 0 } else { last }),
            (Some(at), Some(last)) if down => Some((at + 1).min(last)),
            (Some(at), Some(last)) => Some(at.min(last).saturating_sub(1)),
        };
        self.reveal = true;
    }

    /// A frame's Down, Up and Enter among `rows` list rows: the focus moved
    /// from the tab's `current` row, and the row Enter takes, if pressed.
    pub fn keys(
        &mut self,
        pressed: [bool; 3],
        rows: usize,
        current: Option<usize>,
    ) -> Option<usize> {
        let [down, up, enter] = pressed;
        if down || up {
            self.move_focus(down, rows, current);
        }
        enter.then(|| self.enter(rows)).flatten()
    }

    /// The row Enter takes among `rows`: the focused one, else the first
    /// match while the filter narrows the list, while the list holds it.
    #[must_use]
    pub fn enter(&self, rows: usize) -> Option<usize> {
        let first = self.filtering().then_some(0);
        self.focus.or(first).filter(|&at| at < rows)
    }

    /// The list was drawn scrolled to `offset` points.
    pub fn scrolled(&mut self, offset: f32) {
        self.scroll = offset;
    }

    /// The offset the list draws at this frame, in whole points so its rows
    /// and the rule under it land on pixels: [`Self::reveal`]'s once the menu
    /// is `ready` to show, else where it was.
    pub fn offset(&mut self, current: Option<usize>, view_pt: f32, ready: bool) -> f32 {
        let revealed = ready.then(|| self.reveal(current, view_pt)).flatten();
        revealed.unwrap_or(self.scroll).round()
    }

    /// The offset the list scrolls to this frame, if an opening or a moved
    /// focus asks: the least move that shows the focus, else the `current`
    /// row, whole in a list `view_pt` tall.
    pub fn reveal(&mut self, current: Option<usize>, view_pt: f32) -> Option<f32> {
        if !std::mem::take(&mut self.reveal) {
            return None;
        }
        let top = self.focus.or(current).unwrap_or(0) as f32 * CHIP_MENU_ROW_PT;
        let view = (view_pt - CHIP_MENU_LIST_PAD_PT).max(CHIP_MENU_ROW_PT);
        let offset = if top < self.scroll {
            top
        } else {
            self.scroll.max(top + CHIP_MENU_ROW_PT - view)
        };
        self.scroll = offset;
        Some(offset)
    }

    pub fn close_chip_menu(&mut self) {
        self.chip_menu = None;
        self.colour = None;
    }

    /// Open the Manage workspaces dialog in the window presenting `tab`.
    pub fn open_manage(&mut self, tab: u64) {
        self.manage = Some(ManageDialog::new(tab));
    }

    pub fn close_manage(&mut self) {
        self.manage = None;
    }

    /// Open a surface on `tab` as a capture asks (`QUANTICK_WORKSPACE_MENU`):
    /// `cell` the cell's menu, `cell:colour` with its palette popover,
    /// `cell:colour:ask=<word>` with the swap question on that colour,
    /// `cell:colour:custom=<hex>` with Custom open on those words, `manage`
    /// the dialog, `manage:<face>` on one of [`ManageDialog::stage`]'s faces,
    /// `save_as` and `new` the create dialogs (their name and colour are the
    /// handler's to fill), `save_as:custom=<hex>` with Custom open on those
    /// words. Answers false, opening nothing, for any other token.
    pub fn stage(&mut self, tab: u64, token: &str) -> bool {
        let (surface, row) = token.split_once(':').unwrap_or((token, ""));
        if let ("cell", Some(popover)) = (surface, row.strip_prefix("colour")) {
            let mut colour = ColourPopover::default();
            match popover
                .strip_prefix(':')
                .and_then(|face| face.split_once('='))
            {
                None if popover.is_empty() => {}
                Some(("ask", word)) => colour.ask(word),
                Some(("custom", hex)) => (colour.custom, colour.hex) = (true, hex.to_owned()),
                _ => return false,
            }
            self.toggle_chip_menu(tab);
            (self.colour, self.staged) = (Some(colour), true);
            return true;
        }
        match (surface, row) {
            ("cell", "") => {
                self.toggle_chip_menu(tab);
                self.staged = true;
            }
            ("manage", "") => self.open_manage(tab),
            ("manage", face) => {
                let mut dialog = ManageDialog::new(tab);
                if !dialog.stage(face) {
                    return false;
                }
                self.manage = Some(dialog);
            }
            ("save_as" | "new", face) => {
                let custom = face.strip_prefix("custom=");
                if !face.is_empty() && custom.is_none() {
                    return false;
                }
                if surface == "new" {
                    self.open_new(tab);
                } else {
                    self.open_save_as(tab);
                }
                if let (Some(hex), Some(dialog)) = (custom, self.dialog.as_mut()) {
                    (dialog.palette.custom, dialog.palette.hex) = (true, hex.to_owned());
                }
            }
            _ => return false,
        }
        self.staged_dialog = surface != "cell";
        true
    }

    /// The first frame presenting a market tab carries a staged dialog
    /// there, as [`Self::follow_cell`] carries a staged menu.
    pub fn follow_staged(&mut self, tab: u64, shown: bool) {
        if shown && std::mem::take(&mut self.staged_dialog) {
            if let Some(dialog) = &mut self.manage {
                dialog.tab = tab;
            }
            if let Some(dialog) = &mut self.dialog {
                dialog.tab = tab;
            }
        }
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
        if shown && std::mem::take(&mut self.staged) && self.chip_menu.is_some() {
            self.chip_menu = Some(tab);
        }
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
            colour: None,
            palette: ColourPopover::default(),
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

/// The cell's menu's width, in points (designer B): fixed, so nothing in
/// it is measured before it shows.
pub const CHIP_MENU_WIDTH_PT: f32 = 320.0;
/// How far below the capsule the menu hangs, in points.
pub const CHIP_MENU_DROP_PT: f32 = 4.0;
/// What the menu keeps clear of the window's bottom edge, in points.
pub const CHIP_MENU_EDGE_PT: f32 = 8.0;
/// The menu's border, each side, and its corners' radius.
pub const CHIP_MENU_BORDER_PT: f32 = 1.0;
pub const CHIP_MENU_RADIUS_PT: f32 = 6.0;
/// The header block that never scrolls: the current line and its buttons.
pub const CHIP_MENU_HEADER_PT: f32 = 81.0;
/// The WORKSPACES caption over the list.
pub const CHIP_MENU_CAPTION_PT: f32 = 24.0;
/// One workspace row: its name over its summary.
pub const CHIP_MENU_ROW_PT: f32 = 34.0;
/// The list's padding either side and below its last row.
pub const CHIP_MENU_LIST_PAD_PT: f32 = 4.0;
/// The tallest the list draws: nine and a half rows, so the cut row says
/// there is more below.
pub const CHIP_MENU_LIST_MAX_PT: f32 = 323.0;
/// From this many workspaces a filter field stands over the list.
pub const CHIP_MENU_FILTER_FROM: usize = 13;
/// The filter field and the gap under it.
pub const CHIP_MENU_FILTER_PT: f32 = 28.0;
/// The footer block that never scrolls: Autosave, then New and Manage.
pub const CHIP_MENU_FOOTER_PT: f32 = 65.0;
/// The workspace rows the list keeps in view however short the window. Below
/// that the menu overruns the window and is pushed up rather than show none.
pub const CHIP_MENU_MIN_LIST_ROWS: usize = 3;
/// The most workspace rows the window menu's Workspace submenu shows at once.
pub const CHIP_MENU_MAX_LIST_ROWS: usize = 12;
/// The height egui gives a horizontal separator, before the gap after it.
pub const CHIP_MENU_SEPARATOR_PT: f32 = 6.0;

/// Whether `filter` keeps the workspace `name`: any part of it, whatever
/// its case; an empty filter keeps every name.
#[must_use]
pub fn filter_matches(filter: &str, name: &str) -> bool {
    name.to_lowercase().contains(&filter.trim().to_lowercase())
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

/// The cell's menu's size this frame, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChipMenuLayout {
    pub width: f32,
    /// The filter field stands over the list.
    pub filter: bool,
    /// The list's height, its padding in: the only part that scrolls.
    pub list_height: f32,
    /// Whether the list holds more rows than that height shows.
    pub scrolls: bool,
    /// The whole menu, its border in: over the room below the capsule only
    /// when the window cannot hold [`CHIP_MENU_MIN_LIST_ROWS`] rows.
    pub height: f32,
}

/// The cell's menu on `screen`, hanging below `anchor`, for `workspaces`
/// in all of which the filter shows `shown`: [`CHIP_MENU_WIDTH_PT`] wide or
/// the window's width, and ending [`CHIP_MENU_EDGE_PT`] above the window's
/// bottom, the list shrinking to scroll first; every size in whole points, so
/// the rows, the rules and the border land on pixels.
#[must_use]
pub fn chip_menu_layout(
    screen: MenuBounds,
    anchor: MenuBounds,
    workspaces: usize,
    shown: usize,
) -> ChipMenuLayout {
    let filter = workspaces >= CHIP_MENU_FILTER_FROM;
    let fixed = CHIP_MENU_HEADER_PT
        + CHIP_MENU_CAPTION_PT
        + if filter { CHIP_MENU_FILTER_PT } else { 0.0 }
        + CHIP_MENU_FOOTER_PT
        + 2.0 * CHIP_MENU_BORDER_PT;
    let room = screen.bottom - CHIP_MENU_EDGE_PT - anchor.bottom - CHIP_MENU_DROP_PT;
    let rows = |count: usize| count as f32 * CHIP_MENU_ROW_PT + CHIP_MENU_LIST_PAD_PT;
    let whole = rows(shown);
    let floor = rows(shown.min(CHIP_MENU_MIN_LIST_ROWS));
    let list_height = whole
        .min(CHIP_MENU_LIST_MAX_PT)
        .min((room - fixed).floor())
        .max(floor);
    let window = (screen.right - screen.left - 2.0 * CHIP_MENU_EDGE_PT)
        .max(0.0)
        .floor();
    ChipMenuLayout {
        width: CHIP_MENU_WIDTH_PT.min(window),
        filter,
        list_height,
        scrolls: list_height < whole,
        height: fixed + list_height,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn managing() -> ManageDialog {
        let mut state = WorkspaceMenuState::default();
        state.open_manage(7);
        state.manage.expect("opened")
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
        let manage = state.manage.as_ref().unwrap();
        assert_eq!(manage.rename.as_ref().unwrap().from, "Scalp", "inline");
        assert!(state.stage(7, "save_as"));
        assert_eq!(
            state.dialog.as_ref().unwrap().mode,
            NameDialogMode::SaveAsNew
        );
        assert!(state.stage(7, "new"));
        assert_eq!(state.dialog.as_ref().unwrap().mode, NameDialogMode::New);
        assert!(state.stage(7, "save_as:custom=#5DBDEB"));
        let custom = &state.dialog.as_ref().unwrap().palette;
        assert!(custom.custom && custom.hex == "#5DBDEB");
        assert!(!WorkspaceMenuState::default().stage(7, "save_as:paint=1"));
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

    fn full_hd() -> MenuBounds {
        rect(0.0, 0.0, 1920.0, 1000.0)
    }

    fn opened() -> WorkspaceMenuState {
        let mut state = WorkspaceMenuState::default();
        state.toggle_chip_menu(7);
        state
    }

    /// Designer B's whole menu at nine workspaces: 81 + 24 + 310 + 65 + 2.
    #[test]
    fn nine_workspaces_make_the_mock_s_482_point_menu() {
        let layout = chip_menu_layout(full_hd(), capsule(), 9, 9);
        assert_eq!(layout.width, CHIP_MENU_WIDTH_PT);
        assert_eq!(CHIP_MENU_WIDTH_PT, 320.0);
        assert!(!layout.filter, "no filter below thirteen");
        assert!(!layout.scrolls, "nine rows show whole");
        assert_eq!(layout.list_height, 310.0);
        assert_eq!(layout.height, 482.0);
    }

    /// Thirty workspaces: 81 + 24 + 28 + 323 + 65 + 2, nine and a half rows
    /// in view so the cut row says more below.
    #[test]
    fn thirty_workspaces_make_the_mock_s_523_point_menu_showing_nine_and_a_half_rows() {
        let layout = chip_menu_layout(full_hd(), capsule(), 30, 30);
        assert!(layout.filter && layout.scrolls);
        assert_eq!(layout.list_height, 323.0);
        assert_eq!(layout.list_height / CHIP_MENU_ROW_PT, 9.5);
        assert_eq!(layout.height, 523.0);
    }

    #[test]
    fn one_workspace_makes_a_short_menu() {
        let layout = chip_menu_layout(full_hd(), capsule(), 1, 1);
        assert!(!layout.filter && !layout.scrolls);
        assert_eq!(layout.list_height, CHIP_MENU_ROW_PT + CHIP_MENU_LIST_PAD_PT);
        assert_eq!(layout.height, 81.0 + 24.0 + 38.0 + 65.0 + 2.0);
    }

    #[test]
    fn the_filter_shows_from_thirteen_workspaces_and_stays_while_it_narrows() {
        assert_eq!(CHIP_MENU_FILTER_FROM, 13);
        assert!(!chip_menu_layout(full_hd(), capsule(), 12, 12).filter);
        let thirteen = chip_menu_layout(full_hd(), capsule(), 13, 13);
        assert!(thirteen.filter && thirteen.scrolls);
        let narrowed = chip_menu_layout(full_hd(), capsule(), 13, 2);
        assert!(narrowed.filter, "the field stays while it narrows");
        assert!(!narrowed.scrolls);
        let two = 2.0 * CHIP_MENU_ROW_PT + CHIP_MENU_LIST_PAD_PT;
        assert_eq!(narrowed.list_height, two);
    }

    #[test]
    fn a_short_window_ends_the_menu_eight_points_above_its_edge() {
        let screen = rect(0.0, 0.0, 1280.0, 560.0);
        let layout = chip_menu_layout(screen, capsule(), 30, 30);
        let top = capsule().bottom + CHIP_MENU_DROP_PT;
        assert_eq!(layout.height, screen.bottom - CHIP_MENU_EDGE_PT - top);
        assert!(layout.list_height < CHIP_MENU_LIST_MAX_PT && layout.scrolls);
    }

    #[test]
    fn a_tiny_window_keeps_three_rows_and_overruns() {
        let screen = rect(0.0, 0.0, 640.0, 360.0);
        let layout = chip_menu_layout(screen, capsule(), 40, 40);
        let three = CHIP_MENU_MIN_LIST_ROWS as f32 * CHIP_MENU_ROW_PT + CHIP_MENU_LIST_PAD_PT;
        assert_eq!(layout.list_height, three);
        let room = screen.bottom - CHIP_MENU_EDGE_PT - capsule().bottom - CHIP_MENU_DROP_PT;
        assert!(layout.height > room, "the window pushes the menu up");
    }

    #[test]
    fn a_narrow_window_narrows_the_menu_and_an_offset_one_measures_from_its_edges() {
        let narrow = chip_menu_layout(rect(0.0, 0.0, 300.0, 1000.0), capsule(), 9, 9);
        assert_eq!(narrow.width, 300.0 - 2.0 * CHIP_MENU_EDGE_PT);
        let shifted = chip_menu_layout(
            rect(100.0, 200.0, 2020.0, 1200.0),
            rect(1000.0, 240.0, 1100.0, 266.0),
            40,
            40,
        );
        assert_eq!(shifted, chip_menu_layout(full_hd(), capsule(), 40, 40));
    }

    /// On open the list scrolls so the current row is in view, once.
    #[test]
    fn opening_scrolls_the_current_row_into_view_once() {
        let mut state = opened();
        let view = CHIP_MENU_LIST_MAX_PT;
        let offset = state.reveal(Some(29), view).expect("the opening reveals");
        let top = 29.0 * CHIP_MENU_ROW_PT;
        assert!(top >= offset, "{offset}");
        assert!(top + CHIP_MENU_ROW_PT <= offset + view - CHIP_MENU_LIST_PAD_PT);
        assert_eq!(state.reveal(Some(29), view), None, "only the opening");
        let mut state = opened();
        assert_eq!(state.reveal(Some(0), view), Some(0.0), "the top needs none");
    }

    #[test]
    fn up_and_down_move_the_focus_from_the_current_row_and_stop_at_the_ends() {
        let mut state = opened();
        state.move_focus(true, 9, Some(3));
        assert_eq!(state.focus, Some(4));
        state.move_focus(false, 9, Some(3));
        state.move_focus(false, 9, Some(3));
        assert_eq!(state.focus, Some(2));
        for _ in 0..20 {
            state.move_focus(false, 9, Some(3));
        }
        assert_eq!(state.focus, Some(0));
        for _ in 0..20 {
            state.move_focus(true, 9, Some(3));
        }
        assert_eq!(state.focus, Some(8));
        assert_eq!(state.enter(9), Some(8), "Enter takes the focused row");
        assert_eq!(state.enter(5), None, "a row the list no longer holds");
        let mut fresh = opened();
        fresh.move_focus(true, 9, None);
        assert_eq!(fresh.focus, Some(0));
        let mut fresh = opened();
        fresh.move_focus(false, 9, None);
        assert_eq!(fresh.focus, Some(8));
        assert_eq!(opened().enter(9), None, "no focus, nothing to take");
        let mut empty = opened();
        empty.move_focus(true, 0, None);
        assert_eq!(empty.focus, None);
    }

    /// One frame's keys: Down moves from the tab's row, Enter takes the
    /// focus; Enter with no focus takes nothing.
    #[test]
    fn a_frame_s_down_then_enter_take_the_next_row() {
        let mut state = opened();
        assert_eq!(state.keys([false, false, true], 9, Some(3)), None);
        assert_eq!(state.keys([true, false, false], 9, Some(3)), None);
        assert_eq!(state.keys([false, false, true], 9, Some(3)), Some(4));
        assert_eq!(state.keys([false, true, true], 9, Some(3)), Some(3));
    }

    /// A focus moved out of view scrolls the list to it.
    #[test]
    fn a_moved_focus_is_scrolled_into_view() {
        let mut state = opened();
        let view = CHIP_MENU_LIST_MAX_PT;
        state.reveal(Some(0), view);
        state.move_focus(false, 30, None);
        assert_eq!(state.focus, Some(29), "Up from no row is the last");
        let offset = state.reveal(Some(0), view).expect("the focus moved");
        let top = 29.0 * CHIP_MENU_ROW_PT;
        assert!(top >= offset && top + CHIP_MENU_ROW_PT <= offset + view);
    }

    #[test]
    fn the_filter_matches_any_part_of_a_name_whatever_its_case() {
        assert!(filter_matches("", "Scalp"));
        assert!(filter_matches("  ", "Scalp"));
        assert!(filter_matches("calp", "Scalp"));
        assert!(filter_matches("SCA", "Scalp"));
        assert!(filter_matches(" scalp ", "Scalp"));
        assert!(!filter_matches("tape", "Scalp"));
    }

    #[test]
    fn from_the_filter_enter_takes_the_first_match_and_down_steps_onto_it() {
        let mut state = opened();
        state.filter = "desk".to_owned();
        state.filter_changed();
        assert_eq!(state.focus, None, "the field keeps the focus");
        assert_eq!(state.enter(5), Some(0), "Enter takes the first match");
        assert_eq!(state.enter(0), None, "no match, nothing taken");
        state.move_focus(true, 5, Some(3));
        assert_eq!(state.focus, Some(0), "Down steps onto the first match");
        state.move_focus(true, 5, Some(3));
        assert_eq!(state.focus, Some(1));
        state.filter.clear();
        state.filter_changed();
        assert_eq!((state.focus, state.enter(5)), (None, None));
    }

    /// The list draws at whole points however it was scrolled, and its
    /// height is whole in any window.
    #[test]
    fn the_list_lands_on_whole_points() {
        let mut state = opened();
        for offset in [0.0, 12.4, 33.5, 140.5, 701.75] {
            state.scrolled(offset);
            let drawn = state.offset(None, CHIP_MENU_LIST_MAX_PT, false);
            assert_eq!(drawn, drawn.round(), "{offset}");
        }
        let mut state = opened();
        assert_eq!(
            state.offset(Some(29), CHIP_MENU_LIST_MAX_PT, false),
            0.0,
            "unseen"
        );
        let revealed = state.offset(Some(29), 300.5, true);
        assert_eq!(revealed, revealed.round());
        for bottom in [777.5, 560.25, 999.9] {
            let screen = rect(0.0, 0.0, 1280.5, bottom);
            let layout = chip_menu_layout(screen, rect(900.0, 40.0, 1000.0, 66.5), 30, 30);
            assert_eq!(layout.list_height, layout.list_height.round(), "{bottom}");
            assert_eq!(layout.height, layout.height.round(), "{bottom}");
            assert_eq!(layout.width, layout.width.round());
        }
    }

    #[test]
    fn reopening_forgets_the_filter_the_focus_and_the_scroll() {
        let mut state = opened();
        state.filter = "desk".to_owned();
        state.filter_changed();
        state.scrolled(120.0);
        state.toggle_chip_menu(7);
        state.toggle_chip_menu(7);
        assert!(state.filter.is_empty());
        assert_eq!((state.focus, state.scroll), (None, 0.0));
        assert!(
            state.reveal(Some(0), 100.0).is_some(),
            "a new opening reveals"
        );
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
}
