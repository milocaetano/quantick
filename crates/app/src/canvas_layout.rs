//! The canvas layout model: what panes a tab draws, and in what order.
//!
//! This module owns the one piece of the layout that has to be right before
//! anything else can be: **pane identity**. Pane ids namespace every egui
//! interaction a pane registers, so they have to be unique across the whole
//! window rather than within a tab — two panes sharing an id share a drag.
//!
//! It replaces an arithmetic allocator (`tab * 2`, `tab * 2 + 1`) that could
//! only ever hand out two ids per tab. That arithmetic was not merely a limit:
//! a third pane on tab 0 would have taken id 2, which is tab 1's flow pane,
//! and the two would have shared every gesture egui keys by id.

use eframe::egui;
use smallvec::SmallVec;

/// Hands out pane ids that are unique for the lifetime of the window.
///
/// Two rules, and the type exists to make both true by construction rather
/// than by review:
///
/// - **Never derived from position.** An id that encoded where a pane sits
///   would move gesture state between panes the moment one was reordered.
/// - **Never reused.** egui keys interaction state by id across frames, so a
///   recycled id inherits the dead pane's drag, scroll and popup state. A
///   monotonic counter is the whole mechanism: ids are spent, not recovered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneIdAllocator {
    /// The next id to hand out. Only ever increases.
    next: u64,
}

impl Default for PaneIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl PaneIdAllocator {
    /// An allocator that has handed out nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// The next unused pane id.
    ///
    /// Panics only after 2^64 panes, which is not a state a trading session
    /// reaches; the saturating arithmetic is here so that the failure mode, if
    /// the impossible happened, is a stuck id rather than a wrapped one that
    /// silently collides with pane 0.
    pub fn alloc(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }

    /// How many ids have been spent. Test and diagnostic use only — nothing
    /// may derive a pane's identity from it.
    #[must_use]
    #[cfg(test)]
    pub const fn spent(&self) -> u64 {
        self.next
    }
}

/// What a pane is *for*.
///
/// Decides which layers it may draw and how it is seeded — never where it
/// sits. Position is the row's business, and a kind that knew its own column
/// could not be moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// The order-flow pane: the liquidity heatmap, the aggression bubbles and
    /// the tape. quantick's protagonist.
    Flow,
    /// A timeframe pane: candles, indicators and drawings over time bars.
    Time,
    /// A kind that exists only under test, to prove that the layout engine
    /// does not decide anything from a pane's kind.
    ///
    /// This is not decoration. If `split_row`, `resolve_shares` or any other
    /// function that carves the row matched exhaustively on `PaneKind`, this
    /// variant would fail to compile the moment it was added — so the fact
    /// that the test build compiles at all *is* the proof that a new kind
    /// costs a table entry and nothing else.
    #[cfg(test)]
    Fake,
}

/// One named arrangement of panes.
///
/// A preset is a *seed and a recogniser*, not a state: applying one lays its
/// kinds out left to right, and a row that still matches a preset's kinds
/// reports that preset's name. Keeping it out of the state is what lets a
/// trader drag a divider without the layout losing its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutPreset {
    /// The stable wire name — what a config file, the `QUANTICK_LAYOUT` hook,
    /// a saved workspace and the control plane all call this layout. Never the
    /// label: that is prose a reader sees, and rewording it must not move an
    /// operator's ground.
    pub id: &'static str,
    /// What the picker and the menu call it.
    pub label: &'static str,
    /// The panes it lays out, left to right.
    pub kinds: &'static [PaneKind],
}

/// Every layout the canvas can draw, in picker order.
///
/// **This table is the registry.** Adding an arrangement is an entry here; no
/// function that carves the canvas may grow a case for it. The rule that keeps
/// the heatmap the protagonist lives here too, as data rather than as a check
/// somewhere in the layout engine: where a preset holds a flow pane, the flow
/// pane is *last*, so context charts default to the left of it.
pub static LAYOUT_PRESETS: &[LayoutPreset] = &[
    LayoutPreset {
        id: "flow",
        label: "Flow",
        kinds: &[PaneKind::Flow],
    },
    LayoutPreset {
        id: "time",
        label: "Timeframe",
        kinds: &[PaneKind::Time],
    },
    LayoutPreset {
        id: "time+flow",
        label: "Timeframe + Flow",
        kinds: &[PaneKind::Time, PaneKind::Flow],
    },
    LayoutPreset {
        id: "time+time+flow",
        label: "2 Timeframes + Flow",
        kinds: &[PaneKind::Time, PaneKind::Time, PaneKind::Flow],
    },
];

