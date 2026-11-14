use super::*;
use quantick_sim::ExitReason;
use rust_decimal::Decimal;

fn trade(open_s: i64, close_s: i64, pnl: i64) -> ClosedTrade {
    ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: open_s * 1000,
        closed_ms: close_s * 1000,
        pnl_points: Decimal::from(pnl),
        exit_reason: ExitReason::TakeProfit,
        entry_agg_id: Some(1),
        exit_agg_id: Some(2),
        mae_points: Some(Decimal::ZERO),
        mfe_points: Some(Decimal::from(pnl.max(0))),
    }
}

/// What one paint pass put on screen.
struct Painted {
    /// Every shape emitted, marks and connectors alike.
    shapes: usize,
    /// Text galleys: the cap notice and the hover tooltip.
    texts: Vec<String>,
    /// The convex polygons — the entry triangles and exit diamonds — as
    /// their vertices, in paint order.
    polygons: Vec<Vec<egui::Pos2>>,
}

/// Run one paint pass against a real context, the pane's time→slot
/// mapping covering the whole tape.
fn painted(trades: &[ClosedTrade], pointer: Option<egui::Pos2>) -> Painted {
    painted_over(trades, pointer, |ms| usize::try_from(ms / 1000).ok())
}

/// Run one paint pass with `slot_at_time` standing in for the pane's
/// own lookup — `None` is how a pane says its bars do not reach that
/// instant, which is the whole subject of the tests below.
fn painted_over(
    trades: &[ClosedTrade],
    pointer: Option<egui::Pos2>,
    slot_at_time: impl Fn(i64) -> Option<usize>,
) -> Painted {
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 400.0));
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let scale = PriceScale::from_range(80.0, 120.0, 0.0, 400.0);
            let frame = TradePaintFrame {
                painter: &painter,
                chart_rect: screen,
                scale: &scale,
                background: egui::Color32::BLACK,
                pointer,
                tz: TzOffset::default(),
                pnl: quantick_paper::pnl::PnlReader::plain().of("WINV26"),
            };
            draw(&frame, trades, None, &slot_at_time, |slot| {
                ((slot % 100) as f32) * 4.0
            });
        },
    );
    let mut texts = Vec::new();
    let mut polygons = Vec::new();
    let shapes = output.shapes.len();
    for shape in output.shapes {
        match shape.shape {
            egui::epaint::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            egui::epaint::Shape::Path(path) => polygons.push(path.points.to_vec()),
            _ => {}
        }
    }
    Painted {
        shapes,
        texts,
        polygons,
    }
}

/// A mapping that answers only for the window `[from, to]` seconds — a
/// pane whose bars start and end there, in the tests' own units.
fn tape(from: i64, to: i64) -> impl Fn(i64) -> Option<usize> {
    move |ms: i64| {
        (from * 1_000..=to * 1_000)
            .contains(&ms)
            .then(|| usize::try_from(ms / 1_000).ok())
            .flatten()
    }
}

#[test]
fn a_round_trip_paints_marks_and_nothing_paints_empty() {
    let empty = painted(&[], None);
    let one = painted(&[trade(10, 20, 5)], None);
    assert!(
        one.shapes > empty.shapes,
        "a closed trade adds its marks: {} vs {}",
        one.shapes,
        empty.shapes
    );
    assert!(
        one.texts.is_empty(),
        "no cap note under the limit: {:?}",
        one.texts
    );
}

#[test]
fn the_cap_discloses_what_it_withheld() {
    let trades: Vec<ClosedTrade> = (0..260).map(|index| trade(index, index + 1, 1)).collect();
    let out = painted(&trades, None);
    assert!(
        out.texts
            .iter()
            .any(|text| text == "trade paint: 200 of 260 shown"),
        "withheld marks are counted out loud: {:?}",
        out.texts
    );
}

#[test]
fn a_hovered_mark_speaks_the_ledgers_words() {
    // Entry: slot 10 → x 40; price 100 → y 200 on this scale.
    let out = painted(&[trade(10, 20, 5)], Some(egui::pos2(41.0, 202.0)));
    let tooltip = out.texts.join(" ");
    assert!(
        tooltip.contains("LONG 1 · 100 → 105 · +5 pts"),
        "the tooltip heads with the ledger row's words: {:?}",
        out.texts
    );
    assert!(
        tooltip.contains("take profit"),
        "and tells the exit reason: {:?}",
        out.texts
    );
}

