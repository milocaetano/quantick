//! What a workspace keeps: one saved way of reading a market.
//!
//! A recipe holds the canvas, the context intervals, the indicators on each
//! pane by role, the layer toggles and the footprint style. It never holds
//! drawings, anchored studies, the flow bar size or the paper position: those
//! belong to the market ([`crate::market_settings`]).
//!
//! Indicators are copies, not layout ids. A layout id points into the shared
//! [`crate::layout_document::LayoutBook`], so an edit on a shared id would
//! reach every tab holding it, and a workspace's whole promise is that nothing
//! reaches another tab until Save to.
use crate::indicator_document::SavedIndicator;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The canvas layout a feed declares its tabs open on (`default_layout` in
/// the TOML), named for what each layout shows.
///
/// A config-side twin of `crate::tab::CanvasLayout` rather than that enum
/// itself, so the TOML vocabulary — part of the user-facing config contract —
/// cannot drift when the canvas grows a layout a config should not name.
/// Serialized as well as deserialized: the saved workspace
/// (`crate::ui_state`) writes a canvas layout back out, and it must speak
/// the vocabulary the config reads — one name for one layout, in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum DeclaredLayout {
    /// The flow pane alone — the factory default.
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
/// bottom, the same place `context_bars` gives its interval.
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
    /// Every context chart's interval, top to bottom, as `kind:parameter`.
    #[serde(default)]
    pub context_bars: Vec<String>,
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
            context_bars: Vec::new(),
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
}
