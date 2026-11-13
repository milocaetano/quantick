//! What the window reads off a canvas to paint and compare it: where each
//! docked pane draws, which built chart each time slot shows, and the
//! readings two canvases are compared by.
//!
//! The window converts these rectangles to its own type and paints them;
//! nothing here knows a pixel it did not compute from the tree.

use super::presets::{LAYOUT_PRESETS, LayoutPreset, PaneKind, PresetGroup};
use super::state::{CanvasState, FloatHost, PaneMode};
use super::tree::{CanvasRect, PaneSlot, PaneTree};
use std::collections::BTreeSet;

/// Which built time chart each time slot shows, kept across edits so a
/// removed chart never moves another; and which charts a removal freed, so
/// a slot seated on one later takes it fresh, never the removed chart's
/// interval, indicators and drawings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seats {
    held: Vec<(PaneSlot, usize)>,
    freed: BTreeSet<usize>,
    reseed: Vec<usize>,
}

impl Seats {
    /// Seat every chart afresh, as a picked layout does: on the charts
    /// already built, in slot order, with nothing to reseed.
    pub fn clear(&mut self) {
        self.held.clear();
        self.freed.clear();
        self.reseed.clear();
    }

    /// The next built chart a new slot sat on after a removal freed it, to
    /// be built again from the tape; each is answered once.
    pub fn take_reseed(&mut self) -> Option<usize> {
        (!self.reseed.is_empty()).then(|| self.reseed.remove(0))
    }

    /// Every slot with the chart it shows.
    #[must_use]
    pub fn held(&self) -> &[(PaneSlot, usize)] {
        &self.held
    }

    /// These seats brought up to date for `state` as the next frame seats
    /// them, so a read between an edit and that frame is not stale.
    #[must_use]
    pub fn current(&self, state: &CanvasState) -> Self {
        let mut seats = self.clone();
        seated(state, &mut seats);
        seats
    }
}

/// One docked pane of the tree, ready to paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeBand {
    /// The pane this band draws.
    pub slot: PaneSlot,
    /// The built time chart this slot shows; `None` for the flow pane.
    pub seat: Option<usize>,
    /// Where it draws: its tree rectangle less half a divider on each seam.
    pub rect: CanvasRect,
    /// Folded to its strip by the canvas model.
    pub minimized: bool,
    /// Folded with its node to one rail ([`super::tree_rails`] paints it),
    /// so there is no strip to draw: `minimized` too.
    pub railed: bool,
}

/// Where each docked pane of `state` draws when the canvas is `area`, with
/// `divider` points between neighbours.
///
/// Slots map to the window's charts by kind: the flow slot is the flow pane,
/// and each time slot keeps the chart `seats` gave it, so removing one chart
/// never moves another onto a different one. A slot the window has no pane
/// for (a second flow) is left out.
#[must_use]
pub fn tree_bands(
    state: &CanvasState,
    seats: &mut Seats,
    area: CanvasRect,
    divider: f32,
) -> Vec<TreeBand> {
    let panes = seated(state, seats);
    let seat = |slot| seat_of(&panes, seats, slot);
    (state.rects(area).into_iter())
        .filter_map(|(slot, rect)| {
            Some(TreeBand {
                slot,
                seat: seat(slot)?,
                rect: inset_seams(rect, area, divider),
                minimized: state.mode(slot) == Some(PaneMode::Minimized),
                railed: state.railed().contains(&slot),
            })
        })
        .collect()
}

/// The built chart `slot` shows: `Some(None)` for the flow pane, `None` for
/// a slot the window has no pane for.
///
/// Slots map to the window's charts by kind: the flow slot is the flow pane,
/// and each time slot keeps the chart `seats` gave it, so removing one chart
/// never moves another onto a different one.
pub fn slot_seat(state: &CanvasState, seats: &mut Seats, slot: PaneSlot) -> Option<Option<usize>> {
    seat_of(&seated(state, seats), seats, slot)
}

/// The slot that shows built chart `seat` (`None` for the flow pane): the
/// inverse of [`slot_seat`].
pub fn seat_slot(state: &CanvasState, seats: &mut Seats, seat: Option<usize>) -> Option<PaneSlot> {
    let panes = seated(state, seats);
    let mut slots = panes.iter().map(|(slot, ..)| *slot);
    slots.find(|slot| seat_of(&panes, seats, *slot) == Some(seat))
}

