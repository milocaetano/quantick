//! A chart's ⋯ menu and the dialog before a saved chart goes, as words and
//! decisions: which rows the menu offers, what a folded strip is called, and
//! what the dialog tells the trader. The window paints these; it words
//! nothing itself.

use super::presets::PaneKind;
use super::state::CanvasState;
use super::tree::PaneSlot;

/// How wide the ⋯ stands at a header's right end, in points.
pub const PANE_MENU_BUTTON_PX: f32 = 24.0;
/// What a layout the catalogue has no name for is called.
const CUSTOM_LAYOUT: &str = "a custom layout";

/// One row of a chart's ⋯ menu, or the restore on its folded strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneMenuItem {
    Minimize,
    Maximize,
    /// Back to its place: from the strip, or from filling the canvas.
    Restore,
    Remove,
}

impl PaneMenuItem {
    /// The words the row shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Minimize => "Minimize",
            Self::Maximize => "Maximize",
            Self::Restore => "Restore size",
            Self::Remove => "Remove chart",
        }
    }

    /// The stable name a hook and the scene use.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Minimize => "minimize",
            Self::Maximize => "maximize",
            Self::Restore => "restore",
            Self::Remove => "remove",
        }
    }

    /// The row an [`Self::id`] names.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        [Self::Minimize, Self::Maximize, Self::Restore, Self::Remove]
            .into_iter()
            .find(|item| item.id() == id.trim())
    }
}

/// A `QUANTICK_PANE_MENU` value: `<slot>` opens that chart's menu,
/// `<slot>:<row>` presses one of its rows.
#[must_use]
pub fn menu_hook(value: &str) -> Option<(PaneSlot, Option<PaneMenuItem>)> {
    let (slot, row) = match value.split_once(':') {
        Some((slot, row)) => (slot, Some(PaneMenuItem::parse(row)?)),
        None => (value, None),
    };
    Some((slot.trim().parse().ok()?, row))
}

/// The rows of `slot`'s menu, top to bottom; a separator stands before
/// Remove. A maximized chart offers "Restore size" where Maximize stood, and
/// a row the canvas would refuse (folding or removing its last open chart)
/// is left out rather than offered and refused.
#[must_use]
pub fn pane_menu(state: &CanvasState, slot: PaneSlot) -> Vec<PaneMenuItem> {
    let second = if state.maximized() == Some(slot) {
        PaneMenuItem::Restore
    } else {
        PaneMenuItem::Maximize
    };
    let minimize = state.clone().minimize(slot).is_ok();
    let remove = state.clone().remove(slot).is_ok();
    let rows = [
        (PaneMenuItem::Minimize, minimize),
        (second, true),
        (PaneMenuItem::Remove, remove),
    ];
    rows.into_iter()
        .filter_map(|(item, offered)| offered.then_some(item))
        .collect()
}

/// The name a folded chart's strip writes along itself: its kind, its bars
/// (`1m` for a time chart, `Tick · 50` for the flow) and its market.
#[must_use]
pub fn strip_label(kind: PaneKind, bars: &str, symbol: &str) -> String {
    let name = if kind == PaneKind::Time {
        "Time"
    } else {
        "Order flow"
    };
    format!("{name} · {bars} · {symbol}")
}

/// The remove dialog's three answers, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveAnswer {
    MinimizeInstead,
    Cancel,
    Remove,
}

impl RemoveAnswer {
    pub const ALL: [Self; 3] = [Self::MinimizeInstead, Self::Cancel, Self::Remove];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::MinimizeInstead => "Minimize instead",
            Self::Cancel => "Cancel",
            Self::Remove => "Remove",
        }
    }

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::MinimizeInstead => "minimize_instead",
            Self::Cancel => "cancel",
            Self::Remove => "remove",
        }
    }
}

/// What the remove dialog says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovePrompt {
    pub title: String,
    pub lines: [String; 3],
}

/// The dialog before `slot` leaves `live`, when the workspace `name` was
/// saved as `saved`. With auto save on, the last line says the removal saves
/// at once.
#[must_use]
pub fn remove_prompt(
    name: &str,
    saved: &CanvasState,
    live: &CanvasState,
    slot: PaneSlot,
    autosave: bool,
) -> RemovePrompt {
    let label = |state: &CanvasState| state.preset().map_or(CUSTOM_LAYOUT, |preset| preset.label);
    let mut after = live.clone();
    let after = match after.remove(slot) {
        Ok(_) => label(&after),
        Err(_) => label(live),
    };
    let last = if autosave {
        format!("Auto save is on, so removing it saves “{name}” at once.")
    } else {
        format!("The saved “{name}” keeps the chart until you save.")
    };
    RemovePrompt {
        title: format!("Remove this chart from “{name}”?"),
        lines: [
            format!("“{name}” is saved with this chart ({}).", label(saved)),
            format!("Removing it changes the layout to {after}."),
            last,
        ],
    }
}
