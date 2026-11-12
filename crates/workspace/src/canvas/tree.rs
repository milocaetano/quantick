//! The split tree a canvas is carved by, and the geometry it resolves to.
//!
//! A leaf is a pane slot; a split lays its children along one axis by shares.
//! The same type describes a registry preset (built `const`, borrowed) and a
//! live canvas (owned, edited by docking and removing), so a canvas and the
//! preset it came from are compared by one rule: [`PaneTree::shape`].

use super::presets::PaneKind;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

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
        self.rects_with(area, &|_| None)
    }

    /// [`Self::rects`] where `fixed` may pin a pane's extent along its parent
    /// split's axis (a minimized strip); the others share what is left.
    pub(crate) fn rects_with(
        &self,
        area: CanvasRect,
        fixed: &dyn Fn(PaneSlot) -> Option<f32>,
    ) -> Vec<(PaneSlot, CanvasRect)> {
        let mut out = Vec::new();
        self.carve(area, fixed, &mut out);
        out.sort_by_key(|(slot, _)| *slot);
        out
    }

    fn carve(
        &self,
        area: CanvasRect,
        fixed: &dyn Fn(PaneSlot) -> Option<f32>,
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
        let extent = match axis {
            SplitAxis::Row => area.w,
            SplitAxis::Column => area.h,
        };
        let mut pinned: Vec<Option<f32>> = children
            .iter()
            .map(|child| {
                (children.len() > 1)
                    .then(|| child.folded_extent(axis, fixed))
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
            let end = if index + 1 == children.len() {
                extent
            } else {
                end
            };
            let piece = match axis {
                SplitAxis::Row => CanvasRect::new(area.x + start, area.y, end - start, area.h),
                SplitAxis::Column => CanvasRect::new(area.x, area.y + start, area.w, end - start),
            };
            child.carve(piece, fixed, out);
            start = end;
        }
    }

    /// How far this node reaches along `axis` when `fixed` pins every pane
    /// it holds: a pinned pane its strip, a split of pinned panes one strip
    /// across `axis` or its strips end to end along it. `None` while any
    /// pane it holds is open.
    fn folded_extent(
        &self,
        axis: SplitAxis,
        fixed: &dyn Fn(PaneSlot) -> Option<f32>,
    ) -> Option<f32> {
        match self {
            Self::Pane { slot, .. } => fixed(*slot),
            Self::Split {
                axis: own,
                children,
                ..
            } => {
                let extents = (children.iter())
                    .map(|child| child.folded_extent(axis, fixed))
                    .collect::<Option<Vec<f32>>>()?;
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

    /// `pane` inserted at the start or end of the root along `axis`, taking an
    /// even share of it: what docking at a canvas edge does.
    #[must_use]
    pub fn docked(&self, pane: Self, axis: SplitAxis, at_start: bool) -> Self {
        let (mut children, mut shares) = match self {
            Self::Split {
                axis: root,
                children,
                shares,
            } if *root == axis => (children.to_vec(), normalised(shares)),
            _ => (vec![self.clone()], vec![1.0]),
        };
        let count = children.len() as f32;
        let new_share = 1.0 / (count + 1.0);
        shares
            .iter_mut()
            .for_each(|share| *share *= 1.0 - new_share);
        if at_start {
            children.insert(0, pane);
            shares.insert(0, new_share);
        } else {
            children.push(pane);
            shares.push(new_share);
        }
        Self::owned_split(axis, children, shares)
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
        match self {
            Self::Pane { slot, kind } => Self::pane(map(*slot), *kind),
            Self::Split {
                axis,
                children,
                shares,
            } => Self::Split {
                axis: *axis,
                children: Cow::Owned(children.iter().map(|child| child.relabelled(map)).collect()),
                shares: shares.clone(),
            },
        }
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
