//! What a workspace keeps: one saved way of reading a market.
//!
//! A recipe holds the canvas, the indicators on each pane by role, the layer
//! toggles and the footprint style. It never holds drawings, anchored studies,
//! a pane's timeframe, the flow bar size or the paper position: those belong to
//! the market the tab shows, so a workspace switch never moves them. A recipe
//! written while it still kept `context_bars` reads with that key ignored, and
//! the tab keeps the intervals it had.
//!
//! Indicators are copies, not layout ids. A layout id points into the shared
//! [`crate::layout_document::LayoutBook`], so an edit on a shared id would
//! reach every tab holding it, and a workspace's whole promise is that nothing
//! reaches another tab until Save to.
use crate::indicator_document::SavedIndicator;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What the factory `Default`'s summary says: it opens as the feed declares.
pub const FACTORY_SUMMARY: &str = "as the feed opens";
const SUMMARY_SEPARATOR: &str = " \u{b7} ";

/// What the menu calls a context pane: its interval is the market's, not the
/// workspace's.
const CONTEXT_PANE_LABEL: &str = "Time";

/// The canvas layout a feed declares its tabs open on (`default_layout` in
/// the TOML), named for what each layout shows.
///
/// A config-side twin of `crate::tab::CanvasLayout` rather than that enum
/// itself, so the TOML vocabulary — part of the user-facing config contract —
/// cannot drift when the canvas grows a layout a config should not name.
/// Serialized as well as deserialized: the saved workspace
/// (`crate::ui_state`) writes a canvas layout back out, and it must speak
/// the vocabulary the config reads — one name for one layout, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
pub enum DeclaredLayout {
    /// The flow pane alone — the factory default.
    #[default]
    #[serde(rename = "flow")]
    Flow,
    /// A full-window timeframe chart.
    #[serde(rename = "time")]
    Time,
    /// Timeframe left, flow right, on the draggable divider.
    #[serde(rename = "time+flow")]
    TimeAndFlow,
    /// Two timeframe charts stacked left, flow right.
    #[serde(rename = "time+time+flow")]
    TimeTimeAndFlow,
}

impl DeclaredLayout {
    /// Parse the same names the serde renames above accept, for callers
    /// outside serde (the `QUANTICK_LAYOUT` env hook). One vocabulary, one
    /// place: a name added here must be added to the renames, and the
    /// `layout_names_agree_between_serde_and_parse` test holds the two
    /// together.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "flow" => Some(DeclaredLayout::Flow),
            "time" => Some(DeclaredLayout::Time),
            "time+flow" => Some(DeclaredLayout::TimeAndFlow),
            "time+time+flow" => Some(DeclaredLayout::TimeTimeAndFlow),
            _ => None,
        }
    }
}

impl DeclaredLayout {
    /// How many context charts this layout shows.
    #[must_use]
    pub const fn context_panes(self) -> usize {
        match self {
            Self::Flow => 0,
            Self::Time | Self::TimeAndFlow => 1,
            Self::TimeTimeAndFlow => 2,
        }
    }
}

/// What a recipe keeps for one context pane; its role is its place, top to
/// bottom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPaneRecipe {
    #[serde(default)]
    pub indicators: Vec<SavedIndicator>,
    /// This pane's layer switches, as [`WorkspaceRecipe::layers`] for the flow pane.
    #[serde(default)]
    pub layers: BTreeMap<String, bool>,
}

/// One saved way of reading a market.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRecipe {
    /// Which charts the canvas shows.
    pub layout: DeclaredLayout,
    /// The context column's share of the canvas width.
    #[serde(default)]
    pub split_fraction: Option<f32>,
    /// Whether the context column is folded to its rail.
    #[serde(default)]
    pub context_collapsed: bool,
    /// Whether the flow chart is folded to its rail.
    #[serde(default)]
    pub flow_collapsed: bool,
    /// Context pane shares; zero means automatic sizing.
    #[serde(default)]
    pub context_heights: Vec<f32>,
    /// Folded context panes, top to bottom.
    #[serde(default)]
    pub context_panes_collapsed: Vec<bool>,
    /// The flow pane's indicators, in display order.
    #[serde(default)]
    pub flow_indicators: Vec<SavedIndicator>,
    /// Each context pane's indicators and layers, top to bottom.
    #[serde(default)]
    pub context_panes: Vec<ContextPaneRecipe>,
    /// The flow pane's layer switches by stable layer id; an absent layer keeps
    /// what the app decided, as in the chart-layers file.
    #[serde(default)]
    pub layers: BTreeMap<String, bool>,
    /// The footprint style's id; absent keeps the window's footprint style.
    #[serde(default)]
    pub footprint_style: Option<String>,
}

impl Default for WorkspaceRecipe {
    /// What `Default` holds in a book that never saved anything. It is never
    /// painted as it stands: see [`WorkspaceRecipe::is_factory`].
    fn default() -> Self {
        Self {
            layout: DeclaredLayout::Flow,
            split_fraction: None,
            context_collapsed: false,
            flow_collapsed: false,
            context_heights: Vec::new(),
            context_panes_collapsed: Vec::new(),
            flow_indicators: Vec::new(),
            context_panes: Vec::new(),
            layers: BTreeMap::new(),
            footprint_style: None,
        }
    }
}