/// How built chart `seat` (`None` for the flow pane) shows by `seats`, as
/// [`Seats::current`] reads them; `None` when the canvas holds no pane for it.
#[must_use]
pub fn seat_mode(state: &CanvasState, seats: &Seats, seat: Option<usize>) -> Option<PaneMode> {
    let slot = match seat {
        Some(seat) => (seats.held.iter()).find_map(|(slot, held)| (*held == seat).then_some(*slot)),
        None => (state.panes().into_iter())
            .find_map(|(slot, kind, _)| (kind != PaneKind::Time).then_some(slot)),
    };
    state.mode(slot?)
}

/// Whether built chart `seat` (`None` for the flow pane) is on screen:
/// docked with nothing maximized over it, maximized itself, or floating. A
/// folded chart is only its strip, and a chart the canvas lacks is nowhere.
#[must_use]
pub fn seat_drawn(state: &CanvasState, seats: &Seats, seat: Option<usize>) -> bool {
    match seat_mode(state, seats, seat) {
        Some(PaneMode::Docked) => state.maximized().is_none(),
        Some(PaneMode::Minimized) | None => false,
        Some(_) => true,
    }
}

/// `state`'s panes, with `seats` brought up to date: gone slots dropped,
/// new time slots seated on the lowest free chart in the order the tree
/// draws them, then the floating ones, so a picked layout shows its first
/// built chart first whatever the slots' numbers.
fn seated(state: &CanvasState, seats: &mut Seats) -> Vec<(PaneSlot, PaneKind, PaneMode)> {
    let panes = state.panes();
    let time = |slot: PaneSlot| {
        (panes.iter()).any(|(held, kind, _)| *held == slot && *kind == PaneKind::Time)
    };
    let Seats {
        held,
        freed,
        reseed,
    } = seats;
    held.retain(|&(slot, seat)| {
        let kept = time(slot);
        if !kept {
            freed.insert(seat);
        }
        kept
    });
    let docked = state.tree().panes().into_iter();
    let floating = (state.floating().iter()).map(|pane| (pane.slot, pane.kind));
    for (slot, kind) in docked.chain(floating) {
        if kind == PaneKind::Time && !held.iter().any(|(on, _)| *on == slot) {
            let free = (0..).find(|pane| !held.iter().any(|(_, seat)| seat == pane));
            let free = free.unwrap_or_default();
            if freed.remove(&free) {
                reseed.push(free);
            }
            held.push((slot, free));
        }
    }
    panes
}

fn seat_of(
    panes: &[(PaneSlot, PaneKind, PaneMode)],
    seats: &Seats,
    slot: PaneSlot,
) -> Option<Option<usize>> {
    let flow = panes.iter().find(|(_, kind, _)| *kind != PaneKind::Time);
    match seats.held.iter().find(|(held, _)| *held == slot) {
        Some((_, seat)) => Some(Some(*seat)),
        None => (flow.map(|(held, ..)| *held) == Some(slot)).then_some(None),
    }
}

/// `rect` less half a `divider` on every edge inside `area`.
#[must_use]
pub fn inset_seams(rect: CanvasRect, area: CanvasRect, divider: f32) -> CanvasRect {
    const EDGE_SLACK: f32 = 0.5;
    let half = divider / 2.0;
    let left = rect.x
        + if rect.x > area.x + EDGE_SLACK {
            half
        } else {
            0.0
        };
    let top = rect.y
        + if rect.y > area.y + EDGE_SLACK {
            half
        } else {
            0.0
        };
    let inner = |edge: f32, outer: f32| if edge < outer - EDGE_SLACK { half } else { 0.0 };
    let right = rect.right() - inner(rect.right(), area.right());
    let bottom = rect.bottom() - inner(rect.bottom(), area.bottom());
    CanvasRect::new(left, top, right - left, bottom - top)
}

/// The recipe signal's reading of a canvas: every pane's slot, kind and
/// mode, the matched preset, each floating pane's place and the tree's
/// shares, bitwise.
pub type Signal = (
    Vec<(u32, u8, u8)>,
    Option<&'static str>,
    Vec<[u32; 4]>,
    Vec<u32>,
);