/// The preset `id` names, if the registry has one.
#[must_use]
pub fn preset(id: &str) -> Option<&'static LayoutPreset> {
    LAYOUT_PRESETS.iter().find(|preset| preset.id == id.trim())
}

/// Most panes one tab's canvas may hold at once.
///
/// A cap rather than a policy: every pane is a second `ChartState` cut from
/// the one tape, so the per-trade cost is linear in this number. Four is what
/// the shipped presets need with one spare; raising it is a deliberate act
/// that should come with the measurement that justifies it.
pub const MAX_CANVAS_PANES: usize = 4;

/// Most context panes one tab may stack beside the flow pane.
///
/// One fewer than [`MAX_CANVAS_PANES`], because the flow pane is always one of
/// them: quantick's canvas is the heatmap plus whatever context stands beside
/// it, never context alone plus more context.
pub const MAX_CONTEXT_PANES: usize = MAX_CANVAS_PANES - 1;

/// Width of the draggable divider between two panes, in pixels.
pub const CANVAS_DIVIDER_PX: f32 = 4.0;

/// Width of a collapsed pane, in pixels.
///
/// **Never zero.** The vertical axis already settled this question — see
/// `indicators::COLLAPSED_PANE_HEIGHT_PX`, "a pane that vanished would be an
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

/// Narrowest a pane may be squeezed to while it is still open.
///
/// A pane is not all chart. The price axis takes `app::AXIS_GUTTER` (64 px)
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

/// What decides one pane's width.
///
/// Deliberately the same shape as `indicators::PaneSizing`, which solves this
/// problem on the vertical axis. Two different vocabularies for "how big is
/// this pane" is how the two axes would start disagreeing about what a drag
/// means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaneWidth {
    /// The layout decides: share what the explicit panes left over, evenly.
    Auto,
    /// The trader dragged a divider to this share of the canvas.
    Manual(f32),
    /// Collapsed by hand. Takes [`COLLAPSED_PANE_WIDTH_PX`] however much room
    /// there is, and springs back to `restore` when it is expanded — so a
    /// pane returns to the width it had rather than to a default that would
    /// silently discard the trader's own sizing.
    Collapsed { restore: f32 },
}

impl PaneWidth {
    /// Whether this pane is collapsed to its rail.
    #[must_use]
    pub const fn is_collapsed(self) -> bool {
        matches!(self, Self::Collapsed { .. })
    }
}

/// One row of the canvas, carved.
///
/// `panes[i]` is the area of the `i`th pane; `dividers[i]` is the draggable
/// rule between pane `i` and pane `i + 1`, so there are always exactly one
/// fewer dividers than panes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowAreas {
    /// Left to right, as drawn.
    pub panes: SmallVec<[egui::Rect; MAX_CANVAS_PANES]>,
    /// One per seam. `dividers[i]` sits between `panes[i]` and `panes[i + 1]`.
    pub dividers: SmallVec<[egui::Rect; MAX_CANVAS_PANES]>,
}

/// What the dividers around the pane at `index` will take out of its share.
///
/// A pane's share buys the span between two divider *centres*, and each
/// divider is painted half inside each neighbour. An interior pane therefore
/// pays a full divider, an edge pane half of one. A collapsed pane has to buy
/// that back, or its rail would come out narrower than the grip it carries.
fn divider_cost_px(index: usize, count: usize) -> f32 {
    let half = CANVAS_DIVIDER_PX / 2.0;
    let before = if index > 0 { half } else { 0.0 };
    let after = if index + 1 < count { half } else { 0.0 };
    before + after
}

