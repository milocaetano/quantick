//! What a folded pane's rail paints inside its 8 px, and what it says.
//!
//! One look for every rail on both painters: a band, an edge along each long
//! side that does not meet the window's chrome (which already draws that
//! line), a chevron pointing the way the pane opens and one square tick per
//! pane folded behind it. Each mark names which of the theme's three rail
//! inks it wears; the theme maps those to tokens
//! (`quantick_appearance::rail::rail_inks`, in [`RailInk`] order).

use super::presets::PaneKind;
use super::state::CanvasState;
use super::tree::CanvasRect;

/// The edge along a long side of the rail.
pub const RAIL_EDGE_PX: f32 = 1.0;
/// The chevron: this many one-pixel rows along the rail, each a stroke this
/// thick across it, stepping toward the side the pane opens to and back.
pub const RAIL_CHEVRON_ROWS: usize = 5;
pub const RAIL_CHEVRON_STROKE_PX: f32 = 2.0;
/// A tick's side, and the room between two ticks and before the first.
pub const RAIL_TICK_PX: f32 = 2.0;
pub const RAIL_TICK_GAP_PX: f32 = 2.0;
pub const RAIL_TICKS_AFTER_PX: f32 = 4.0;
/// The most ticks a rail lays: past it, the count reads in its words.
pub const RAIL_MAX_TICKS: usize = 8;

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

    /// How many panes it holds: one tick each.
    #[must_use]
    pub const fn panes(self) -> usize {
        self.charts + self.flow as usize
    }

    /// What the rail says on hover and in its scene entry: a lone pane by
    /// `lone`, its own name (a strip's), else by count, then how to open it.
    #[must_use]
    pub fn words(self, lone: Option<&str>) -> String {
        let what = match (self.panes(), lone) {
            (1, Some(name)) => name.to_owned(),
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
    /// Ticks to lay, one per pane behind it.
    pub ticks: usize,
    /// Each long side, near then far, that lies on the canvas's own edge,
    /// where the window's chrome draws the line the rail would.
    pub on_chrome: [bool; 2],
}

impl RailLook {
    /// A rail clear of the window's chrome on both long sides, `ticks` long.
    #[must_use]
    pub const fn between(flat: bool, opens_forward: bool, ticks: usize) -> Self {
        Self {
            flat,
            opens_forward,
            ticks,
            on_chrome: [false; 2],
        }
    }

    /// The look of `rail` on `canvas`, with one tick for each of `ticks`
    /// panes behind it.
    #[must_use]
    pub fn on(
        rail: CanvasRect,
        canvas: CanvasRect,
        (flat, forward): (bool, bool),
        ticks: usize,
    ) -> Self {
        let (near, far, low, high) = if flat {
            (rail.y, rail.bottom(), canvas.y, canvas.bottom())
        } else {
            (rail.x, rail.right(), canvas.x, canvas.right())
        };
        Self {
            flat,
            opens_forward: forward,
            ticks,
            on_chrome: [(near - low).abs() < 0.5, (far - high).abs() < 0.5],
        }
    }
}

/// What `rail` paints, back to front, each mark in its ink: the band, the
/// edges `look` keeps, the chevron centred along it and the ticks after it.
/// A rail too short for the chevron and its ticks drops the ticks, then
/// keeps as many chevron rows as fit; nothing lands outside `rail`.
#[must_use]
pub fn rail_marks(rail: CanvasRect, look: RailLook) -> Vec<(CanvasRect, RailInk)> {
    let (along, across) = if look.flat {
        (rail.w, rail.h)
    } else {
        (rail.h, rail.w)
    };
    // A mark `length` long from `from` along the rail, `thick` from `at` across.
    let mark = |from: f32, length: f32, at: f32, thick: f32| {
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
    let rows = RAIL_CHEVRON_ROWS.min(along.max(0.0) as usize);
    let ticks = look.ticks.min(RAIL_MAX_TICKS);
    let pitch = RAIL_TICK_PX + RAIL_TICK_GAP_PX;
    let start = ((along - rows as f32) / 2.0).floor();
    let tail = start + rows as f32 + RAIL_TICKS_AFTER_PX + ticks as f32 * pitch - RAIL_TICK_GAP_PX;
    let centre = (across / 2.0).floor();
    let stroke = RAIL_CHEVRON_STROKE_PX.min(across);
    for row in 0..rows {
        let step = row.min(rows - 1 - row) as f32;
        let at = if look.opens_forward {
            centre - stroke + step
        } else {
            centre - step
        };
        let at = at.clamp(0.0, (across - stroke).max(0.0));
        marks.push((mark(start + row as f32, 1.0, at, stroke), RailInk::Grip));
    }
    if ticks > 0 && tail <= along {
        let side = RAIL_TICK_PX.min(across);
        for tick in 0..ticks {
            let from = start + rows as f32 + RAIL_TICKS_AFTER_PX + tick as f32 * pitch;
            marks.push((mark(from, side, centre - side / 2.0, side), RailInk::Grip));
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
    /// own (`flow`) or the charts beside it: the column painter's two rails.
    #[must_use]
    pub fn side_rail_holds(&self, flow: bool) -> RailHolds {
        if flow {
            return RailHolds::of([PaneKind::Flow]);
        }
        let panes = self.tree().panes();
        RailHolds::of(
            panes
                .into_iter()
                .map(|(_, kind)| kind)
                .filter(|kind| *kind == PaneKind::Time),
        )
    }
}
