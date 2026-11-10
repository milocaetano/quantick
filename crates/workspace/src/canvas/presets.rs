//! The layout registry: every arrangement the canvas offers, each with the
//! pane tree it draws.
//!
//! **This table is the registry.** The popover, the window menu, the number
//! shortcuts and the control plane all read it; adding an arrangement is an
//! entry here. The trader picked the catalogue on 2026-11-10: eight layouts
//! WITH FLOW, then eight TIMEFRAMES layouts, in this order. "2 Flows" was
//! rejected and stays out.

use super::tree::{PaneTree, SplitAxis};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// What a pane is *for*.
///
/// Decides which layers it may draw and how it is seeded — never where it
/// sits. Position is the tree's business, and a kind that knew its own place
/// could not be moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    /// The order-flow pane: the liquidity heatmap, the aggression bubbles and
    /// the tape. quantick's protagonist.
    Flow,
    /// A timeframe pane: candles, indicators and drawings over time bars.
    Time,
    /// A kind that exists only under test, to prove that the layout engine
    /// does not decide anything from a pane's kind.
    ///
    /// This is not decoration. If `split_row`, `resolve_shares` or any other
    /// function that carves the canvas matched exhaustively on `PaneKind`,
    /// this variant would fail to compile the moment it was added — so the
    /// fact that the test build compiles at all *is* the proof that a new kind
    /// costs a table entry and nothing else.
    #[cfg(test)]
    Fake,
}

/// Which column of the catalogue a preset sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PresetGroup {
    /// Layouts that hold the order-flow pane.
    WithFlow,
    /// Layouts of timeframe charts only.
    Timeframes,
}

/// One named arrangement of panes.
///
/// A preset is a *seed and a recogniser*, not a state: applying one lays its
/// tree out, and a canvas whose tree still has a preset's shape reports that
/// preset's name ([`matching_preset`]). Keeping it out of the state is what
/// lets a trader drag a divider without the layout losing its name.
#[derive(Debug, Clone, Copy)]
pub struct LayoutPreset {
    /// The stable wire name — what a config file, the `QUANTICK_LAYOUT` hook,
    /// a saved workspace and the control plane all call this layout. Never the
    /// label: that is prose a reader sees, and rewording it must not move an
    /// operator's ground.
    pub id: &'static str,
    /// What the picker and the menu call it.
    pub label: &'static str,
    /// The panes it lays out, by slot: `kinds[slot]` is the kind of the
    /// tree's pane `slot`. A flow pane is always the last slot.
    pub kinds: &'static [PaneKind],
    /// The catalogue column it belongs to.
    pub group: PresetGroup,
    /// How the canvas is split between the panes.
    pub tree: &'static PaneTree,
}