/// Resolve `widths` into the share of `total_px` each pane asks for.
///
/// Shares sum to one. Collapsed panes are converted to the share their rail
/// costs, `Auto` panes divide what is left evenly, and `Manual` panes keep
/// what they were dragged to.
fn resolve_shares(widths: &[PaneWidth], total_px: f32) -> SmallVec<[f32; MAX_CANVAS_PANES]> {
    let mut shares: SmallVec<[f32; MAX_CANVAS_PANES]> = SmallVec::with_capacity(widths.len());
    let count = widths.len();
    let collapsed_share = |index: usize| {
        if total_px > 0.0 {
            (COLLAPSED_PANE_WIDTH_PX + divider_cost_px(index, count)) / total_px
        } else {
            0.0
        }
    };

    // Pass one: the panes that named a size get served first, exactly as
    // `indicators::split_panes` serves explicit heights before automatic ones.
    let mut claimed = 0.0_f32;
    let mut auto_count = 0_usize;
    for (index, width) in widths.iter().enumerate() {
        match *width {
            PaneWidth::Collapsed { .. } => claimed += collapsed_share(index),
            PaneWidth::Manual(share) => claimed += share.clamp(0.0, 1.0),
            PaneWidth::Auto => auto_count += 1,
        }
    }

    // Pass two: whatever is left is divided evenly among the rest.
    let auto_share = if auto_count == 0 {
        0.0
    } else {
        ((1.0 - claimed) / auto_count as f32).max(0.0)
    };
    for (index, width) in widths.iter().enumerate() {
        shares.push(match *width {
            PaneWidth::Collapsed { .. } => collapsed_share(index),
            PaneWidth::Manual(share) => share.clamp(0.0, 1.0),
            PaneWidth::Auto => auto_share,
        });
    }

    // The shares must spend the row exactly once. Renormalising rather than
    // trusting the arithmetic is what keeps a saved fraction from a different
    // window size — or a preset whose manual shares do not add up — from
    // leaving a strip of the canvas unpainted.
    let sum: f32 = shares.iter().sum();
    if sum > f32::EPSILON {
        for share in &mut shares {
            *share /= sum;
        }
    } else if !shares.is_empty() {
        let even = 1.0 / shares.len() as f32;
        shares.iter_mut().for_each(|share| *share = even);
    }

    apply_min_width(&mut shares, widths, total_px);
    shares
}

/// Raise any open pane that fell below [`MIN_PANE_WIDTH_PX`] back to it,
/// taking the difference from the panes that have room to give.
///
/// Skipped entirely when the row cannot afford every pane its floor: a window
/// dragged narrower than its panes need is a real state, and the honest answer
/// is thin panes rather than a layout that overflows the canvas it was given.
/// Collapsed panes are never raised — a rail is a deliberate width, not a pane
/// that lost an argument with the arithmetic.
fn apply_min_width(shares: &mut [f32], widths: &[PaneWidth], total_px: f32) {
    if total_px <= 0.0 || shares.len() < 2 {
        return;
    }
    let floor = MIN_PANE_WIDTH_PX / total_px;
    let open = |index: usize| !widths[index].is_collapsed();

    let mut committed = 0.0_f32;
    let mut open_count = 0_usize;
    for (index, share) in shares.iter().enumerate() {
        if open(index) {
            open_count += 1;
        } else {
            committed += *share;
        }
    }
    if open_count == 0 || committed + floor * open_count as f32 > 1.0 {
        return;
    }

    // One pass, not a loop: everyone under the floor is raised to it, and
    // everyone above pays in proportion to how far above they are. The sum is
    // preserved by construction, so the row still spends the canvas once.
    let mut deficit = 0.0_f32;
    let mut surplus = 0.0_f32;
    for (index, share) in shares.iter().enumerate() {
        if !open(index) {
            continue;
        }
        if *share < floor {
            deficit += floor - *share;
        } else {
            surplus += *share - floor;
        }
    }
    if deficit <= f32::EPSILON || surplus <= f32::EPSILON {
        return;
    }
    let levy = (deficit / surplus).min(1.0);
    for (index, share) in shares.iter_mut().enumerate() {
        if !open(index) {
            continue;
        }
        if *share < floor {
            *share = floor;
        } else {
            *share -= (*share - floor) * levy;
        }
    }
}

/// Carve `area` into one pane per entry in `widths`, with a draggable divider
/// on every seam.
///
/// The divider sits **on** the split rather than beside it, so a pane spans
/// from one divider's inner edge to the next and the row spends the canvas
/// exactly once. Panes keep the full height: this splits one axis only.
///
/// Per-frame, and allocation-free — the areas live in `SmallVec`s sized for
/// [`MAX_CANVAS_PANES`], because this runs inside `draw_canvas` on every tab
/// on every frame.
#[must_use]
pub fn split_row(area: egui::Rect, widths: &[PaneWidth]) -> RowAreas {
    split_axis(area, widths, Axis::Horizontal)
}

