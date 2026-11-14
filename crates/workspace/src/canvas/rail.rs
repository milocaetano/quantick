//! The rail a pane folds to when its divider is dragged shut, and the drag
//! that folds and reopens it on the tree.
//!
//! One rule for both painters. The column painter's divider and every seam of
//! the tree fold a side within [`COLLAPSE_AT_PX`] of the pair's edge to a
//! [`COLLAPSED_PANE_WIDTH_PX`] rail a pointer finds over [`COLLAPSED_HIT_PX`],
//! and open it again when dragged back past the threshold. The constants have
//! their one owner in [`super::metrics`]; `canvas_layout` re-exports them.

use super::metrics::COLLAPSE_AT_PX;
#[cfg(doc)]
use super::metrics::{COLLAPSED_HIT_PX, COLLAPSED_PANE_WIDTH_PX};
use super::presets::PaneKind;
use super::seams::{
    MIN_TREE_PANE_PX, SeamAddress, along, bounds, extent, node_at, node_at_mut, partners, pinned,
    railed,
};
use super::state::{CanvasRefusal, CanvasState};
use super::tree::{CanvasRect, PaneSlot, PaneTree, SplitAxis};
use serde::{Deserialize, Serialize};

/// A seam drag in flight: the seam, where the first of its pair ended at the
/// press, the pointer's travel since, the split's shares then, and the
/// pair's shares before any fold of it: what a fold keeps for a restore.
///
/// Every step lays the pair out afresh from the press, so travel is never
/// lost against a floor, and a drag that comes back to where it started puts
/// the shares back as they were.
#[derive(Clone, Debug, PartialEq)]
pub struct SeamDrag {
    seam: SeamAddress,
    origin: f32,
    travel: f32,
    shares: Vec<f32>,
    open: [f32; 2],
}

/// What a seam fold took from its open partner: the folded node's first
/// slot, the partner's, and the two shares they had before the fold. A
/// restore puts those two back and leaves the split's other children alone;
/// the fold had to grow the partner's share, in their units, to fill the
/// space.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Reopen {
    pub(super) folded: PaneSlot,
    partner: PaneSlot,
    shares: [f32; 2],
}

impl SeamDrag {
    /// Add the pointer's `delta` points along the split to the travel.
    pub fn travel_by(&mut self, delta: f32) {
        if delta.is_finite() {
            self.travel += delta;
        }
    }
}

/// The pair a seam drag moves: the split's axis, the two children's indices
/// and nodes, and the split's shares.
type DragPair = (SplitAxis, [usize; 2], [PaneTree; 2], Vec<f32>);

fn slots(node: &PaneTree) -> Vec<PaneSlot> {
    node.panes().into_iter().map(|(slot, _)| slot).collect()
}

impl CanvasState {
    /// Take hold of `seam` on a canvas of `area`.
    ///
    /// # Errors
    /// [`CanvasRefusal::NoSeam`] when the tree holds no such seam, a chart is
    /// maximized, or one side holds only minimized strips.
    pub fn begin_seam_drag(
        &self,
        seam: &SeamAddress,
        area: CanvasRect,
    ) -> Result<SeamDrag, CanvasRefusal> {
        let (axis, at, [first, second], shares) = self.drag_pair(seam)?;
        let origin = extent(bounds(&first, &self.rects(area)), axis);
        let (one, two) = (slots(&first)[0], slots(&second)[0]);
        // A pair already folded opens, and folds again, at its pre-fold shares.
        let kept = (self.reopen.iter()).find_map(|kept| match (kept.folded, kept.partner) {
            pair if pair == (one, two) => Some(kept.shares),
            pair if pair == (two, one) => Some([kept.shares[1], kept.shares[0]]),
            _ => None,
        });
        Ok(SeamDrag {
            seam: seam.clone(),
            origin,
            travel: 0.0,
            open: kept.unwrap_or([shares[at[0]], shares[at[1]]]),
            shares,
        })
    }

