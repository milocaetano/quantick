//! The book the approved WIN tape paints under its prints, pinned like them.
//!
//! The marks golden replays a recording that carries no book: version 1 of
//! the replay format has no depth, so the tape it draws has none either. The
//! live WIN tape does, from the MT5 bridge's DOM, and that book is part of
//! the approved picture. So the same 3,267 real prints are replayed here with
//! a book beside them, and what the tape paints of that book is pinned: every
//! lane cell's rectangle and fill, and every reduction front the tape draws.
//!
//! The book is synthetic and labelled so, because no WIN depth was ever
//! recorded: an MT5-shaped DOM of [`BOOK_LEVELS`] levels a side on WIN's
//! five-point grid around the newest print, read every frame, stamped
//! [`BOOK_LAG_MS`] behind the tape clock as the bridge's stamps trail it, and
//! turned into a snapshot and absolute deltas by the same differ the bridge
//! uses. Its sizes are a fixed function of price and time, so the book moves
//! the way a real one does — levels shift with the price and resize on their
//! own — and the replay stays deterministic.
//!
//! The painter's half is mirrored from the app (`orderflow_view/frame.rs`
//! `draw_background`, `orderflow_render/heatmap.rs` and `layout.rs`); every
//! decision it reads is the engine's, the pane tape's or the style's. A change
//! to that glue has to be mirrored here, as in the parent module.
//!
//! The goldens are what main ef882174 painted through this harness, its glue
//! mirrored as it stood there: no native inset on the lane's book and no
//! lane-only opening in `band`, which paint the same as here while the
//! candles follow the live edge.
//!
//! When this fails, the depth the WIN tape paints changed. The same rule as
//! the marks golden applies: do not edit the golden to match.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use quantick_chart::viewport::Viewport;
use quantick_engine::fixture::parse_trades;
use quantick_engine::{Side, Trade};
use quantick_orderbook::{BookCoverage, BookLevel, DepthEvent, ImageOutcome, SnapshotDiffer};
use quantick_orderflow::config::theme::{OrderflowRenderStyle, heat_fill_parts};
use quantick_orderflow::engine::VisibleOrderflow;
use quantick_orderflow::projection::TapeHorizontalGeometry;
use quantick_orderflow::{HeatmapConfig, LiquidityEvidence};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::{
    BARS, CHART_HEIGHT_PX, FRAME_STEP_MS, INSTANTS_MS, LANE_WIDTH_PX, Mode, Pane, Path,
    SLICE_END_MS, SLICE_START_MS, TRADES, local_time,
};

/// What the depth golden holds for the WIN tape beside the candles.
const GOLDEN_BESIDE: &str =
    include_str!("../fixtures/win_2026_09_22_approved_depth.native_tape.golden.txt");
/// What it holds for the tape alone.
const GOLDEN_TAPE_ONLY: &str =
    include_str!("../fixtures/win_2026_09_22_approved_depth.tape_only.golden.txt");

/// The book's instrument, as the engine is told it.
const SYMBOL: &str = "WINV26";
/// WIN's price increment: the grid its book rests on.
const BOOK_TICK: i64 = 5;
/// Levels a side the synthetic DOM shows.
const BOOK_LEVELS: i64 = 8;
/// How far the book's stamp trails the tape clock. The trader's WIN frames
/// measured 17-152 ms between the newest print and the book's confirmation.
const BOOK_LAG_MS: i64 = 100;
/// How long a level keeps one size before it moves on its own.
const BOOK_SIZE_HOLD_MS: i64 = 2_500;
/// The smallest size a synthetic level rests with.
const BOOK_SIZE_FLOOR: i64 = 10;
/// The spread of sizes above the floor, a prime so neighbours differ.
const BOOK_SIZE_SPREAD: i64 = 53;
/// The capture generation the book is published under.
const BOOK_GENERATION: u64 = 1;
/// The canvas the app paints on, as the parent's marks are drawn.
const BACKGROUND: [u8; 4] = [0, 0, 0, 255];

/// An MT5-shaped DOM, read every frame and differed into depth events.
struct Book {
    differ: SnapshotDiffer,
}

impl Book {
    fn new() -> Self {
        Self {
            differ: SnapshotDiffer::new(BookCoverage::Limited {
                levels_per_side: BOOK_LEVELS as usize,
            }),
        }
    }