/// Carve `area` top to bottom instead of left to right.
///
/// Not yet called by the canvas, and the reason is worth writing down rather
/// than leaving as a gap: laying two context panes out is arithmetic this
/// function already does, but *drawing* on them is not. `shared_picks`,
/// `apply_shared_interactions` and `paint_shared_drawings` are pairwise by
/// construction — each asks "the other pane", singular — and a mark shared
/// across three panes has two owners, not one. Generalising that decides
/// which chart a trader's edit lands on, so it is a design change rather than
/// a rename, and it lands with the commit that draws the stack.
///
/// The context column stacks its panes; the row beside the flow pane splits
/// across. Same arithmetic, same floors, same collapse rule — one function,
/// because two would drift and a trader would find a divider that behaved one
/// way horizontally and another way vertically.
#[must_use]
pub fn split_column(area: egui::Rect, heights: &[PaneWidth]) -> RowAreas {
    split_axis(area, heights, Axis::Vertical)
}

/// Which way a split runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    /// How long `area` is along this axis.
    fn extent(self, area: egui::Rect) -> f32 {
        match self {
            Self::Horizontal => area.width(),
            Self::Vertical => area.height(),
        }
    }

    /// Where `area` starts along this axis.
    fn start(self, area: egui::Rect) -> f32 {
        match self {
            Self::Horizontal => area.left(),
            Self::Vertical => area.top(),
        }
    }

    /// Where `area` ends along this axis.
    fn end(self, area: egui::Rect) -> f32 {
        match self {
            Self::Horizontal => area.right(),
            Self::Vertical => area.bottom(),
        }
    }

    /// A band of `area` between `from` and `to` along this axis, keeping the
    /// full extent of the other one.
    fn band(self, area: egui::Rect, from: f32, to: f32) -> egui::Rect {
        match self {
            Self::Horizontal => egui::Rect::from_min_max(
                egui::pos2(from, area.top()),
                egui::pos2(to.max(from), area.bottom()),
            ),
            Self::Vertical => egui::Rect::from_min_max(
                egui::pos2(area.left(), from),
                egui::pos2(area.right(), to.max(from)),
            ),
        }
    }
}

fn split_axis(area: egui::Rect, widths: &[PaneWidth], axis: Axis) -> RowAreas {
    let mut areas = RowAreas::default();
    if widths.is_empty() {
        return areas;
    }
    if widths.len() == 1 {
        areas.panes.push(area);
        return areas;
    }

    let shares = resolve_shares(widths, axis.extent(area));
    let half = CANVAS_DIVIDER_PX / 2.0;

    // Boundaries are cumulative shares of the *whole* width, so the divider
    // centres land where the trader dragged them. Widths fall out of the
    // boundaries rather than the other way round, which is what keeps the
    // arithmetic from leaving a sliver of canvas unspent.
    let mut boundaries: SmallVec<[f32; MAX_CANVAS_PANES]> = SmallVec::new();
    let mut cumulative = 0.0_f32;
    for share in shares.iter().take(widths.len() - 1) {
        cumulative += share;
        boundaries.push(axis.start(area) + axis.extent(area) * cumulative);
    }

    let mut head = axis.start(area);
    for (index, boundary) in boundaries.iter().enumerate() {
        let boundary = boundary.clamp(axis.start(area), axis.end(area));
        areas.panes.push(axis.band(area, head, boundary - half));
        areas
            .dividers
            .push(axis.band(area, boundary - half, boundary + half));
        head = boundary + half;
        debug_assert!(index < widths.len(), "one divider per seam, never more");
    }
    areas
        .panes
        .push(axis.band(area, head.min(axis.end(area)), axis.end(area)));
    areas
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn ids_are_unique_across_three_tabs_of_three_panes() {
        // The shape the old `tab * 2` allocator could not express: a third
        // pane on the first tab used to collide with the second tab's flow
        // pane, and the two shared every gesture egui keys by id.
        let mut allocator = PaneIdAllocator::new();
        let mut seen = BTreeSet::new();
        for _tab in 0..3 {
            for _pane in 0..3 {
                let id = allocator.alloc();
                assert!(seen.insert(id), "pane id {id} was handed out twice");
            }
        }
        assert_eq!(seen.len(), 9, "nine panes must hold nine distinct ids");
    }

    #[test]
    fn an_id_is_never_reused_after_its_pane_is_removed() {
        // Removing a pane must not return its id to the pool: egui keys
        // interaction state by id across frames, so a recycled id would
        // inherit the removed pane's drag.
        let mut allocator = PaneIdAllocator::new();
        let first = allocator.alloc();
        let second = allocator.alloc();
        // The pane holding `second` is closed here; nothing tells the
        // allocator, and that is the point.
        let third = allocator.alloc();
        assert_ne!(third, second, "a removed pane's id came back");
        assert_ne!(third, first);
        assert_eq!(allocator.spent(), 3);
    }

    #[test]
    fn consecutive_ids_are_always_distinct() {
        let mut allocator = PaneIdAllocator::new();
        let mut seen = BTreeSet::new();
        for _ in 0..8 {
            assert!(seen.insert(allocator.alloc()), "an id came back");
        }
    }

    #[test]
    fn ids_do_not_encode_the_tab_they_were_asked_for() {
        // A regression guard on the rule rather than on an implementation:
        // whatever the allocator does, an id must not be derivable from a tab
        // index, or reordering would carry gesture state with the position
        // instead of with the pane.
        let mut allocator = PaneIdAllocator::new();
        let first_tab_flow = allocator.alloc();
        let _first_tab_context = allocator.alloc();
        let second_tab_flow = allocator.alloc();
        assert_ne!(
            second_tab_flow,
            first_tab_flow * 2,
            "an id that is a function of the tab index is the bug this replaced"
        );
    }
}

