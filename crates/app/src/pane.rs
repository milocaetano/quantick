//! One chart pane: everything that answers "what is on this canvas".
//!
//! A pane owns the bar series it aggregates, the viewport and price scale it is
//! read through, the drawings anchored to its bar indices and the indicator
//! slots computed over it. What it deliberately does *not* own is the market
//! feeding it — feed channels, connection state and notices belong to the tab
//! around it — or the window chrome (menus, toolbar, dock, status bar), which
//! belongs to the application around that.
//!
//! That split is what lets one tab hold two panes over the same trades: a flow
//! pane and a time-frame pane, the split view of `docs/ux/ui-design-model.md`
//! §11. Every egui interaction id a pane registers is derived from
//! [`ChartPane::id`] for the same reason — two panes registering one id would
//! share a drag.

use std::collections::BTreeSet;

use eframe::egui;
use rust_decimal::prelude::ToPrimitive as _;

use crate::bands;
use crate::chart::PriceScale;
use crate::chart_layers::{ChartLayer, LayerActions};
use crate::config::FeedCapabilities;
use crate::drawings::{self, Drawings};
use crate::indicator_worker::{IndicatorWorker, LaneTransport, MAX_LANE_RUNGS, SlotId};
use crate::indicators::{IndicatorViews, PaneSizing};
use crate::orderflow_view::OrderflowView;
use crate::paper_trading::PaperTrading;
use crate::plot_area::{self, PlotAreas, plot_split};
use crate::pointer_compass;
use crate::price_view::PriceView;
use crate::state::{BarSpec, ChartState, SpecSelector};
use crate::style::ChartStyle;
use crate::theme;
use crate::timezone::TzOffset;
use crate::toolrail::ToolRail;
use crate::viewport::Viewport;

// The tests in `pane/tests/` reach these through `use super::*`; the production
// code that read them moved to the siblings, so only the tests still need
// them here.
#[cfg(test)]
use crate::bands::BandLabel;
#[cfg(test)]
use crate::drawings::{ChartPoint, DrawingBand};
#[cfg(test)]
use crate::indicator_render;
#[cfg(test)]
use crate::plot_area::split_time_strip;
#[cfg(test)]
use crate::toolrail::Tool;
#[cfg(test)]
use pointer_hit::PLOT_PICK_TOLERANCE_PX;

mod axes_and_chrome;
mod axes_and_panes;
// `pub(crate)`, like `app::launch_hooks`: `split_time_pane` returns
// `TimePaneAreas`, which nothing outside names yet, so a `pub use` of it is an
// unused import under the workspace's deny-warnings policy while a public
// module keeps it nameable as `pane::canvas_split::TimePaneAreas`.
pub(crate) mod canvas_split;
mod context_menu;
mod draw_chart;
mod draw_frame;
mod drawing_gestures;
mod drawing_paint;
mod footprint;
mod frame;
mod gestures;
mod layer_painters;
mod layers;
mod menus;
mod pointer_hit;
mod primary_button;
mod series;
mod shared_marks;
mod strategies;
mod strategy_badges;
mod tape_switch;

/// Every sub-struct of a pane is `Pane*`, without exception: prefixing only
/// where the bare noun clashes is how one ends up beside a `PaneFrame`.
pub use context_menu::PaneContextMenu;
pub use footprint::PaneFootprint;
pub use frame::PaneFrame;
pub use gestures::PaneGestures;
pub use strategies::PaneStrategies;

/// The canvas split and the shared-mark contract keep their public paths
/// here: the tab, the layouts and the control plane name them as `pane::`.
pub use canvas_split::{
    CANVAS_DIVIDER_HANDLE_PX, DEFAULT_PANE_FRACTION, PaneSide, clamp_pane_fraction, split_time_pane,
};
pub(crate) use pointer_hit::{ControlDrawingHit, ControlPointerHit};
pub use shared_marks::{PaneIndex, SharedEdit, SharedInteraction, SharedPick};
use shared_marks::{SharedDrag, SharedPointer};
pub(crate) use tape_switch::tape_switch_rect;

/// Hit radius for selecting a drawing anchor, in logical pixels.
const DRAWING_SELECT_RADIUS_PX: f32 = 10.0;
/// Hit radius for a selected drawing's editable anchor.
pub const DRAWING_ANCHOR_RADIUS_PX: f32 = 12.0;
/// Minimum pointer travel that turns one press/release into drag placement.
/// How near the pointer must be to a bar's open / high / low / close for
/// the magnet to take the anchor. Generous enough to catch the swing you
/// aimed at, tight enough to still draw a free diagonal between bars.
const MAGNET_REACH_PX: f32 = 12.0;
/// The candle magnet has no reach: [`drawings::AnchorSnap::NearestOhlc`]
/// never lets go, however far the pointer floats from the candle.
const MAGNET_REACH_UNLIMITED_PX: f32 = f32::INFINITY;
const DRAWING_DRAG_THRESHOLD_PX: f32 = 4.0;

/// How far a press must travel before its release counts as "the trader
/// dragged this object out" instead of "the trader clicked".
///
/// Deliberately larger than [`DRAWING_DRAG_THRESHOLD_PX`], which answers a
/// different question: whether a gesture already under way is a drag at all.
/// This one decides whether a *release* finishes an object, and it was
/// sharing that four-pixel answer — which is inside the wander of an ordinary
/// click.
///
/// What that cost, reported from the running build: a click meant to start a
/// fixed-range profile placed **both** its anchors, so the object was born
/// less than one bar wide ("1 of 1 bars"), and completing it disarmed the
/// tool. Moving the pointer afterwards then did nothing at all, which reads
/// exactly like a frozen chart — the trader is waiting for a range to follow
/// their hand and there is no longer a draft to follow it.
///
/// A release under this distance leaves the draft alive instead, so the
/// gesture becomes the click-move-click the hand was already doing.
const DRAWING_DRAG_COMPLETES_PX: f32 = 12.0;

/// A pointer and a modifier for a run with nobody at the keyboard — see
/// [`PaneGestures::parked_hand`]. Never constructed outside the harness hook.
#[derive(Debug, Clone, Copy)]
pub struct ParkedHand {
    pub position: egui::Pos2,
    pub constrain: drawings::Constrain,
}
/// How far the pointer must travel before a freehand stroke records another
/// point. Chosen so a hand-drawn circle keeps its shape while a half-second
/// scribble stores tens of anchors instead of hundreds — every anchor is
/// paint and hit-test work on every frame for the rest of the session.
const FREEHAND_MIN_STEP_PX: f32 = 4.0;
/// Hard ceiling on one stroke, so a pointer that never stops moving cannot
/// turn a single drawing into an unbounded cost.
const FREEHAND_MAX_POINTS: usize = 512;

