//! What a workspace keeps: one saved way of reading a market.
//!
//! A recipe holds the canvas, the indicators on each pane by role, the layer
//! toggles, the footprint style and each pane's bar rule (the flow bar and the
//! context charts' intervals), so a workspace reopens on the timeframes it
//! saved. It never holds drawings, anchored studies or the paper position:
//! those belong to the market the tab shows. `Default` names no bar rule: its
//! tabs, and every new tab, follow what their market remembers, so one
//! market's flow bar never reaches another through it. A recipe that names
//! no rule leaves the tab's own, which is no edit. A recipe written before
//! 13f24fa3 kept its context intervals apart, under `context_bars`; the book
//! moves them onto the panes when it reads one.
//!
//! Indicators are copies, not layout ids. A layout id points into the shared
//! [`crate::layout_document::LayoutBook`], so an edit on a shared id would
//! reach every tab holding it, and a workspace's whole promise is that nothing
//! reaches another tab until Save to.
use crate::canvas::{CanvasState, preset};
use crate::indicator_document::SavedIndicator;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What the factory `Default`'s summary says: it opens as the feed declares.
pub const FACTORY_SUMMARY: &str = "as the feed opens";
const SUMMARY_SEPARATOR: &str = " \u{b7} ";

/// What the menu calls a context pane.
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
    /// The layout registry's id for this layout: the same word serde writes.
    #[must_use]
    pub const fn preset_id(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Time => "time",
            Self::TimeAndFlow => "time+flow",
            Self::TimeTimeAndFlow => "time+time+flow",
        }
    }

    /// The nearest of the four words for `preset`: the canvas a recipe
    /// carries beside it names the rest.
    #[must_use]
    pub fn nearest(preset: &crate::canvas::LayoutPreset) -> Self {
        let flow = preset.kinds.contains(&crate::canvas::PaneKind::Flow);
        match (preset.time_panes(), flow) {
            (0, _) => Self::Flow,
            (_, false) => Self::Time,
            (1, true) => Self::TimeAndFlow,
            (_, true) => Self::TimeTimeAndFlow,
        }
    }

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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextPaneRecipe {
    #[serde(default)]
    pub indicators: Vec<SavedIndicator>,
    /// This pane's layer switches, as [`WorkspaceRecipe::layers`] for the flow pane.
    #[serde(default)]
    pub layers: BTreeMap<String, bool>,
    /// This pane's bar rule, e.g. `time:300000`; absent keeps the tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bars: Option<String>,
}

/// One saved way of reading a market.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredRecipe")]
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
    /// The flow pane's bar rule, e.g. `tick:2000`; absent keeps the tab's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_bars: Option<String>,
    /// The context intervals a recipe written before 13f24fa3 kept apart from
    /// its panes, read and never written: [`Self::adopt_legacy_bars`] moves
    /// them onto the panes.
    #[serde(default, rename = "context_bars", skip_serializing)]
    pub legacy_context_bars: Vec<String>,
    /// The canvas: its pane tree, folds and floating panes. Absent in a
    /// recipe written before the canvas model, which means the canvas
    /// `layout` names, as [`Self::canvas_state`] reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<CanvasState>,
}

/// A recipe as a save holds it, healed as it is read: a canvas saved before
/// rails, beside a column fold flag that outlived a maximize, a float or an
/// add, carries that fold as its rail, so the tab it opens and the save read
/// alike ([`CanvasState::with_folds`]).
#[derive(Deserialize)]
struct StoredRecipe {
    layout: DeclaredLayout,
    #[serde(default)]
    split_fraction: Option<f32>,
    #[serde(default)]
    context_collapsed: bool,
    #[serde(default)]
    flow_collapsed: bool,
    #[serde(default)]
    context_heights: Vec<f32>,
    #[serde(default)]
    context_panes_collapsed: Vec<bool>,
    #[serde(default)]
    flow_indicators: Vec<SavedIndicator>,
    #[serde(default)]
    context_panes: Vec<ContextPaneRecipe>,
    #[serde(default)]
    layers: BTreeMap<String, bool>,
    #[serde(default)]
    footprint_style: Option<String>,
    #[serde(default)]
    flow_bars: Option<String>,
    #[serde(default, rename = "context_bars")]
    legacy_context_bars: Vec<String>,
    #[serde(default)]
    canvas: Option<CanvasState>,
}

