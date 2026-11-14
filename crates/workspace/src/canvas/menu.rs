//! A chart's ⋯ menu and the dialog before a saved chart goes, as words and
//! decisions: which rows the menu offers, what a folded strip is called, and
//! what the dialog tells the trader. The window paints these; it words
//! nothing itself.

use super::presets::PaneKind;
use super::state::{CanvasState, PaneMode};
use super::tree::{CanvasRect, Edge, PaneSlot};
use super::widgets::{WIDGETS, WidgetSpec};

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
    /// Out of the docked layout, floating over the canvas.
    Float,
    /// From floating into the layout at this canvas edge.
    Dock(Edge),
    /// Into its own OS window.
    OpenInNewWindow,
    /// From its own OS window back over the canvas.
    ReturnToCanvas,
    Remove,
}

impl PaneMenuItem {
    /// Every row, in menu order.
    pub const ALL: [Self; 11] = [
        Self::Minimize,
        Self::Maximize,
        Self::Restore,
        Self::Float,
        Self::Dock(Edge::Left),
        Self::Dock(Edge::Right),
        Self::Dock(Edge::Top),
        Self::Dock(Edge::Bottom),
        Self::OpenInNewWindow,
        Self::ReturnToCanvas,
        Self::Remove,
    ];

    /// The words the row shows.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Minimize => "Minimize",
            Self::Maximize => "Maximize",
            Self::Restore => "Restore size",
            Self::Float => "Float",
            Self::Dock(Edge::Left) => "Dock left",
            Self::Dock(Edge::Right) => "Dock right",
            Self::Dock(Edge::Top) => "Dock top",
            Self::Dock(Edge::Bottom) => "Dock bottom",
            Self::OpenInNewWindow => "Open in new window",
            Self::ReturnToCanvas => "Return to canvas",
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
            Self::Float => "float",
            Self::Dock(Edge::Left) => "dock_left",
            Self::Dock(Edge::Right) => "dock_right",
            Self::Dock(Edge::Top) => "dock_top",
            Self::Dock(Edge::Bottom) => "dock_bottom",
            Self::OpenInNewWindow => "open_in_new_window",
            Self::ReturnToCanvas => "return_to_canvas",
            Self::Remove => "remove",
        }
    }

    /// The row an [`Self::id`] names.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.id() == id.trim())
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
/// Remove. A folded chart offers "Restore size" where Minimize stood and a
/// maximized one where Maximize stood; a docked chart offers "Float" and one
/// floating over the canvas a "Dock" row per canvas edge; a chart in its own
/// window offers "Return to canvas" instead of folding or filling, and a row
/// the canvas would refuse or that would change nothing (folding a folded
/// chart, or folding, floating or removing its last open chart) is left out
/// rather than offered.
#[must_use]
pub fn pane_menu(state: &CanvasState, slot: PaneSlot) -> Vec<PaneMenuItem> {
    let windowed = state.mode(slot) == Some(PaneMode::OsWindow);
    let floating = state.mode(slot) == Some(PaneMode::Floating);
    let unit = CanvasRect::new(0.0, 0.0, 1.0, 1.0);
    let float = !floating && !windowed && state.clone().float(slot, unit).is_ok();
    let second = if state.maximized() == Some(slot) {
        PaneMenuItem::Restore
    } else {
        PaneMenuItem::Maximize
    };
    let folded = state.mode(slot) == Some(PaneMode::Minimized);
    let (first, first_offered) = if folded {
        (PaneMenuItem::Restore, true)
    } else {
        (
            PaneMenuItem::Minimize,
            state.clone().minimize(slot) == Ok(true),
        )
    };
    let maximize = state.clone().maximize(slot).is_ok() || second == PaneMenuItem::Restore;
    let pop_out = !windowed && state.clone().pop_out(slot, "", unit).is_ok();
    let remove = state.clone().remove(slot).is_ok();
    let docks = Edge::ALL.map(|edge| (PaneMenuItem::Dock(edge), floating));
    let rows = [
        (first, first_offered),
        (second, maximize),
        (PaneMenuItem::Float, float),
    ];
    let tail = [
        (PaneMenuItem::OpenInNewWindow, pop_out),
        (PaneMenuItem::ReturnToCanvas, windowed),
        (PaneMenuItem::Remove, remove),
    ];
    (rows.into_iter().chain(docks).chain(tail))
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

/// What the add menu says of a widget surface its feed does not serve.
pub const SURFACE_UNSERVED: &str = "this feed does not serve it";

/// One add menu row: the widget, and whether it can be added now or, in
/// the canvas's own words, why not.
pub type AddRow = (&'static WidgetSpec, Result<(), String>);

/// The add menu's rows, in registry order, each with whether it can be
/// added now or, in the canvas's own words, why not: a chart while the
/// canvas has room, the order flow while the canvas holds none, and a widget
/// surface when `surface_ready` says its feed serves it.
#[must_use]
pub fn add_menu(state: &CanvasState, surface_ready: impl Fn(&WidgetSpec) -> bool) -> Vec<AddRow> {
    (WIDGETS.iter())
        .map(|widget| {
            let ready = match widget.kind {
                Some(kind) => state
                    .add_refusal(kind)
                    .map_or(Ok(()), |no| Err(no.to_string())),
                None if surface_ready(widget) => Ok(()),
                None => Err(SURFACE_UNSERVED.to_owned()),
            };
            (widget, ready)
        })
        .collect()
}