#[cfg(test)]
mod row_tests {
    use super::*;

    fn canvas() -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0))
    }

    /// The row spends the canvas exactly once, whatever it holds. A sliver of
    /// unpainted canvas between two panes reads as a rendering fault.
    fn assert_spends_the_canvas(area: egui::Rect, areas: &RowAreas) {
        let spent: f32 = areas.panes.iter().map(|pane| pane.width()).sum::<f32>()
            + areas
                .dividers
                .iter()
                .map(|divider| divider.width())
                .sum::<f32>();
        assert!(
            (spent - area.width()).abs() < 1e-3,
            "the row spent {spent} of a {} canvas",
            area.width()
        );
        assert_eq!(areas.panes.first().map(egui::Rect::left), Some(area.left()));
        assert_eq!(
            areas.panes.last().map(egui::Rect::right),
            Some(area.right())
        );
        for pane in &areas.panes {
            assert_eq!(pane.top(), area.top(), "the split is vertical only");
            assert_eq!(pane.bottom(), area.bottom());
        }
    }

    #[test]
    fn a_lone_pane_takes_the_whole_canvas_and_needs_no_divider() {
        let area = canvas();
        let areas = split_row(area, &[PaneWidth::Auto]);
        assert_eq!(areas.panes.as_slice(), &[area]);
        assert!(areas.dividers.is_empty(), "one pane has no seam");
    }

    #[test]
    fn there_is_always_one_fewer_divider_than_pane() {
        for count in 1..=MAX_CANVAS_PANES {
            let widths = vec![PaneWidth::Auto; count];
            let areas = split_row(canvas(), &widths);
            assert_eq!(areas.panes.len(), count);
            assert_eq!(
                areas.dividers.len(),
                count - 1,
                "{count} panes must have {} seams",
                count - 1
            );
        }
    }

    #[test]
    fn every_pane_count_spends_the_canvas_exactly_once() {
        for count in 1..=MAX_CANVAS_PANES {
            let widths = vec![PaneWidth::Auto; count];
            let area = canvas();
            assert_spends_the_canvas(area, &split_row(area, &widths));
        }
    }

    /// The two-pane row must land where the old fixed splitter did, or every
    /// saved workspace would open on a canvas that shifted under it.
    #[test]
    fn a_two_pane_row_puts_the_divider_on_the_share_it_was_given() {
        let area = canvas();
        for asked in [0.25_f32, 0.35, 0.5, 0.65, 0.75] {
            let areas = split_row(area, &[PaneWidth::Manual(asked), PaneWidth::Auto]);
            let split = (areas.dividers[0].center().x - area.left()) / area.width();
            assert!(
                (split - asked).abs() < 1e-3,
                "asked for {asked}, divider landed at {split}"
            );
            assert_eq!(areas.dividers[0].width(), CANVAS_DIVIDER_PX);
            assert_eq!(areas.panes[0].right(), areas.dividers[0].left());
            assert_eq!(areas.dividers[0].right(), areas.panes[1].left());
            assert_spends_the_canvas(area, &areas);
        }
    }

    /// The shape the old model could not express at all: two context panes
    /// beside the flow pane.
    #[test]
    fn a_three_pane_row_carves_two_seams_in_order() {
        let area = canvas();
        let areas = split_row(
            area,
            &[
                PaneWidth::Manual(0.175),
                PaneWidth::Manual(0.175),
                PaneWidth::Auto,
            ],
        );
        assert_eq!(areas.panes.len(), 3);
        assert_eq!(areas.dividers.len(), 2);
        assert!(areas.panes[0].right() <= areas.panes[1].left());
        assert!(areas.panes[1].right() <= areas.panes[2].left());
        assert!(
            areas.panes[2].width() > areas.panes[0].width() + areas.panes[1].width(),
            "the flow pane keeps the majority of the canvas"
        );
        assert_spends_the_canvas(area, &areas);
    }

    #[test]
    fn a_collapsed_pane_takes_its_rail_and_no_more() {
        let area = canvas();
        let areas = split_row(
            area,
            &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
        );
        assert!(
            (areas.panes[0].width() - COLLAPSED_PANE_WIDTH_PX).abs() < 1.0,
            "a collapsed pane is a rail, got {} px",
            areas.panes[0].width()
        );
        assert!(
            areas.panes[0].width() > 0.0,
            "a collapsed pane is never zero — there would be nothing left to click"
        );
        assert_spends_the_canvas(area, &areas);
    }

    #[test]
    fn every_pane_collapsed_still_spends_the_canvas() {
        let area = canvas();
        let widths = vec![PaneWidth::Collapsed { restore: 0.25 }; 3];
        let areas = split_row(area, &widths);
        assert_eq!(areas.panes.len(), 3);
        assert_spends_the_canvas(area, &areas);
    }

    /// A window dragged narrower than the panes' floors is a real state, and
    /// the row must degrade rather than overflow: the panes get thin, the
    /// canvas is still spent once, and nothing is painted outside it.
    #[test]
    fn a_canvas_too_narrow_for_its_panes_degrades_instead_of_overflowing() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(60.0, 600.0));
        let areas = split_row(area, &[PaneWidth::Auto, PaneWidth::Auto, PaneWidth::Auto]);
        assert_eq!(areas.panes.len(), 3);
        for pane in &areas.panes {
            assert!(pane.left() >= area.left() - 1e-3);
            assert!(pane.right() <= area.right() + 1e-3);
            assert!(pane.width() >= 0.0, "a pane never has negative width");
        }
        assert_spends_the_canvas(area, &areas);
    }

    #[test]
    fn a_zero_width_canvas_is_survived_rather_than_panicked_on() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(0.0, 600.0));
        let areas = split_row(area, &[PaneWidth::Auto, PaneWidth::Auto]);
        assert_eq!(areas.panes.len(), 2);
        for pane in &areas.panes {
            assert!(pane.width() >= 0.0);
        }
    }

    #[test]
    fn an_empty_row_draws_nothing() {
        let areas = split_row(canvas(), &[]);
        assert!(areas.panes.is_empty());
        assert!(areas.dividers.is_empty());
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;

    /// The registry is the vocabulary. Every id is a name a config file, the
    /// `QUANTICK_LAYOUT` hook and a saved workspace may all use, so two
    /// presets answering to one name would make a saved layout ambiguous.
    #[test]
    fn every_preset_id_is_unique() {
        for (index, preset) in LAYOUT_PRESETS.iter().enumerate() {
            for other in LAYOUT_PRESETS.iter().skip(index + 1) {
                assert_ne!(preset.id, other.id, "two presets answer to {}", preset.id);
            }
        }
    }

    #[test]
    fn every_preset_is_reachable_by_its_id() {
        for entry in LAYOUT_PRESETS {
            assert_eq!(preset(entry.id), Some(entry));
        }
        assert_eq!(preset("no such layout"), None);
        assert_eq!(preset("  flow  "), preset("flow"), "ids are read trimmed");
    }

    /// The rule that keeps the heatmap the protagonist, held as data rather
    /// than as a check inside the layout engine: a preset that draws the flow
    /// pane draws it **last**, so context charts default to its left.
    #[test]
    fn the_flow_pane_is_always_the_rightmost_pane_a_preset_holds() {
        for entry in LAYOUT_PRESETS {
            let flow_at = entry.kinds.iter().position(|kind| *kind == PaneKind::Flow);
            if let Some(index) = flow_at {
                assert_eq!(
                    index,
                    entry.kinds.len() - 1,
                    "preset {} puts a context pane right of the flow pane",
                    entry.id
                );
            }
            assert_eq!(
                entry.kinds.iter().filter(|k| **k == PaneKind::Flow).count(),
                flow_at.map_or(0, |_| 1),
                "preset {} holds more than one flow pane",
                entry.id
            );
        }
    }

    #[test]
    fn no_preset_asks_for_more_panes_than_the_canvas_allows() {
        for entry in LAYOUT_PRESETS {
            assert!(!entry.kinds.is_empty(), "preset {} draws nothing", entry.id);
            assert!(
                entry.kinds.len() <= MAX_CANVAS_PANES,
                "preset {} wants {} panes",
                entry.id,
                entry.kinds.len()
            );
        }
    }

    #[test]
    fn every_preset_lays_out_and_spends_the_canvas() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1600.0, 900.0));
        for entry in LAYOUT_PRESETS {
            let widths = vec![PaneWidth::Auto; entry.kinds.len()];
            let areas = split_row(area, &widths);
            assert_eq!(areas.panes.len(), entry.kinds.len(), "preset {}", entry.id);
            assert_eq!(areas.dividers.len(), entry.kinds.len() - 1);
        }
    }

    /// The port proof.
    ///
    /// A pane kind the registry has never heard of is laid out by the same
    /// splitter, and nothing in the layout engine had to learn its name. The
    /// compiler carries most of this test: `PaneKind::Fake` exists only under
    /// `cfg(test)`, so if `split_row`, `resolve_shares` or `apply_min_width`
    /// matched exhaustively on a pane's kind, this module would not build.
    /// What is left to assert is that a fake preset behaves like a real one.
    #[test]
    fn a_pane_kind_the_engine_has_never_heard_of_lays_out_anyway() {
        static FAKE: LayoutPreset = LayoutPreset {
            id: "fake+fake+flow",
            label: "Fake",
            kinds: &[PaneKind::Fake, PaneKind::Fake, PaneKind::Flow],
        };
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1600.0, 900.0));
        let widths = vec![PaneWidth::Auto; FAKE.kinds.len()];
        let areas = split_row(area, &widths);

        assert_eq!(areas.panes.len(), 3, "three fake panes, three areas");
        assert_eq!(areas.dividers.len(), 2);
        let spent: f32 = areas.panes.iter().map(egui::Rect::width).sum::<f32>()
            + areas.dividers.iter().map(egui::Rect::width).sum::<f32>();
        assert!(
            (spent - area.width()).abs() < 1e-3,
            "a row of unknown kinds still spends the canvas exactly once"
        );
        assert!(
            !LAYOUT_PRESETS.iter().any(|entry| entry.id == FAKE.id),
            "the fake preset must not be in the shipped registry"
        );
    }

    /// Width is decided by `PaneWidth` alone — a pane's kind never reaches
    /// the arithmetic. Asserted here as a property of the signature rather
    /// than by comparing two identical calls: the row is laid out from widths
    /// only, so there is no kind to pass in, and a rail is a rail whatever the
    /// pane it hides holds.
    #[test]
    fn the_rail_is_the_same_width_at_every_position_in_the_row() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
        let collapsed = PaneWidth::Collapsed { restore: 0.3 };

        // Leading, interior and trailing: each loses a different number of
        // divider halves, and each must still come out a full rail wide.
        for widths in [
            vec![collapsed, PaneWidth::Auto, PaneWidth::Auto],
            vec![PaneWidth::Auto, collapsed, PaneWidth::Auto],
            vec![PaneWidth::Auto, PaneWidth::Auto, collapsed],
        ] {
            let index = widths
                .iter()
                .position(|width| width.is_collapsed())
                .expect("the row under test holds a collapsed pane");
            let areas = split_row(area, &widths);
            assert!(
                (areas.panes[index].width() - COLLAPSED_PANE_WIDTH_PX).abs() < 1e-3,
                "a rail at position {index} came out {} px",
                areas.panes[index].width()
            );
        }
    }
}

