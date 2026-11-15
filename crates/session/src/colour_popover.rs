//! The palette popover under the workspace menu's header swatch, apart from
//! its pixels and its colours: which face it shows (the grid, Custom open
//! under it, or the swap question in its place), the focus the keys move and
//! what a key does. Colours travel as the words a call takes (`sky`,
//! `#3A7BD5`); what a pick does is decided where the book is seen.

/// What a press in the popover asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColourIntent {
    /// The header swatch: open the popover, or close it.
    Toggle,
    Close,
    /// A swatch, by the word a call names it with: a preset's key or
    /// `last_custom`.
    Pick(String),
    /// Custom…: fold the picker open on the current colour as `dark` paints
    /// it, or away.
    Custom {
        dark: bool,
    },
    /// The Custom field's colour.
    Apply,
    /// The question: Swap (`true`) or Cancel.
    Answer(bool),
    /// A press or drag on the saturation and value square, in thousandths
    /// from its left and top.
    Square {
        x: u16,
        y: u16,
    },
    /// A press or drag on the hue strip, in thousandths from its left.
    Hue(u16),
}

/// A key the popover reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Tab,
    Enter,
    Space,
    Escape,
}

impl Key {
    /// The name the window's toolkit knows the key by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "ArrowLeft",
            Self::Right => "ArrowRight",
            Self::Up => "ArrowUp",
            Self::Down => "ArrowDown",
            Self::Tab => "Tab",
            Self::Enter => "Enter",
            Self::Space => "Space",
            Self::Escape => "Escape",
        }
    }

    pub const ALL: [Self; 8] = [
        Self::Left,
        Self::Right,
        Self::Up,
        Self::Down,
        Self::Tab,
        Self::Enter,
        Self::Space,
        Self::Escape,
    ];
}

/// The keys pressed this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Keys([bool; 8]);

impl Keys {
    pub fn press(&mut self, key: Key) {
        self.0[key as usize] = true;
    }

    /// From one flag per [`Key::ALL`].
    #[must_use]
    pub const fn of(pressed: [bool; 8]) -> Self {
        Self(pressed)
    }

    fn has(self, key: Key) -> bool {
        self.0[key as usize]
    }
}

/// What a frame's keys did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAct {
    None,
    Close,
    /// The focused grid cell: a swatch, or Custom… last.
    Press(usize),
    /// Custom's Apply, the Enter default while it is open.
    Apply,
    /// The question's Swap, reached by Tab.
    Swap,
    /// The question's Cancel, reached by Tab.
    Cancel,
}

/// The grid's width in cells.
pub const COLUMNS: usize = 4;

/// The popover's state while it is open.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ColourPopover {
    /// The grid cell the arrows moved to, or the question's button Tab
    /// reached (0 Swap, 1 Cancel).
    pub focus: Option<usize>,
    /// The colour word the swap question asks about, in the grid's place.
    pub asked: Option<String>,
    /// Custom is folded open under the grid.
    pub custom: bool,
    /// Custom's field, which the window's text field edits in place.
    pub hex: String,
    /// Where the square and the strip were last pressed: hue, saturation
    /// and value, each 0 to 1. The field's colour moves it when they differ.
    pub hsv: [f32; 3],
}

impl ColourPopover {
    /// Whether the popover takes `key` from the window this frame: every key
    /// it reads, but while Custom's field is typed in only Enter and Esc, so
    /// the arrows move the caret.
    #[must_use]
    pub fn takes(&self, key: Key) -> bool {
        self.asked.is_some() || !self.custom || matches!(key, Key::Enter | Key::Escape)
    }

    /// The question replaces the grid; nothing is focused, so a reflex
    /// Enter answers nothing.
    pub fn ask(&mut self, word: &str) {
        (self.asked, self.focus) = (Some(word.to_owned()), None);
    }

    /// Cancel: back to the grid, Custom still open if it asked.
    pub fn back(&mut self) {
        (self.asked, self.focus) = (None, None);
    }

    /// Custom… folds the picker open on `hex` at `hsv`, or away again.
    pub fn open_custom(&mut self, hex: String, hsv: [f32; 3]) {
        self.custom = !self.custom;
        if self.custom {
            (self.hex, self.hsv) = (hex, hsv);
        }
    }

    /// A frame's keys over `cells` grid cells, `current` the workspace's own
    /// swatch: Esc closes; the question answers only what Tab reached;
    /// Custom open takes Enter as Apply; the grid moves its focus and
    /// presses it.
    pub fn keys(&mut self, keys: Keys, cells: usize, current: Option<usize>) -> KeyAct {
        let hit = |key| keys.has(key);
        if hit(Key::Escape) {
            return KeyAct::Close;
        }
        let pressed = hit(Key::Enter) || hit(Key::Space);
        if self.asked.is_some() {
            if hit(Key::Tab) {
                self.focus = Some(self.focus.map_or(0, |at| (at + 1) % 2));
            }
            return match (pressed, self.focus) {
                (true, Some(0)) => KeyAct::Swap,
                (true, Some(_)) => KeyAct::Cancel,
                _ => KeyAct::None,
            };
        }
        if self.custom {
            return if hit(Key::Enter) {
                KeyAct::Apply
            } else {
                KeyAct::None
            };
        }
        let moves = [Key::Left, Key::Right, Key::Up, Key::Down];
        if let Some(key) = moves.into_iter().find(|key| hit(*key)) {
            self.focus = Some(match self.focus {
                None => current.unwrap_or(0).min(cells.saturating_sub(1)),
                Some(at) => step(at, key, cells),
            });
        }
        match (pressed, self.focus) {
            (true, Some(at)) if at < cells => KeyAct::Press(at),
            _ => KeyAct::None,
        }
    }
}

/// From cell `at` one step toward `key` in a grid of `cells`, [`COLUMNS`]
/// wide, stopping at its edges.
fn step(at: usize, key: Key, cells: usize) -> usize {
    let rows = cells.div_ceil(COLUMNS);
    let width = |row: usize| (cells - row * COLUMNS).min(COLUMNS);
    let (row, column) = (at / COLUMNS, at % COLUMNS);
    let to = |row: usize| row * COLUMNS + column.min(width(row) - 1);
    match key {
        Key::Left => at - usize::from(column > 0),
        Key::Right => at + usize::from(column + 1 < width(row)),
        Key::Up if row > 0 => to(row - 1),
        Key::Down if row + 1 < rows => to(row + 1),
        _ => at,
    }
}

#[cfg(test)]
#[path = "colour_popover_tests.rs"]
mod tests;
