use super::*;
use quantick_engine::Trade;
use rust_decimal::Decimal;

#[test]
fn drift_reads_the_entry_row_against_the_pane_edges() {
    assert_eq!(entry_drift(50.0, 0.0, 400.0), EntryDrift::OnScreen);
    assert_eq!(entry_drift(-1.0, 0.0, 400.0), EntryDrift::Above);
    assert_eq!(entry_drift(401.0, 0.0, 400.0), EntryDrift::Below);
}

#[test]
fn the_caret_names_the_price_and_the_direction() {
    let summary = PositionSummary {
        side: Side::Buy,
        quantity: Decimal::ONE,
        avg_price: Decimal::new(1055, 1),
        open_points: None,
    };
    assert_eq!(caret_text(&summary, EntryDrift::OnScreen), None);
    let above = caret_text(&summary, EntryDrift::Above).expect("off-screen carets");
    assert!(
        above.contains("105.5") && above.contains("above"),
        "{above}"
    );
    let below = caret_text(&summary, EntryDrift::Below).expect("off-screen carets");
    assert!(below.contains("below"), "{below}");
}

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

/// The HUD reaches real pixels while a position is open, and paints
/// nothing at all while flat.
#[test]
fn the_hud_paints_the_position_and_only_the_position() {
    let ctx = egui::Context::default();
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    let scale = PriceScale::from_range(90.0, 110.0, 0.0, 400.0);
    let mut paper = PaperTrading::new();

    let painted = |paper: &mut PaperTrading| {
        let mut text = String::new();
        for _ in 0..2 {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                draw(ctx, chart, paper, &scale);
            });
            text.clear();
            for shape in output.shapes {
                if let egui::epaint::Shape::Text(galley) = shape.shape {
                    text.push_str(galley.galley.text());
                    text.push(' ');
                }
            }
        }
        text
    };

    assert!(painted(&mut paper).is_empty(), "flat paints no HUD");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let text = painted(&mut paper);
    assert!(text.contains("SIM LONG 1 @ 100"), "painted: {text}");
    assert!(text.contains("Close"), "painted: {text}");
    assert!(text.contains("Reverse"), "painted: {text}");
}