    /// The DOM at `at_ms`, around `last`, the newest print by then: the
    /// print's own price is the best offer it lifted or the best bid it hit.
    fn image(last: &Trade, at_ms: i64) -> (Vec<BookLevel>, Vec<BookLevel>) {
        let price = last.price.to_i64().expect("WIN prints on whole points");
        let best_ask = match last.side {
            Side::Buy => price,
            Side::Sell => price + BOOK_TICK,
        };
        let best_bid = best_ask - BOOK_TICK;
        let hold = at_ms.div_euclid(BOOK_SIZE_HOLD_MS);
        let level = |price: i64| {
            let size =
                BOOK_SIZE_FLOOR + (price / BOOK_TICK * 37 + hold * 11).rem_euclid(BOOK_SIZE_SPREAD);
            BookLevel::new(Decimal::from(price), Decimal::from(size)).expect("a positive level")
        };
        let bids = (0..BOOK_LEVELS)
            .map(|k| level(best_bid - k * BOOK_TICK))
            .collect();
        let asks = (0..BOOK_LEVELS)
            .map(|k| level(best_ask + k * BOOK_TICK))
            .collect();
        (bids, asks)
    }

    /// The depth event one read of the DOM publishes, if it changed.
    fn read(&mut self, last: &Trade, at_ms: i64) -> Option<DepthEvent> {
        let (bids, asks) = Self::image(last, at_ms);
        match self.differ.observe(&bids, &asks) {
            ImageOutcome::Snapshot(snapshot) => Some(DepthEvent::Snapshot {
                symbol: SYMBOL.to_owned(),
                generation: BOOK_GENERATION,
                observed_at_ms: at_ms,
                effective_at_ms: at_ms,
                price_step: Some(Decimal::from(BOOK_TICK)),
                snapshot,
            }),
            ImageOutcome::Delta(delta) => Some(DepthEvent::Update {
                symbol: SYMBOL.to_owned(),
                generation: BOOK_GENERATION,
                event_time_ms: at_ms,
                delta,
            }),
            ImageOutcome::Unchanged => None,
            ImageOutcome::Crossed { .. } => panic!("the synthetic book never crosses"),
        }
    }
}

/// The pane's canvas at one painted frame, as the background pass sees it.
pub(super) struct Canvas<'a> {
    pub(super) chart_width: f32,
    pub(super) chart_height: f32,
    pub(super) lane_width: f32,
    pub(super) viewport: &'a Viewport,
    pub(super) total_bars: usize,
}

/// Writes the tape's depth at the frames the replay asks for.
#[derive(Default)]
pub(super) struct Recorder {
    wanted: bool,
    drawn: Option<String>,
}

impl Recorder {
    /// The background pass for `frame`, written when this frame is wanted.
    pub(super) fn observe(
        &mut self,
        frame: &VisibleOrderflow,
        config: &HeatmapConfig,
        canvas: Canvas<'_>,
    ) {
        if self.wanted {
            self.drawn = Some(paint(frame, config, &canvas));
        }
    }
}

/// `egui::Rect`'s arithmetic, which the painter's glue is written in.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Rect {
    const NOTHING: Self = Self {
        left: f32::INFINITY,
        top: f32::INFINITY,
        right: f32::NEG_INFINITY,
        bottom: f32::NEG_INFINITY,
    };

    fn width(self) -> f32 {
        self.right - self.left
    }

    fn height(self) -> f32 {
        self.bottom - self.top
    }

    fn is_positive(self) -> bool {
        self.width() > 0.0 && self.height() > 0.0
    }

    fn is_finite(self) -> bool {
        [self.left, self.top, self.right, self.bottom]
            .iter()
            .all(|value| value.is_finite())
    }

    fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    fn with_max_x(self, right: f32) -> Self {
        Self { right, ..self }
    }
}

/// `finite_unit_f64`, as the layout clamps a normalized coordinate.
fn finite_unit(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// `layout.rs`'s `ProjectedLayout`, for a live pane that is not inverted.
#[derive(Clone, Copy)]
struct Layout<'a> {
    chart: Rect,
    viewport: &'a Viewport,
    total_bars: usize,
    first_bar_index: usize,
    slot_count: usize,
    lane_width_px: f32,
    lane_only: bool,
    tape_geometry: Option<TapeHorizontalGeometry>,
}

