//! The approved WIN native tape, pinned at live.
//!
//! A fixed WIN-like session — prints on a five-point grid, a book changing at
//! the same instants, tick candles of five prints — is driven through the
//! pane's own calls frame by frame on the supplied live clock, and every shape
//! the tape paints is compared with the output recorded before the book was
//! moved onto the tape's clock. Only the book may change around the tape.
use super::*;
use quantick_engine::Side;
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use quantick_orderflow::engine::VisibleOrderflow;
use quantick_orderflow::projection::PaneGeometry;
use std::sync::Arc;

pub(super) const CHART_PX: f32 = 1_600.0;
pub(super) const HEIGHT_PX: f32 = 400.0;
pub(super) const PRICES: (f64, f64) = (186_900.0, 187_300.0);
pub(super) const GENERATION: u64 = 7;
const PRINTS_PER_BAR: usize = 5;

pub(super) fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).expect("a valid level")
}

/// One chart on the WIN preset, following live, with its book captured.
pub(super) struct WinLive {
    pub(super) view: OrderflowView,
    closed: Vec<Bar>,
    forming: Option<Bar>,
    monotonic_ms: u64,
    last_print_ms: Option<i64>,
    update_id: u64,
    lane_px: f32,
}

impl WinLive {
    /// `tape_only` is the full-width tape the trader approved; otherwise the
    /// same tape sits in a 600 px pane beside the tick candles. Its book is
    /// captured from `at_ms`.
    pub(super) fn new(tape_only: bool, bids: &[BookLevel], asks: &[BookLevel], at_ms: i64) -> Self {
        let mut live = Self::without_book(tape_only);
        live.snapshot(at_ms, bids, asks);
        live
    }

    /// [`Self::new`] before its book's first image.
    pub(super) fn without_book(tape_only: bool) -> Self {
        let mut view = OrderflowView::new("WINV26");
        let before = view.config.clone();
        assert!(view.apply_preset("mini index regions"));
        view.config.live_lane.tape_only = tape_only;
        view.commit_config_changes(before);
        assert!(view.config.native_tape());
        view.observe_tape_price_grid(Decimal::from(5), Some(Decimal::from(187_000)));
        view.set_enabled(true, GENERATION);
        Self {
            view,
            closed: Vec::new(),
            forming: None,
            monotonic_ms: 0,
            last_print_ms: None,
            update_id: 1,
            lane_px: if tape_only { CHART_PX } else { 600.0 },
        }
    }

    /// The book's capture opens at `at_ms` on this image.
    pub(super) fn snapshot(&mut self, at_ms: i64, bids: &[BookLevel], asks: &[BookLevel]) {
        self.view.handle_depth_event(DepthEvent::Snapshot {
            symbol: "WINV26".to_owned(),
            generation: GENERATION,
            observed_at_ms: at_ms,
            effective_at_ms: at_ms,
            price_step: Some(Decimal::from(5)),
            snapshot: BookSnapshot::new(
                1,
                bids.to_vec(),
                asks.to_vec(),
                BookCoverage::Limited {
                    levels_per_side: 20,
                },
            ),
        });
    }

    /// The book changes at `at_ms`, as MetaTrader stamps an image.
    pub(super) fn book(&mut self, at_ms: i64, bids: Vec<BookLevel>, asks: Vec<BookLevel>) {
        self.update_id += 1;
        self.view.handle_depth_event(DepthEvent::Update {
            symbol: "WINV26".to_owned(),
            generation: GENERATION,
            event_time_ms: at_ms,
            delta: BookDelta::new(self.update_id, self.update_id, bids, asks),
        });
    }

    /// A print reaches the chart and its tick candle.
    pub(super) fn print(&mut self, trade: &Trade) {
        self.view.record_trade(trade);
        match self.forming.as_mut() {
            Some(bar) if (bar.trade_count as usize) < PRINTS_PER_BAR => bar.extend(trade),
            _ => {
                self.closed.extend(self.forming.take());
                self.forming = Some(Bar::opened_by(trade));
            }
        }
        self.last_print_ms = Some(trade.timestamp_ms);
    }

    /// `elapsed_ms` of host time pass before the next frame; the tape's
    /// clock runs on from the last applied print, as it does live.
    pub(super) fn advance(&mut self, elapsed_ms: u64) {
        self.monotonic_ms += elapsed_ms;
        self.view
            .set_live_clock_at(self.last_print_ms, self.monotonic_ms);
    }

