use super::*;

#[test]
fn light_pnl_ink_preserves_dark_palette_on_profitable_stop() {
    let ctx = egui::Context::default();
    let position = Position {
        side: quantick_engine::Side::Buy,
        quantity: Decimal::ONE,
        avg_price: Decimal::from(100),
        opened_ms: 0,
        opened_agg_id: 0,
        low_price: Decimal::from(100),
        high_price: Decimal::from(120),
        stop_loss: Some(Decimal::from(110)),
        take_profit: None,
    };
    for light in [false, true] {
        let mut appearance = quantick_appearance::Appearance::default();
        if light {
            appearance.apply(quantick_appearance::LIGHT).unwrap();
        }
        theme::sync(&ctx, &appearance);
        let palette = theme::get(&ctx);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                BracketLeg {
                    stop: true,
                    word: "SL",
                    level: Some((Decimal::from(110), "+10 pts".to_owned())),
                    offset: None,
                    clear_hover: "clear",
                }
                .show(ui, &position);
            });
        });
        let text = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "+10 pts" => Some(text),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            text.galley.job.sections[0].format.color,
            if light {
                points_color(&palette, Decimal::TEN)
            } else {
                palette.sell()
            }
        );
    }
}
