use super::*;

#[test]
fn trade_actions_keep_readable_labels() {
    for width in [320.0, 400.0, 520.0] {
        let ctx = egui::Context::default();
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(quantick_appearance::LIGHT).unwrap();
        crate::theme::sync(&ctx, &appearance);
        let mut paper = super::super::PaperTrading::new();
        let mut strategy = OrderStrategy::starter(0);
        strategy.rows[0].gain_ticks = Some(111);
        strategy.rows[0].loss_ticks = Some(222);
        paper.account.add_order_strategy(strategy.clone());
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 500.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::none())
                        .show(ctx, |ui| {
                            assert!(!draw_strategy_rows(ui, &mut paper.account, Some(0)));
                        });
                },
            );
            let bounds = |wanted: &str| {
                output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == wanted => {
                            Some(text.visual_bounding_rect())
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("missing {wanted}"))
            };
            let gain = bounds("Gain (ticks)");
            let loss = bounds("Loss (ticks)");
            assert!(bounds("111").left() >= gain.left());
            assert!(bounds("111").right() < loss.left());
            assert!(bounds("222").left() >= loss.left());
            assert!(bounds("222").right() <= width);
        }
        assert_eq!(
            paper.account.order_strategies()[0],
            strategy,
            "painting never changes the ladder"
        );
    }
}
