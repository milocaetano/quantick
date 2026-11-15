//! The Manage workspaces dialog's state, apart from its pixels and the book:
//! the row the arrows chose, the one row whose palette, inline rename or
//! armed delete is open, the Undo rows a delete left, and what each key
//! asks. What a press does to the book is the workspace calls' business;
//! the time arrives in the intents that need it.

use crate::colour_popover::{ColourIntent, ColourPopover};

/// What a click on a row's Delete meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteClick {
    /// The first click: the row now states the outcome and asks.
    Armed,
    /// The second click on the same workspace.
    Confirmed,
}

/// A row's name turned into a text field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineRename {
    pub from: String,
    pub name: String,
    /// Why the last Enter was refused, shown under the row.
    pub refusal: Option<String>,
    /// The field takes the keyboard, its name selected, on its first frame.
    pub wants_focus: bool,
}

/// The palette open under one row.
#[derive(Debug, Clone, PartialEq)]
pub struct RowPalette {
    pub workspace: String,
    pub popover: ColourPopover,
}

/// The row a delete leaves where the workspace was, with its Undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRow {
    pub workspace: String,
    /// `Scalp deleted · WIN$N and WDO$N back on Default`.
    pub words: String,
    pub until_ms: i64,
}

/// What a press or a key in the dialog asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManageIntent {
    /// A row's colour chip: open its palette, or close it.
    Chip(String),
    /// A press in the open row's palette.
    Colour(ColourIntent),
    BeginRename(String),
    /// Enter in the name field.
    CommitRename,
    CancelRename,
    /// A row's trash: the row asks, at the same height.
    Arm(String),
    Keep,
    /// The armed row's Delete, at the time told.
    Delete {
        now_ms: i64,
    },
    /// The newest Undo row's Undo, or Ctrl+Z.
    Undo {
        now_ms: i64,
    },
    /// The innermost open thing closes, else the dialog.
    Escape,
    Close,
}

/// A key the dialog reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManageKey {
    Up,
    Down,
    F2,
    Delete,
    Colour,
    Enter,
    Escape,
    /// Ctrl+Z.
    Undo,
}

/// The keys pressed this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManageKeys([bool; 8]);

impl ManageKeys {
    pub fn press(&mut self, key: ManageKey) {
        self.0[key as usize] = true;
    }

    fn has(self, key: ManageKey) -> bool {
        self.0[key as usize]
    }
}

/// The dialog's state while it is open.
#[derive(Debug, Clone, PartialEq)]
pub struct ManageDialog {
    /// The tab whose menu opened it: the dialog shows in that tab's window.
    pub tab: u64,
    /// The workspace whose Delete asks for its confirmation.
    pub armed: Option<String>,
    /// The row Up and Down chose.
    pub focus: Option<usize>,
    pub rename: Option<InlineRename>,
    pub palette: Option<RowPalette>,
    /// The Undo rows, newest last.
    pub undo: Vec<UndoRow>,
    /// The footer's line after a swap or an Undo, also toasted.
    pub note: Option<String>,
}

impl ManageDialog {
    #[must_use]
    pub fn new(tab: u64) -> Self {
        Self {
            tab,
            armed: None,
            focus: None,
            rename: None,
            palette: None,
            undo: Vec::new(),
            note: None,
        }
    }

    /// A click on `name`'s Delete. Arming another workspace disarms the first,
    /// so no confirmation ever lands on a name it did not read.
    pub fn click_delete(&mut self, name: &str) -> DeleteClick {
        if self.armed.as_deref() == Some(name) {
            self.armed = None;
            return DeleteClick::Confirmed;
        }
        self.local(&ManageIntent::Arm(name.to_owned()));
        DeleteClick::Armed
    }

    /// The workspace whose palette is open.
    #[must_use]
    pub fn palette_of(&self) -> Option<&str> {
        self.palette.as_ref().map(|open| open.workspace.as_str())
    }

    /// Carry out an intent that changes only the dialog; answers whether it
    /// was one. One row holds a palette, a rename or an armed delete at a time.
    pub fn local(&mut self, intent: &ManageIntent) -> bool {
        match intent {
            ManageIntent::Chip(name) => {
                let open = self.palette_of() != Some(name.as_str());
                self.clear();
                self.palette = open.then(|| RowPalette {
                    workspace: name.clone(),
                    popover: ColourPopover::default(),
                });
            }
            ManageIntent::BeginRename(name) => {
                self.clear();
                self.rename = Some(InlineRename {
                    from: name.clone(),
                    name: name.clone(),
                    refusal: None,
                    wants_focus: true,
                });
            }
            ManageIntent::CancelRename => self.rename = None,
            ManageIntent::Arm(name) => {
                self.clear();
                self.armed = Some(name.clone());
            }
            ManageIntent::Keep => self.armed = None,
            ManageIntent::Escape => {
                self.escape();
            }
            _ => return false,
        }
        true
    }