impl From<StoredRecipe> for WorkspaceRecipe {
    fn from(stored: StoredRecipe) -> Self {
        let StoredRecipe {
            layout,
            split_fraction,
            context_collapsed,
            flow_collapsed,
            context_heights,
            context_panes_collapsed,
            flow_indicators,
            context_panes,
            layers,
            footprint_style,
            flow_bars,
            legacy_context_bars,
            canvas,
        } = stored;
        Self {
            layout,
            split_fraction,
            context_collapsed,
            flow_collapsed,
            context_heights,
            context_panes_collapsed,
            flow_indicators,
            context_panes,
            layers,
            footprint_style,
            flow_bars,
            legacy_context_bars,
            canvas: canvas.map(|canvas| canvas.with_folds(context_collapsed, flow_collapsed)),
        }
    }
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
            flow_bars: None,
            legacy_context_bars: Vec::new(),
            canvas: None,
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

    /// The canvas this recipe shows: the one it saved, or for a recipe from
    /// before the canvas model the preset its `layout` names.
    #[must_use]
    pub fn canvas_state(&self) -> CanvasState {
        self.canvas.clone().unwrap_or_else(|| {
            CanvasState::from_preset(
                preset(self.layout.preset_id())
                    .expect("every declared layout is a registered preset"),
            )
        })
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
            ..pane.clone()
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
            ..ContextPaneRecipe::default()
        };
        Self {
            layout,
            flow_indicators: indicators.to_vec(),
            context_panes: vec![pane; layout.context_panes()],
            ..Self::default()
        }
    }

    /// The context charts' bar rules over the tab's `tab` ones: the recipe's
    /// rule on each pane that has one, the tab's on the rest. A pane the tab
    /// lacks and the recipe names no rule for is left empty, which opens on
    /// the default interval.
    #[must_use]
    pub fn context_bars_over(&self, tab: &[String]) -> Vec<String> {
        let named = self
            .context_panes
            .iter()
            .rposition(|pane| pane.bars.is_some())
            .map_or(0, |last| last + 1);
        (0..tab.len().max(named))
            .map(|slot| {
                let saved = self
                    .context_panes
                    .get(slot)
                    .and_then(|pane| pane.bars.as_ref());
                saved.or(tab.get(slot)).cloned().unwrap_or_default()
            })
            .collect()
    }

    /// Move the intervals an old recipe kept apart onto its panes, each
    /// pane that names no rule of its own taking its place's.
    pub fn adopt_legacy_bars(&mut self) {
        let legacy = std::mem::take(&mut self.legacy_context_bars);
        for (pane, rule) in self.context_panes.iter_mut().zip(legacy) {
            pane.bars.get_or_insert(rule);
        }
    }

    /// This recipe naming no bar rule, as `Default` keeps it.
    #[must_use]
    pub fn without_bars(&self) -> Self {
        let mut bare = self.clone();
        bare.flow_bars = None;
        for pane in &mut bare.context_panes {
            pane.bars = None;
        }
        bare
    }

    /// `copy` read against this recipe: a bar rule this recipe does not name
    /// is the tab's own, never an edit, so it is left out.
    #[must_use]
    pub fn masked(&self, copy: &Self) -> Self {
        let mut read = copy.clone();
        if self.flow_bars.is_none() {
            read.flow_bars = None;
        }
        for (slot, pane) in read.context_panes.iter_mut().enumerate() {
            if self
                .context_panes
                .get(slot)
                .is_none_or(|mine| mine.bars.is_none())
            {
                pane.bars = None;
            }
        }
        read
    }

    /// Whether the tab holding `copy` shows this recipe, its own bar rules
    /// aside where this recipe names none.
    #[must_use]
    pub fn holds(&self, copy: &Self) -> bool {
        *self == self.masked(copy)
    }

    /// The bar rules the recipe names that differ from the tab's, by pane
    /// address: 0 is the flow pane, then the context panes top to bottom.
    #[must_use]
    pub fn rules_over<'a>(&'a self, flow: &str, context: &[String]) -> Vec<(usize, &'a str)> {
        let panes = self.context_panes.iter().map(|pane| pane.bars.as_deref());
        let tab = std::iter::once(Some(flow))
            .chain((0..).map(|slot| context.get(slot).map(String::as_str)));
        std::iter::once(self.flow_bars.as_deref())
            .chain(panes)
            .zip(tab)
            .enumerate()
            .filter_map(|(address, (rule, shown))| {
                rule.filter(|rule| Some(*rule) != shown)
                    .map(|rule| (address, rule))
            })
            .collect()
    }

    /// What differs from `base`, in the words the close prompt lists: each
    /// indicator added, removed or changed by name, then `panes`, `layers`,
    /// `footprint style` and `timeframe` once each. Empty when nothing differs.
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
                recipe.canvas.clone(),
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
        let bars = |recipe: &Self| {
            std::iter::once(recipe.flow_bars.clone())
                .chain(recipe.context_panes.iter().map(|pane| pane.bars.clone()))
                .collect::<Vec<_>>()
        };
        if bars(self) != bars(base) {
            note("timeframe".to_owned());
        }
        words
    }
}
