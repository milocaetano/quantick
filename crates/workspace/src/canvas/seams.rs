//! The seams between a tree's docked panes, and the one resize a seam drag
//! and a remote caller share.
//!
//! A seam is the gap two neighbours of a split leave between their bands: the
//! window paints it as a divider and drags it to move the boundary. The move
//! trades share between those two neighbours only, so the split's other
//! children keep their place and the shares keep their sum; each neighbour
//! keeps [`MIN_TREE_PANE_PX`] per pane it lays along the split's axis. A
//! folded strip beside a seam is skipped: the seam trades with the nearest
//! open child past it, and the strip slides along.

use super::bands::inset_seams;
use super::metrics::COLLAPSED_HIT_PX;
#[cfg(doc)]
use super::metrics::COLLAPSED_PANE_WIDTH_PX;
use super::presets::{LayoutPreset, PaneKind};
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
    /// Whether a drag moves it: it pairs the nearest open or railed child on
    /// each side, sliding any minimized strip between them along; false when
    /// one side has only strips up to the split's end.
    pub resizable: bool,
    /// The rail folded right beside it, which dragging it reopens.
    pub rail: Option<SeamRail>,
}

/// A node a seam drag folded shut, painted as one rail.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeamRail {
    /// The first pane it folded, in reading order: what a restore names.
    pub slot: PaneSlot,
    /// Where it paints: its tree rectangle less half a divider on each seam,
    /// [`COLLAPSED_PANE_WIDTH_PX`] along `axis`.
    pub rect: CanvasRect,
    /// The axis of the split it folded in: a `Row` rail is vertical.
    pub axis: SplitAxis,
    /// Whether the open side it meets lies after it along `axis`.
    pub meets_after: bool,
}

impl SeamRail {
    /// The scene's name for the rail: its restore, as the strip's is.
    #[must_use]
    pub fn control_id(&self) -> String {
        format!("pane_menu.{}.rail", self.slot)
    }
}

impl TreeSeam {
    /// What takes the drag: the seam with `handle` points either side, or,
    /// beside a rail, the rail's [`COLLAPSED_HIT_PX`] from its outer edge
    /// through the seam, as the column painter's rail reaches.
    #[must_use]
    pub fn grab(&self, handle: f32) -> CanvasRect {
        let (at, row) = (self.rect, self.axis == SplitAxis::Row);
        let (start, end) = if row {
            (at.x - handle, at.right() + handle)
        } else {
            (at.y - handle, at.bottom() + handle)
        };
        let (start, end) = match self.rail {
            Some(rail) if rail.meets_after => {
                let outer = if row { rail.rect.x } else { rail.rect.y };
                (outer, end.max(outer + COLLAPSED_HIT_PX))
            }
            Some(rail) => {
                let outer = if row {
                    rail.rect.right()
                } else {
                    rail.rect.bottom()
                };
                (start.min(outer - COLLAPSED_HIT_PX), outer)
            }
            None => (start, end),
        };
        if row {
            CanvasRect::new(start, at.y, end - start, at.h)
        } else {
            CanvasRect::new(at.x, start, at.w, end - start)
        }
    }
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
    let before_seams = boxes.iter().take(children.len().saturating_sub(1));
    for (index, before) in before_seams.enumerate() {
        let rect = match axis {
            SplitAxis::Row => CanvasRect::new(before.right() - half, cross.y, divider, cross.h),
            SplitAxis::Column => CanvasRect::new(cross.x, before.bottom() - half, cross.w, divider),
        };
        let pair = partners(state, children, index);
        let beside = |at: usize| {
            let railed = railed(state, &children[at]) && pair.is_some();
            railed.then(|| rail_of(&children[at], rects, (area, divider), *axis, at < index + 1))
        };
        out.push(TreeSeam {
            address: SeamAddress {
                path: path.clone(),
                index,
            },
            axis: *axis,
            rect,
            resizable: pair.is_some(),
            rail: beside(index).or_else(|| beside(index + 1)),
        });
    }
    for (index, child) in children.iter().enumerate() {
        if railed(state, child) {
            continue;
        }
        path.push(index);
        collect(state, child, rects, (area, divider), path, out);
        path.pop();
    }
}