/// A hashable reading of `state` for the recipe signal.
#[must_use]
pub fn signal(state: &CanvasState) -> Signal {
    let panes = state.panes().into_iter();
    let panes = panes
        .map(|(slot, kind, mode)| (slot, kind as u8, mode as u8))
        .collect();
    let floating = (state.floating().iter()).map(|pane| match &pane.host {
        FloatHost::Canvas { rect } | FloatHost::OsWindow { rect, .. } => {
            [rect.x, rect.y, rect.w, rect.h].map(f32::to_bits)
        }
    });
    let mut shares = Vec::new();
    share_bits(state.tree(), &mut shares);
    (
        panes,
        state.preset().map(|preset| preset.id),
        floating.collect(),
        shares,
    )
}

/// Every split's shares in `tree`, depth first, bitwise: a seam drag moves
/// them and nothing else.
fn share_bits(tree: &PaneTree, out: &mut Vec<u32>) {
    if let PaneTree::Split {
        children, shares, ..
    } = tree
    {
        out.extend(shares.iter().map(|share| share.to_bits()));
        children.iter().for_each(|child| share_bits(child, out));
    }
}

/// `state` with its slots renumbered in reading order: the tree, the folded
/// and maximized panes by rank, and how many float.
fn arrangement(state: &CanvasState) -> (PaneTree, Vec<usize>, Option<usize>, usize) {
    let order: Vec<u32> = (state.tree().panes().into_iter())
        .map(|(slot, _)| slot)
        .collect();
    let rank = |slot: u32| {
        (order.iter())
            .position(|held| *held == slot)
            .unwrap_or(usize::MAX)
    };
    let tree = state.tree().relabelled(&|slot| rank(slot) as u32);
    let mut folded: Vec<usize> = state.minimized().iter().map(|slot| rank(*slot)).collect();
    folded.sort_unstable();
    let maximized = state.maximized().map(rank);
    (tree, folded, maximized, state.floating().len())
}

/// Whether two canvases show the same layout: the tree, the folds, the
/// maximized and the floating panes; slot numbers aside.
#[must_use]
pub fn same_layout(saved: &CanvasState, live: &CanvasState) -> bool {
    arrangement(saved) == arrangement(live)
}

/// What the toolbar shows of a tab's layout against its workspace's save.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutMarks {
    /// The preset the canvas draws; `None` for a custom arrangement.
    pub current: Option<&'static LayoutPreset>,
    /// The preset the workspace saved.
    pub saved: Option<&'static LayoutPreset>,
    /// The workspace differs from its save, and what differs is the layout:
    /// the miniature's dot and the popover's footer.
    pub layout_unsaved: bool,
}

impl LayoutMarks {
    /// The marks of `live` against `saved`, the workspace differing (`unsaved`).
    #[must_use]
    pub fn of(live: &CanvasState, saved: Option<&CanvasState>, unsaved: bool) -> Self {
        Self {
            current: live.preset(),
            saved: saved.and_then(CanvasState::preset),
            layout_unsaved: unsaved && saved.is_some_and(|saved| !same_layout(saved, live)),
        }
    }
}

impl PresetGroup {
    /// The catalogue's columns, left to right, with their headings.
    pub const COLUMNS: [(Self, &'static str); 2] = [
        (Self::WithFlow, "WITH FLOW"),
        (Self::Timeframes, "TIMEFRAMES"),
    ];

    /// This column's presets, in registry order.
    #[must_use]
    pub fn presets(self) -> Vec<&'static LayoutPreset> {
        (LAYOUT_PRESETS.iter())
            .filter(|preset| preset.group == self)
            .collect()
    }
}

impl LayoutPreset {
    /// How many timeframe charts this preset shows.
    #[must_use]
    pub fn time_panes(&self) -> usize {
        self.kinds
            .iter()
            .filter(|kind| **kind == PaneKind::Time)
            .count()
    }

    /// Where the flow pane's left edge sits, as a share of the canvas width;
    /// `None` without a flow pane.
    #[must_use]
    pub fn flow_left(&self) -> Option<f32> {
        let flow = self.kinds.iter().position(|kind| *kind == PaneKind::Flow)?;
        let unit = CanvasRect::new(0.0, 0.0, 1.0, 1.0);
        let rects = self.tree.rects(unit);
        let rect = rects.iter().find(|(held, _)| *held as usize == flow)?;
        Some(rect.1.x)
    }

    /// The name a catalogue cell shows: a long one wraps at its first " + ".
    #[must_use]
    pub fn cell_label(&self) -> String {
        const ONE_LINE_CHARS: usize = 12;
        let wraps = usize::from(self.label.len() > ONE_LINE_CHARS);
        self.label.replacen(" + ", "\n+ ", wraps)
    }
}
