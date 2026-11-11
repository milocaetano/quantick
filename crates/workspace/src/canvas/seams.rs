//! The seams between a tree's docked panes, and the one resize a seam drag
//! and a remote caller share.
//!
//! A seam is the gap two neighbours of a split leave between their bands: the
//! window paints it as a divider and drags it to move the boundary. The move
//! trades share between those two neighbours only, so the split's other
//! children keep their place and the shares keep their sum; each neighbour
//! keeps [`MIN_TREE_PANE_PX`] per pane it lays along the split's axis.

use super::bands::inset_seams;
use super::state::{CanvasRefusal, CanvasState};
use super::tree::{CanvasRect, PaneSlot, PaneTree, SplitAxis};

/// A boundary that moved less than this many points did not move.
const STILL_PX: f32 = 0.01;

/// The least a docked pane keeps along a split's axis when a seam moves.
pub const MIN_TREE_PANE_PX: f32 = 120.0;

/// A boundary of the tree: the seam after child `index` of the split reached
/// from the root by the child indices in `path`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SeamAddress {
    pub path: Vec<usize>,
    pub index: usize,
}

/// One seam, ready to paint and drag.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeSeam {
    pub address: SeamAddress,
    /// The axis of the split it divides: a `Row` seam is vertical and moves
    /// left and right.
    pub axis: SplitAxis,
    /// The gap between the two neighbours' bands, `divider` thick.
    pub rect: CanvasRect,
    /// Whether it moves: false beside a folded chart, whose strip is pinned.
    pub resizable: bool,
}

/// Every seam of `state`'s docked tree when the canvas is `area` with
/// `divider` points between neighbours: each split's own seams, then its
/// children's, depth first. None while a chart is maximized.
#[must_use]
pub fn tree_seams(state: &CanvasState, area: CanvasRect, divider: f32) -> Vec<TreeSeam> {
    let mut out = Vec::new();
    if state.maximized().is_none() {
        let rects = state.rects(area);
        let mut path = Vec::new();
        collect(
            state,
            state.tree(),
            &rects,
            (area, divider),
            &mut path,
            &mut out,
        );
    }
    out
}

fn collect(
    state: &CanvasState,
    node: &PaneTree,
    rects: &[(PaneSlot, CanvasRect)],
    (area, divider): (CanvasRect, f32),
    path: &mut Vec<usize>,
    out: &mut Vec<TreeSeam>,
) {
    let PaneTree::Split { axis, children, .. } = node else {
        return;
    };
    let boxes: Vec<CanvasRect> = children.iter().map(|child| bounds(child, rects)).collect();
    let Some(first) = boxes.first() else {
        return;
    };
    let cross = inset_seams(boxes.iter().copied().fold(*first, union), area, divider);
    let half = divider / 2.0;
    for index in 0..children.len().saturating_sub(1) {
        let rect = match axis {
            SplitAxis::Row => {
                CanvasRect::new(boxes[index].right() - half, cross.y, divider, cross.h)
            }
            SplitAxis::Column => {
                CanvasRect::new(cross.x, boxes[index].bottom() - half, cross.w, divider)
            }
        };
        let resizable = !pinned(state, &children[index]) && !pinned(state, &children[index + 1]);
        out.push(TreeSeam {
            address: SeamAddress {
                path: path.clone(),
                index,
            },
            axis: *axis,
            rect,
            resizable,
        });
    }
    for (index, child) in children.iter().enumerate() {
        path.push(index);
        collect(state, child, rects, (area, divider), path, out);
        path.pop();
    }
}

/// Whether `child` is a folded pane its split holds to a strip.
fn pinned(state: &CanvasState, child: &PaneTree) -> bool {
    matches!(child, PaneTree::Pane { slot, .. } if state.minimized().contains(slot))
}

/// The rectangle `node`'s panes cover.
fn bounds(node: &PaneTree, rects: &[(PaneSlot, CanvasRect)]) -> CanvasRect {
    let mut covered = (node.panes().into_iter())
        .filter_map(|(slot, _)| rects.iter().find(|(held, _)| *held == slot))
        .map(|(_, rect)| *rect);
    let first = covered
        .next()
        .unwrap_or(CanvasRect::new(0.0, 0.0, 0.0, 0.0));
    covered.fold(first, union)
}