    /// The hand's drag of a seam, one frame: `drag` taken afresh on the
    /// press (`pressed`) or when it is `None` or holds another seam, `delta`
    /// added to its travel, the pair laid out ([`Self::drag_seam`]). A frame
    /// the pointer did not move changes nothing. Answers whether anything
    /// moved.
    ///
    /// # Errors
    /// As [`Self::begin_seam_drag`].
    pub fn step_seam_drag(
        &mut self,
        drag: &mut Option<SeamDrag>,
        (seam, delta, pressed): (&SeamAddress, f32, bool),
        area: CanvasRect,
    ) -> Result<bool, CanvasRefusal> {
        if pressed || drag.as_ref().is_none_or(|held| held.seam != *seam) {
            *drag = Some(self.begin_seam_drag(seam, area)?);
        }
        let Some(held) = drag.as_mut().filter(|_| delta.abs() > f32::EPSILON) else {
            return Ok(false);
        };
        held.travel_by(delta);
        self.drag_seam(held, area)
    }

    /// Lay `drag`'s pair out for the travel so far, on a canvas of `area`:
    /// within [`COLLAPSE_AT_PX`] of the pair's start the first side folds to
    /// its rail, within it of the pair's end the second does, and anywhere
    /// between both stand open with the boundary under the pointer, each
    /// keeping [`MIN_TREE_PANE_PX`] per pane along the split. The open side
    /// fills what a fold leaves and the split's other children keep their
    /// place; a folded side keeps the ratio the pair had at the press, which
    /// is what a restore opens it at. Answers whether anything changed.
    ///
    /// # Errors
    /// As [`Self::begin_seam_drag`].
    pub fn drag_seam(&mut self, drag: &SeamDrag, area: CanvasRect) -> Result<bool, CanvasRefusal> {
        let (axis, [first, second], [one, two], shares) = self.drag_pair(&drag.seam)?;
        let (Some(PaneTree::Split { children, .. }), true) = (
            node_at(self.tree(), &drag.seam.path),
            drag.shares.len() == shares.len(),
        ) else {
            return Err(CanvasRefusal::NoSeam);
        };
        let rects = self.rects(area);
        let px = |node: &PaneTree| extent(bounds(node, &rects), axis);
        let (pair_px, wanted) = (px(&one) + px(&two), drag.origin + drag.travel);
        if !(pair_px > 0.0 && wanted.is_finite()) {
            return Ok(false);
        }
        // The split's other open children keep their points: the pair's
        // share for `points` of it is theirs scaled, or the press's alone.
        let others = (children.iter().enumerate())
            .filter(|(at, child)| *at != first && *at != second && !pinned(self, child));
        let (open_share, open_px) = others
            .fold((0.0_f32, 0.0_f32), |(share, points), (at, child)| {
                (share + shares[at].max(0.0), points + px(child))
            });
        let press = (drag.shares[first].max(0.0), drag.shares[second].max(0.0));
        let paid = |points: f32, alone: f32| {
            if open_share > f32::EPSILON && open_px > f32::EPSILON {
                open_share * points / open_px
            } else if alone > f32::EPSILON {
                alone
            } else {
                1.0
            }
        };
        let (one_slots, two_slots) = (slots(&one), slots(&two));
        let mut next = self.clone();
        next.set_railed(&one_slots, false);
        next.set_railed(&two_slots, false);
        let fold_first = wanted < COLLAPSE_AT_PX;
        let pair = if fold_first || pair_px - wanted < COLLAPSE_AT_PX {
            let (folded, open_press, folded_press) = if fold_first {
                (&one, press.1, press.0)
            } else {
                (&two, press.0, press.1)
            };
            let (kept, partner) = if fold_first { (0, 1) } else { (1, 0) };
            next.set_railed(&slots(folded), true);
            next.reopen.push(Reopen {
                folded: slots(folded)[0],
                partner: slots([&one, &two][partner])[0],
                shares: [drag.open[kept], drag.open[partner]],
            });
            let rail = extent(bounds(folded, &next.rects(area)), axis);
            let open = paid(pair_px - rail, open_press);
            let kept = if open_press > f32::EPSILON {
                open * folded_press / open_press
            } else {
                folded_press
            };
            if fold_first {
                (kept, open)
            } else {
                (open, kept)
            }
        } else {
            let floor = |node: &PaneTree| MIN_TREE_PANE_PX * along(node, axis) as f32;
            // max then min, never `clamp`: a pair narrower than both floors
            // would hand `clamp` a low bound past its high one.
            let at = wanted.max(floor(&one)).min(pair_px - floor(&two));
            let total = paid(pair_px, press.0 + press.1);
            let share = total * (at / pair_px).clamp(0.0, 1.0);
            (share, total - share)
        };
        next.set_pair_shares(&drag.seam.path, [first, second], pair);
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }

