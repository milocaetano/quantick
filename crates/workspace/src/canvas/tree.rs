//! The split tree a canvas is carved by, and the geometry it resolves to.
//!
//! A leaf is a pane slot; a split lays its children along one axis by shares.
//! The same type describes a registry preset (built `const`, borrowed) and a
//! live canvas (owned, edited by docking and removing), so a canvas and the
//! preset it came from are compared by one rule: [`PaneTree::shape`].

use super::metrics::{CANVAS_DIVIDER_PX, COLLAPSED_PANE_WIDTH_PX};
use super::presets::PaneKind;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// How far inside the canvas an edge must sit to meet a neighbour, as
/// [`super::bands::inset_seams`] reads it.
const EDGE_SLACK_PX: f32 = 0.5;

/// How a pane holds its extent when the tree is carved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Pin {
    /// It shares the split by its share.
    Open,
    /// Minimized from its menu: a strip this many points thick.
    Strip(f32),
    /// Dragged shut by a seam: with every pane of its node, one rail.
    Rail,
}

/// A pane's identity on its canvas. Stable for the pane's life and never
/// reused while the canvas lives, so a pane keeps its slot when it floats,
/// docks or is minimized.
pub type PaneSlot = u32;

/// An axis-aligned rectangle in canvas points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CanvasRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl CanvasRect {
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Whether `point` lies inside, edges included.
    #[must_use]
    pub fn contains(self, point: CanvasPoint) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    /// This rect's size, centred in `area`.
    #[must_use]
    pub fn centred_in(self, area: Self) -> Self {
        Self::new(
            area.x + (area.w - self.w) / 2.0,
            area.y + (area.h - self.h) / 2.0,
            self.w,
            self.h,
        )
    }
}

/// A point in canvas points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasPoint {
    pub x: f32,
    pub y: f32,
}

impl CanvasPoint {
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Which way a split lays its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    /// Left to right.
    Row,
    /// Top to bottom.
    Column,
}

/// A side of the canvas, or of a docked pane, a pane can dock at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    /// The stable name the wire and the scene use.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    /// The split axis a pane docked on this side lies along, and whether it
    /// comes first along it.
    #[must_use]
    pub const fn placing(self) -> (SplitAxis, bool) {
        match self {
            Self::Left => (SplitAxis::Row, true),
            Self::Right => (SplitAxis::Row, false),
            Self::Top => (SplitAxis::Column, true),
            Self::Bottom => (SplitAxis::Column, false),
        }
    }
}

/// The canvas split tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum PaneTree {
    /// One pane.
    Pane { slot: PaneSlot, kind: PaneKind },
    /// Children along `axis`, each taking its share. Shares are renormalised
    /// when resolved, so a saved tree whose shares drifted still spends the
    /// area once.
    Split {
        axis: SplitAxis,
        children: Cow<'static, [PaneTree]>,
        shares: Cow<'static, [f32]>,
    },
}

/// A tree's arrangement with slots and shares left out: what decides whether
/// two trees are the same layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shape {
    Pane(PaneKind),
    Split(SplitAxis, Vec<Shape>),
}

impl PaneTree {
    /// A leaf, for the registry's `const` tables.
    #[must_use]
    pub const fn pane(slot: PaneSlot, kind: PaneKind) -> Self {
        Self::Pane { slot, kind }
    }

    /// A borrowed split, for the registry's `const` tables.
    #[must_use]
    pub const fn split(
        axis: SplitAxis,
        children: &'static [PaneTree],
        shares: &'static [f32],
    ) -> Self {
        Self::Split {
            axis,
            children: Cow::Borrowed(children),
            shares: Cow::Borrowed(shares),
        }
    }

    /// Every pane, in reading order (depth first).
    #[must_use]
    pub fn panes(&self) -> Vec<(PaneSlot, PaneKind)> {
        let mut out = Vec::new();
        self.walk(&mut |slot, kind| out.push((slot, kind)));
        out
    }

    fn walk(&self, visit: &mut impl FnMut(PaneSlot, PaneKind)) {
        match self {
            Self::Pane { slot, kind } => visit(*slot, *kind),
            Self::Split { children, .. } => children.iter().for_each(|child| child.walk(visit)),
        }
    }

    /// Whether `slot` is one of this tree's panes.
    #[must_use]
    pub fn holds(&self, slot: PaneSlot) -> bool {
        self.panes().iter().any(|(held, _)| *held == slot)
    }

    /// How many panes the tree lays out.
    #[must_use]
    pub fn len(&self) -> usize {
        self.panes().len()
    }