fn union(a: CanvasRect, b: CanvasRect) -> CanvasRect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    CanvasRect::new(
        x,
        y,
        a.right().max(b.right()) - x,
        a.bottom().max(b.bottom()) - y,
    )
}

fn extent(rect: CanvasRect, axis: SplitAxis) -> f32 {
    match axis {
        SplitAxis::Row => rect.w,
        SplitAxis::Column => rect.h,
    }
}

/// How many panes `node` lays one after another along `axis`.
fn along(node: &PaneTree, axis: SplitAxis) -> usize {
    match node {
        PaneTree::Pane { .. } => 1,
        PaneTree::Split {
            axis: own,
            children,
            ..
        } => {
            let counts = children.iter().map(|child| along(child, axis));
            if *own == axis {
                counts.sum()
            } else {
                counts.max().unwrap_or(1)
            }
        }
    }
}

fn node_at<'tree>(tree: &'tree PaneTree, path: &[usize]) -> Option<&'tree PaneTree> {
    path.iter().try_fold(tree, |node, index| match node {
        PaneTree::Split { children, .. } => children.get(*index),
        PaneTree::Pane { .. } => None,
    })
}

fn node_at_mut<'tree>(tree: &'tree mut PaneTree, path: &[usize]) -> Option<&'tree mut PaneTree> {
    path.iter().try_fold(tree, |node, index| match node {
        PaneTree::Split { children, .. } => children.to_mut().get_mut(*index),
        PaneTree::Pane { .. } => None,
    })
}

/// The deepest split along `axis` holding `slot`: its path, and the child
/// that holds it.
fn branch(tree: &PaneTree, slot: PaneSlot, axis: SplitAxis) -> Option<(Vec<usize>, usize)> {
    let (mut node, mut path, mut found) = (tree, Vec::new(), None);
    while let PaneTree::Split {
        axis: own,
        children,
        ..
    } = node
    {
        let index = children.iter().position(|child| child.holds(slot))?;
        if *own == axis {
            found = Some((path.clone(), index));
        }
        path.push(index);
        node = &children[index];
    }
    found
}

/// The two neighbours a seam divides, as a canvas lays them out: their
/// stored shares, and how much share one point of extent is worth.
struct Pair {
    first_share: f32,
    pair_share: f32,
    /// Share per point along the split's axis.
    unit: f32,
    /// The least share each keeps: [`MIN_TREE_PANE_PX`] per pane it lays
    /// along the axis.
    first_min: f32,
    second_min: f32,
}

impl CanvasState {
    /// The pair `seam` divides on a canvas of `area`.
    fn pair(&self, seam: &SeamAddress, area: CanvasRect) -> Result<Pair, CanvasRefusal> {
        if self.maximized().is_some() {
            return Err(CanvasRefusal::NoSeam);
        }
        let Some(PaneTree::Split {
            axis,
            children,
            shares,
        }) = node_at(self.tree(), &seam.path)
        else {
            return Err(CanvasRefusal::NoSeam);
        };
        let (Some(first), Some(second)) = (children.get(seam.index), children.get(seam.index + 1))
        else {
            return Err(CanvasRefusal::NoSeam);
        };
        if shares.len() != children.len() || pinned(self, first) || pinned(self, second) {
            return Err(CanvasRefusal::NoSeam);
        }
        let first_share = shares[seam.index].max(0.0);
        let pair_share = first_share + shares[seam.index + 1].max(0.0);
        if pair_share <= f32::EPSILON {
            return Err(CanvasRefusal::NoSeam);
        }
        let rects = self.rects(area);
        let pair_px = extent(bounds(first, &rects), *axis) + extent(bounds(second, &rects), *axis);
        let (first_min_px, second_min_px) = (
            MIN_TREE_PANE_PX * along(first, *axis) as f32,
            MIN_TREE_PANE_PX * along(second, *axis) as f32,
        );
        if !(pair_px > 0.0 && pair_px >= first_min_px + second_min_px) {
            return Err(CanvasRefusal::NoRoom);
        }
        let unit = pair_share / pair_px;
        Ok(Pair {
            first_share,
            pair_share,
            unit,
            first_min: first_min_px * unit,
            second_min: second_min_px * unit,
        })
    }

