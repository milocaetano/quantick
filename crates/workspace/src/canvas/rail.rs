//! The rail a pane folds to when its divider is dragged shut, and the drag
//! that folds and reopens it on the tree.
//!
//! One rule for both painters. The column painter's divider and every seam of
//! the tree fold a side within [`COLLAPSE_AT_PX`] of the pair's edge to a
//! [`COLLAPSED_PANE_WIDTH_PX`] rail a pointer finds over [`COLLAPSED_HIT_PX`],
//! and open it again when dragged back past the threshold. The constants have
//! their one owner in [`super::metrics`]; `canvas_layout` re-exports them.

use super::metrics::{COLLAPSE_AT_PX, PICK_SEAM_PX};
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

/// One frame of a hand on a seam, a divider or a rail, as the window reads
/// its pointer's drag. A press let go without a move is no click here: only
/// a drag folds or opens a rail.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hand {
    /// The press landed this frame.
    pub pressed: bool,
    /// Held down and dragging this frame.
    pub held: bool,
    /// The pointer's move along the split this frame, in points.
    pub delta: f32,
    /// Let go this frame.
    pub released: bool,
}

impl Hand {
    /// A frame read off a toolkit's drag: pressed, held and let go, and the
    /// move along the split.
    #[must_use]
    pub const fn of([pressed, held, released]: [bool; 3], delta: f32) -> Self {
        Self {
            pressed,
            held,
            delta,
            released,
        }
    }
}