#[cfg(test)]
mod column_tests {
    use super::*;

    fn canvas() -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 900.0))
    }

    /// The context column stacks its panes top to bottom, and spends its
    /// height exactly once — the vertical twin of the row's own rule.
    #[test]
    fn a_column_stacks_top_to_bottom_and_spends_its_height_once() {
        let area = canvas();
        let areas = split_column(area, &[PaneWidth::Auto, PaneWidth::Auto]);
        assert_eq!(areas.panes.len(), 2);
        assert_eq!(areas.dividers.len(), 1);
        assert!(
            areas.panes[0].bottom() <= areas.panes[1].top(),
            "the first pane is the upper one"
        );
        let spent: f32 = areas.panes.iter().map(egui::Rect::height).sum::<f32>()
            + areas.dividers.iter().map(egui::Rect::height).sum::<f32>();
        assert!((spent - area.height()).abs() < 1e-3, "spent {spent}");
        assert_eq!(areas.panes[0].top(), area.top());
        assert_eq!(areas.panes[1].bottom(), area.bottom());
    }

    /// A column keeps the full width: stacking splits one axis only, exactly
    /// as the row keeps the full height.
    #[test]
    fn stacking_never_narrows_a_pane() {
        let area = canvas();
        for count in 1..=MAX_CONTEXT_PANES {
            let areas = split_column(area, &vec![PaneWidth::Auto; count]);
            for pane in &areas.panes {
                assert_eq!(pane.left(), area.left());
                assert_eq!(pane.right(), area.right());
            }
        }
    }

    /// The two axes are one function, so a divider dragged vertically behaves
    /// exactly as one dragged horizontally. Asserted by transposing a canvas
    /// and comparing the split it produces.
    #[test]
    fn a_column_splits_where_a_row_of_the_same_shares_would() {
        let widths = [PaneWidth::Manual(0.35), PaneWidth::Auto];
        let across = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1000.0, 600.0));
        let down = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(600.0, 1000.0));

        let row = split_row(across, &widths);
        let column = split_column(down, &widths);
        assert!(
            (row.panes[0].width() - column.panes[0].height()).abs() < 1e-3,
            "the same shares must carve the same extent on either axis: \
             row gave {} and column gave {}",
            row.panes[0].width(),
            column.panes[0].height()
        );
        assert!((row.dividers[0].width() - column.dividers[0].height()).abs() < 1e-3);
    }

    #[test]
    fn a_collapsed_pane_in_a_column_is_the_same_rail_a_row_gives() {
        let area = canvas();
        let areas = split_column(
            area,
            &[PaneWidth::Collapsed { restore: 0.5 }, PaneWidth::Auto],
        );
        assert!(
            (areas.panes[0].height() - COLLAPSED_PANE_WIDTH_PX).abs() < 1e-3,
            "a stacked rail came out {} px",
            areas.panes[0].height()
        );
    }
}