    /// One frame: the pane asks, the worker answers, the pane asks again.
    pub(super) fn frame(&mut self) -> Arc<VisibleOrderflow> {
        let partial = self.forming.clone().expect("a forming candle");
        let closed = self.closed.clone();
        let lane_bars: Vec<_> = closed
            .iter()
            .chain(Some(&partial))
            .map(|bar| (bar.open_time, bar.close_time))
            .collect();
        let lane_px = self.lane_px;
        let window_ms = self.view.config.lane_window_ms(reserved_span_ms(&closed));
        let ask = |view: &mut OrderflowView| {
            view.project_visible(
                VisibleBarTimeline::new(closed.len() as u64, 0, &closed, Some(&partial)),
                true,
                true,
                Some(reserved_span_ms(&closed)),
                PRICES,
                Some(PaneGeometry {
                    px_per_bar: 12.0,
                    lane_width_px: lane_px,
                    lane_window_ms: window_ms,
                    height_px: HEIGHT_PX,
                    lane_bars: lane_bars.clone(),
                }),
            )
        };
        let _ = ask(&mut self.view);
        self.view.flush_for_test();
        ask(&mut self.view).expect("the tape projects")
    }

    fn painted(&self, frame: &VisibleOrderflow, book: bool) -> Vec<egui::epaint::ClippedShape> {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(CHART_PX, HEIGHT_PX));
        let mut viewport = Viewport::new();
        viewport.set_px_per_bar(12.0);
        let total = self.closed.len() + 1;
        let ctx = egui::Context::default();
        ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            if book {
                self.view.draw_background(
                    &painter,
                    rect,
                    &viewport,
                    total,
                    frame,
                    egui::Color32::BLACK,
                    self.lane_px,
                    false,
                );
            } else {
                self.view.draw_aggressions(
                    &painter,
                    rect,
                    &viewport,
                    total,
                    frame,
                    egui::Color32::BLACK,
                    self.lane_px,
                    false,
                    PRICES,
                );
            }
        })
        .shapes
    }

    /// Screen x where the tape's pane begins.
    pub(super) fn tape_left(&self) -> f32 {
        CHART_PX - self.lane_px
    }

    /// Every shape the tape paints for `frame`.
    pub(super) fn tape(&self, frame: &VisibleOrderflow) -> Vec<egui::epaint::ClippedShape> {
        self.painted(frame, false)
    }

    /// Every shape the depth pass paints behind the tape for `frame`.
    pub(super) fn book_pass(&self, frame: &VisibleOrderflow) -> Vec<egui::epaint::ClippedShape> {
        self.painted(frame, true)
    }
}

/// A deterministic WIN-like session: sixty prints over about twelve seconds
/// on a five-point grid, both sides, and a book image at every other print's
/// instant.
fn session(live: &mut WinLive) {
    let gaps = [40_i64, 90, 15, 260, 130, 700, 55, 20, 310, 180];
    let steps = [0_i64, 5, 5, 10, 5, 0, -5, -5, 0, 5, 10, 15, 10, 5, -5, -10];
    let mut seed = 0x5eed_u64;
    let mut at = 2_000_i64;
    let mut previous = at;
    for index in 0..60_usize {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let quantity = 1 + i64::try_from((seed >> 33) % 35).expect("small");
        let price =
            187_100 + steps[index % steps.len()] + 5 * i64::try_from(index / 16).expect("small");
        let side = if (seed >> 40).is_multiple_of(3) {
            Side::Sell
        } else {
            Side::Buy
        };
        if index % 2 == 0 {
            live.book(
                at,
                vec![level(price - 5, 10 + quantity * 3)],
                vec![level(price + 5, 12 + quantity * 2)],
            );
        }
        live.print(&Trade {
            agg_id: 1_000 + index as u64,
            timestamp_ms: at,
            price: Decimal::from(price),
            quantity: Decimal::from(quantity),
            side,
        });
        live.advance(u64::try_from(at - previous).expect("forward"));
        let _ = live.frame();
        previous = at;
        at += gaps[index % gaps.len()];
    }
}

/// The resting book the session opens on, five points apart around 187,100.
pub(super) fn opening_book() -> (Vec<BookLevel>, Vec<BookLevel>) {
    let bids = (0..20)
        .map(|index| level(187_095 - index * 5, 20 + (index * 7) % 60))
        .collect();
    let asks = (0..20)
        .map(|index| level(187_100 + index * 5, 25 + (index * 11) % 60))
        .collect();
    (bids, asks)
}