    /// Never true: a tree always holds a pane. Present for the `len` pair.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Each pane's rectangle when the tree fills `area`, in slot order.
    ///
    /// Pure shares of the area: the window insets the dividers it paints.
    /// Boundaries are cumulative, so neighbours meet exactly and the panes
    /// spend the area once.
    #[must_use]
    pub fn rects(&self, area: CanvasRect) -> Vec<(PaneSlot, CanvasRect)> {
        self.rects_with(area, &|_| Pin::Open)
    }

    /// [`Self::rects`] where `pin` may hold a pane's extent along its parent
    /// split's axis (a minimized strip, a rail); the others share what is
    /// left.
    pub(crate) fn rects_with(
        &self,
        area: CanvasRect,
        pin: &dyn Fn(PaneSlot) -> Pin,
    ) -> Vec<(PaneSlot, CanvasRect)> {
        let mut out = Vec::new();
        self.carve((area, area), pin, &mut out);
        out.sort_by_key(|(slot, _)| *slot);
        out
    }

    /// Whether any pane this node holds passes `test`, without allocating.
    pub(crate) fn any_pane(&self, test: &dyn Fn(PaneSlot) -> bool) -> bool {
        match self {
            Self::Pane { slot, .. } => test(*slot),
            Self::Split { children, .. } => children.iter().any(|child| child.any_pane(test)),
        }
    }

    /// Whether every pane this node holds is folded to a rail: the node is
    /// one rail, however many panes it lays out.
    pub(crate) fn railed(&self, pin: &dyn Fn(PaneSlot) -> Pin) -> bool {
        match self {
            Self::Pane { slot, .. } => pin(*slot) == Pin::Rail,
            Self::Split { children, .. } => children.iter().all(|child| child.railed(pin)),
        }
    }

    fn carve(
        &self,
        (area, root): (CanvasRect, CanvasRect),
        pin: &dyn Fn(PaneSlot) -> Pin,
        out: &mut Vec<(PaneSlot, CanvasRect)>,
    ) {
        let (axis, children, shares) = match self {
            Self::Pane { slot, .. } => {
                out.push((*slot, area));
                return;
            }
            Self::Split {
                axis,
                children,
                shares,
            } => (*axis, children, shares),
        };
        let (extent, (first, last), (root_first, root_last)) = match axis {
            SplitAxis::Row => (area.w, (area.x, area.right()), (root.x, root.right())),
            SplitAxis::Column => (area.h, (area.y, area.bottom()), (root.y, root.bottom())),
        };
        let count = children.len();
        // A rail buys back the half divider the window insets on each of its
        // edges that meets a neighbour, so it paints the column painter's width.
        let rail = |index: usize| {
            let before = index > 0 || first > root_first + EDGE_SLACK_PX;
            let after = index + 1 < count || last < root_last - EDGE_SLACK_PX;
            COLLAPSED_PANE_WIDTH_PX
                + f32::from(u8::from(before) + u8::from(after)) * CANVAS_DIVIDER_PX / 2.0
        };
        let mut pinned: Vec<Option<f32>> = (children.iter().enumerate())
            .map(|(index, child)| {
                (count > 1)
                    .then(|| child.folded_extent(axis, pin, rail(index)))
                    .flatten()
            })
            .collect();
        let mut pinned_total: f32 = pinned.iter().flatten().sum();
        // Every child folded: this split is itself a strip its parent pinned,
        // so its strips share whatever it was given in proportion.
        if pinned.iter().all(Option::is_some) && pinned_total > f32::EPSILON {
            let scale = extent / pinned_total;
            pinned.iter_mut().flatten().for_each(|px| *px *= scale);
            pinned_total = extent;
        }
        let free = (extent - pinned_total).max(0.0);
        let open_share: f32 = shares
            .iter()
            .zip(&pinned)
            .filter(|(_, pin)| pin.is_none())
            .map(|(share, _)| share.max(0.0))
            .sum();
        let open_count = pinned.iter().filter(|pin| pin.is_none()).count().max(1);
        let mut start = 0.0_f32;
        let mut spent_share = 0.0_f32;
        for (index, child) in children.iter().enumerate() {
            let end = if let Some(px) = pinned[index] {
                start + px
            } else {
                let share = shares.get(index).copied().unwrap_or(0.0).max(0.0);
                spent_share += if open_share > f32::EPSILON {
                    share / open_share
                } else {
                    1.0 / open_count as f32
                };
                pinned_before(&pinned, index) + free * spent_share
            };
            // Rails wider than a narrow canvas end at its edge, never past
            // it or before their own start.
            let end = if index + 1 == children.len() {
                extent
            } else {
                end.min(extent)
            }
            .max(start);
            let piece = match axis {
                SplitAxis::Row => CanvasRect::new(area.x + start, area.y, end - start, area.h),
                SplitAxis::Column => CanvasRect::new(area.x, area.y + start, area.w, end - start),
            };
            child.carve((piece, root), pin, out);
            start = end;
        }
    }

