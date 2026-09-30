//! A tape held in the past keeps its book: the depth recorded for that
//! window, from the same retained history the live tape painted, placed on
//! the held tape's clock under its own prints. Where no depth was ever
//! captured, or it is no longer retained, the tape leaves it blank and says
//! so; it never fills a past window with today's book.
use super::book_at_print_tests::{book_rects, dot_centres};
use super::tape_golden_tests::{HEIGHT_PX, PRICES, WinLive, level};
use super::*;
use quantick_engine::Side;
use quantick_orderbook::BookLevel;
use quantick_orderflow::engine::VisibleOrderflow;
use quantick_orderflow::tape_view::TapeEnd;
use std::sync::Arc;

/// Screen y of `price` on the fixed price window every frame here uses.
fn y_of(price: i64) -> f32 {
    ((PRICES.1 - price as f64) / (PRICES.1 - PRICES.0)) as f32 * HEIGHT_PX
}

/// The book's rectangles on the row a print at `price` is drawn on.
fn row(book: &[egui::Rect], price: i64) -> Vec<egui::Rect> {
    let y = y_of(price);
    book.iter()
        .filter(|rect| rect.top() - 1.0 <= y && y <= rect.bottom() + 1.0)
        .copied()
        .collect()
}

/// The x of the tape's dots on the row of `price`, left to right.
fn dots_at(dots: &[egui::Pos2], price: i64) -> Vec<f32> {
    dots.iter()
        .filter(|dot| (dot.y - y_of(price)).abs() < 0.5)
        .map(|dot| dot.x)
        .collect()
}

/// Every text the depth pass painted.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

fn opening_book() -> (Vec<BookLevel>, Vec<BookLevel>) {
    (
        (0..10)
            .map(|index| level(186_995 - index * 5, 40))
            .collect(),
        (0..10)
            .map(|index| level(187_250 + index * 5, 40))
            .collect(),
    )
}

/// One level a book image sets at an instant, and the print there trades on
/// it: `(at_ms, price, quantity, bid)`.
type Wall = (i64, i64, i64, bool);

/// A print every 500 ms from 2 s to 40 s; from `capture_from_ms` a book image
/// at every print's instant, opening on [`opening_book`] unless the chart
/// already holds one. Where `walls` sets a level, the print trades there.
fn session(live: &mut WinLive, capture_from_ms: i64, walls: &[Wall], marks: &[(i64, i64)]) {
    let (bids, asks) = opening_book();
    let mut previous = None;
    for (index, at) in (2_000..=40_000_i64).step_by(500).enumerate() {
        let wall = walls.iter().find(|wall| wall.0 == at);
        if at == capture_from_ms {
            live.snapshot(at, &bids, &asks);
        } else if at > capture_from_ms {
            match wall {
                Some(&(_, price, quantity, true)) => {
                    live.book(at, vec![level(price, quantity)], Vec::new());
                }
                Some(&(_, price, quantity, false)) => {
                    live.book(at, Vec::new(), vec![level(price, quantity)]);
                }
                None => live.book(at, vec![level(186_995, 40 + (at / 500) % 7)], Vec::new()),
            }
        }
        let price = wall
            .map(|wall| wall.1)
            .or_else(|| marks.iter().find(|mark| mark.0 == at).map(|mark| mark.1))
            .unwrap_or(187_100 + 5 * (at / 500 % 2));
        live.print(&Trade {
            agg_id: 1 + u64::try_from(index).expect("small"),
            timestamp_ms: at,
            price: Decimal::from(price),
            quantity: Decimal::from(3),
            side: Side::Buy,
        });
        live.advance(previous.map_or(0, |previous: i64| {
            u64::try_from(at - previous).expect("forward")
        }));
        let _ = live.frame();
        previous = Some(at);
    }
}

/// Hold the tape at `end_ms`, clamped as a drag would be, and draw it.
fn hold(live: &mut WinLive, end_ms: i64) -> Arc<VisibleOrderflow> {
    live.view.set_tape_end(TapeEnd::Past { end_ms });
    let _ = live.frame();
    live.frame()
}