/// One painted shape as the golden records it: its kind, its bounds to a
/// hundredth of a pixel and its colour.
fn describe(shape: &egui::Shape) -> (String, [f32; 4], [u8; 4]) {
    let bounds = shape.visual_bounding_rect();
    let round = |value: f32| (value * 100.0).round() / 100.0;
    let color = match shape {
        egui::Shape::Circle(circle) => circle.fill,
        egui::Shape::Mesh(mesh) => mesh
            .vertices
            .first()
            .map_or(egui::Color32::TRANSPARENT, |vertex| vertex.color),
        egui::Shape::Text(text) => text.fallback_color,
        egui::Shape::Path(path) => solid(&path.stroke.color),
        egui::Shape::LineSegment { stroke, .. } => solid(&stroke.color),
        _ => egui::Color32::TRANSPARENT,
    };
    let kind = match shape {
        egui::Shape::Circle(_) => "circle",
        egui::Shape::Mesh(_) => "mesh",
        egui::Shape::Text(_) => "text",
        egui::Shape::Path(_) => "path",
        egui::Shape::LineSegment { .. } => "line",
        _ => "other",
    };
    (
        kind.to_owned(),
        [
            round(bounds.left()),
            round(bounds.top()),
            round(bounds.right()),
            round(bounds.bottom()),
        ],
        color.to_array(),
    )
}

fn solid(color: &egui::epaint::ColorMode) -> egui::Color32 {
    match color {
        egui::epaint::ColorMode::Solid(color) => *color,
        egui::epaint::ColorMode::UV(_) => egui::Color32::TRANSPARENT,
    }
}

fn golden_lines(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .map(|shape| {
            let (kind, [left, top, right, bottom], [r, g, b, a]) = describe(&shape.shape);
            format!("{kind} {left:.2} {top:.2} {right:.2} {bottom:.2} {r} {g} {b} {a}")
        })
        .collect()
}

/// The recorded shapes of one `[section]` of [`GOLDEN`].
fn golden(section: &str) -> Vec<&'static str> {
    let header = format!("[{section}]");
    GOLDEN
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn assert_golden(actual: &[String], expected: &[&str], why: &str) {
    let dump = actual.join("\n");
    assert!(!expected.is_empty(), "{why}: no golden recorded");
    assert_eq!(
        actual.len(),
        expected.len(),
        "{why}: shape count; painted:\n{dump}"
    );
    for (actual, expected) in actual.iter().zip(expected) {
        let (a, e): (Vec<&str>, Vec<&str>) =
            (actual.split(' ').collect(), expected.split(' ').collect());
        assert_eq!(a.len(), e.len(), "{why}: {actual} against {expected}");
        assert_eq!(a[0], e[0], "{why}: {actual} against {expected}");
        for (a, e) in a[1..5].iter().zip(&e[1..5]) {
            let (a, e): (f32, f32) = (a.parse().expect("bound"), e.parse().expect("bound"));
            assert!(
                (a - e).abs() <= 0.02,
                "{why}: {actual} against {expected}; painted:\n{dump}"
            );
        }
        assert_eq!(a[5..], e[5..], "{why}: {actual} against {expected}");
    }
}

/// What the approved tape paints at live, with the book changing under it:
/// just after the newest print, while its dot still forms at now, and once
/// the clock has run on and every dot sits at its own execution time.
#[test]
fn the_win_native_tape_paints_its_approved_marks_at_live() {
    for (tape_only, pane) in [(true, "tape only"), (false, "beside the candles")] {
        let (bids, asks) = opening_book();
        let mut live = WinLive::new(tape_only, &bids, &asks, 1_000);
        session(&mut live);
        live.advance(60);
        let frame = live.frame();
        let why = format!("{pane}, newest dot forming");
        assert_golden(&golden_lines(&live.tape(&frame)), &golden(&why), &why);
        live.advance(900);
        let frame = live.frame();
        let why = format!("{pane}, clock ran on");
        assert_golden(&golden_lines(&live.tape(&frame)), &golden(&why), &why);
    }
}

/// The recorded tape: see the file's own header.
const GOLDEN: &str = include_str!("win_tape_golden.txt");