    /// How far this node reaches along `axis` when `pin` holds every pane
    /// it holds: a node folded to a rail `rail`, however many panes it lays
    /// out; a minimized pane its strip, and a split of them one strip across
    /// `axis` or its strips end to end along it. `None` while any pane it
    /// holds is open.
    fn folded_extent(
        &self,
        axis: SplitAxis,
        pin: &dyn Fn(PaneSlot) -> Pin,
        rail: f32,
    ) -> Option<f32> {
        match self {
            Self::Pane { slot, .. } => match pin(*slot) {
                Pin::Strip(px) => Some(px),
                Pin::Rail => Some(rail),
                Pin::Open => None,
            },
            Self::Split {
                axis: own,
                children,
                ..
            } => {
                let extents = (children.iter())
                    .map(|child| child.folded_extent(axis, pin, rail))
                    .collect::<Option<Vec<f32>>>()?;
                // Asked only once every pane is folded, so an open tree pays
                // nothing for it.
                if self.railed(pin) {
                    return Some(rail);
                }
                if *own == axis {
                    Some(extents.iter().sum())
                } else {
                    extents.into_iter().reduce(f32::max)
                }
            }
        }
    }

    /// The arrangement without slots or shares, with a split nested in a split
    /// of the same axis flattened into it: what [`matching_preset`] compares.
    ///
    /// [`matching_preset`]: super::presets::matching_preset
    pub(crate) fn shape(&self) -> Shape {
        match self {
            Self::Pane { kind, .. } => Shape::Pane(*kind),
            Self::Split { axis, children, .. } => {
                let mut flat = Vec::new();
                for child in children.iter() {
                    match child.shape() {
                        Shape::Split(inner, grand) if inner == *axis => flat.extend(grand),
                        other => flat.push(other),
                    }
                }
                if flat.len() == 1 {
                    flat.remove(0)
                } else {
                    Shape::Split(*axis, flat)
                }
            }
        }
    }

    /// The tree without `slot`; `None` when `slot` was the whole tree. A
    /// split left with one child becomes that child, and its shares are
    /// renormalised over who remains.
    #[must_use]
    pub fn without(&self, slot: PaneSlot) -> Option<Self> {
        match self {
            Self::Pane { slot: held, .. } => (*held != slot).then(|| self.clone()),
            Self::Split {
                axis,
                children,
                shares,
            } => {
                let mut kept = Vec::new();
                let mut kept_shares = Vec::new();
                for (child, share) in children.iter().zip(shares.iter()) {
                    if let Some(child) = child.without(slot) {
                        kept.push(child);
                        kept_shares.push(*share);
                    }
                }
                match kept.len() {
                    0 => None,
                    1 => kept.pop(),
                    _ => Some(Self::owned_split(*axis, kept, kept_shares)),
                }
            }
        }
    }

    /// `pane` inserted at the start or end of the root along `axis`, taking a
    /// fair share of it: one more of the panes the tree lays side by side
    /// along `axis`, so a dock beside a 2x2 grid takes a third, not half.
    /// What docking at a canvas edge does.
    #[must_use]
    pub fn docked(&self, pane: Self, axis: SplitAxis, at_start: bool) -> Self {
        let new_share = 1.0 / (self.across(axis) as f32 + 1.0);
        let (mut children, mut shares) = match self {
            Self::Split {
                axis: root,
                children,
                shares,
            } if *root == axis => (children.to_vec(), normalised(shares)),
            _ => (vec![self.clone()], vec![1.0]),
        };
        shares
            .iter_mut()
            .for_each(|share| *share *= 1.0 - new_share);
        let at = if at_start { 0 } else { children.len() };
        children.insert(at, pane);
        shares.insert(at, new_share);
        Self::owned_split(axis, children, shares)
    }

    /// How many panes this node lays side by side along `axis`: a pane one,
    /// a split along `axis` its children's sum, one across it their most.
    fn across(&self, axis: SplitAxis) -> usize {
        match self {
            Self::Pane { .. } => 1,
            Self::Split {
                axis: own,
                children,
                ..
            } => {
                let each = children.iter().map(|child| child.across(axis));
                if *own == axis {
                    each.sum()
                } else {
                    each.max().unwrap_or(1)
                }
            }
        }
    }