/// Every rail of `state`'s docked tree on a canvas of `area` with `divider`
/// points between neighbours: what the window paints over the folded nodes.
#[must_use]
pub fn tree_rails(state: &CanvasState, area: CanvasRect, divider: f32) -> Vec<SeamRail> {
    fn walk(
        state: &CanvasState,
        node: &PaneTree,
        at: (&[(PaneSlot, CanvasRect)], CanvasRect, f32),
        out: &mut Vec<SeamRail>,
    ) {
        let PaneTree::Split { axis, children, .. } = node else {
            return;
        };
        for (index, child) in children.iter().enumerate() {
            if railed(state, child) {
                out.push(rail_of(
                    child,
                    at.0,
                    (at.1, at.2),
                    *axis,
                    index + 1 < children.len(),
                ));
            } else {
                walk(state, child, at, out);
            }
        }
    }
    let mut out = Vec::new();
    // Nothing folded, nothing to carve: the frame's usual case costs nothing.
    if state.maximized().is_none() && !state.railed().is_empty() {
        let rects = state.rects(area);
        walk(state, state.tree(), (&rects, area, divider), &mut out);
    }
    out
}

/// `node`, folded in a split along `axis`, as the rail it paints.
fn rail_of(
    node: &PaneTree,
    rects: &[(PaneSlot, CanvasRect)],
    (area, divider): (CanvasRect, f32),
    axis: SplitAxis,
    meets_after: bool,
) -> SeamRail {
    SeamRail {
        slot: node.panes().first().map_or(0, |(slot, _)| *slot),
        rect: inset_seams(bounds(node, rects), area, divider),
        axis,
        meets_after,
    }
}

/// Whether `child` is folded: a pane minimized to its strip or railed, or a
/// split whose every pane is.
pub(super) fn pinned(state: &CanvasState, child: &PaneTree) -> bool {
    match child {
        PaneTree::Pane { slot, .. } => state.folded(*slot),
        PaneTree::Split { children, .. } => children.iter().all(|child| pinned(state, child)),
    }
}

/// Whether `child` is folded to one rail.
pub(super) fn railed(state: &CanvasState, child: &PaneTree) -> bool {
    !state.railed().is_empty() && child.railed(&|slot| state.pin(slot))
}

/// The two children the seam after child `index` trades between: the
/// nearest open or railed one at or before it and the nearest after it. A
/// strip between them keeps its extent and slides along. `None` when either
/// side holds only strips up to the split's end.
pub(super) fn partners(
    state: &CanvasState,
    children: &[PaneTree],
    index: usize,
) -> Option<(usize, usize)> {
    let partner = |at: &usize| !pinned(state, &children[*at]) || railed(state, &children[*at]);
    let first = (0..=index).rev().find(partner)?;
    let second = (index + 1..children.len()).find(partner)?;
    Some((first, second))
}

/// The rectangle `node`'s panes cover.
pub(super) fn bounds(node: &PaneTree, rects: &[(PaneSlot, CanvasRect)]) -> CanvasRect {
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

pub(super) fn extent(rect: CanvasRect, axis: SplitAxis) -> f32 {
    match axis {
        SplitAxis::Row => rect.w,
        SplitAxis::Column => rect.h,
    }
}

/// How many panes `node` lays one after another along `axis`.
pub(super) fn along(node: &PaneTree, axis: SplitAxis) -> usize {
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

pub(super) fn node_at<'tree>(tree: &'tree PaneTree, path: &[usize]) -> Option<&'tree PaneTree> {
    path.iter().try_fold(tree, |node, index| match node {
        PaneTree::Split { children, .. } => children.get(*index),
        PaneTree::Pane { .. } => None,
    })
}

pub(super) fn node_at_mut<'tree>(
    tree: &'tree mut PaneTree,
    path: &[usize],
) -> Option<&'tree mut PaneTree> {
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