/// Alpha of the last-price line: legible at a glance without competing with a
/// candle or a bubble for attention.
const LAST_PRICE_LINE_ALPHA: f32 = 0.55;
/// Dash length, in pixels, of the last-price line. Dashed so it never reads as
/// a level someone drew.
const LAST_PRICE_DASH_PX: f32 = 4.0;
/// See [`LAST_PRICE_DASH_PX`].
const LAST_PRICE_GAP_PX: f32 = 4.0;
/// Ink on the last-price chip. The chip is filled with a saturated candle
/// colour, so its text is the one place on the chrome that goes dark.
const LAST_PRICE_CHIP_TEXT: egui::Color32 = egui::Color32::from_rgb(0x0E, 0x12, 0x1A);

/// Dash length, in pixels, of the venue↔prints seam marker. Long enough to
/// read as deliberate beside the solid backfill divider, short enough not to
/// be mistaken for one.
const SEAM_DASH_PX: f32 = 5.0;
/// See [`SEAM_DASH_PX`].
const SEAM_GAP_PX: f32 = 4.0;
/// Font size of the caption beside a dashed vertical mark, in points.
///
/// Shared by the venue seam and the tape-gap mark rather than written at each:
/// they are the same kind of caption answering the same question about the bars
/// either side of a line, and two copies would drift the first time one moved.
const SEAM_LABEL_PT: f32 = 11.0;
/// Gap between a dashed vertical mark and its caption, in pixels — and the
/// caption's drop from the top of the pane.
const SEAM_LABEL_INSET_PX: f32 = 4.0;

/// A dashed vertical rule down `rect` at `x`.
///
/// The same construction the heatmap's own boundary marks use: egui has no
/// dashed line primitive for a single segment, so the dashes are drawn.
fn draw_dashed_vertical(
    painter: &egui::Painter,
    x: f32,
    rect: egui::Rect,
    dash: f32,
    gap: f32,
    color: egui::Color32,
) {
    let dash = dash.max(0.5);
    let gap = gap.max(0.0);
    let stroke = egui::Stroke::new(1.0_f32, color);
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [
                egui::pos2(x, y),
                egui::pos2(x, (y + dash).min(rect.bottom())),
            ],
            stroke,
        );
        y += dash + gap;
    }
}

/// Half-width, in pixels, of the grab area over the live lane's divider.
///
/// The line itself stays a hairline — it marks where the present begins and a
/// thick rule there would read as a wall in the data. The handle around it is
/// what makes it draggable, and the resize cursor is the only thing that says
/// so.
const LANE_HANDLE_HALF_WIDTH_PX: f32 = 5.0;

/// Type size of the axis under the tape.
const LANE_AXIS_FONT_PX: f32 = 10.0;

/// Breathing room between the tape's two axis labels, and between the warning
/// and the strip's own right edge.
const LANE_AXIS_GAP_PX: f32 = 8.0;

/// Pixels of lane the ladder spends one rung on.
///
/// A rung is a full evaluation of every hosted indicator, so this is the knob
/// that trades cost against smoothness. Six pixels draws a curve that reads as
/// continuous at the sizes the lane is ever given, and keeps a wide lane well
/// under [`MAX_LANE_RUNGS`].
const LANE_RUNG_PX: f32 = 6.0;

/// How many rungs a lane this wide is worth sampling at.
///
/// Zero for a chart with no lane — the signal that no ladder is walked at all.
fn lane_rungs(lane_width_px: f32) -> usize {
    if !lane_width_px.is_finite() || lane_width_px <= 0.0 {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rungs = (lane_width_px / LANE_RUNG_PX) as usize;
    rungs.clamp(1, MAX_LANE_RUNGS)
}

/// Wheel travel that doubles or halves what a time axis shows.
///
/// One number for the candles, the lane and every pane body, so a scroll means
/// the same amount of zoom wherever the pointer happens to be resting. It was
/// already one number — written out four times.
const SCROLL_ZOOM_PX: f32 = 300.0;

/// Width of the jump-to-live chip on the time strip, in pixels.
const LIVE_CHIP_WIDTH_PX: f32 = 56.0;
/// Gap between the chip and the strip's right edge, in pixels.
const LIVE_CHIP_MARGIN_PX: f32 = 6.0;
/// Vertical inset of the chip inside the strip, in pixels.
const LIVE_CHIP_VPAD_PX: f32 = 3.0;

/// Where the jump-to-live chip sits (audit F6): right-aligned inside the
/// history segment of the time strip — the live end of the axis, which is
/// where the eye looks for the way back. One geometry for the input region
/// and the paint, so the click can never miss the pixels.
fn live_chip_rect(history_strip: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            history_strip.right() - LIVE_CHIP_MARGIN_PX - LIVE_CHIP_WIDTH_PX,
            history_strip.top() + LIVE_CHIP_VPAD_PX,
        ),
        egui::pos2(
            history_strip.right() - LIVE_CHIP_MARGIN_PX,
            history_strip.bottom() - LIVE_CHIP_VPAD_PX,
        ),
    )
}

/// Paint the jump-to-live chip: solid accent, dark ink — the chip language
/// of the price gutter, because this too is a statement about the axis.
/// Accent, not amber: it is a control, not a provenance statement.
fn draw_live_chip(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, egui::Rounding::same(3.0), theme::ACCENT);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "» live",
        egui::FontId::proportional(11.0),
        theme::CHIP_INK,
    );
}

/// Whether a freshly folded prefix differs from the one already installed.
///
/// Length and the two end open-times, not a full comparison: the fold is
/// deterministic over the same base, so two runs agreeing on how many bars
/// they produced and which windows the first and last cover agree on
/// everything between. The full compare was ~129k `Decimal`s on every frame
/// of a settled interval drag.
fn prefix_differs(current: &[quantick_engine::Bar], next: &[quantick_engine::Bar]) -> bool {
    if current.len() != next.len() {
        return true;
    }
    let ends = |bars: &[quantick_engine::Bar]| {
        (
            bars.first().map(|bar| bar.open_time),
            bars.last().map(|bar| bar.open_time),
        )
    };
    ends(current) != ends(next)
}