/// Ids are unique in the registry (a test holds that), so a preset is its id.
impl PartialEq for LayoutPreset {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for LayoutPreset {}

/// Most panes one tab's canvas may hold at once.
///
/// A cap rather than a policy: every pane is a second `ChartState` cut from
/// the one tape, so the per-trade cost is linear in this number. Nine is what
/// the largest preset (4 TF + Flow, 8 charts) needs with one spare.
pub const MAX_CANVAS_PANES: usize = 9;

/// The context column's share of the canvas width beside the flow pane, as
/// the trader's mock drew it.
const CONTEXT_SHARE: f32 = 0.4;
/// The flow pane's share of the height when it sits on top.
const FLOW_TOP_SHARE: f32 = 0.62;

const T: PaneKind = PaneKind::Time;
const F: PaneKind = PaneKind::Flow;

const fn t(slot: u32) -> PaneTree {
    PaneTree::Pane { slot, kind: T }
}

const fn f(slot: u32) -> PaneTree {
    PaneTree::Pane { slot, kind: F }
}

/// A borrowed split, written where `&[..]` extends to `'static`.
macro_rules! split {
    ($axis:ident, [$($share:expr),+ $(,)?], [$($child:expr),+ $(,)?]) => {
        PaneTree::Split {
            axis: SplitAxis::$axis,
            children: Cow::Borrowed(&[$($child),+]),
            shares: Cow::Borrowed(&[$($share),+]),
        }
    };
}

const CTX: f32 = CONTEXT_SHARE;
const FLOW: f32 = 1.0 - CONTEXT_SHARE;
const TOP: f32 = FLOW_TOP_SHARE;
const BELOW: f32 = 1.0 - FLOW_TOP_SHARE;

/// Every layout the canvas can draw, in catalogue order: WITH FLOW, then
/// TIMEFRAMES. The first four ids are the ones configs and saved workspaces
/// have always used, kept.
pub static LAYOUT_PRESETS: &[LayoutPreset] = &[
    LayoutPreset {
        id: "flow",
        label: "Flow",
        kinds: &[F],
        group: PresetGroup::WithFlow,
        tree: &f(0),
    },
    LayoutPreset {
        id: "time+flow",
        label: "1 TF + Flow",
        kinds: &[T, F],
        group: PresetGroup::WithFlow,
        tree: &split!(Row, [CTX, FLOW], [t(0), f(1)]),
    },
    LayoutPreset {
        id: "time+time+flow",
        label: "2 TF + Flow",
        kinds: &[T, T, F],
        group: PresetGroup::WithFlow,
        tree: &split!(Row, [CTX / 2.0, CTX / 2.0, FLOW], [t(0), t(1), f(2)]),
    },
    LayoutPreset {
        id: "stack2+flow",
        label: "2 stacked + Flow",
        kinds: &[T, T, F],
        group: PresetGroup::WithFlow,
        tree: &split!(
            Row,
            [CTX, FLOW],
            [split!(Column, [0.5, 0.5], [t(0), t(1)]), f(2)]
        ),
    },
    LayoutPreset {
        id: "stack3+flow",
        label: "3 stacked + Flow",
        kinds: &[T, T, T, F],
        group: PresetGroup::WithFlow,
        tree: &split!(
            Row,
            [CTX, FLOW],
            [split!(Column, [1.0, 1.0, 1.0], [t(0), t(1), t(2)]), f(3)]
        ),
    },
    LayoutPreset {
        id: "grid4+flow",
        label: "4 TF + Flow",
        kinds: &[T, T, T, T, F],
        group: PresetGroup::WithFlow,
        tree: &split!(
            Row,
            [CTX, FLOW],
            [
                split!(
                    Column,
                    [0.5, 0.5],
                    [
                        split!(Row, [0.5, 0.5], [t(0), t(1)]),
                        split!(Row, [0.5, 0.5], [t(2), t(3)])
                    ]
                ),
                f(4)
            ]
        ),
    },
    LayoutPreset {
        id: "flow-over-time",
        label: "Flow over TF",
        kinds: &[T, F],
        group: PresetGroup::WithFlow,
        tree: &split!(Column, [TOP, BELOW], [f(1), t(0)]),
    },
    LayoutPreset {
        id: "flow-over-2time",
        label: "Flow over 2 TF",
        kinds: &[T, T, F],
        group: PresetGroup::WithFlow,
        tree: &split!(
            Column,
            [TOP, BELOW],
            [f(2), split!(Row, [0.5, 0.5], [t(0), t(1)])]
        ),
    },
    LayoutPreset {
        id: "time",
        label: "1 chart",
        kinds: &[T],
        group: PresetGroup::Timeframes,
        tree: &t(0),
    },
    LayoutPreset {
        id: "charts-2",
        label: "2 charts",
        kinds: &[T, T],
        group: PresetGroup::Timeframes,
        tree: &split!(Row, [1.0, 1.0], [t(0), t(1)]),
    },
    LayoutPreset {
        id: "charts-3",
        label: "3 charts",
        kinds: &[T, T, T],
        group: PresetGroup::Timeframes,
        tree: &split!(Row, [1.0, 1.0, 1.0], [t(0), t(1), t(2)]),
    },
    LayoutPreset {
        id: "charts-4",
        label: "4 charts",
        kinds: &[T, T, T, T],
        group: PresetGroup::Timeframes,
        tree: &split!(
            Column,
            [0.5, 0.5],
            [
                split!(Row, [0.5, 0.5], [t(0), t(1)]),
                split!(Row, [0.5, 0.5], [t(2), t(3)])
            ]
        ),
    },
    LayoutPreset {
        id: "charts-5",
        label: "5 charts",
        kinds: &[T, T, T, T, T],
        group: PresetGroup::Timeframes,
        tree: &split!(
            Column,
            [0.5, 0.5],
            [
                split!(Row, [1.0, 1.0, 1.0], [t(0), t(1), t(2)]),
                split!(Row, [0.5, 0.5], [t(3), t(4)])
            ]
        ),
    },
    LayoutPreset {
        id: "charts-6",
        label: "6 charts",
        kinds: &[T, T, T, T, T, T],
        group: PresetGroup::Timeframes,
        tree: &split!(
            Column,
            [0.5, 0.5],
            [
                split!(Row, [1.0, 1.0, 1.0], [t(0), t(1), t(2)]),
                split!(Row, [1.0, 1.0, 1.0], [t(3), t(4), t(5)])
            ]
        ),
    },
    LayoutPreset {
        id: "charts-7",
        label: "7 charts",
        kinds: &[T, T, T, T, T, T, T],
        group: PresetGroup::Timeframes,
        tree: &split!(
            Column,
            [0.5, 0.5],
            [
                split!(Row, [1.0, 1.0, 1.0, 1.0], [t(0), t(1), t(2), t(3)]),
                split!(Row, [1.0, 1.0, 1.0], [t(4), t(5), t(6)])
            ]
        ),
    },
    LayoutPreset {
        id: "charts-8",
        label: "8 charts",
        kinds: &[T, T, T, T, T, T, T, T],
        group: PresetGroup::Timeframes,
        tree: &split!(
            Column,
            [0.5, 0.5],
            [
                split!(Row, [1.0, 1.0, 1.0, 1.0], [t(0), t(1), t(2), t(3)]),
                split!(Row, [1.0, 1.0, 1.0, 1.0], [t(4), t(5), t(6), t(7)])
            ]
        ),
    },
];

/// The preset `id` names, if the registry has one.
#[must_use]
pub fn preset(id: &str) -> Option<&'static LayoutPreset> {
    LAYOUT_PRESETS.iter().find(|preset| preset.id == id.trim())
}

/// The preset whose arrangement `tree` has, slots and shares aside; `None`
/// for a custom arrangement (a dock the catalogue has no name for).
#[must_use]
pub fn matching_preset(tree: &PaneTree) -> Option<&'static LayoutPreset> {
    let shape = tree.shape();
    LAYOUT_PRESETS
        .iter()
        .find(|preset| preset.tree.shape() == shape)
}

/// The TIMEFRAMES preset with `count` charts, if the catalogue has one.
#[must_use]
pub fn timeframes_with(count: usize) -> Option<&'static LayoutPreset> {
    LAYOUT_PRESETS
        .iter()
        .find(|preset| preset.group == PresetGroup::Timeframes && preset.kinds.len() == count)
}