    /// Give the child before `seam` `wanted` of the pair's share, held to
    /// each neighbour's minimum: the one place a drag and a call land.
    fn place_seam(
        &mut self,
        seam: &SeamAddress,
        pair: &Pair,
        wanted: f32,
    ) -> Result<bool, CanvasRefusal> {
        let first = wanted.clamp(pair.first_min, pair.pair_share - pair.second_min);
        if (first - pair.first_share).abs() < STILL_PX * pair.unit {
            return Ok(false);
        }
        let Some(PaneTree::Split { shares, .. }) = node_at_mut(self.tree_mut(), &seam.path) else {
            return Err(CanvasRefusal::NoSeam);
        };
        let shares = shares.to_mut();
        shares[seam.index] = first;
        shares[seam.index + 1] = pair.pair_share - first;
        Ok(true)
    }

    /// Move the seam at `seam` by `delta` points along its split, on a canvas
    /// of `area`: the child before it grows by what the child after it gives,
    /// each held to [`MIN_TREE_PANE_PX`] per pane along the axis. Answers
    /// whether the boundary moved.
    ///
    /// # Errors
    /// [`CanvasRefusal::NoSeam`] when the tree holds no such seam, a chart is
    /// maximized, or a neighbour is folded; [`CanvasRefusal::NoRoom`] when
    /// the pair is narrower than both minimums together.
    pub fn move_seam(
        &mut self,
        seam: &SeamAddress,
        delta: f32,
        area: CanvasRect,
    ) -> Result<bool, CanvasRefusal> {
        let pair = self.pair(seam, area)?;
        if !delta.is_finite() {
            return Ok(false);
        }
        self.place_seam(seam, &pair, pair.first_share + delta * pair.unit)
    }

    /// Give `slot`'s branch `share` (0..1) of the extent of the deepest split
    /// along `axis` that holds it, as [`Self::pane_share`] reads it back. It
    /// trades with the neighbour whose seam a hand could drag: the next one,
    /// else the previous, never a folded chart; held to the minimum a drag
    /// keeps. Answers whether anything moved.
    ///
    /// # Errors
    /// [`CanvasRefusal::UnknownPane`] or [`CanvasRefusal::NotDocked`] for a
    /// pane not in the tree; [`CanvasRefusal::NoSeam`] when no split along
    /// `axis` holds it or no seam beside it moves; [`CanvasRefusal::NoRoom`]
    /// as [`Self::move_seam`].
    pub fn resize_pane(
        &mut self,
        slot: PaneSlot,
        axis: SplitAxis,
        share: f32,
        area: CanvasRect,
    ) -> Result<bool, CanvasRefusal> {
        self.docked(slot)?;
        let (path, index) = branch(self.tree(), slot, axis).ok_or(CanvasRefusal::NoSeam)?;
        let (seam, pair) = [Some(index), index.checked_sub(1)]
            .into_iter()
            .flatten()
            .map(|before| {
                let seam = SeamAddress {
                    path: path.clone(),
                    index: before,
                };
                let pair = self.pair(&seam, area);
                (seam, pair)
            })
            .find(|(_, pair)| !matches!(pair, Err(CanvasRefusal::NoSeam)))
            .ok_or(CanvasRefusal::NoSeam)?;
        let pair = pair?;
        let rects = self.rects(area);
        let split = node_at(self.tree(), &path).ok_or(CanvasRefusal::NoSeam)?;
        let own = share.clamp(0.0, 1.0) * extent(bounds(split, &rects), axis) * pair.unit;
        let first = if seam.index == index {
            own
        } else {
            pair.pair_share - own
        };
        self.place_seam(&seam, &pair, first)
    }

    /// `slot`'s branch's extent over that of the deepest split along `axis`
    /// that holds it, on a canvas of `area`: what the screen shows, a folded
    /// chart's strip included. `None` when no such split holds it, or while
    /// a chart is maximized over every split.
    #[must_use]
    pub fn pane_share(&self, slot: PaneSlot, axis: SplitAxis, area: CanvasRect) -> Option<f32> {
        if self.maximized().is_some() {
            return None;
        }
        let (path, index) = branch(self.tree(), slot, axis)?;
        let split = node_at(self.tree(), &path)?;
        let PaneTree::Split { children, .. } = split else {
            return None;
        };
        let rects = self.rects(area);
        let whole = extent(bounds(split, &rects), axis);
        let own = extent(bounds(children.get(index)?, &rects), axis);
        (whole > f32::EPSILON).then(|| own / whole)
    }
}