    fn drag_pair(&self, seam: &SeamAddress) -> Result<DragPair, CanvasRefusal> {
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
        let (first, second) = partners(self, children, seam.index).ok_or(CanvasRefusal::NoSeam)?;
        let nodes = [children[first].clone(), children[second].clone()];
        Ok((*axis, [first, second], nodes, shares.to_vec()))
    }

    fn set_pair_shares(&mut self, path: &[usize], [first, second]: [usize; 2], to: (f32, f32)) {
        if let Some(PaneTree::Split { shares, .. }) = node_at_mut(self.tree_mut(), path) {
            let shares = shares.to_mut();
            (shares[first], shares[second]) = to;
        }
    }

    /// Open the rail `slot` folded to, every pane of it, giving the pair a
    /// seam fold traded between the shares it had before; answers whether
    /// it was folded to one.
    pub(super) fn unrail(&mut self, slot: PaneSlot) -> bool {
        let (mut node, mut path) = (self.tree(), Vec::new());
        while let PaneTree::Split { children, .. } = node {
            let Some(index) = children.iter().position(|child| child.holds(slot)) else {
                return false;
            };
            if railed(self, &children[index]) {
                let folded = slots(&children[index]);
                let kept = self.reopen.iter().find(|kept| kept.folded == folded[0]);
                let back = kept.and_then(|kept| {
                    let partner = children
                        .iter()
                        .position(|child| child.holds(kept.partner))?;
                    (!pinned(self, &children[partner])).then_some((partner, kept.shares))
                });
                if let Some((partner, [one, two])) = back {
                    self.set_pair_shares(&path, [index, partner], (one, two));
                }
                return self.set_railed(&folded, false);
            }
            path.push(index);
            node = &children[index];
        }
        false
    }

    /// Where the flow pane sits: the path of the split holding it and its
    /// index there. `None` without one, or when it is the whole tree.
    fn flow_place(&self) -> Option<(Vec<usize>, usize)> {
        let panes = self.tree().panes();
        let flow = panes
            .into_iter()
            .find(|(_, kind)| *kind == PaneKind::Flow)?
            .0;
        let (mut node, mut path) = (self.tree(), Vec::new());
        while let PaneTree::Split { children, .. } = node {
            let index = children.iter().position(|child| child.holds(flow))?;
            if matches!(children[index], PaneTree::Pane { .. }) {
                return Some((path, index));
            }
            path.push(index);
            node = &children[index];
        }
        None
    }

    /// The flow pane and the charts beside it in its split, as slots.
    fn flow_sides(&self) -> Option<(Vec<PaneSlot>, Vec<PaneSlot>)> {
        let (path, index) = self.flow_place()?;
        let Some(PaneTree::Split { children, .. }) = node_at(self.tree(), &path) else {
            return None;
        };
        let beside = (children.iter().enumerate()).filter(|(at, _)| *at != index);
        let context = beside.flat_map(|(_, child)| slots(child)).collect();
        Some((slots(&children[index]), context))
    }

    /// Fold the order-flow pane to its rail (`flow`) or the charts beside it
    /// in its split, or open them (`folded` false): what the column painter's
    /// collapse calls do on a canvas the tree paints. The two sides never
    /// fold together, so folding one opens the other; a fold that would
    /// leave no chart open does nothing. Answers whether anything changed.
    pub fn fold_flow_side(&mut self, flow: bool, folded: bool) -> bool {
        let Some((own, context)) = self.flow_sides() else {
            return false;
        };
        let (fold, other) = if flow { (own, context) } else { (context, own) };
        if !folded {
            return self.set_railed(&fold, false);
        }
        let mut next = self.clone();
        next.set_railed(&other, false);
        if other.iter().all(|slot| next.folded(*slot)) {
            return false;
        }
        next.set_railed(&fold, true);
        let changed = next != *self;
        *self = next;
        changed
    }

