//! The rail a pane folds to when its divider is dragged shut, and the drag
//! that folds and reopens it on the tree.
//!
//! One rule for both painters. The column painter's divider and every seam of
//! the tree fold a side within [`COLLAPSE_AT_PX`] of the pair's edge to a
//! [`COLLAPSED_PANE_WIDTH_PX`] rail a pointer finds over [`COLLAPSED_HIT_PX`],
//! and open it again when dragged back past the threshold. The constants have
//! this one owner; `canvas_layout` re-exports them for the column painter.

use super::presets::PaneKind;
use super::seams::{
    MIN_TREE_PANE_PX, SeamAddress, along, bounds, extent, node_at, node_at_mut, partners, pinned,
    railed,
};
use super::state::{CanvasRefusal, CanvasState};
use super::tree::{CanvasRect, PaneSlot, PaneTree, SplitAxis};

/// Width of the draggable divider between two panes, in pixels.
pub const CANVAS_DIVIDER_PX: f32 = 4.0;

/// Width of a collapsed pane, in pixels.
///
/// **Never zero.** The vertical axis already settled this question — see
/// `quantick_chart::indicator_views::COLLAPSED_PANE_HEIGHT_PX`, "a pane that vanished would be an
/// indicator the user added and the chart silently dropped" — and the answer
/// is the same here: a pane with no width has no handle, and a pane with no
/// handle cannot be brought back from the canvas it left.
///
/// The two axes differ in pixels because they differ in what they must hold. A
/// collapsed indicator pane keeps a readable row of text, so it needs 20 px of
/// height; a collapsed chart pane keeps only a grip, so 8 px of width is
/// enough. What they share — and what may not drift — is the rule that
/// collapse leaves something to click.
pub const COLLAPSED_PANE_WIDTH_PX: f32 = 8.0;

/// How narrow a divider drag has to get before the pane is dismissed rather
/// than squeezed.
///
/// **In pixels, deliberately, and below [`MIN_PANE_WIDTH_PX`].** A share of
/// the canvas cannot be both: a fraction that sits under the floor on a
/// trading monitor sits over it on a laptop, and over it the floor never
/// binds — the pane would be dismissed while it still had room to be a chart,
/// and every width between the threshold and the floor would be unreachable.
///
/// Half the floor is the gesture: drag to the minimum and the pane holds
/// there; keep going, well past the point it stopped narrowing, and it is
/// dismissed. The travel past the floor is what makes the dismissal
/// deliberate.
pub const COLLAPSE_AT_PX: f32 = MIN_PANE_WIDTH_PX / 2.0;

/// How wide the collapsed rail's *hit* area is, against the
/// [`COLLAPSED_PANE_WIDTH_PX`] it paints.
///
/// The rail gives up almost all its width and keeps all of its reachability:
/// the extra reaches into the neighbouring chart, where it costs nothing but
/// the pointer's first few pixels. 24 px is the floor a pointer target is held
/// to, and a rail that photographed well but could not be hit would be a
/// picture of an affordance rather than one.
pub const COLLAPSED_HIT_PX: f32 = 24.0;

/// Narrowest the column painter squeezes a pane to while it is still open.
///
/// A pane is not all chart. The price axis takes `plot_area`'s gutter constant (64 px)
/// off its right edge before a single candle is drawn, and the flow pane's
/// tape takes more. This was 120 px and the doc said "below this a chart stops
/// being one" — which was true of 120 px itself: a floored pane came out with
/// **22 px** of chart on a 1124 px canvas, the axis having claimed the rest.
/// The number has to leave a chart behind, not just a pane.
///
/// A trader who wants less than this wants the pane collapsed, which is a
/// different request with a different affordance — and, at half this width,
/// one the same drag reaches by carrying on.
pub const MIN_PANE_WIDTH_PX: f32 = 240.0;

/// The floor a pointer target is held to, checked where it cannot be missed.
///
/// A compile error rather than a failing test: this is a property of the
/// constant above, and a reader lowering it should be stopped by the build
/// rather than by a test run they might not reach.
const _: () = assert!(
    COLLAPSED_HIT_PX >= 24.0,
    "the collapsed rail's hit area is under the minimum size a pointer target is held to"
);
const _: () = assert!(
    COLLAPSE_AT_PX > 0.0,
    "a collapse threshold at zero can never be crossed, so the pane could never be dismissed"
);
const _: () = assert!(
    COLLAPSE_AT_PX < MIN_PANE_WIDTH_PX,
    "a collapse threshold at or above the pane's own floor makes the floor unreachable:      the pane would be dismissed while it still had room to be a chart"
);

/// A seam drag in flight: the seam, where the first of its pair ended at the
/// press, the pointer's travel since, and the split's shares then.
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
}

impl SeamDrag {
    /// The seam held.
    #[must_use]
    pub fn seam(&self) -> &SeamAddress {
        &self.seam
    }

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
        let (axis, _, [first, _], shares) = self.drag_pair(seam)?;
        let origin = extent(bounds(&first, &self.rects(area)), axis);
        Ok(SeamDrag {
            seam: seam.clone(),
            origin,
            travel: 0.0,
            shares,
        })
    }

    /// The hand's drag of a seam, one frame: `drag` taken afresh when it is
    /// `None` or holds another seam, `delta` added to its travel, the pair
    /// laid out ([`Self::drag_seam`]). Answers whether anything moved.
    ///
    /// # Errors
    /// As [`Self::begin_seam_drag`].
    pub fn step_seam_drag(
        &mut self,
        drag: &mut Option<SeamDrag>,
        (seam, delta): (&SeamAddress, f32),
        area: CanvasRect,
    ) -> Result<bool, CanvasRefusal> {
        if drag.as_ref().is_none_or(|held| held.seam != *seam) {
            *drag = Some(self.begin_seam_drag(seam, area)?);
        }
        let Some(held) = drag.as_mut() else {
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
            next.set_railed(&slots(folded), true);
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

    /// Open the rail `slot` folded to, every pane of it; answers whether it
    /// was folded to one.
    pub(super) fn unrail(&mut self, slot: PaneSlot) -> bool {
        let mut node = self.tree();
        while let PaneTree::Split { children, .. } = node {
            let Some(child) = children.iter().find(|child| child.holds(slot)) else {
                return false;
            };
            if railed(self, child) {
                let folded = slots(child);
                return self.set_railed(&folded, false);
            }
            node = child;
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
        if other.iter().all(|slot| next.minimized().contains(slot)) {
            return false;
        }
        next.set_railed(&fold, true);
        let changed = next != *self;
        *self = next;
        changed
    }

    /// Whether the charts beside the flow pane, and the flow pane itself, are
    /// folded to their rails: what [`Self::fold_flow_side`] reads back.
    #[must_use]
    pub fn flow_side_folds(&self) -> (bool, bool) {
        let railed = |side: &[PaneSlot]| {
            !side.is_empty() && side.iter().all(|slot| self.railed().contains(slot))
        };
        self.flow_sides().map_or((false, false), |(own, context)| {
            (railed(&context), railed(&own))
        })
    }
}
