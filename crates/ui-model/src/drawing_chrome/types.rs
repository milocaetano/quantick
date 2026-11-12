use super::Drawing;
use quantick_chart_interaction::pane::PaneSide;

/// Which inspector tab is open. Tabs exist per capability: every tool gets
/// Style and Coordinates; a tool that brings its own tab (the Fib level
/// editor) mounts it as Extra without the central code knowing its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    Style,
    Extra,
    Coordinates,
}

/// Which default-style button the Style tab was pressed on, so the caller can
/// say out loud that something was remembered — a silent save leaves the
/// trader wondering whether it took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavedDefault {
    OneTool,
    EveryTool,
    Forgotten,
}

impl SavedDefault {
    pub const fn message(self) -> &'static str {
        match self {
            Self::OneTool => "Saved - new drawings of this tool open configured like this one.",
            Self::EveryTool => "Saved - every new drawing opens with this colour, width and fill.",
            Self::Forgotten => "Reset - this tool goes back to how it opened out of the box.",
        }
    }
}

/// What the inspector body asked for this frame. The applier owns every
/// mutation, so the pinned panel and the floating window share one rule set.
#[derive(Debug, Default, Clone, Copy)]
pub struct InspectorActions {
    pub toggle_hidden: bool,
    pub toggle_lock: bool,
    pub toggle_pin: bool,
    pub delete: bool,
    pub cancel_delete: bool,
    pub force_delete: bool,
    pub close: bool,
    pub edited: bool,
    /// Which default-style button was pressed, if any.
    pub saved_default: Option<SavedDefault>,
}

impl InspectorActions {
    /// Fold another frame section's requests into this one — the title bar
    /// and the body each report intent, the applier takes the union.
    pub fn merge(&mut self, other: Self) {
        self.toggle_hidden |= other.toggle_hidden;
        self.toggle_lock |= other.toggle_lock;
        self.toggle_pin |= other.toggle_pin;
        self.delete |= other.delete;
        self.cancel_delete |= other.cancel_delete;
        self.force_delete |= other.force_delete;
        self.close |= other.close;
        self.edited |= other.edited;
        self.saved_default = self.saved_default.or(other.saved_default);
    }
}

/// An edit in flight: which object, on which pane of which tab, and what it
/// said before the first keystroke or the first drag.
///
/// One record for both kinds of edit — the inline field's and the inspector's
/// — because they are the same fact and end at the same call. Two identical
/// structs would be two merge rules and two host branches to keep in step, and
/// a fix applied to one would not reach the other.
///
/// The tab and the pane, not the index alone: an index identifies nothing once
/// there are two panes, let alone two tabs. Switching tabs with an editor open
/// would otherwise record the note's undo entry against whatever object
/// happened to sit at that index on the tab now in front — swapping an
/// unrelated drawing for a copy of the note on the next Ctrl+Z.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawingEdit {
    pub tab: u64,
    pub side: PaneSide,
    pub index: usize,
    pub before: Drawing,
}