    /// The split holding the docked flow pane and its index there, found
    /// without allocating: what every frame's fold readback walks.
    fn flow_split(&self) -> Option<(&[PaneTree], usize)> {
        fn find(node: &PaneTree) -> Option<(&[PaneTree], usize)> {
            let PaneTree::Split { children, .. } = node else {
                return None;
            };
            let flow = |child: &PaneTree| matches!(child, PaneTree::Pane { kind, .. } if *kind == PaneKind::Flow);
            (children
                .iter()
                .position(flow)
                .map(|index| (&children[..], index)))
            .or_else(|| children.iter().find_map(find))
        }
        find(self.tree())
    }

    /// Whether the flow pane stands docked beside charts, so either side of
    /// it can fold.
    #[must_use]
    pub fn has_flow_sides(&self) -> bool {
        self.flow_split().is_some()
    }

    /// Whether every chart on one side of the flow pane in its split, and
    /// the flow pane itself, is on a rail: what [`Self::fold_flow_side`]
    /// reads back, and the column painter's two flags. A chart folded inside
    /// the stack or the grid beside the flow, with others there open, is no
    /// context fold.
    #[must_use]
    pub fn flow_side_folds(&self) -> (bool, bool) {
        let split = self.flow_split().filter(|_| !self.railed().is_empty());
        let Some((children, index)) = split else {
            return (false, false);
        };
        let whole = |child: &PaneTree| child.railed(&|slot| self.pin(slot));
        let side = |side: &[PaneTree]| !side.is_empty() && side.iter().all(whole);
        let context = side(&children[..index]) || side(&children[index + 1..]);
        (context, whole(&children[index]))
    }

    /// Whether the rails, if any, are exactly one whole side of the flow
    /// pane: the fold the column painter draws as its own.
    pub(super) fn one_side_railed(&self) -> bool {
        if self.railed().is_empty() {
            return true;
        }
        let Some((children, index)) = self.flow_split() else {
            return false;
        };
        let whole = |child: &PaneTree| child.railed(&|slot| self.pin(slot));
        let beside = (children.iter().enumerate()).filter(|(at, _)| *at != index);
        let (context, flow) = self.flow_side_folds();
        let context_whole = beside.clone().all(|(_, child)| whole(child));
        let held: usize = beside.map(|(_, child)| child.len()).sum();
        (flow && !context && whole(&children[index]) && self.railed().len() == 1)
            || (context && !flow && context_whole && self.railed().len() == held)
    }

    /// The scene's name for the rail of one side of the flow pane, the flow
    /// pane's own (`flow`) or the charts beside it, while it is folded.
    /// Walks the tree without allocating: only the name is built.
    #[must_use]
    pub fn side_rail_id(&self, flow: bool) -> Option<String> {
        fn first(node: &PaneTree, railed: &dyn Fn(PaneSlot) -> bool) -> Option<PaneSlot> {
            match node {
                PaneTree::Pane { slot, .. } => railed(*slot).then_some(*slot),
                PaneTree::Split { children, .. } => {
                    children.iter().find_map(|child| first(child, railed))
                }
            }
        }
        let (children, index) = self.flow_split()?;
        let railed = |slot: PaneSlot| self.railed().contains(&slot);
        let mut side = (children.iter().enumerate()).filter(|(at, _)| (*at == index) == flow);
        let slot = side.find_map(|(_, child)| first(child, &railed))?;
        Some(format!("pane_menu.{slot}.rail"))
    }

    /// This canvas with the column painter's two flags folded in: what a
    /// save keeping only a preset, a split and the flags rebuilds, and what a
    /// save from before rails holds beside a canvas (the flags outlived a
    /// maximize, a float or an add there). A canvas holding a rail already
    /// says its own folds, and is kept as it is.
    #[must_use]
    pub fn with_folds(mut self, context: bool, flow: bool) -> Self {
        if (context || flow) && self.railed().is_empty() {
            self.fold_flow_side(!context, true);
        }
        self
    }
}