    /// `pane` docked on `side` of the pane `slot`. When `slot`'s split
    /// already lays along that side's axis, `pane` joins it beside `slot` and
    /// takes a fair share of it; otherwise `slot` becomes a split of the two,
    /// half each. `None` when the tree holds no `slot`.
    #[must_use]
    pub fn docked_beside(&self, slot: PaneSlot, pane: Self, side: Edge) -> Option<Self> {
        if !self.holds(slot) {
            return None;
        }
        let (axis, at_start) = side.placing();
        Some(self.beside(slot, &pane, axis, at_start))
    }

    fn beside(&self, slot: PaneSlot, pane: &Self, axis: SplitAxis, at_start: bool) -> Self {
        let pair = |held: &Self| {
            let pair = if at_start {
                vec![pane.clone(), held.clone()]
            } else {
                vec![held.clone(), pane.clone()]
            };
            Self::owned_split(axis, pair, vec![0.5, 0.5])
        };
        let Self::Split {
            axis: own,
            children,
            shares,
        } = self
        else {
            return pair(self);
        };
        let leaf = |child: &Self| matches!(child, Self::Pane { slot: held, .. } if *held == slot);
        if let Some(index) = children.iter().position(leaf)
            && *own == axis
        {
            let new_share = 1.0 / (children.len() as f32 + 1.0);
            let mut shares = normalised(shares);
            shares
                .iter_mut()
                .for_each(|share| *share *= 1.0 - new_share);
            let at = if at_start { index } else { index + 1 };
            let mut children = children.to_vec();
            children.insert(at, pane.clone());
            shares.insert(at, new_share);
            return Self::owned_split(axis, children, shares);
        }
        let children = (children.iter())
            .map(|child| match child {
                child if leaf(child) => pair(child),
                child if child.holds(slot) => child.beside(slot, pane, axis, at_start),
                child => child.clone(),
            })
            .collect();
        Self::owned_split(*own, children, shares.to_vec())
    }

    /// The tree with the panes `a` and `b` in each other's place, each
    /// keeping its slot and kind; shares stay with the places. Unchanged
    /// unless the tree holds both.
    #[must_use]
    pub fn swap(&self, a: PaneSlot, b: PaneSlot) -> Self {
        let panes = self.panes();
        let kind = |slot| {
            panes
                .iter()
                .find(|(held, _)| *held == slot)
                .map(|(_, k)| *k)
        };
        let (Some(kind_a), Some(kind_b)) = (kind(a), kind(b)) else {
            return self.clone();
        };
        self.replaced(&|slot, kind| match slot {
            slot if slot == a => (b, kind_b),
            slot if slot == b => (a, kind_a),
            _ => (slot, kind),
        })
    }

    /// The same tree with every pane `(slot, kind)` replaced by
    /// `map(slot, kind)`; shares stay.
    pub(crate) fn replaced(
        &self,
        map: &impl Fn(PaneSlot, PaneKind) -> (PaneSlot, PaneKind),
    ) -> Self {
        match self {
            Self::Pane { slot, kind } => {
                let (slot, kind) = map(*slot, *kind);
                Self::pane(slot, kind)
            }
            Self::Split {
                axis,
                children,
                shares,
            } => Self::Split {
                axis: *axis,
                children: Cow::Owned(children.iter().map(|child| child.replaced(map)).collect()),
                shares: shares.clone(),
            },
        }
    }

    fn owned_split(axis: SplitAxis, children: Vec<Self>, shares: Vec<f32>) -> Self {
        Self::Split {
            axis,
            children: Cow::Owned(children),
            shares: Cow::Owned(normalised(&shares)),
        }
    }

    /// The same tree with every pane's slot replaced by `map(slot)`.
    #[must_use]
    pub fn relabelled(&self, map: &impl Fn(PaneSlot) -> PaneSlot) -> Self {
        self.replaced(&|slot, kind| (map(slot), kind))
    }
}

fn pinned_before(pinned: &[Option<f32>], index: usize) -> f32 {
    pinned[..=index].iter().flatten().sum()
}

fn normalised(shares: &[f32]) -> Vec<f32> {
    let sum: f32 = shares.iter().map(|share| share.max(0.0)).sum();
    if sum > f32::EPSILON {
        shares.iter().map(|share| share.max(0.0) / sum).collect()
    } else {
        vec![1.0 / shares.len().max(1) as f32; shares.len()]
    }
}