impl Layout<'_> {
    fn with_lane_only(mut self) -> Self {
        self.lane_only = true;
        self
    }

    fn x(self, normalized: f64) -> f32 {
        let normalized = finite_unit(normalized) as f32;
        let regions = self.slot_count as f32;
        let region_pos = normalized * regions;
        let boundary = regions - 1.0;
        match self.lane_left_x() {
            Some(divider) if regions >= 1.0 && (self.lane_only || region_pos > boundary) => {
                divider
                    + self.tape_geometry.map_or_else(
                        || (region_pos - boundary).max(0.0) * self.lane_width_px,
                        |geometry| geometry.x(f64::from((region_pos - boundary).max(0.0))),
                    )
            }
            _ => self.x_at_ext(region_pos),
        }
    }

    fn x_at_ext(self, ext_pos: f32) -> f32 {
        let position = self.first_bar_index as f32 - 0.5 + ext_pos;
        self.viewport
            .x_at_bar_position(position, self.history_right(), self.total_bars)
    }

    fn lane_left_x(self) -> Option<f32> {
        let width = self.lane_width_px;
        (width.is_finite() && width > 0.0 && width <= self.chart.width())
            .then_some(self.chart.right - width)
    }

    fn history_right(self) -> f32 {
        self.lane_left_x().unwrap_or(self.chart.right)
    }

    fn history_rect(self) -> Rect {
        self.chart.with_max_x(self.history_right())
    }

    fn lane_rect(self) -> Rect {
        Rect {
            left: self.history_right(),
            ..self.chart
        }
    }

    fn in_lane(self, normalized: f64) -> bool {
        let regions = self.slot_count as f64;
        self.lane_left_x().is_some()
            && self.slot_count >= 1
            && (self.lane_only || finite_unit(normalized) > (regions - 1.0) / regions)
    }

    fn pane(self, normalized: f64) -> Rect {
        if self.lane_left_x().is_none() {
            return self.chart;
        }
        if self.in_lane(normalized) {
            self.lane_rect()
        } else {
            self.history_rect()
        }
    }

    fn span_pane(self, x0: f64, x1: f64) -> Rect {
        let low = self.pane(x0.min(x1));
        let high = self.pane(x0.max(x1));
        if low == high {
            return low;
        }
        if self.x(x0.min(x1)) >= self.history_right() {
            return self.lane_rect();
        }
        self.chart
    }

    fn layer_clip(self, chart: bool, lane: bool) -> Option<Rect> {
        match (chart, lane) {
            (true, true) => Some(self.chart),
            (true, false) => Some(if self.lane_left_x().is_some() {
                self.history_rect()
            } else {
                self.chart
            }),
            (false, true) => self.lane_left_x().is_some().then(|| self.lane_rect()),
            (false, false) => None,
        }
    }

    fn y(self, normalized: f64) -> f32 {
        let unit = finite_unit(normalized) as f32;
        self.chart.top + unit * self.chart.height()
    }

    fn band(self, x0: f64, x1: f64, y0: f64, y1: f64, min_height: f32) -> Rect {
        let lane_start = 1.0 - 1.0 / self.slot_count.max(1) as f64;
        let time = if self.lane_left_x().is_some() && x0 >= lane_start && x1 > lane_start {
            self.with_lane_only()
        } else {
            self
        };
        let (left, right) = (time.x(x0), time.x(x1));
        let (top, bottom) = (self.y(y0), self.y(y1));
        readable_band(
            Rect {
                left: left.min(right),
                top: top.min(bottom),
                right: left.max(right),
                bottom: top.max(bottom),
            },
            min_height,
            self.span_pane(x0, x1),
        )
    }

    /// `(x, top, bottom)` of a reduction's front.
    fn event_band(self, x: f64, y0: f64, y1: f64, min_height: f32) -> (f32, f32, f32) {
        let row = self.band(x, x, y0, y1, min_height);
        let lane_start = 1.0 - 1.0 / self.slot_count.max(1) as f64;
        let front = if self.tape_geometry.is_some() && x >= lane_start {
            self.with_lane_only().x(x)
        } else {
            self.x(x)
        };
        (front, row.top, row.bottom)
    }
}

fn readable_band(rect: Rect, minimum_height: f32, clip: Rect) -> Rect {
    if !rect.is_finite() {
        return Rect::NOTHING;
    }
    let minimum_height = minimum_height.max(0.5);
    let readable = if rect.height() < minimum_height {
        let (center_x, center_y) = (
            (rect.left + rect.right) / 2.0,
            (rect.top + rect.bottom) / 2.0,
        );
        let half_width = rect.width().max(0.5) / 2.0;
        Rect {
            left: center_x - half_width,
            top: center_y - minimum_height / 2.0,
            right: center_x + half_width,
            bottom: center_y + minimum_height / 2.0,
        }
    } else {
        rect
    };
    readable.intersect(clip)
}