impl WorkspaceRecipe {
    /// Whether this is `Default` as the factory left it, nothing saved into it.
    /// It means "what a new tab on this feed opens with", which differs by feed,
    /// so a tab on it keeps what it opened with and is never laid bare. A recipe
    /// read off a tab always carries layers, so no saved recipe reads as this.
    #[must_use]
    pub fn is_factory(&self) -> bool {
        *self == Self::default()
    }

    /// What the pane at `index` keeps, by its place: 0 is the flow pane, then
    /// the context panes top to bottom. A pane past the recipe keeps nothing.
    #[must_use]
    pub fn for_pane(&self, index: usize) -> (&[SavedIndicator], &BTreeMap<String, bool>) {
        static NOTHING: BTreeMap<String, bool> = BTreeMap::new();
        match index.checked_sub(1) {
            None => (&self.flow_indicators, &self.layers),
            Some(slot) => self
                .context_panes
                .get(slot)
                .map_or((&[], &NOTHING), |pane| (&pane.indicators, &pane.layers)),
        }
    }

    /// What a new workspace starts from: this recipe with every pane bare of
    /// indicators. The canvas, its sizes and folds, the
    /// layer switches and the footprint style stay as the tab reads them back,
    /// so the tab on it reads clean once its panes stand again.
    #[must_use]
    pub fn emptied(&self) -> Self {
        let bare = |pane: &ContextPaneRecipe| ContextPaneRecipe {
            indicators: Vec::new(),
            layers: pane.layers.clone(),
        };
        Self {
            flow_indicators: Vec::new(),
            context_panes: self.context_panes.iter().map(bare).collect(),
            ..self.clone()
        }
    }

    /// One line the workspace menu shows beside the name: the panes, then the
    /// indicator count when there is one, e.g. `Time · Flow · 3 ind`. The factory `Default` names no
    /// arrangement, so it says what it means instead.
    #[must_use]
    pub fn summary(&self) -> String {
        if self.is_factory() {
            return FACTORY_SUMMARY.to_owned();
        }
        let mut parts: Vec<String> = (0..self.layout.context_panes())
            .map(|_| CONTEXT_PANE_LABEL.to_owned())
            .collect();
        if self.layout != DeclaredLayout::Time {
            parts.push("Flow".to_owned());
        }
        let indicators = self.flow_indicators.len()
            + self
                .context_panes
                .iter()
                .map(|pane| pane.indicators.len())
                .sum::<usize>();
        if indicators > 0 {
            parts.push(format!("{indicators} ind"));
        }
        parts.join(SUMMARY_SEPARATOR)
    }

    /// What the factory `Default` amounts to on a tab whose feed declares
    /// `layout` and whose strip layout holds `indicators`.
    #[must_use]
    pub fn new_tab_arrangement(layout: DeclaredLayout, indicators: &[SavedIndicator]) -> Self {
        let pane = ContextPaneRecipe {
            indicators: indicators.to_vec(),
            layers: BTreeMap::new(),
        };
        Self {
            layout,
            flow_indicators: indicators.to_vec(),
            context_panes: vec![pane; layout.context_panes()],
            ..Self::default()
        }
    }

    /// What differs from `base`, in the words the close prompt lists: each
    /// indicator added, removed or changed by name, then `panes`, `layers` and
    /// `footprint style` once each. Empty when nothing differs.
    #[must_use]
    pub fn changes_from(&self, base: &Self) -> Vec<String> {
        let mut words: Vec<String> = Vec::new();
        let mut note = |word: String| {
            if !words.contains(&word) {
                words.push(word);
            }
        };
        let sets = |recipe: &Self| -> Vec<Vec<SavedIndicator>> {
            std::iter::once(recipe.flow_indicators.clone())
                .chain(
                    recipe
                        .context_panes
                        .iter()
                        .map(|pane| pane.indicators.clone()),
                )
                .collect()
        };
        let (mine, theirs) = (sets(self), sets(base));
        for slot in 0..mine.len().max(theirs.len()) {
            let (held, was) = (mine.get(slot), theirs.get(slot));
            let (held, was) = (
                held.map_or(&[][..], Vec::as_slice),
                was.map_or(&[][..], Vec::as_slice),
            );
            for indicator in held.iter().filter(|one| !was.contains(one)) {
                note(indicator.kind.display_name());
            }
            for indicator in was.iter().filter(|one| !held.contains(one)) {
                note(indicator.kind.display_name());
            }
        }
        let canvas = |recipe: &Self| {
            (
                recipe.layout,
                recipe.split_fraction.map(f32::to_bits),
                recipe.context_collapsed,
                recipe.flow_collapsed,
                recipe
                    .context_heights
                    .iter()
                    .map(|h| h.to_bits())
                    .collect::<Vec<_>>(),
                recipe.context_panes_collapsed.clone(),
                recipe.context_panes.len(),
            )
        };
        if canvas(self) != canvas(base) {
            note("panes".to_owned());
        }
        let layers = |recipe: &Self| {
            std::iter::once(recipe.layers.clone())
                .chain(recipe.context_panes.iter().map(|pane| pane.layers.clone()))
                .collect::<Vec<_>>()
        };
        if layers(self) != layers(base) {
            note("layers".to_owned());
        }
        if self.footprint_style != base.footprint_style {
            note("footprint style".to_owned());
        }
        words
    }
}