/// Convert an explicit unmultiplied RGBA style colour to egui.
fn color32([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// The canvas background colour `style` asks for.
pub fn background_color(style: &ChartStyle) -> egui::Color32 {
    color32(style.canvas.background_rgba())
}

/// The chart-grid colour `style` asks for. `TRANSPARENT` disables grid painting
/// without branching throughout the axis code.
pub fn grid_color(style: &ChartStyle) -> egui::Color32 {
    style
        .canvas
        .grid_rgba()
        .map_or(egui::Color32::TRANSPARENT, color32)
}

/// Why an armed instance's region cannot honestly be tested right now, or
/// `None` when it can.
///
/// One rule, two readers: [`ChartPane::strategy_region`] shuts the gate on it
/// and [`ChartPane::badge_text_for`] prints it. Two copies would let the chart
/// paint a running bot over a region every bar is refused against — the
/// divergence a trader only discovers by watching a setup go by, which is
/// exactly how this was found.
///
/// The order is the order the trader can act on: another market needs the
/// region redrawn, a lost series needs the drawing re-anchored, a hidden one
/// needs a click.
fn region_pause(drawing: &drawings::Drawing, all_hidden: bool) -> Option<&'static str> {
    if drawing.foreign_market {
        return Some("region on another market — paused");
    }
    if drawing.off_series {
        return Some("region off its series — paused");
    }
    if drawing.hidden || all_hidden {
        return Some("region hidden — paused");
    }
    None
}

/// What the pointer is currently doing to a drawing, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawingDrag {
    #[default]
    None,
    Translate,
    /// A grab on one of the object's handles. `handle` indexes the tool's own
    /// handle list, which is the anchors for almost every tool but not for
    /// all of them — a channel's rail handles move anchors they do not sit
    /// on, so only the tool may turn this index into new geometry.
    Handle {
        drawing_index: usize,
        handle: usize,
    },
    /// The press landed on a locked drawing: the gesture belongs to the
    /// object (the chart must not pan) but the geometry stays put.
    Blocked,
}

impl DrawingDrag {
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// One price a drawing declares for the price axis to tag.
///
/// Carries both the pixel and the price because the two answer different
/// questions and are read off one scale: the painter needs the height, and
/// anything reading the trader's levels as data needs the number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PriceAxisLevel {
    /// The object that declared it.
    pub id: drawings::DrawingId,
    /// Where it sits on the axis, in screen pixels.
    pub y: f32,
    /// What that height reads as on the pane's price scale.
    pub price: f64,
    /// The object's own colour — the tag is the object, said on the axis.
    pub color: egui::Color32,
}

/// What the price axis may not write a round number over this frame.
///
/// Two sources, kept apart because they are *stored* differently and not
/// because they mean different things: the chips the axis draws itself are a
/// pair that fits inline, and the levels are the list already gathered for
/// painting, borrowed rather than copied into a third container once a frame.
struct PriceAxisClaims<'a> {
    /// The pointer's tag and the last-price chip.
    marks: pointer_compass::AxisClaims,
    /// One per level a drawing declared.
    levels: &'a [PriceAxisLevel],
}

impl PriceAxisClaims<'_> {
    /// Every claimed height, from both sources, allocating nothing.
    fn heights(&self) -> impl Iterator<Item = f32> + '_ {
        self.marks
            .iter()
            .copied()
            .chain(self.levels.iter().map(|level| level.y))
    }
}

/// What the pointer's compass will draw this frame, and where.
///
/// One decision, read twice: the axes consult it before labelling themselves
/// so they can leave the coordinate alone, and the paint pass draws exactly
/// what it says.
struct PointerCompass {
    readout: pointer_compass::PointerReadout,
    /// The price half is drawn — its layer is on, the pointer is over the
    /// price band, and the crosshair is not already writing one.
    price: bool,
    /// The time half is drawn — its layer is on and a bar is under the
    /// pointer.
    time: bool,
}