/// `draw_background` for a live pane: the depth map, then the reductions,
/// written for every cell and front on the tape, relative to its divider.
fn paint(frame: &VisibleOrderflow, config: &HeatmapConfig, canvas: &Canvas<'_>) -> String {
    let mut out = String::new();
    let style = OrderflowRenderStyle::from_config(config, BACKGROUND);
    let lane_width_px = if canvas.lane_width.is_finite() {
        canvas.lane_width.max(0.0)
    } else {
        0.0
    };
    let chart = Rect {
        left: 0.0,
        top: 0.0,
        right: canvas.chart_width,
        bottom: canvas.chart_height,
    };
    // `RenderContext::new`.
    let layout = Layout {
        chart,
        viewport: canvas.viewport,
        total_bars: canvas.total_bars,
        first_bar_index: frame.first_bar_index,
        slot_count: frame.slot_count,
        lane_width_px,
        lane_only: false,
        tape_geometry: TapeHorizontalGeometry::from_style(&style, lane_width_px, chart.height()),
    };
    let Some(divider) = layout.lane_left_x() else {
        out.push_str("no tape\n");
        return out;
    };
    let projection = &frame.projection;
    let lane_start = 1.0 - 1.0 / frame.slot_count.max(1) as f64;

    // `draw_heatmap_background`.
    let style = style.sanitized();
    assert!(
        !style.live_lane.depth_time_averaging,
        "the approved WIN tape paints its book exact, never averaged"
    );
    let Some(region) = layout.layer_clip(style.depth_layer, style.lane_depth_layer) else {
        out.push_str("depth off\n");
        return out;
    };
    // The live pane's own clip, `chart_rect.with_max_x(right)`.
    let clip = region.intersect(chart);
    let mut cells = Vec::new();
    for cell in projection.cells.iter().filter(|cell| cell.x1 > lane_start) {
        let rect = layout.band(cell.x0, cell.x1, cell.y0, cell.y1, style.min_cell_height);
        if !rect.is_positive() && !cell.current_book {
            continue;
        }
        let Some((rgb, alpha)) = heat_fill_parts(&style, cell.side, cell.intensity, cell.alpha)
        else {
            continue;
        };
        assert!(
            style.edge_glow <= 0.0,
            "the WIN look paints no edge glow; mirror it before relying on this"
        );
        let painted = rect.intersect(clip);
        if !painted.is_positive() {
            continue;
        }
        cells.push(format!(
            "{} price={} qty={} x={:.3}..{:.3} y={:.3}..{:.3} rgb={},{},{} alpha={:.4}",
            match cell.side {
                quantick_orderflow::RestingSide::Bid => "bid",
                quantick_orderflow::RestingSide::Ask => "ask",
            },
            cell.price_bucket.normalize(),
            cell.quantity.normalize(),
            painted.left - divider,
            painted.right - divider,
            painted.top,
            painted.bottom,
            rgb[0],
            rgb[1],
            rgb[2],
            alpha
        ));
    }

    // `draw_liquidity_events`: where each front the tape draws stands.
    let mut fronts = Vec::new();
    for event in &projection.liquidity_events {
        if !layout.in_lane(event.x) || !style.lane_depth_layer {
            continue;
        }
        let shown = match event.evidence {
            LiquidityEvidence::AggressionAligned => style.show_aligned,
            LiquidityEvidence::DepthOnly => style.show_unattributed,
        };
        if !shown {
            continue;
        }
        let (x, top, bottom) =
            layout.event_band(event.x, event.y0, event.y1, style.min_cell_height);
        if x < chart.left - 1.0 || x > chart.right + 1.0 {
            continue;
        }
        fronts.push(format!(
            "{} {} price={} removed={} full={} x={:.3} y={:.3}..{:.3}",
            match event.evidence {
                LiquidityEvidence::AggressionAligned => "aligned",
                LiquidityEvidence::DepthOnly => "unattributed",
            },
            match event.side {
                quantick_orderflow::RestingSide::Bid => "bid",
                quantick_orderflow::RestingSide::Ask => "ask",
            },
            event.price_bucket.normalize(),
            event.removed.normalize(),
            event.full_removal,
            x - divider,
            top,
            bottom
        ));
    }

    let _ = writeln!(out, "cells {}", cells.len());
    for (index, cell) in cells.iter().enumerate() {
        let _ = writeln!(out, "{} {cell}", index + 1);
    }
    let _ = writeln!(out, "fronts {}", fronts.len());
    for (index, front) in fronts.iter().enumerate() {
        let _ = writeln!(out, "{} {front}", index + 1);
    }
    out
}

