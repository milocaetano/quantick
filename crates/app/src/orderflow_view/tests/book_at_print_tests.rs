//! Each print on the native tape is drawn against the book as it stood when
//! it traded, and the book reaches exactly as far as it is known.
//!
//! The trader's report (live WIN, 2026-09-30): the aggression bubbles run on
//! past the book behind them, so a print never shows the resting book it hit
//! until later. Here the book and the prints change at the same instants, so
//! wherever the painter puts a print, the level that appeared at that instant
//! has to begin under it.
use super::tape_golden_tests::{WinLive, level};
use super::*;
use quantick_engine::Side;
use quantick_orderflow::engine::VisibleOrderflow;

/// Eight prints, 900 ms apart, each where a new ask level appears at the
/// same instant: 30 points apart, in a gap of the opening book, so no other
/// liquidity shares a print's row.
const PRINTS: i64 = 8;

fn instant(index: i64) -> i64 {
    2_000 + index * 900
}

fn price(index: i64) -> i64 {
    187_010 + index * 30
}

/// Pixel centres of the tape's dots, left to right.
pub(super) fn dot_centres(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Pos2> {
    let mut centres: Vec<egui::Pos2> = Vec::new();
    for shape in shapes {
        if let egui::Shape::Circle(circle) = &shape.shape
            && !centres
                .iter()
                .any(|centre| centre.distance(circle.center) < 0.5)
        {
            centres.push(circle.center);
        }
    }
    centres.sort_by(|a, b| a.x.total_cmp(&b.x));
    centres
}

/// Every visible rectangle the depth pass paints in the tape's pane, cut to
/// its clip: beside the candles, their own pane carries each bar's summary
/// band instead. The fixtures here make no reduction, so every such
/// rectangle is resting liquidity.
pub(super) fn book_rects(shapes: &[egui::epaint::ClippedShape], tape_left: f32) -> Vec<egui::Rect> {
    let mut rects = Vec::new();
    for shape in shapes {
        let egui::Shape::Mesh(mesh) = &shape.shape else {
            continue;
        };
        for quad in mesh.vertices.chunks(4) {
            let rect =
                egui::Rect::from_points(&quad.iter().map(|vertex| vertex.pos).collect::<Vec<_>>())
                    .intersect(shape.clip_rect);
            if rect.is_positive() && rect.left() >= tape_left - 0.5 {
                rects.push(rect);
            }
        }
    }
    rects
}

/// Check one painted frame: under every print but the newest, the level
/// that appeared with it begins at the print's own x; and the book ends at
/// the newest print, where the newest image left it known.
fn assert_book_at_the_prints(live: &WinLive, frame: &VisibleOrderflow, prints: usize, why: &str) {
    let dots = dot_centres(&live.tape(frame));
    let book = book_rects(&live.book_pass(frame), live.tape_left());
    let why = format!("{why}; dots {dots:?}; book {book:?}");
    assert_eq!(dots.len(), prints, "{why}");
    let (newest, earlier) = dots.split_last().expect("a print");
    for (index, dot) in earlier.iter().enumerate() {
        let starts = book
            .iter()
            .filter(|rect| rect.top() - 1.0 <= dot.y && dot.y <= rect.bottom() + 1.0)
            .map(|rect| rect.left())
            .fold(f32::INFINITY, f32::min);
        assert!(
            (starts - dot.x).abs() <= 1.0,
            "print {index} at x {} is drawn over the book of another instant: the level \
             that appeared with it begins at x {starts}; {why}",
            dot.x,
        );
    }
    let book_end = book
        .iter()
        .map(|rect| rect.right())
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        (book_end - newest.x).abs() <= 1.0,
        "the book is known up to the newest print, at x {}, and is drawn to x {book_end}; \
         {why}",
        newest.x,
    );
}

#[test]
fn every_print_sits_on_the_book_of_its_own_instant_up_to_the_newest() {
    for tape_only in [true, false] {
        let bids: Vec<_> = (0..10)
            .map(|index| level(186_995 - index * 5, 40))
            .collect();
        let asks: Vec<_> = (0..10)
            .map(|index| level(187_250 + index * 5, 40))
            .collect();
        let mut live = WinLive::new(tape_only, &bids, &asks, 1_000);
        for index in 0..PRINTS {
            let at = instant(index);
            live.book(at, Vec::new(), vec![level(price(index), 60)]);
            live.print(&Trade {
                agg_id: 1 + u64::try_from(index).expect("small"),
                timestamp_ms: at,
                price: Decimal::from(price(index)),
                quantity: Decimal::from(5),
                side: Side::Buy,
            });
            live.advance(if index == 0 { 0 } else { 900 });
            let frame = live.frame();
            let prints = usize::try_from(index + 1).expect("small");
            if prints > 1 {
                let why = format!("tape_only={tape_only}, frame of print {index}");
                assert_book_at_the_prints(&live, &frame, prints, &why);
            }
        }
        // The clock runs on past the newest print with nothing new arriving:
        // every dot moves on to its own execution time, and the book, known
        // only up to the newest image, moves with them and stretches nowhere.
        for elapsed in [1_500, 400] {
            live.advance(elapsed);
            let frame = live.frame();
            let why = format!("tape_only={tape_only}, {elapsed} ms later");
            assert_book_at_the_prints(&live, &frame, 8, &why);
        }
    }
}