/// Which handle of a projected object `pos` grabs, if any.
///
/// The one handle rule, shared by a pane's own marks and the mirrored ones,
/// so the two can never disagree about what a press landed on.
fn anchor_hit(points: &[egui::Pos2], pos: egui::Pos2) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .map(|(index, point)| (index, point.distance_sq(pos)))
        .filter(|(_, distance_sq)| {
            *distance_sq <= DRAWING_ANCHOR_RADIUS_PX * DRAWING_ANCHOR_RADIUS_PX
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

/// What a pane borrows from the window around it for one frame.
///
/// All of it is single-instance chrome: there is one toolbox, one preset
/// store, one appearance and one timezone however many panes are on screen —
/// and, per tab, one simulator, because one market holds one position.
/// The input pass takes this by `&mut` because placing a drawing re-arms the
/// tool; the draw pass takes it by `&`, which is what stops a paint from
/// arming anything.
pub struct PaneChrome<'a> {
    pub toolrail: &'a mut ToolRail,
    pub presets: &'a drawings::presets::PresetStore,
    /// Raised when a tool whose content is words was just placed, so the
    /// host puts the caret in the object it just made.
    ///
    /// Selecting a drawing raises the context bar, which is everything a
    /// note needs *except* somewhere to type. This is that somewhere, and it
    /// is on the chart: a note typed in a panel is read with the eye crossing
    /// the screen between keystrokes, and until the first word lands the
    /// object under the pointer just says "Note" in grey.
    pub begin_text_edit: &'a mut bool,
    pub style: &'a ChartStyle,
    pub tz: TzOffset,
    /// The symbol to name while the series is still empty.
    pub symbol: &'a str,
    /// The tab's paper-trading simulator. Both panes *draw* its lines — the
    /// same instrument at the same prices — while only one *handles* them.
    pub paper: &'a mut PaperTrading,
    /// Whether this pane is the one paper trading takes its pointer from.
    ///
    /// Whether this pane's pointer drives order entry this frame.
    ///
    /// True on the pane the pointer is *in* — every visible pane is a
    /// trading surface, and a level is as true on a context chart as on the
    /// flow chart, so holding the buy modifier over any of them aims there.
    /// While a paper line is being dragged it stays with the pane the drag
    /// started in: the grabbed price must not jump to a different scale
    /// halfway through the gesture.
    pub paper_takes_input: bool,
    /// Whether the position HUD anchors on this pane. Follows *focus*, not
    /// the pointer: there is one HUD, it must not flicker between panes as
    /// the hand crosses them, and focus is the app's existing answer to
    /// "which pane is the trader working in".
    pub paper_hud_here: bool,
    /// What the pointer grabs among the *other* pane's shared marks, and
    /// whether that mark is locked.
    ///
    /// Resolved by the tab before the panes are borrowed one at a time, since
    /// answering it needs both panes at once. `None` on an unsplit tab, and
    /// on any tab where nothing is shared.
    pub shared_pick: Option<SharedPick>,
    /// What this pane did to a shared mark, for the tab to apply to the pane
    /// that owns it.
    pub shared: SharedInteraction,
    /// Stretches of market time this tab's tape does not cover, left by a
    /// reconnect that kept the timeline (see [`quantick_feed::FeedGap`]).
    ///
    /// Passed per frame rather than held per pane: the holes belong to the
    /// tab's one tape, and every pane cuts its own bars from that same tape.
    /// A copy per pane would be the same list written twice, and two lists
    /// that can disagree about where the market went quiet is exactly the
    /// class of bug the honesty rule exists to prevent.
    pub feed_gaps: &'a [quantick_feed::FeedGap],
    /// What the running source can actually produce. The layer menu offers a
    /// layer this feed has no data for as disabled-with-a-reason rather than as
    /// a switch that would do nothing — the wording the toolbar already uses.
    pub capabilities: FeedCapabilities,
    /// Whether the running feed *infers* the aggressor side (MT5 tick rule,
    /// or a replay of such a session) instead of the venue reporting it. The
    /// footprint layer's content is entirely buyer-vs-seller, so it carries
    /// this label in its own legend — the status bar's note is not enough
    /// there (data honesty).
    pub side_inferred: bool,
    /// The window's footprint setup — the last one the trader used anywhere
    /// (env > `config/footprint.toml` > saved edits > defaults). A chart
    /// configured on its own overrides it; see
    /// [`ChartPane::footprint_config`].
    pub footprint: &'a crate::footprint_config::FootprintConfig,
    /// Where the layer menu leaves the two switches the pane does not own.
    /// Drained by the app once the canvas is done (see [`LayerActions`]).
    pub layers: &'a mut LayerActions,
}

/// Which side of the candles a drawing pass paints on.
///
/// One function serves both, taking this rather than being copied: the
/// projection, the band filter, the off-series fade and the style resolution
/// are the same work whichever side is being drawn, and a second copy of them
/// would drift on the first change to any of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawPass {
    /// Before the candles: the object's own body, for the few tools that are
    /// context rather than annotation. See [`DrawingToolImpl::paint_under`].
    UnderCandles,
    /// After the candles: everything else, plus halo, handles and badges.
    OverCandles,
}

/// One chart pane. See the module docs for what does and does not live here.
pub struct ChartPane {
    /// Namespaces this pane's egui interaction ids. Ids are the one piece of
    /// gesture state egui keeps on our behalf, so two panes sharing an id
    /// would share a drag.
    pub id: u64,
    pub state: ChartState,
    /// Identity of the closed-bar prefix used by append-only control-plane
    /// pagination.
    ///
    /// A live bar closing appends beyond an existing page's high-water mark
    /// and deliberately leaves this unchanged. Anything that can rewrite,
    /// prepend, remove, or re-cut a closed bar advances it, so a cursor can
    /// reject a mixed view instead of silently continuing over changed data.
    pagination_revision: u64,
    /// The tape, and everything read off it: the live lane, the heatmap, the
    /// bubbles, the live strip.
    ///
    /// `None` is what makes a time pane a time pane. §11 keeps the flow layers
    /// on the flow pane, and a pane that will never draw them has no business
    /// running a book worker thread to feed them.
    pub orderflow: Option<OrderflowView>,
    /// Background thread owning the `IndicatorHost`; the UI only sends
    /// commands and applies the delta events back.
    pub indicator_worker: IndicatorWorker,
    /// The UI's copy of every indicator's plot columns (see
    /// [`crate::indicators`]).
    pub indicators: IndicatorViews,
    /// Whether the app has put the active layout on this pane — its
    /// indicators and its market's drawings. `false` from construction until
    /// the first frame that sees the pane, so a pane opened by any path (a
    /// new tab, a split, a restore) is seeded exactly once.
    pub layout_seeded: bool,
    /// Which of the workspace's layouts this pane shows — its indicator set
    /// and the drawings it keeps. `None` until the app seeds the pane, when
    /// it takes the focused pane's layout (or the book's default); a restored
    /// workspace sets it before seeding. Per pane, because two charts side by
    /// side are two readings of one market, and a CVD on one is not a CVD the
    /// other asked for.
    pub layout: Option<crate::layouts::LayoutId>,
    /// The layout's name, for the pane to show beside its own controls. A
    /// copy the app refreshes on a switch or a rename, so the header — drawn
    /// by the tab, which has no book — never looks it up per frame.
    ///
    /// Drawn today only by a *context* pane, in the strip that carries its
    /// timeframe chips ([`crate::time_header`]). The flow pane has no such
    /// strip, so while a context pane holds the focus its layout is named
    /// nowhere on screen — the footer strip lights the focused pane's. The
    /// copy is kept on every pane rather than on the ones that draw it, so
    /// the flow pane's own readout is a draw call and not a second
    /// bookkeeping path.
    pub layout_label: String,
    /// Which market and pane address the drawings on this pane belong to,
    /// once the layout put them here. The app compares it with the tab's
    /// market every frame and swaps the set when they part.
    pub drawings_key: Option<crate::layouts::DrawingKey>,
    /// The drawings revision last copied into the layout; a different
    /// reading means the layout is behind this pane.
    pub drawings_saved_revision: u64,
    /// Whether this pane's on-chart indicator legend is folded to its count
    /// puck. Per pane, not per window: a split is two readings of the same
    /// market, and the corner pressure that makes a trader fold the flow
    /// pane's legend — bubbles, book, the position HUD — is not on the time
    /// pane at all. Expanded by default, which is what every chart did before
    /// the fold existed.
    pub legend_collapsed: bool,
    /// Whether the user wants the live strip shown. The pixels it actually
    /// gets are still capability-gated — see [`Self::live_strip_width`].
    pub live_strip_visible: bool,
    /// The candle footprint layer as this pane has it — see
    /// [`PaneFootprint`].
    pub footprint: PaneFootprint,