/// The two open children a seam trades between, as a canvas lays them out:
/// their indices, stored shares, and how much share one point of extent is
/// worth.
struct Pair {
    first: usize,
    second: usize,
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
        if seam.index + 1 >= children.len() || shares.len() != children.len() {
            return Err(CanvasRefusal::NoSeam);
        }
        let (at_first, at_second) =
            partners(self, children, seam.index).ok_or(CanvasRefusal::NoSeam)?;
        let (first, second) = (&children[at_first], &children[at_second]);
        // A rail reopens by a drag ([`Self::drag_seam`]), never by a nudge.
        if railed(self, first) || railed(self, second) {
            return Err(CanvasRefusal::NoSeam);
        }
        let first_share = shares[at_first].max(0.0);
        let pair_share = first_share + shares[at_second].max(0.0);
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
            first: at_first,
            second: at_second,
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
        // max then min, never `clamp`: a pair exactly as wide as both minimums
        // can round its low bound past its high one, and `clamp` panics there.
        let first = wanted
            .max(pair.first_min)
            .min(pair.pair_share - pair.second_min);
        if (first - pair.first_share).abs() < STILL_PX * pair.unit {
            return Ok(false);
        }
        let Some(PaneTree::Split { shares, .. }) = node_at_mut(self.tree_mut(), &seam.path) else {
            return Err(CanvasRefusal::NoSeam);
        };
        let shares = shares.to_mut();
        shares[pair.first] = first;
        shares[pair.second] = pair.pair_share - first;
        Ok(true)
    }

    /// Move the seam at `seam` by `delta` points along its split, on a canvas
    /// of `area`: the child before it grows by what the child after it gives,
    /// each held to [`MIN_TREE_PANE_PX`] per pane along the axis. Answers
    /// whether the boundary moved.
    ///
    /// # Errors
    /// [`CanvasRefusal::NoSeam`] when the tree holds no such seam, a chart is
    /// maximized, or one side holds only folded charts; [`CanvasRefusal::NoRoom`] when
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
    /// trades with the open child whose seam a hand could drag: the next one,
    /// else the previous, past any folded strip; a folded chart itself never
    /// resizes. Held to the minimum a drag
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
        let split = node_at(self.tree(), &path).ok_or(CanvasRefusal::NoSeam)?;
        if let PaneTree::Split { children, .. } = split
            && children.get(index).is_none_or(|child| pinned(self, child))
        {
            return Err(CanvasRefusal::NoSeam);
        }
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
        let own = share.clamp(0.0, 1.0) * extent(bounds(split, &rects), axis) * pair.unit;
        let first = if pair.first == index {
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

    /// `preset` laid out with the column painter's `split`, if any, in its
    /// row: what a save keeping only the preset's name and the split
    /// rebuilds, and so what it may leave its tree out for.
    #[must_use]
    pub fn from_preset_split(preset: &LayoutPreset, split: Option<f32>) -> Self {
        let mut state = Self::from_preset(preset);
        if let Some(mut split) = split {
            state.mirror_split(preset, &mut split, true);
        }
        state
    }

    /// The model a save carries for this canvas, laid out as `preset` with the
    /// column painter's `split`, which owns it while `columns` draw: the tree
    /// as the next frame draws it, or `None` when the preset's name and the
    /// split rebuild it ([`Self::from_preset_split`]).
    #[must_use]
    pub fn saved_form(&self, preset: &LayoutPreset, split: f32, columns: bool) -> Option<Self> {
        let (mut state, mut split) = (self.clone(), split);
        state.mirror_split(preset, &mut split, columns);
        // A column fold is the save's own two flags: rebuilt, not carried.
        let (context, flow) = state.flow_side_folds();
        let rebuilt = Self::from_preset_split(preset, Some(split)).with_folds(context, flow);
        let plain = columns && state == rebuilt;
        (!plain).then_some(state)
    }

    /// Keep the column painter's `split` and this tree's row share one number
    /// while the tree is laid out as the column layout `preset`, floats and
    /// folds aside: the share its first timeframe chart's branch stores along
    /// the row. The painter drawing (`columns`) owns it and the other follows,
    /// so a fold, a float or a call never jumps the divider. With `columns`
    /// the tree takes `split`, its other children sharing the rest as they
    /// did; else `split` takes the tree's. Answers whether either changed.
    pub fn mirror_split(&mut self, preset: &LayoutPreset, split: &mut f32, columns: bool) -> bool {
        let time = (self.tree().panes().into_iter()).find(|(_, kind)| *kind == PaneKind::Time);
        let Some((path, index)) = time
            .filter(|_| self.preset() == Some(preset))
            .and_then(|(slot, _)| branch(self.tree(), slot, SplitAxis::Row))
        else {
            return false;
        };
        let Some(PaneTree::Split { shares, .. }) = node_at_mut(self.tree_mut(), &path) else {
            return false;
        };
        let sum: f32 = shares.iter().map(|share| share.max(0.0)).sum();
        let held = shares.get(index).map_or(0.0, |share| share.max(0.0));
        let rest = sum - held;
        if sum <= f32::EPSILON || (*split - held / sum).abs() <= f32::EPSILON {
            return false;
        }
        if !columns {
            *split = held / sum;
            return true;
        }
        let others = (shares.len() - 1).max(1) as f32;
        for (at, share) in shares.to_mut().iter_mut().enumerate() {
            *share = match at {
                _ if at == index => *split,
                _ if rest > f32::EPSILON => share.max(0.0) / rest * (1.0 - *split),
                _ => (1.0 - *split) / others,
            };
        }
        true
    }
}
