//! What a folded pane's rail paints inside its 8 px, what it says, and
//! when it lights.
//!
//! One look for every rail on both painters, as the trader picked it: a
//! band, an edge along each long side that does not meet the window's chrome
//! (which already draws that line), a chevron pointing the way the pane
//! opens and one dash per pane folded behind it. Each mark names which of the
//! theme's three rail inks it wears; the theme maps those to tokens
//! (`quantick_appearance::rail::rail_inks`, in [`RailInk`] order).

use super::presets::PaneKind;
use super::state::CanvasState;
use super::tree::CanvasRect;

/// The edge along a long side of the rail.
pub const RAIL_EDGE_PX: f32 = 1.0;
/// The chevron: this many one-pixel rows along the rail, each a stroke this
/// thick across it, stepping toward the side the pane opens to and back,
/// inside a box this wide centred on the band's middle column.
pub const RAIL_CHEVRON_ROWS: usize = 7;
pub const RAIL_CHEVRON_STROKE_PX: f32 = 2.0;
pub const RAIL_CHEVRON_BOX_PX: f32 = 5.0;
/// A dash: one pixel along the rail, this long across it, one per pane,
/// this far apart, the first this far past the chevron's last row.
pub const RAIL_DASH_PX: f32 = 3.0;
pub const RAIL_DASH_PITCH_PX: f32 = 4.0;
pub const RAIL_DASHES_AFTER_PX: f32 = 4.0;
/// The most dashes a rail lays: past it, the count reads in its words.
pub const RAIL_MAX_DASHES: usize = 8;

/// Whether a rail lights this frame, one rule for every painter: while the
/// pointer is on its hit area (`hovered`), and while a drag it began is held
/// (`pressed_on_it` with `dragging`), never while a drag that began anywhere
/// else is under way.
#[must_use]
pub const fn rail_lit(hovered: bool, pressed_on_it: bool, dragging: bool) -> bool {
    if dragging { pressed_on_it } else { hovered }
}

/// Which of the theme's three rail inks a mark wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RailInk {
    Band,
    Edge,
    Grip,
}

impl RailInk {
    /// Its place in the theme's rail inks.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// What is folded behind one rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RailHolds {
    pub charts: usize,
    pub flow: bool,
}

impl RailHolds {
    /// The panes of these kinds.
    pub fn of(kinds: impl IntoIterator<Item = PaneKind>) -> Self {
        kinds.into_iter().fold(Self::default(), |held, kind| Self {
            charts: held.charts + usize::from(kind == PaneKind::Time),
            flow: held.flow || kind == PaneKind::Flow,
        })
    }

    /// How many panes it holds: one dash each.
    #[must_use]
    pub const fn panes(self) -> usize {
        self.charts + self.flow as usize
    }

    /// What the rail says on hover and in its scene entry: a lone pane by
    /// `lone`, its own name (a strip's), asked only then, else by count,
    /// then how to open it.
    #[must_use]
    pub fn words(self, lone: impl FnOnce() -> Option<String>) -> String {
        let one = (self.panes() == 1).then(lone).flatten();
        let what = match (self.panes(), one) {
            (1, Some(name)) => name,
            (1, None) if self.flow => "Order flow".to_owned(),
            (1, None) => "1 chart".to_owned(),
            (n, _) if self.flow => format!("{n} panes"),
            (n, _) => format!("{n} charts"),
        };
        format!("{what} · drag to open")
    }
}

/// How one rail is drawn, besides where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RailLook {
    /// Folded across a column: its length runs left to right.
    pub flat: bool,
    /// The pane opens toward the far side across it: right, or down.
    pub opens_forward: bool,
    /// Dashes to lay, one per pane behind it.
    pub dashes: usize,
    /// Each long side, near then far, that lies on the canvas's own edge,
    /// where the window's chrome draws the line the rail would.
    pub on_chrome: [bool; 2],
}

impl RailLook {
    /// A rail clear of the window's chrome on both long sides, holding
    /// `dashes` panes.
    #[must_use]
    pub const fn between(flat: bool, opens_forward: bool, dashes: usize) -> Self {
        Self {
            flat,
            opens_forward,
            dashes,
            on_chrome: [false; 2],
        }
    }

    /// The look of `rail` on `canvas`, with one dash for each of `dashes`
    /// panes behind it.
    #[must_use]
    pub fn on(
        rail: CanvasRect,
        canvas: CanvasRect,
        (flat, forward): (bool, bool),
        dashes: usize,
    ) -> Self {
        let (near, far, low, high) = if flat {
            (rail.y, rail.bottom(), canvas.y, canvas.bottom())
        } else {
            (rail.x, rail.right(), canvas.x, canvas.right())
        };
        Self {
            flat,
            opens_forward: forward,
            dashes,
            on_chrome: [(near - low).abs() < 0.5, (far - high).abs() < 0.5],
        }
    }
}