    /// Layers switched off that nothing else on this pane owns.
    ///
    /// The rest of the right-click menu resolves to the field that already owns
    /// its layer (see [`Self::layer_visible`]); only the chart's own marks —
    /// which had no switch before the menu existed — are held here, so the menu
    /// can never hold a second opinion about a pixel.
    pub hidden_layers: BTreeSet<ChartLayer>,
    /// Where each layer's switch landed in the last menu frame, so a test can
    /// click the real widget instead of calling the setter behind it.
    #[cfg(test)]
    pub layer_menu_rects: Vec<(ChartLayer, egui::Rect)>,

    /// The bar rule this pane is on, and the parameter every other kind is
    /// holding for the trader — see [`SpecSelector`].
    pub spec: SpecSelector,

    // Pan/zoom navigation over the bar series. It owns the history pane only:
    // the live lane is a band of screen to its right that answers to nothing
    // it does.
    pub viewport: Viewport,
    /// What the last draw measured, for the passes that are not the draw — see
    /// [`PaneFrame`].
    pub frame: PaneFrame,
    // The price-axis levels this frame's drawings declare. Per-frame by
    // nature, reused as a container for the same reason the band carve is —
    // and gathered before the axis labels itself, because the axis stands
    // aside where one of these is going to land.
    price_axis_levels: Vec<PriceAxisLevel>,
    // How many rungs the lane was wide enough for at the last draw, and so
    // how finely the next publish samples the forming bar across it. `0` when
    // there is no lane: the worker then walks no ladder and the panes draw
    // nothing on the tape, which is the whole cost of this feature on a chart
    // that has no tape to draw on.
    lane: LaneTransport,
    // Manual price-axis pan/zoom (auto-fit until the user drags vertically).
    pub price_view: PriceView,
    /// The price band's label, shared into every carve instead of cloned.
    price_band_label: std::sync::Arc<str>,
    // Pointer position over the plot this frame, for the crosshair.
    pub hover_pos: Option<egui::Pos2>,
    /// Whether the pointer is over the tape switch in the canvas's top-right
    /// corner. Read by the paint pass, which runs after the input pass and has
    /// no `Ui` of its own to ask.
    tape_switch_hovered: bool,

    /// Venue candles standing in front of the trade-derived series, already
    /// folded to this pane's interval.
    ///
    /// Deliberately outside `ChartState`: that rebuilds its bars from retained
    /// trades on every spec change, and a prefix living inside them would be
    /// eaten by the first chip click. Kept here, it is composed with the
    /// engine's bars at the points that read them — [`Self::slots`],
    /// [`Self::closed_bar`], the rebuild payload and the draw — and survives
    /// every rebuild the engine does.
    ///
    /// Non-empty on any pane cutting by a foldable time interval — the
    /// split's time pane, and the flow pane whenever its spec is
    /// `BarSpec::Time` (audit S1). A venue candle has no tape in it, so over
    /// the prefix the flow layers simply draw nothing.
    pub history_prefix: Vec<quantick_engine::Bar>,

    /// Where the position HUD anchors this frame: the chart rect and price
    /// scale, cached by the draw while the paper layer is painted on the
    /// pane that owns order entry. The HUD itself draws in `tab.rs`, where
    /// the paper host is mutably reachable outside the chrome's shared
    /// borrow.
    paper_hud_anchor: Option<(egui::Rect, PriceScale)>,
    /// What the right-click that opened the layer menu resolved — see
    /// [`PaneContextMenu`].
    pub context_menu: PaneContextMenu,
    /// The strategies armed on this pane's drawings — see
    /// [`PaneStrategies`].
    pub strategies: PaneStrategies,

    /// User drawings live entirely in the app overlay layer, never in market
    /// state, so chart/backtest/bot determinism stays untouched.
    pub drawings: Drawings,
    /// A drawing gesture in flight — see [`PaneGestures`].
    pub gestures: PaneGestures,
    /// A re-anchor owed to the drawings, holding the slot count of the series
    /// they were last anchored to.
    ///
    /// A reset empties the pane, and an empty series cannot say where an
    /// instant lands — so the answer is deferred to the first frame that has
    /// bars again, rather than clamping every mark onto a series that is not
    /// there yet.
    pending_reanchor: Option<usize>,
    /// The pane opened by a click on its own collapsed strip, carried to the
    /// frame that may hold the second half of a double click.
    ///
    /// A collapsed strip changes shape the instant it is clicked, so a gesture
    /// made *of* two clicks cannot be read from one frame's geometry. This is
    /// the one piece of state that spans them.
    strip_expanded: Option<SlotId>,
    /// An indicator whose settings a gesture on this pane asked for, waiting
    /// for the app to open the dialog.
    ///
    /// Parked rather than acted on: the dialog is the app's — one dialog for
    /// the whole window — and the gestures that ask for it are read deep inside
    /// this pane's input pass, holding borrows the app's state cannot cross.
    /// The same shape [`SpecSelector::pending`] uses for the other direction.
    pending_settings: Option<SlotId>,
}

impl ChartPane {
    /// The flow pane: quantick's own view of `symbol`, opening on bar `spec`,
    /// with the tape and every layer read off it.
    #[must_use]
    pub fn flow(id: u64, spec: BarSpec, symbol: String) -> Self {
        Self::new(id, spec, Some(OrderflowView::new(symbol)))
    }

    /// The time pane: the context view beside the flow pane (§11). Time bars
    /// of `interval_ms`, no tape and no flow layers.
    #[must_use]
    pub fn time(id: u64, interval_ms: i64) -> Self {
        Self::new(id, BarSpec::Time(interval_ms.max(1)), None)
    }