/// The covered trade's geometry, pinned: the rule below removes marks
/// the tape cannot prove, and this is the half that must not move.
#[test]
fn a_covered_round_trip_lands_on_its_own_bars() {
    // Entry slot 15 → x 60, price 100 → y 200, apex one gap under it
    // because a long points down. Exit slot 25 → x 100, price 105 → y
    // 150, diamond centred there.
    let out = painted_over(&[trade(15, 25, 5)], None, tape(10, 30));
    assert_eq!(out.polygons.len(), 2, "one triangle and one diamond");
    let apex = out.polygons[0][0];
    assert!(
        (apex.x - 60.0).abs() < 0.01 && (apex.y - 203.0).abs() < 0.01,
        "the entry apex sits on the bar holding the fill: {apex:?}"
    );
    let top = out.polygons[1][0];
    assert!(
        (top.x - 100.0).abs() < 0.01 && (top.y - 145.5).abs() < 0.01,
        "and the exit diamond on its own: {top:?}"
    );
}

/// The rule itself: a fill the bars on screen do not reach paints no
/// mark — not a triangle, not a diamond, not half a round trip — and
/// says so instead of leaving an empty chart to explain itself.
#[test]
fn a_trade_the_tape_does_not_cover_paints_nothing() {
    for (name, round_trip) in [
        ("older than the oldest bar", trade(2, 5, 5)),
        ("newer than the newest print", trade(40, 50, 5)),
        ("entry off the tape, exit on it", trade(5, 20, 5)),
        ("entry on the tape, exit past it", trade(20, 40, 5)),
    ] {
        let out = painted_over(&[round_trip], None, tape(10, 30));
        assert!(out.polygons.is_empty(), "{name}: a mark was painted");
        assert_eq!(
            out.texts,
            vec!["trade paint: 1 off the bars on screen".to_owned()],
            "{name}: the chart did not say what it left off"
        );
    }
}

/// One end covered is not half a trade — a connector to a bar the fill
/// has nothing to do with is the lie the rule exists to refuse — and an
/// unpainted mark answers no pointer.
#[test]
fn an_off_tape_trade_cannot_be_hovered_either() {
    // The pixel the entry mark would have occupied had it been clamped
    // onto the tape's first bar (slot 10 → x 40, price 100 → y 200).
    let out = painted_over(
        &[trade(2, 5, 5)],
        Some(egui::pos2(41.0, 202.0)),
        tape(10, 30),
    );
    let text = out.texts.join(" ");
    assert!(
        !text.contains("LONG"),
        "an unpainted mark still answered the pointer: {:?}",
        out.texts
    );
}

/// The cap counts paint, not ledger rows: trades the tape does not
/// cover were never candidates, so they are reported as what they are
/// rather than as "withheld".
#[test]
fn the_cap_counts_only_what_the_tape_covers() {
    let trades: Vec<ClosedTrade> = (0..260).map(|index| trade(index, index + 1, 1)).collect();
    // The rebuilt tape holds the last hundred round trips; the other
    // hundred and sixty are off it.
    let out = painted_over(&trades, None, tape(160, 400));
    assert_eq!(
        out.texts,
        vec!["trade paint: 160 off the bars on screen".to_owned()],
        "a hundred marks fit under the cap of 200, and the rest are named"
    );
    assert_eq!(
        out.polygons.len(),
        200,
        "two marks each for the hundred the tape proves"
    );
}

/// Both counts at once: the cap bit *and* the tape fell short.
#[test]
fn the_notice_carries_the_cap_and_the_tape_together() {
    let trades: Vec<ClosedTrade> = (0..500).map(|index| trade(index, index + 1, 1)).collect();
    let out = painted_over(&trades, None, tape(100, 500));
    assert_eq!(
        out.texts,
        vec!["trade paint: 200 of 400 shown · 100 off the bars on screen".to_owned()],
        "the corner reports both reasons a round trip is not on screen"
    );
}