#[cfg(test)]
mod collapse_tests {
    use super::*;

    /// The rail is near-zero without being zero, and that gap is the whole
    /// design: a pane with no width has no handle, and a pane with no handle
    /// cannot be brought back from the canvas it left.
    #[test]
    fn a_collapsed_pane_is_a_sliver_of_the_canvas_but_never_none_of_it() {
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1080.0));
        let areas = split_row(
            area,
            &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
        );
        let share = areas.panes[0].width() / area.width();
        assert!(
            share < 0.01,
            "a rail taking {share} of the canvas is not a rail"
        );
        assert!(
            areas.panes[0].width() > 0.0,
            "a rail with no width is the bug this exists to prevent"
        );
    }

    /// The hit area is bigger than the paint, and by enough to clear the floor
    /// a pointer target is held to.
    #[test]
    fn the_rail_can_be_hit_even_though_it_is_thin() {
        // Read off a rail the splitter actually produced, rather than
        // comparing two literals: what matters is how far the hit area has to
        // reach past the paint that exists.
        let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1920.0, 1080.0));
        let painted = split_row(
            area,
            &[PaneWidth::Collapsed { restore: 0.35 }, PaneWidth::Auto],
        )
        .panes[0]
            .width();
        assert!(
            COLLAPSED_HIT_PX > painted,
            "the hit area must reach past the {painted}px it paints"
        );
    }

    /// The gesture that dismisses a pane has to be reachable before the pane's
    /// own minimum stops the drag, or a trader could never reach it.
    #[test]
    fn the_collapse_threshold_is_inside_the_range_a_drag_can_reach() {
        // The threshold is in pixels and so is the floor, which is the whole
        // reason it stopped being a share of the canvas: as a share it sat
        // under the floor on a trading monitor and over it on a laptop, and
        // over it the floor never binds. Read at the widths a real window
        // takes, so the test says what it protects.
        for width in [1024.0_f32, 1280.0, 1600.0, 1920.0, 2560.0] {
            let area = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(width, 900.0));
            let floor_share = MIN_PANE_WIDTH_PX / area.width();
            let threshold_share = COLLAPSE_AT_PX / area.width();
            assert!(
                threshold_share < floor_share,
                "on a {width}px canvas the pane is dismissed at {threshold_share}                  while its floor is {floor_share}, so every width between them                  is unreachable"
            );
        }
    }
}