/// A wall that stood from 10 s to 16 s, and one that appeared at 30 s: the
/// tape held at 20 s shows the first between the prints that met it, and
/// not the second.
#[test]
fn a_held_tape_draws_the_book_recorded_for_its_window_and_never_todays() {
    for tape_only in [true, false] {
        let (bids, asks) = opening_book();
        let mut live = WinLive::new(tape_only, &bids, &asks, 1_000);
        let walls = [
            (10_000, 187_150, 90, false),
            (16_000, 187_150, 0, false),
            (30_000, 187_050, 90, true),
        ];
        session(&mut live, 1_000, &walls, &[]);
        let frame = hold(&mut live, 20_000);
        assert_eq!(live.view.tape_end(), TapeEnd::Past { end_ms: 20_000 });
        let book = book_rects(&live.book_pass(&frame), live.tape_left());
        let met = dots_at(&dot_centres(&live.tape(&frame)), 187_150);
        let why = format!("tape_only={tape_only}; wall prints at {met:?}; book {book:?}");
        assert_eq!(met.len(), 2, "{why}");
        let wall = row(&book, 187_150);
        assert!(!wall.is_empty(), "the held tape drew no book; {why}");
        let starts = wall.iter().map(|rect| rect.left()).fold(f32::MAX, f32::min);
        let ends = wall
            .iter()
            .map(|rect| rect.right())
            .fold(f32::MIN, f32::max);
        assert!(
            (starts - met[0]).abs() <= 1.0 && (ends - met[1]).abs() <= 1.0,
            "the wall stood between the prints at x {} and x {}, and is drawn from x \
             {starts} to x {ends}; {why}",
            met[0],
            met[1],
        );
        assert!(
            row(&book, 187_050).is_empty(),
            "a wall that appeared at 30 s is today's book, not the held window's; {why}"
        );
        assert!(
            !row(&book, 186_995).is_empty(),
            "the book that rested through the window is drawn; {why}"
        );
    }
}

/// Prints from 2 s, but the book captured only from 12 s: the held window's
/// earlier stretch has no book, and says it was never captured.
#[test]
fn a_held_tape_leaves_uncaptured_depth_blank_and_labelled() {
    for tape_only in [true, false] {
        let mut live = WinLive::without_book(tape_only);
        session(&mut live, 12_000, &[], &[(12_000, 187_200)]);
        let frame = hold(&mut live, 20_000);
        let pass = live.book_pass(&frame);
        let book = book_rects(&pass, live.tape_left());
        let first = dots_at(&dot_centres(&live.tape(&frame)), 187_200);
        let why = format!(
            "tape_only={tape_only}; first captured print at {first:?}; book {book:?}; \
             texts {:?}",
            texts(&pass)
        );
        assert_eq!(first.len(), 1, "{why}");
        assert!(!book.is_empty(), "the captured stretch has its book; {why}");
        assert!(
            book.iter().all(|rect| rect.left() >= first[0] - 1.0),
            "no book before the capture began; {why}"
        );
        assert!(
            texts(&pass)
                .iter()
                .any(|text| text == "L2 unavailable before capture"),
            "the uncaptured stretch is labelled; {why}"
        );
    }
}

/// Retention keeps 20 s of history: held at its earliest, the window reaches
/// back past what the book still retains, and says so.
#[test]
fn a_held_tape_leaves_evicted_depth_blank_and_labelled() {
    for tape_only in [true, false] {
        let (bids, asks) = opening_book();
        let mut live = WinLive::new(tape_only, &bids, &asks, 1_000);
        let before = live.view.config.clone();
        live.view.config.retention_ms = 20_000;
        live.view.commit_config_changes(before);
        session(&mut live, 1_000, &[], &[]);
        let frame = hold(&mut live, 0);
        let TapeEnd::Past { end_ms } = live.view.tape_end() else {
            panic!("the tape holds its earliest retained window");
        };
        let pass = live.book_pass(&frame);
        let book = book_rects(&pass, live.tape_left());
        let why = format!(
            "tape_only={tape_only}; held at {end_ms}; book {book:?}; texts {:?}",
            texts(&pass)
        );
        assert!(end_ms < 35_000, "{why}");
        assert!(!book.is_empty(), "the retained stretch has its book; {why}");
        assert!(
            texts(&pass)
                .iter()
                .any(|text| text == "L2 history no longer retained"),
            "the evicted stretch is labelled; {why}"
        );
    }
}