/// Replay the fixture with the synthetic book beside it, painting through the
/// app's path, and write the tape's depth at each instant.
fn replay(mode: Mode) -> String {
    let trades = parse_trades(TRADES).expect("the fixture is an engine trade file");
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# The depth the approved WIN native tape paints. A test compares every byte:\n\
         # read crates/stores/tests/approved_win_tape/depth.rs before changing anything here.\n\
         fixture win_2026_09_22_approved_tape.csv trades={} with a synthetic {BOOK_LEVELS}-level book \
         every {FRAME_STEP_MS} ms, stamped {BOOK_LAG_MS} ms behind the tape\n\
         preset \"mini index regions\" bars={BARS} lane={LANE_WIDTH_PX}x{CHART_HEIGHT_PX}px mode={}",
        trades.len(),
        mode.label()
    );
    let mut pane = Pane::new(mode, Path::Overlay);
    pane.view.set_enabled(true, BOOK_GENERATION);
    let mut book = Book::new();
    let started = Instant::now();
    let mut next = 0;
    let mut position_ms = SLICE_START_MS;
    while position_ms <= SLICE_END_MS {
        while next < trades.len() && trades[next].timestamp_ms <= position_ms {
            pane.ingest(&trades[next]);
            next += 1;
        }
        let read_ms = position_ms - BOOK_LAG_MS;
        if let Some(last) = trades[..next]
            .iter()
            .rev()
            .find(|trade| trade.timestamp_ms <= read_ms)
            && let Some(event) = book.read(last, read_ms)
        {
            pane.view.handle_depth_event_at(event, position_ms);
        }
        let elapsed_ms = position_ms - SLICE_START_MS;
        let instant = INSTANTS_MS.iter().position(|at| *at == elapsed_ms);
        pane.depth = Recorder {
            wanted: instant.is_some(),
            drawn: None,
        };
        let next_ms = trades.get(next).map(|trade| trade.timestamp_ms);
        pane.frame(position_ms, next_ms, None);
        if let Some(index) = instant {
            let _ = writeln!(
                out,
                "\ninstant {} at {}",
                index + 1,
                local_time(position_ms)
            );
            out.push_str(pane.depth.drawn.as_deref().unwrap_or("no depth drawn\n"));
        }
        pane.view
            .worker
            .run_batch(started + Duration::from_millis(elapsed_ms as u64));
        position_ms += FRAME_STEP_MS;
    }
    out
}

fn assert_approved(mode: Mode) {
    let actual = replay(mode);
    let (golden, name) = match mode {
        Mode::Beside => (GOLDEN_BESIDE, "native_tape"),
        Mode::TapeOnly => (GOLDEN_TAPE_ONLY, "tape_only"),
    };
    if actual == golden {
        return;
    }
    let written = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("win_2026_09_22_approved_depth.{name}.golden.txt"));
    let saved = std::fs::write(&written, &actual).map_or_else(
        |error| format!("could not be written ({error})"),
        |()| format!("is in {}", written.display()),
    );
    let (line, approved, now) = golden
        .lines()
        .map(Some)
        .chain(std::iter::repeat(None))
        .zip(actual.lines().map(Some).chain(std::iter::repeat(None)))
        .take(golden.lines().count().max(actual.lines().count()) + 1)
        .enumerate()
        .find(|(_, (approved, now))| approved != now)
        .map_or((0, None, None), |(index, (approved, now))| {
            (index + 1, approved, now)
        });
    panic!(
        "The depth the approved WIN tape paints changed ({}). The trader approved this tape, \
         and it must not change without the trader's approval.\n\
         First difference, line {line} of the golden:\n  approved: {}\n  now:      {}\n\
         The whole new depth {saved}. Do not edit the golden to match: fix the change, \
         or show the trader both and replace the golden only once the trader approves.",
        mode.label(),
        approved.unwrap_or("<end of file>"),
        now.unwrap_or("<end of file>"),
    );
}

#[test]
fn the_native_tape_beside_the_candles_paints_the_approved_win_depth() {
    assert_approved(Mode::Beside);
}

#[test]
fn tape_only_paints_the_approved_win_depth() {
    assert_approved(Mode::TapeOnly);
}