    /// `id` namespaces the pane's egui interaction ids and must be unique
    /// among the panes on screen.
    fn new(id: u64, spec: BarSpec, orderflow: Option<OrderflowView>) -> Self {
        // Defaults for every kind, with the initial spec's parameter applied.
        let selector = SpecSelector::new(spec.clone());

        Self {
            id,
            spec: selector,
            state: ChartState::new(spec),
            pagination_revision: 0,
            orderflow,
            indicator_worker: IndicatorWorker::spawn(),
            indicators: IndicatorViews::new(),
            layout_seeded: false,
            layout: None,
            layout_label: String::new(),
            drawings_key: None,
            drawings_saved_revision: 0,
            legend_collapsed: false,
            live_strip_visible: false,
            footprint: PaneFootprint::default(),
            // The backfill divider opens off: it is a full-height rule across
            // the candles for a boundary that matters once, when reading how
            // far the live tape goes back. Nothing is hidden about the data —
            // the mark is one click away in the layer menu, and the bars
            // either side of it are exactly what they were.
            hidden_layers: BTreeSet::from([ChartLayer::BackfillDivider]),
            #[cfg(test)]
            layer_menu_rects: Vec::new(),
            viewport: Viewport::new(),
            frame: PaneFrame::default(),
            price_axis_levels: Vec::new(),
            lane: LaneTransport::default(),
            price_view: PriceView::new(),
            price_band_label: std::sync::Arc::from(bands::PRICE_BAND_LABEL),
            hover_pos: None,
            tape_switch_hovered: false,
            history_prefix: Vec::new(),
            paper_hud_anchor: None,
            context_menu: PaneContextMenu::default(),
            strategies: PaneStrategies::default(),
            drawings: Drawings::default(),
            gestures: PaneGestures::default(),
            pending_reanchor: None,
            strip_expanded: None,
            pending_settings: None,
        }
    }

    /// Whether any placed or in-flight drawing is a fixed-range volume
    /// profile — the second consumer of the footprint ladders, keeping
    /// accumulation on while the layer itself is hidden. O(drawings), once
    /// per frame, never on the ingestion path.
    fn wants_range_profile(&self) -> bool {
        self.drawings
            .items()
            .iter()
            .map(|drawing| drawing.tool)
            .chain(self.drawings.draft().map(|draft| draft.tool))
            .any(|tool| tool.id() == crate::frvp::TOOL_ID)
    }