    fn clear(&mut self) {
        (self.armed, self.rename, self.palette) = (None, None, None);
    }

    /// Esc: the field, else the palette, else the armed row closes; `false`
    /// when nothing inner was open and the dialog itself closes.
    pub fn escape(&mut self) -> bool {
        if self.rename.take().is_some() || self.palette.take().is_some() {
            return true;
        }
        self.armed.take().is_some()
    }

    /// A rename was refused: the field stays with the name typed and why.
    pub fn refuse_rename(&mut self, reason: String) {
        if let Some(rename) = &mut self.rename {
            (rename.refusal, rename.wants_focus) = (Some(reason), true);
        }
    }

    /// A delete landed: its row disarms and an Undo row takes its place.
    pub fn deleted(&mut self, row: UndoRow) {
        self.clear();
        self.undo.retain(|kept| kept.workspace != row.workspace);
        self.undo.push(row);
    }

    /// An Undo landed: its row goes.
    pub fn undone(&mut self, name: &str) {
        self.undo.retain(|row| row.workspace != name);
    }

    /// The Undo row of `name` standing at `now_ms`.
    #[must_use]
    pub fn undo_row(&self, now_ms: i64) -> Option<&UndoRow> {
        self.undo.iter().rev().find(|row| now_ms < row.until_ms)
    }

    /// The frame's time: Undo rows whose ten seconds ended go.
    pub fn tick(&mut self, now_ms: i64) {
        self.undo.retain(|row| now_ms < row.until_ms);
    }

    /// A frame's keys over `rows` (each name, and whether it is protected
    /// from rename and delete) at `now_ms`: the arrows move the row focus and
    /// stop at the ends; F2, Del and C act on the focused row; Enter confirms
    /// an armed delete; Esc closes the innermost; Ctrl+Z undoes.
    pub fn keys(
        &mut self,
        keys: ManageKeys,
        rows: &[(String, bool)],
        now_ms: i64,
    ) -> Option<ManageIntent> {
        let last = rows.len().checked_sub(1);
        for (key, down) in [(ManageKey::Down, true), (ManageKey::Up, false)] {
            if keys.has(key)
                && let Some(last) = last
            {
                self.focus = Some(match (self.focus, down) {
                    (None, true) => 0,
                    (None, false) => last,
                    (Some(at), true) => (at + 1).min(last),
                    (Some(at), false) => at.min(last).saturating_sub(1),
                });
            }
        }
        if keys.has(ManageKey::Escape) {
            return Some(ManageIntent::Escape);
        }
        if keys.has(ManageKey::Undo) && self.undo_row(now_ms).is_some() {
            return Some(ManageIntent::Undo { now_ms });
        }
        if keys.has(ManageKey::Enter) && self.armed.is_some() {
            return Some(ManageIntent::Delete { now_ms });
        }
        let (name, protected) = self.focus.and_then(|at| rows.get(at))?;
        let name = name.clone();
        match () {
            () if keys.has(ManageKey::Colour) => Some(ManageIntent::Chip(name)),
            () if *protected => None,
            () if keys.has(ManageKey::F2) => Some(ManageIntent::BeginRename(name)),
            () if keys.has(ManageKey::Delete) => Some(ManageIntent::Arm(name)),
            () => None,
        }
    }

    /// A capture's face (`QUANTICK_WORKSPACE_MENU=manage:<face>`):
    /// `delete=<name>` armed, `rename=<name>` its field open,
    /// `colour=<name>` its palette open, `colour=<name>:ask=<word>` on the
    /// swap question, `focus=<row>` the row chosen. False for any other.
    pub fn stage(&mut self, face: &str) -> bool {
        let Some((what, name)) = face.split_once('=') else {
            return false;
        };
        match what {
            "delete" => self.local(&ManageIntent::Arm(name.to_owned())),
            "rename" => self.local(&ManageIntent::BeginRename(name.to_owned())),
            "colour" => {
                let (name, ask) = name.split_once(":ask=").unwrap_or((name, ""));
                self.palette = None;
                self.local(&ManageIntent::Chip(name.to_owned()));
                if let Some(open) = self.palette.as_mut().filter(|_| !ask.is_empty()) {
                    open.popover.ask(ask);
                }
                true
            }
            "focus" => {
                self.focus = name.parse().ok();
                self.focus.is_some()
            }
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "manage_workspaces_tests.rs"]
mod tests;