/// What `rail` paints, back to front, each mark in its ink: the band, the
/// edges `look` keeps, the chevron centred along it on the band's middle
/// column, and the dashes after it. A rail too short for the chevron and its
/// dashes drops the dashes, then keeps as many chevron rows as fit; nothing
/// lands outside `rail`, however narrow.
#[must_use]
pub fn rail_marks(rail: CanvasRect, look: RailLook) -> Vec<(CanvasRect, RailInk)> {
    let (along, across) = if look.flat {
        (rail.w, rail.h)
    } else {
        (rail.h, rail.w)
    };
    // A mark `length` long from `from` along the rail, `thick` from `at`
    // across, held inside the rail.
    let mark = |from: f32, length: f32, at: f32, thick: f32| {
        let thick = thick.min(across);
        let at = at.clamp(0.0, across - thick);
        if look.flat {
            CanvasRect::new(rail.x + from, rail.y + at, length, thick)
        } else {
            CanvasRect::new(rail.x + at, rail.y + from, thick, length)
        }
    };
    let mut marks = vec![(rail, RailInk::Band)];
    let edge = RAIL_EDGE_PX.min(across / 2.0);
    for (side, at) in [(0, 0.0), (1, across - edge)] {
        if !look.on_chrome[side] {
            marks.push((mark(0.0, along, at, edge), RailInk::Edge));
        }
    }
    // The band less its edges, and the chevron's box on its middle column.
    let kept = |side: usize| if look.on_chrome[side] { 0.0 } else { edge };
    let inner = across - kept(0) - kept(1);
    let low = kept(0) + ((inner - RAIL_CHEVRON_BOX_PX) / 2.0).floor();
    let rows = RAIL_CHEVRON_ROWS.min(along.max(0.0) as usize);
    let start = ((along - rows as f32) / 2.0).floor();
    let stroke = RAIL_CHEVRON_STROKE_PX;
    let reach = RAIL_CHEVRON_BOX_PX - stroke;
    for row in 0..rows {
        let step = row.min(rows - 1 - row) as f32;
        let at = low
            + if look.opens_forward {
                step
            } else {
                reach - step
            };
        marks.push((mark(start + row as f32, 1.0, at, stroke), RailInk::Grip));
    }
    let dashes = look.dashes.min(RAIL_MAX_DASHES);
    let first = start + rows as f32 - 1.0 + RAIL_DASHES_AFTER_PX;
    if dashes > 0 && first + (dashes - 1) as f32 * RAIL_DASH_PITCH_PX + 1.0 <= along {
        let at = low + (RAIL_CHEVRON_BOX_PX - RAIL_DASH_PX) / 2.0;
        for dash in 0..dashes {
            let from = first + dash as f32 * RAIL_DASH_PITCH_PX;
            marks.push((mark(from, 1.0, at, RAIL_DASH_PX), RailInk::Grip));
        }
    }
    marks
}

impl CanvasState {
    /// The column painter's rail of one side of the flow pane, the flow
    /// pane's own (`flow`) or the charts beside it, at `rail` on `canvas`:
    /// what it holds and how it is drawn, its chevron toward the open side.
    #[must_use]
    pub fn side_rail(
        &self,
        flow: bool,
        rail: CanvasRect,
        canvas: CanvasRect,
    ) -> (RailHolds, RailLook) {
        let held = self.side_rail_holds(flow);
        (
            held,
            RailLook::on(rail, canvas, (false, !flow), held.panes()),
        )
    }

    /// What the rail of one side of the flow pane holds, the flow pane's
    /// own (`flow`) or the charts beside it in its split, those on a rail
    /// when one side is folded: the column painter's two rails.
    #[must_use]
    pub fn side_rail_holds(&self, flow: bool) -> RailHolds {
        if flow {
            return RailHolds::of([PaneKind::Flow]);
        }
        let Some((children, index)) = self.flow_split() else {
            return RailHolds::default();
        };
        let beside = (children.iter().enumerate()).filter(|(at, _)| *at != index);
        let folded = |child: &&super::tree::PaneTree| child.railed(&|slot| self.pin(slot));
        let any = beside.clone().any(|(_, child)| folded(&child));
        let held = beside.filter(|(_, child)| !any || folded(child));
        RailHolds::of(
            held.flat_map(|(_, child)| child.panes())
                .map(|(_, kind)| kind),
        )
    }
}