    /// The footprint setup this chart draws with: its own once configured
    /// here, else the window's last one. See [`PaneFootprint::config`].
    pub fn footprint_config<'a>(
        &'a self,
        window: &'a crate::footprint_config::FootprintConfig,
    ) -> &'a crate::footprint_config::FootprintConfig {
        self.footprint.config.as_ref().unwrap_or(window)
    }

    /// Put this pane on `spec` outright, selectors included.
    ///
    /// Startup-scoped: the caller is a workspace restoring the bar rule this
    /// pane was last read on, into a pane that has not drawn a frame yet. A
    /// live change goes through [`SpecSelector::pending`] instead, so the frame carrying
    /// it paints the loading overlay before the rebuild replays the tape —
    /// there is no tape to replay here, and nothing to paint over.
    ///
    /// The selectors move with the spec, because the BARS group reads *them*:
    /// setting the state alone would restore a chart whose own controls
    /// disagreed with it, and the trader's first touch of the parameter would
    /// snap the chart back to a rule they never chose.
    pub fn set_spec(&mut self, spec: BarSpec) {
        let changed = self.state.spec() != &spec;
        self.spec.set(spec.clone());
        self.state.set_spec(spec);
        if changed {
            self.bump_pagination_revision();
        }
    }

    /// Revision protecting the closed-bar prefix exposed through paginated
    /// chart-window reads. Live appends do not advance it; rewrites do.
    #[must_use]
    pub fn pagination_revision(&self) -> u64 {
        self.pagination_revision
    }

    fn bump_pagination_revision(&mut self) {
        self.pagination_revision = self.pagination_revision.saturating_add(1);
    }

    #[cfg(test)]
    /// Give this chart its own footprint setup, the way the settings window
    /// does when a knob moves on it.
    pub fn set_footprint_override(
        &mut self,
        config: Option<crate::footprint_config::FootprintConfig>,
    ) {
        self.footprint.config = config;
    }

    /// An egui interaction id scoped to this pane.
    fn interaction_id(&self, name: &'static str) -> egui::Id {
        egui::Id::new((name, self.id))
    }

    /// Width reserved for the live strip this frame. No capability gate any
    /// more: the aggression histogram runs on the trade stream, which every
    /// source provides (replay included), and without book data the strip
    /// honestly degrades to that histogram alone. A pane with no tape has no
    /// strip at all (§11).
    pub fn live_strip_width(&self, capabilities: FeedCapabilities) -> f32 {
        // Both halves of the strip come from the source: resting depth and the
        // aggressions landing into it. A source that produces neither fills
        // none of it, so the band would be an empty rect permanently narrowing
        // the candles — which is what the shipped default made reachable, the
        // layer having opened off until now. The claim that these pixels were
        // capability-gated predates the gate by some months; this is it.
        //
        // Passed in rather than cached on the pane for the reason
        // `layer_blocked` states: the running feed is resolved once per frame
        // by the caller, and a copy kept here would be one more thing to keep
        // in step when MetaTrader narrows its capabilities mid-session.
        let source_fills_it = capabilities.book_capture || capabilities.traded_volume;
        if self.live_strip_visible && self.orderflow.is_some() && source_fills_it {
            crate::live_strip::LIVE_STRIP_WIDTH_PX
        } else {
            0.0
        }
    }

    /// This pane's regions inside `area`, carved once so the input handler and
    /// the renderer can never disagree about a boundary.
    fn plot_areas(&self, area: egui::Rect, capabilities: FeedCapabilities) -> PlotAreas {
        let mut sizing = [PaneSizing::Auto; crate::indicators::MAX_PANES];
        plot_split(
            area,
            self.live_strip_width(capabilities),
            self.indicators.pane_sizing(&mut sizing),
        )
    }

    /// Re-anchor as soon as there are bars to anchor to, after a reset left
    /// the pane empty. Cheap enough to ask every frame: it is a flag test.
    /// The indicator a gesture on this pane asked to configure, if any, taken
    /// so a request is acted on exactly once.
    pub fn take_settings_request(&mut self) -> Option<SlotId> {
        self.pending_settings.take()
    }

    /// Stand in for the gesture that raises a settings request, so the app's
    /// side of the wiring can be tested without driving egui through a pane
    /// layout it would have to re-derive.
    #[cfg(test)]
    pub fn request_settings(&mut self, slot: SlotId) {
        self.pending_settings = Some(slot);
    }

    /// Handle mouse navigation, TradingView-style:
    /// - drag the candles → pan time (x, moves the whole chart) and price (y);
    /// - scroll over them → zoom time;
    /// - drag the bottom time strip left/right → zoom time (spread candles);
    /// - drag the right price gutter up/down → zoom the price scale;
    /// - scroll over either axis → zoom that axis;
    /// - double-click → reset to the live edge and auto-fit price.
    ///
    /// The live lane is a pane of its own and answers to none of it: a gesture
    /// that starts inside the tape moves nothing, and scrolling there zooms the
    /// tape's own window instead of the candles.
    pub fn handle_navigation(
        &mut self,
        ui: &egui::Ui,
        area: egui::Rect,
        chrome: &mut PaneChrome<'_>,
    ) {
        self.unhide_layer_for_armed_tool(chrome);
        // The magnet applies to every anchor gesture, placement and re-drag
        // alike: a handle that snaps only while you first draw it would make
        // the second edit undo the precision of the first.
        let magnet = chrome.toolrail.magnet();
        // Remembered for inspector placement and manager centring: the pane
        // where drawings live, already free of both axes and the live lane.
        self.frame.plot_area = Some(area);
        let areas = self.plot_areas(area, chrome.capabilities);
        self.frame.chart_area = Some(areas.chart);
        // One carve, consumed by placement, hit-testing, dragging and — after
        // the panes have drawn — painting.
        let bands = self.bands(&areas);
        // A drawing tool consumes the *primary button*, not the chart. Pan,
        // wheel zoom, the pane dividers and the collapse chevrons all keep
        // working while one is armed: an armed tool used to return early from
        // here, which left the trader unable to move the chart they were
        // annotating (audit S2).
        let tool_armed = self.handle_drawing_placement(ui, &areas, &bands, chrome);
        let auto = self.frame.auto_range;
        let height = self.frame.chart_height;
        let total = self.slots();
        let divider = self.frame.lane_divider_x;
        // Only the divider's own handle is off limits to the pan, not the whole
        // band: the resize gesture and the pan must never both fire on one
        // pixel, which is what `gesture_hits_lane` was written for — but
        // spending a third of the canvas to protect a ten-pixel handle is a
        // dead zone, not a guard.
        let on_divider = |position: egui::Pos2| {
            plot_area::gesture_hits_lane_divider(divider, position.x, LANE_HANDLE_HALF_WIDTH_PX)
        };

        // Chart body: drag pans both axes; scroll zooms time.
        let chart = ui.interact(
            areas.chart,
            self.interaction_id("chart_nav"),
            egui::Sense::click_and_drag(),
        );
        // While a tool is armed the placement surface owns the hover: it spans
        // every band, and the candles' own response would report `None` for a
        // pointer one pane below them.
        if !tool_armed {
            self.hover_pos = chart.hover_pos();
        }
        // The tape switch, in the canvas's top-right corner. Registered after
        // the chart body so the chip is on top of it — the lane divider's and
        // the jump-to-live chip's rule — and it is the *only* way back once the
        // tape is off: with no band there is no tape to right-click, so a
        // switch that lived only in that menu would be a one-way door.
        self.handle_tape_switch(ui, areas.chart, chrome);
        // The paper lines and the right-click price live on the candles, and
        // only there: an order is a price, not a value on someone's oscillator.
        let price_band = &bands[0];
        self.handle_context_menu(&chart, &areas, &bands, chrome);
        let history_right = self.frame.lane_divider_x.unwrap_or(areas.chart.right());
        let drawing_area = price_band.rect;
        let (primary_pressed, primary_down, primary_released, pointer_position, pointer_delta) = ui
            .input(|input| {
                (
                    input.pointer.primary_pressed(),
                    input.pointer.primary_down(),
                    input.pointer.primary_released(),
                    input.pointer.latest_pos(),
                    input.pointer.delta(),
                )
            });
        // Floating chrome (inspector, manager, toast, flyouts) is opaque to
        // the pointer: while it sits under the cursor the chart neither sets
        // a cursor nor selects nor starts a drag. The gate applies at press
        // time only — a drag that started on the canvas keeps running while
        // the pointer travels across a panel (continuity, not priority).
        let over_chrome = pointer_position
            .and_then(|position| ui.ctx().layer_id_at(position))
            .is_some_and(|layer| layer != ui.layer_id());
        let pointer = SharedPointer {
            position: pointer_position,
            area: drawing_area,
            over_chrome,
            pressed: primary_pressed,
            down: primary_down,
            released: primary_released,
            history_right,
            total,
            magnet,
        };
        let paper_gesture =
            self.handle_paper_input(ui, chrome, &areas, &bands, &pointer, tool_armed);
        let drawing_drag_consumes_gesture = self.handle_pointer_tool(
            ui,
            chrome,
            &chart,
            &areas,
            &bands,
            &pointer,
            pointer_delta,
            paper_gesture,
        );
        // Whether the primary button is still the chart's this frame. An
        // armed tool, a drawing being dragged and a grabbed paper line each
        // take it — and only it. Everything that is not the primary button
        // keeps answering throughout: the wheel over the candles and over
        // every pane, both axis gutters, the time strip, the lane and pane
        // dividers, the collapse chevrons, and the middle-button pan added
        // below. A primary *drag* with a tool armed is that tool's second
        // anchor, so it cannot also pan — which is exactly why the middle
        // button does.
        let primary_free = !tool_armed && !drawing_drag_consumes_gesture && !paper_gesture;
        // Anywhere on the canvas, tape band included — the same call the wheel
        // already answers this way, and for the same reason. The lane used to
        // swallow the drag while the pointer was over it: a third of the canvas
        // where pressing and pulling did nothing at all, with nothing on screen
        // saying why. That was survivable while a lane only existed on a feed
        // with a book; now that the tape is anchored on prints it appears on
        // every feed, and the dead zone became the first thing a trader hits.
        //
        // The lane keeps the gestures that are unambiguously its own: the
        // divider resizes it, and its own time strip sets its window. A drag
        // across the band is not one of them — the tape does not pan, it is
        // pinned to the live edge, so a drag there had no second meaning to
        // protect.
        let grabbing_divider = chart.interact_pointer_pos().is_some_and(&on_divider);
        if total > 0 && chart.dragged() && !grabbing_divider && primary_free {
            let drag = chart.drag_delta();
            self.viewport.pan_pixels(drag.x, total);
            if let Some(auto) = auto
                && drag.y != 0.0
                && height > 1.0
            {
                let (lo, hi) = self.price_view.resolve(auto);
                let price_per_px = (hi - lo) / f64::from(height);
                self.price_view
                    .pan_screen(f64::from(drag.y), price_per_px, auto);
            }
        }
        // Not while a tool is armed: two placement clicks in a row are two
        // anchors, never a request to jump back to the live edge.
        if chart.double_clicked() && primary_free {
            // On an overlay's own line the gesture means that line: a trader
            // pointing at a curve and double clicking is asking about the
            // curve, not about the viewport. Everywhere else on the canvas it
            // still snaps back to the live edge, which is the only reading a
            // double click on empty chart can have.
            match chart
                .interact_pointer_pos()
                .and_then(|pos| self.overlay_plot_at(pos))
            {
                Some(slot) => self.pending_settings = Some(slot),
                None => {
                    self.viewport.snap_to_live();
                    self.price_view.reset();
                }
            }
        }
        // One wheel, one meaning at a time. While an aim is up the ruler has
        // already spent this frame's travel walking a bracket out, and the
        // same roll must not also rescale the plot under it. The guard is
        // here as well as on the stacked panes' accumulated gesture, because
        // this is the canvas the aim actually lives on.
        if chart.hovered() && !chrome.paper.consumed_scroll() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                // Scroll up (positive) zooms in — the candles, wherever on the
                // canvas the pointer rests, tape band included.
                //
                // The lane used to steal this wheel while the pointer was over
                // it: one gesture, two meanings, and nothing on screen saying
                // which one you were about to get, so crossing a hairline
                // divider mid-scroll zoomed the tape instead of the chart. The
                // lane's window still zooms — from the lane's own time strip
                // below it (drag or scroll), which is the grammar every other
                // axis here already follows: an axis zooms its axis, the canvas
                // zooms the chart.
                self.viewport.zoom(2.0_f32.powf(scroll / SCROLL_ZOOM_PX));
            }
        }
        // The middle button pans, always — including mid-placement, which is
        // the whole reason it exists. A drag with a tool armed is that tool's
        // second anchor, so the primary button genuinely cannot pan then; a
        // trader who drops one end of a trend line and finds the other end
        // off screen would otherwise have to cancel the object to go look for
        // it. Same axes, same feel, a button the tools never take.
        if total > 0 {
            let (middle_down, delta) =
                ui.input(|input| (input.pointer.middle_down(), input.pointer.delta()));
            if middle_down
                && chart
                    .hover_pos()
                    .is_some_and(|position| areas.chart.contains(position) && !on_divider(position))
            {
                self.viewport.pan_pixels(delta.x, total);
                if let Some(auto) = auto
                    && delta.y != 0.0
                    && height > 1.0
                {
                    let (lo, hi) = self.price_view.resolve(auto);
                    let price_per_px = (hi - lo) / f64::from(height);
                    self.price_view
                        .pan_screen(f64::from(delta.y), price_per_px, auto);
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
        }

        self.handle_axis_gestures(ui, &areas, chrome);

        self.handle_indicator_pane_gestures(ui, &areas, chrome, primary_free);
    }

    /// The HUD anchor cached by the last draw, if the paper layer was
    /// painted on the pane that owns order entry.
    #[must_use]
    pub fn paper_hud_anchor(&self) -> Option<(egui::Rect, PriceScale)> {
        self.paper_hud_anchor
    }
}

