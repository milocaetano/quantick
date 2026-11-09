//! Render the actual ticket and ledger with complete financial facts.
use super::*;
use crate::paper_chrome::position_word;
use eframe::egui;

fn check_shapes(shapes: &[egui::epaint::ClippedShape], width: f32) -> String {
    let mut text = String::new();
    let mut painted: Vec<(&str, egui::Rect)> = Vec::new();
    for clipped in shapes {
        if let egui::Shape::Text(shape) = &clipped.shape {
            if shape.galley.text().is_empty() {
                continue;
            }
            let bounds = shape.visual_bounding_rect();
            assert!(
                bounds.right() <= width + 1.0,
                "{} overflows {width}: {bounds:?}",
                shape.galley.text()
            );
            assert!(
                clipped.clip_rect.expand(1.0).contains_rect(bounds),
                "{} is clipped: {bounds:?} by {:?}",
                shape.galley.text(),
                clipped.clip_rect
            );
            text.push_str(shape.galley.text());
            text.push('\n');
            for (previous, rect) in &painted {
                let overlap = rect.intersect(bounds);
                assert!(
                    overlap.width() <= 0.5
                        || overlap.height() <= 0.5
                        // The same text again, or its bold strike under a
                        // pixel to the right.
                        || (*previous == shape.galley.text()
                            && (rect.size() - bounds.size()).length() < 0.01
                            && (rect.min - bounds.min).length() < 1.0),
                    "{previous} overlaps {}: {rect:?} / {bounds:?}",
                    shape.galley.text()
                );
            }
            painted.push((shape.galley.text(), bounds));
        }
    }
    text
}

#[test]
fn paper_values_and_actions_fit_narrow_panels() {
    for light in [false, true] {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for width in [320.0, 400.0, 520.0] {
                for side in [Side::Buy, Side::Sell] {
                    let ctx = egui::Context::default();
                    let mut appearance = quantick_appearance::Appearance::default();
                    if light {
                        appearance.apply(quantick_appearance::LIGHT).unwrap();
                    }
                    crate::theme::sync(&ctx, &appearance);
                    let mut paper = PaperTrading::new();
                    paper.set_symbol("LONG-INSTRUMENT-USDT");
                    paper.desk.ticket.qty_text = "123456.789".to_owned();
                    paper.desk.ticket.stop_offset_text = "25".to_owned();
                    paper.desk.ticket.profit_offset_text = "50".to_owned();
                    paper.seed(&print(0, 123456789));
                    paper.market(side);
                    paper.on_trade(&print(1, 123456789));
                    paper.close_position();
                    paper.on_trade(&print(2, 123456799));
                    paper.market(side);
                    paper.on_trade(&print(3, 123456799));
                    let open_text = paper.account.open_pnl_text().unwrap();
                    let closed_text = paper
                        .account
                        .symbol_pnl()
                        .closed(&paper.session_trades()[0]);
                    for ledger in [false, true] {
                        let mut words = String::new();
                        for _ in 0..3 {
                            let mut input = egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 4000.0),
                                )),
                                ..Default::default()
                            };
                            input
                                .viewports
                                .get_mut(&egui::ViewportId::ROOT)
                                .unwrap()
                                .native_pixels_per_point = Some(scale);
                            let output = ctx.run(input, |ctx| {
                                egui::CentralPanel::default()
                                    .frame(egui::Frame::none())
                                    .show(ctx, |ui| {
                                        if ledger {
                                            paper.draw_trades_tab(ui, TzOffset::new(0));
                                        } else {
                                            paper.draw_trading_tab(ui);
                                        }
                                    });
                            });
                            words = check_shapes(&output.shapes, width);
                        }
                        assert!(words.contains(position_word(side)), "{words}");
                        assert!(
                            words.contains(&open_text),
                            "complete open calculation: {words}"
                        );
                        if ledger {
                            assert!(
                                words.contains(&closed_text),
                                "complete closed calculation: {words}"
                            );
                        }
                        assert!(words.contains("123456799"), "entry stays complete: {words}");
                        assert!(!words.contains("1234567…"), "no price elision");
                        if !ledger {
                            for label in [
                                "SL",
                                "TP",
                                "Report",
                                "Flatten all",
                                "Close 50%",
                                "BUY",
                                "SELL",
                            ] {
                                assert!(words.contains(label), "missing {label}: {words}");
                            }
                            assert!(words.contains("Market orders fill at the next print."));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn paper_values_and_actions_fit_narrow_panels_report_action_scrolls_into_view() {
    for width in [320.0, 400.0, 520.0] {
        let ctx = egui::Context::default();
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 123456789));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 123456789));
        let mut saw_report = false;
        let mut initially_visible = false;
        for frame in 0..80 {
            let mut input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 400.0),
                )),
                time: Some(f64::from(frame) / 60.0),
                ..Default::default()
            };
            if (5..35).contains(&frame) {
                input.events = vec![
                    egui::Event::PointerMoved(egui::pos2(100.0, 200.0)),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, -80.0),
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            paper.draw_trading_tab(ui);
                        });
                });
            });
            let visible = output.shapes.iter().any(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains("Report") => {
                    shape.clip_rect.contains_rect(text.visual_bounding_rect())
                }
                _ => false,
            });
            if frame == 3 {
                initially_visible = visible;
            }
            saw_report |= visible;
        }
        assert!(!initially_visible, "fixture requires scrolling");
        assert!(
            saw_report,
            "ordinary wheel input exposes complete Report action at {width}"
        );
        assert!(
            paper.account.venue().position().is_some(),
            "scrolling never changes position"
        );
    }
}