/// A seam drag in flight: the seam, where the first of its pair ended at the
/// press, the pointer's travel since, the split's shares then, the pair's
/// shares before any fold of it (what a fold keeps for a restore), and the
/// rails inside the pair an earlier drag folded.
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
    /// Rails inside either side at the press that this drag did not fold
    /// whole: a fold of a chart within it. Every step leaves them folded,
    /// so a second fold never reopens the first.
    inner: Vec<PaneSlot>,
    /// The seam the hand pressed, which may hand the drag to `seam`: a rail
    /// between two seams goes to the one on the side of the first move.
    grabbed: SeamAddress,
    /// Whether the hand's travel has picked the seam.
    picked: bool,
    /// The travel along the split before the pick, applied whole once made.
    pending: f32,
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
        // A side folded whole is this seam's own rail, which the drag opens;
        // a rail inside a side that is not is another seam's, and stays.
        let inner = |node: &PaneTree| {
            let own = railed(self, node);
            let held = slots(node).into_iter();
            held.filter(move |slot| !own && self.railed().contains(slot))
        };
        Ok(SeamDrag {
            seam: seam.clone(),
            origin,
            travel: 0.0,
            open: kept.unwrap_or([shares[at[0]], shares[at[1]]]),
            shares,
            inner: inner(&first).chain(inner(&second)).collect(),
            grabbed: seam.clone(),
            picked: false,
            pending: 0.0,
        })
    }

    /// The seams either side of the rail `seam` borders, before it and
    /// after it, when that rail has a partner on both sides: a press there
    /// leaves the first move to pick which seam it drags. A strip beside the
    /// rail is skipped as the seams skip it.
    fn rail_seams(&self, seam: &SeamAddress) -> Option<[SeamAddress; 2]> {
        let Some(PaneTree::Split { children, .. }) = node_at(self.tree(), &seam.path) else {
            return None;
        };
        let (first, second) = partners(self, children, seam.index)?;
        let rail = [first, second]
            .into_iter()
            .find(|at| railed(self, &children[*at]))?;
        let paired = |index: usize| partners(self, children, index);
        let before =
            (rail.checked_sub(1)).filter(|at| paired(*at).is_some_and(|(_, to)| to == rail));
        let after = paired(rail).filter(|(from, _)| *from == rail).map(|_| rail);
        let at = |index| SeamAddress {
            path: seam.path.clone(),
            index,
        };
        Some([at(before?), at(after?)])
    }

    /// The hand on `seam`, one frame: [`Self::step_seam_drag`] while it is
    /// held, and the drag dropped when it lets go. A press on a rail between
    /// two seams waits until its travel along the split passes
    /// [`PICK_SEAM_PX`], then drags the seam on that side, backward the one
    /// before the rail and forward the one after, with all that travel, so
    /// the rail opens either way and a jitter picks nothing. Answers whether anything moved; a refused step moves
    /// nothing.
    pub fn seam_hand(
        &mut self,
        drag: &mut Option<SeamDrag>,
        (seam, hand): (&SeamAddress, Hand),
        area: CanvasRect,
    ) -> bool {
        if !hand.held {
            drag.take_if(|_| hand.released);
            return false;
        }
        if hand.pressed || drag.as_ref().is_none_or(|held| held.grabbed != *seam) {
            *drag = self.begin_seam_drag(seam, area).ok();
        }
        let mut delta = hand.delta;
        if let Some(held) = drag
            .as_mut()
            .filter(|held| !held.picked && delta.is_finite())
        {
            let Some([before, after]) = self.rail_seams(seam) else {
                held.picked = true;
                return self.seam_step(drag, delta, area);
            };
            held.pending += delta;
            if held.pending.abs() < PICK_SEAM_PX {
                return false;
            }
            (delta, held.picked) = (held.pending, true);
            let way = if delta < 0.0 { before } else { after };
            if way != held.seam
                && let Ok(taken) = self.begin_seam_drag(&way, area)
            {
                *held = SeamDrag {
                    grabbed: seam.clone(),
                    picked: true,
                    ..taken
                };
            }
        }
        let moved = self.seam_step(drag, delta, area);
        drag.take_if(|_| hand.released);
        moved
    }

    /// One step of `drag` by `delta` along its own seam.
    fn seam_step(&mut self, drag: &mut Option<SeamDrag>, delta: f32, area: CanvasRect) -> bool {
        let at = drag.as_ref().map(|held| held.seam.clone());
        at.is_some_and(|at| {
            let step = self.step_seam_drag(drag, (&at, delta, false), area);
            step.unwrap_or(false)
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
    /// is what a restore opens it at. A rail an earlier drag folded inside
    /// either side stays folded. Answers whether anything changed.
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
        let opened: Vec<PaneSlot> = (one_slots.iter().chain(&two_slots))
            .filter(|slot| !drag.inner.contains(slot))
            .copied()
            .collect();
        next.set_railed(&opened, false);
        // This pair's own fold is remembered only while it stands: a rail
        // kept inside a side must not keep a stale one alive.
        let own = [(one_slots[0], two_slots[0]), (two_slots[0], one_slots[0])];
        (next.reopen).retain(|kept| !own.contains(&(kept.folded, kept.partner)));
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
                // A rail inside it may share its first slot: take the fold
                // whose partner stands beside it in this split.
                let mut kept = (self.reopen.iter()).filter(|kept| kept.folded == folded[0]);
                let back = kept.find_map(|kept| {
                    let partner = children
                        .iter()
                        .position(|child| child.holds(kept.partner))
                        .filter(|partner| *partner != index)?;
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
    /// in its split, or open them (`folded` false), a side a drag folded at
    /// the shares it had before ([`Self::open_side`]): what the column
    /// painter's collapse calls do on a canvas the tree paints. The two
    /// sides never fold together, so folding one opens the other; a fold
    /// that would leave no chart open does nothing. Answers whether anything
    /// changed.
    pub fn fold_flow_side(&mut self, flow: bool, folded: bool) -> bool {
        let Some((own, context)) = self.flow_sides() else {
            return false;
        };
        let (fold, other) = if flow { (own, context) } else { (context, own) };
        if !folded {
            return self.open_side(&fold);
        }
        let mut next = self.clone();
        // The two sides never fold together: the other opens when it is
        // folded whole, as an open does, keeping the charts folded inside.
        if other.iter().all(|slot| next.railed().contains(slot)) {
            next.open_side(&other);
        }
        if other.iter().all(|slot| next.folded(*slot)) {
            return false;
        }
        // The charts folded inside it stay folded when it reopens.
        let inner: Vec<PaneSlot> = (fold.iter().copied())
            .filter(|slot| self.railed().contains(slot))
            .collect();
        next.set_railed(&fold, true);
        if inner.len() < fold.len() {
            next.kept.extend(inner);
        }
        let changed = next != *self;
        *self = next;
        changed
    }

    /// The context column's collapse call: [`Self::fold_flow_side`] for the
    /// charts beside a docked flow pane. With none, every chart is context:
    /// an expand opens every rail through [`Self::open_side`], the path the
    /// flow side's expand takes, at its pre-fold shares, and a collapse has
    /// no column to fold. Answers whether anything changed.
    pub fn fold_context(&mut self, folded: bool) -> bool {
        if folded || self.has_flow_sides() {
            return self.fold_flow_side(false, folded);
        }
        let rails: Vec<PaneSlot> = self.railed().iter().copied().collect();
        self.open_side(&rails)
    }

    /// Open every rail among `side`, the last folded first, each through
    /// [`Self::unrail`] as a restore does, so a pair a drag folded gets back
    /// the shares it had before; a rail no fold remembers just opens.
    /// Answers whether anything changed.
    fn open_side(&mut self, side: &[PaneSlot]) -> bool {
        // A side folded whole reopens with the charts folded inside it before.
        let whole = side.iter().all(|slot| self.railed().contains(slot));
        let kept = |slot: &PaneSlot| whole && self.kept.contains(slot);
        let opened: Vec<PaneSlot> = side.iter().copied().filter(|slot| !kept(slot)).collect();
        self.kept.retain(|slot| !side.contains(slot));
        let side = opened;
        let remembered: Vec<PaneSlot> = (self.reopen.iter().rev())
            .map(|kept| kept.folded)
            .filter(|slot| side.contains(slot))
            .collect();
        let mut changed = false;
        for slot in remembered {
            changed |= self.railed().contains(&slot) && self.unrail(slot);
        }
        self.set_railed(&side, false) || changed
    }

    /// The split holding the docked flow pane and its index there, found
    /// without allocating: what every frame's fold readback walks.
    pub(super) fn flow_split(&self) -> Option<(&[PaneTree], usize)> {
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

    /// Whether the rails, if any, are the folds the column painter draws as
    /// its own: one whole side of the flow pane, and charts folded inside
    /// the stack beside it while some of it stands open.
    pub(super) fn one_side_railed(&self) -> bool {
        if self.railed().is_empty() {
            return true;
        }
        let Some((children, index)) = self.flow_split() else {
            return false;
        };
        let (context, flow) = self.flow_side_folds();
        let beside = |slot: PaneSlot| {
            let mut sides = children.iter().enumerate();
            sides.any(|(at, child)| at != index && child.holds(slot))
        };
        let stack = self.stack_column().filter(|_| !context);
        let stacked = |slot: PaneSlot| stack.as_ref().is_some_and(|stack| stack.contains(&slot));
        !(context && flow)
            && self.railed().iter().all(|slot| {
                (flow && children[index].holds(*slot))
                    || (context && beside(*slot))
                    || stacked(*slot)
            })
    }

    /// The charts of the stack beside the flow pane, top to bottom in the
    /// tree: the one column of charts on its side, which the column painter
    /// draws as its stack. Empty for any other canvas.
    #[must_use]
    pub fn stack_slots(&self) -> Vec<PaneSlot> {
        self.stack_column().unwrap_or_default()
    }

    /// Whether stacked chart `slot` is folded inside its stack: on its own
    /// rail, or kept for the reopening of a stack folded whole.
    #[must_use]
    pub fn stack_folded(&self, slot: PaneSlot) -> bool {
        let stack = self.stack_slots();
        let whole = !stack.is_empty() && stack.iter().all(|at| self.railed().contains(at));
        if whole {
            self.kept.contains(&slot)
        } else {
            self.railed().contains(&slot)
        }
    }

    /// The stack's own folds, top to bottom in the tree: what a save keeps
    /// as the stack's flags ([`Self::with_stack_folds`] rebuilds them).
    #[must_use]
    pub fn stack_folds(&self) -> Vec<bool> {
        let stack = self.stack_slots().into_iter();
        stack.map(|slot| self.stack_folded(slot)).collect()
    }

    /// This canvas with the stack's flags folded in, top to bottom in the
    /// tree: the mirror of what a save leaves out.
    #[must_use]
    pub fn with_stack_folds(mut self, folded: &[bool]) -> Self {
        let slots = self.stack_slots();
        self.rail_stack(&slots, folded);
        self
    }

    fn stack_column(&self) -> Option<Vec<PaneSlot>> {
        let (children, index) = self.flow_split()?;
        let mut beside = (children.iter().enumerate()).filter(|(at, _)| *at != index);
        let (Some((_, column)), None) = (beside.next(), beside.next()) else {
            return None;
        };
        let PaneTree::Split {
            axis: SplitAxis::Column,
            children: stack,
            ..
        } = column
        else {
            return None;
        };
        (stack.iter())
            .map(|chart| match chart {
                PaneTree::Pane { slot, .. } => Some(*slot),
                PaneTree::Split { .. } => None,
            })
            .collect()
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
