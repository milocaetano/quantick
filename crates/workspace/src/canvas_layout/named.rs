//! The canvas layouts the window names: the four arrangements the column
//! painter draws, and any other registry entry, drawn from its tree.

use super::{LayoutPreset, PaneKind};
use crate::canvas::CanvasState;
use crate::workspace_recipe::{DeclaredLayout, WorkspaceRecipe};

/// How many charts a tab's canvas shows for its market (§11), and which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasLayout {
    /// The flow pane alone — quantick's default and its identity.
    #[default]
    Single,
    /// The time pane alone: a full-window timeframe chart, header included,
    /// with no split. The flow pane keeps being fed off screen, exactly as
    /// the time pane does while Single is showing.
    Time,
    /// Time pane left, flow pane right, on a draggable divider.
    TimeAndFlow,
    /// Two time panes stacked in the left column, flow pane right: the
    /// registry's "2 stack + Flow".
    TimeTimeAndFlow,
    /// Any other registry entry, drawn from its pane tree.
    Preset(&'static LayoutPreset),
}

impl CanvasLayout {
    /// The registry entry this layout is a name for.
    ///
    /// The one place a variant is turned into panes. Everything that wants to
    /// know what a layout *holds* reads the table through here rather than
    /// matching on the variant, so an arrangement added to the registry does
    /// not have to be taught to every caller one at a time.
    #[must_use]
    pub fn preset(self) -> &'static LayoutPreset {
        let id = match self {
            CanvasLayout::Single => "flow",
            CanvasLayout::Time => "time",
            CanvasLayout::TimeAndFlow => "time+flow",
            CanvasLayout::TimeTimeAndFlow => "stack2+flow",
            CanvasLayout::Preset(preset) => return preset,
        };
        super::preset(id).expect("every canvas layout names a registered preset")
    }

    /// The layout a registry entry names. The inverse of [`Self::preset`]:
    /// the four arrangements the column painter draws keep their variants,
    /// every other entry is drawn from its tree.
    #[must_use]
    pub fn from_preset(preset: &'static LayoutPreset) -> Self {
        match preset.id {
            "flow" => CanvasLayout::Single,
            "time" => CanvasLayout::Time,
            "time+flow" => CanvasLayout::TimeAndFlow,
            "stack2+flow" => CanvasLayout::TimeTimeAndFlow,
            _ => CanvasLayout::Preset(preset),
        }
    }

    /// The panes this layout draws, left to right.
    #[must_use]
    pub fn kinds(self) -> &'static [PaneKind] {
        self.preset().kinds
    }

    /// Whether this layout draws the time pane at all.
    #[must_use]
    pub fn shows_time(self) -> bool {
        self.kinds().contains(&PaneKind::Time)
    }

    /// Whether this layout draws the flow pane at all.
    #[must_use]
    pub fn shows_flow(self) -> bool {
        self.kinds().contains(&PaneKind::Flow)
    }

    /// Whether the column painter draws this layout; every other one is
    /// drawn from the canvas's pane tree.
    #[must_use]
    pub const fn is_column(self) -> bool {
        !matches!(self, CanvasLayout::Preset(_))
    }
}

impl CanvasLayout {
    /// The layout a recipe opens on: its saved canvas's preset, else its
    /// declared layout.
    #[must_use]
    pub fn of_recipe(recipe: &WorkspaceRecipe) -> Self {
        (recipe.canvas.as_ref())
            .and_then(CanvasState::preset)
            .map_or_else(|| Self::from(recipe.layout), Self::from_preset)
    }
}

/// The canvas model a recipe restores, saved or rebuilt, its fold flags in:
/// what a tab put on it shows and what the workspace menu pictures.
#[must_use]
pub fn recipe_canvas(recipe: &WorkspaceRecipe) -> CanvasState {
    let (preset, split) = (
        CanvasLayout::of_recipe(recipe).preset(),
        recipe.split_fraction,
    );
    let rebuilt = || CanvasState::from_preset_split(preset, split);
    let state = (recipe.canvas.clone()).unwrap_or_else(rebuilt);
    state.with_folds(recipe.context_collapsed, recipe.flow_collapsed)
}

impl From<DeclaredLayout> for CanvasLayout {
    fn from(declared: DeclaredLayout) -> Self {
        match declared {
            DeclaredLayout::Flow => CanvasLayout::Single,
            DeclaredLayout::Time => CanvasLayout::Time,
            DeclaredLayout::TimeAndFlow => CanvasLayout::TimeAndFlow,
            DeclaredLayout::TimeTimeAndFlow => CanvasLayout::TimeTimeAndFlow,
        }
    }
}

/// The way back, for the saved workspace, which has to
/// write a layout out in the vocabulary a config reads.
///
/// A canvas layout a config should not name would have to answer for itself
/// here — which is the point of keeping the two enums apart, and the reason
/// this conversion is total today and may not always be.
impl From<CanvasLayout> for DeclaredLayout {
    fn from(layout: CanvasLayout) -> Self {
        match layout {
            CanvasLayout::Single => DeclaredLayout::Flow,
            CanvasLayout::Time => DeclaredLayout::Time,
            CanvasLayout::TimeAndFlow => DeclaredLayout::TimeAndFlow,
            CanvasLayout::TimeTimeAndFlow => DeclaredLayout::TimeTimeAndFlow,
            CanvasLayout::Preset(preset) => DeclaredLayout::nearest(preset),
        }
    }
}