/// Chip metrics for the placement hint: it rides beside the cursor without
/// sitting under it, and it is the same 10 px plate the ruler readout uses.
const HINT_CURSOR_OFFSET_PX: egui::Vec2 = egui::vec2(14.0, 14.0);
const HINT_TEXT_PX: f32 = 10.0;
const HINT_PAD_X_PX: f32 = 5.0;
const HINT_PAD_Y_PX: f32 = 3.0;
const HINT_RADIUS_PX: f32 = 3.0;
const HINT_PLATE: egui::Color32 = egui::Color32::from_rgba_premultiplied(14, 18, 26, 216);

/// Tell the trader what the next click does, beside the cursor.
///
/// A tool that knows says so in words (`placement_hint`); one that does not
/// still reports its progress, because "2/3" beats an object that appears to
/// have stopped responding. Nothing is drawn once the last anchor is placed —
/// there is no next click to describe.
fn paint_placement_hint(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    cursor: egui::Pos2,
    tool: drawings::DrawingTool,
    placed: usize,
) {
    let required = tool.required_points();
    if required < 2 || placed == 0 || placed >= required {
        return;
    }
    let text = tool
        .placement_hint(placed)
        .map_or_else(|| format!("{placed}/{required}"), str::to_owned);
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(HINT_TEXT_PX),
        theme::TEXT_PRIMARY,
    );
    let size = galley.size() + egui::vec2(2.0 * HINT_PAD_X_PX, 2.0 * HINT_PAD_Y_PX);
    // Flip to the other side of the cursor rather than let the chip leave the
    // chart: a hint half off-screen is worse than no hint.
    let mut min = cursor + HINT_CURSOR_OFFSET_PX;
    if min.x + size.x > chart_rect.right() {
        min.x = cursor.x - HINT_CURSOR_OFFSET_PX.x - size.x;
    }
    if min.y + size.y > chart_rect.bottom() {
        min.y = cursor.y - HINT_CURSOR_OFFSET_PX.y - size.y;
    }
    let plate = egui::Rect::from_min_size(min, size);
    painter.rect_filled(plate, egui::Rounding::same(HINT_RADIUS_PX), HINT_PLATE);
    painter.galley(
        plate.min + egui::vec2(HINT_PAD_X_PX, HINT_PAD_Y_PX),
        galley,
        theme::TEXT_PRIMARY,
    );
}

/// The open / high / low / close of `candle` nearest to the pointer on
/// screen, when one is within reach.
///
/// This is the difference between a line that *looks* drawn off the swing
/// high and one that is (`docs/ux/drawing-tools-2026-08.md` §D6). Nothing in
/// reach returns `None` and the free price is used — a magnet that always
/// snaps is a magnet you cannot draw a diagonal with.
/// Clamp a fractional bar coordinate onto the bars that exist:
/// `0 ..= total - 1`. The candle magnet's time half — a snap that reads a
/// candle must stand on one.
fn snap_bar_to_tape(bar: f32, total: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    bar.clamp(0.0, total.saturating_sub(1) as f32)
}

fn magnet_price_of(
    candle: &quantick_engine::Bar,
    pointer_y: f32,
    scale: &PriceScale,
    reach_px: f32,
) -> Option<f64> {
    [candle.open, candle.high, candle.low, candle.close]
        .into_iter()
        .filter_map(|price| {
            let price = price.to_f64()?;
            let distance = (scale.y(price) - pointer_y).abs();
            (distance <= reach_px).then_some((distance, price))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, price)| price)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "pane/tests/lane_transport_tests.rs"]
mod lane_transport_tests;
