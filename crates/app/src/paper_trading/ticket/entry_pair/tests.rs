use super::*;

#[test]
fn trade_actions_keep_readable_labels() {
    let ctx = egui::Context::default();
    let mut appearance = quantick_appearance::Appearance::default();
    appearance.apply(quantick_appearance::LIGHT).unwrap();
    theme::sync(&ctx, &appearance);
    for ready in [false, true] {
        for armed in [false, true] {
            let mut ticket = PaperTrading::new();
            ticket.desk.ticket.order_type = EntryKind::Limit;
            if armed {
                ticket.desk.armed = Some(super::super::super::ArmedPlacement {
                    side: Side::Buy,
                    kind: EntryKind::Limit,
                });
            }
            for state in 0..3 {
                let hovered = state == 1;
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
                let mut press = None;
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(320.0, 200.0),
                        )),
                        events: if hovered {
                            vec![egui::Event::PointerMoved(egui::pos2(40.0, 20.0))]
                        } else if state == 2 {
                            vec![
                                egui::Event::PointerGone,
                                egui::Event::Key {
                                    key: egui::Key::Tab,
                                    physical_key: None,
                                    pressed: true,
                                    repeat: false,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ]
                        } else {
                            vec![egui::Event::PointerGone]
                        },
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            press = entry_pair(ui, &ticket, ready);
                        });
                    },
                );
                assert_eq!(press, None, "painting or hovering never submits an order");
                if state == 2 {
                    let id = ctx
                        .data(|data| {
                            data.get_temp::<egui::Id>(egui::Id::new("ticket_buy_response"))
                        })
                        .unwrap();
                    assert_eq!(
                        ctx.memory(|memory| memory.has_focus(id)),
                        ready,
                        "BUY focus follows enabled state"
                    );
                }

                let shape = output
                    .shapes
                    .iter()
                    .rev()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.text().starts_with(if armed {
                                "Click a price"
                            } else {
                                "BUY"
                            }) =>
                        {
                            Some(text)
                        }
                        _ => None,
                    })
                    .expect("the complete BUY label is painted");
                let ink = shape
                    .override_text_color
                    .unwrap_or_else(|| shape.galley.rows[0].visuals.mesh.vertices[0].color);
                let fill = output
                    .shapes
                    .iter()
                    .rev()
                    .find_map(|paint| match &paint.shape {
                        egui::Shape::Rect(rect)
                            if rect.rect.contains(shape.visual_bounding_rect().center())
                                && rect.fill.a() == 255 =>
                        {
                            Some(rect.fill)
                        }
                        _ => None,
                    })
                    .expect("painted button surface");
                let a = theme::relative_luminance(ink);
                let b = theme::relative_luminance(fill);
                assert!(
                    (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                    "ready={ready} armed={armed} hovered={hovered}: {ink:?}"
                );
                assert!(shape.visual_bounding_rect().right() <= 164.0);
            }
            let mut clicked = None;
            for pressed in [true, false] {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(320.0, 200.0),
                        )),
                        events: vec![
                            egui::Event::PointerMoved(egui::pos2(40.0, 20.0)),
                            egui::Event::PointerButton {
                                pos: egui::pos2(40.0, 20.0),
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            clicked = entry_pair(ui, &ticket, ready);
                        });
                    },
                );
            }
            assert_eq!(
                clicked,
                if !ready {
                    None
                } else if armed {
                    Some(EntryPress::Disarm)
                } else {
                    Some(EntryPress::Arm(Side::Buy))
                }
            );
        }
    }
}
