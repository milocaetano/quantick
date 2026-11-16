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

impl ManageKey {
    pub const ALL: [Self; 8] = [
        Self::Up,
        Self::Down,
        Self::F2,
        Self::Delete,
        Self::Colour,
        Self::Enter,
        Self::Escape,
        Self::Undo,
    ];

    /// The name the window's toolkit knows the key by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Up => "ArrowUp",
            Self::Down => "ArrowDown",
            Self::F2 => "F2",
            Self::Delete => "Delete",
            Self::Colour => "C",
            Self::Enter => "Enter",
            Self::Escape => "Escape",
            Self::Undo => "Z",
        }
    }

    /// Pressed with Ctrl (Cmd on a Mac).
    #[must_use]
    pub const fn command(self) -> bool {
        matches!(self, Self::Undo)
    }
}

/// The keys pressed this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManageKeys([bool; 8]);

impl ManageKeys {
    /// From one flag per [`ManageKey::ALL`].
    #[must_use]
    pub const fn of(pressed: [bool; 8]) -> Self {
        Self(pressed)
    }

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
    /// A capture staged the armed row's Delete for the first frame.
    pub confirm_staged: bool,
    /// The chosen row or the row just opened scrolls into view next frame.
    pub reveal: bool,
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
            confirm_staged: false,
            reveal: false,
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

    /// Whether the dialog takes `key` from the window at `now_ms` over
    /// `rows` (each name and whether it is protected): only a key that acts.
    /// None while a row's palette takes its own keys, only Esc while a name
    /// is typed; C with a chosen row, F2 and Del with a chosen row that is
    /// not protected, Enter on an armed one, Ctrl+Z while an Undo row stands.
    #[must_use]
    pub fn takes(&self, key: ManageKey, now_ms: i64, rows: &[(String, bool)]) -> bool {
        let open = (self.focus)
            .and_then(|at| rows.get(at))
            .is_some_and(|row| !row.1);
        let free = self.palette.is_none() && (self.rename.is_none() || key == ManageKey::Escape);
        free && match key {
            ManageKey::F2 | ManageKey::Delete => open,
            ManageKey::Colour => self.focus.is_some(),
            ManageKey::Enter => self.armed.is_some(),
            ManageKey::Undo => self.undo_row(now_ms).is_some(),
            ManageKey::Up | ManageKey::Down | ManageKey::Escape => true,
        }
    }

    /// What the window types into: the rename field with its first-focus
    /// flag, and the open row palette.
    pub fn typed(&mut self) -> (Option<(&mut String, &mut bool)>, Option<&mut ColourPopover>) {
        let field = (self.rename.as_mut()).map(|it| (&mut it.name, &mut it.wants_focus));
        (field, self.palette.as_mut().map(|open| &mut open.popover))
    }

    /// The workspace whose palette is open.
    #[must_use]
    pub fn palette_of(&self) -> Option<&str> {
        self.palette.as_ref().map(|open| open.workspace.as_str())
    }

    /// Carry out an intent that changes only the dialog; answers whether it
    /// was one. One row holds a palette, a rename or an armed delete at a time.
    pub fn local(&mut self, intent: &ManageIntent) -> bool {
        self.reveal = true;
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
            _ => {
                self.reveal = false;
                return false;
            }
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

    /// The frame's time: Undo rows whose ten seconds ended go. Answers
    /// whether one still stands, so the frame keeps its line moving.
    pub fn tick(&mut self, now_ms: i64) -> bool {
        self.undo.retain(|row| now_ms < row.until_ms);
        !self.undo.is_empty()
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
                self.reveal = true;
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
        let staged = std::mem::take(&mut self.confirm_staged);
        if (staged || keys.has(ManageKey::Enter)) && self.armed.is_some() {
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
    /// swap question, `focus=<row>` the row chosen, `undo=<name>` deleted on
    /// the first frame so its Undo row stands. False for any other.
    pub fn stage(&mut self, face: &str) -> bool {
        let Some((what, name)) = face.split_once('=') else {
            return false;
        };
        match what {
            "delete" => self.local(&ManageIntent::Arm(name.to_owned())),
            "undo" => {
                self.confirm_staged = self.local(&ManageIntent::Arm(name.to_owned()));
                true
            }
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
