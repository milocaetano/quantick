use super::*;
#[cfg(test)]
mod confirmation {
    use super::*;

    pub(super) fn frame(
        surface: &mut AlertSurface,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, SurfaceResponse) {
        let book = AlertBook::default();
        let context = context();
        let env = SurfaceEnv {
            alerts: Some(&book),
            alert_context: Some(&context),
            ..SurfaceEnv::quiet(std::time::Instant::now())
        };
        let mut response = SurfaceResponse::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 800.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                response = surface.draw(ctx, &env);
            },
        );
        (output, response)
    }

    pub(super) fn click(
        surface: &mut AlertSurface,
        ctx: &egui::Context,
        label: &str,
    ) -> SurfaceResponse {
        let _ = frame(surface, ctx, vec![]);
        let (output, _) = frame(surface, ctx, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().ends_with(label) => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("button {label} rendered"));
        frame(
            surface,
            ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        )
        .1
    }

    #[test]
    fn alert_delete_requires_confirmation_and_escape_keeps_draft() {
        let ctx = egui::Context::default();
        let mut surface = AlertSurface {
            open: true,
            ..Default::default()
        };
        surface.create(&context(), None);
        surface.draft.as_mut().unwrap().revision = Some(3);
        surface.draft.as_mut().unwrap().spec.id = AlertId(7);
        assert!(click(&mut surface, &ctx, "Delete").alerts.is_none());
        assert!(surface.draft.as_ref().unwrap().confirming_delete);
        let (_, response) = frame(
            &mut surface,
            &ctx,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(response.alerts.is_none());
        assert!(!surface.draft.as_ref().unwrap().confirming_delete);
        assert!(click(&mut surface, &ctx, "Delete").alerts.is_none());
        assert!(click(&mut surface, &ctx, "Keep alert").alerts.is_none());
        assert!(!surface.draft.as_ref().unwrap().confirming_delete);
        assert!(click(&mut surface, &ctx, "Delete").alerts.is_none());
        let response = click(&mut surface, &ctx, "Confirm delete");
        assert_eq!(
            response.alerts.unwrap().command,
            Some(AlertCommand::Delete {
                id: AlertId(7),
                expected_revision: 3
            })
        );
    }
}
fn context() -> Context {
    Context {
        market: MarketRef {
            source: SourceContext::Live {
                feed: "fixture".into(),
            },
            symbol: "TEST".into(),
            aggregation: "tick:100".into(),
        },
        price: Some(100.0),
        utc_ms: 1_800_000_000_000,
        display_offset_minutes: 0,
        drawings: vec![],
    }
}

#[cfg(test)]
#[test]
fn alert_target_controls_keep_operators_compatible() {
    for (kind, initial, expected) in [
        (TargetKind::Channel, Operator::Crossing, Operator::Entering),
        (
            TargetKind::Rectangle,
            Operator::Crossing,
            Operator::GreaterThan,
        ),
        (
            TargetKind::Rectangle,
            Operator::LessThan,
            Operator::LessThan,
        ),
    ] {
        let ctx = egui::Context::default();
        let selector = DrawingSelector {
            tab_id: 1,
            pane_id: 2,
            drawing_id: 3,
        };
        let target = DurableDrawingRef {
            workspace: "w".into(),
            origin: "o".into(),
            pane: "p".into(),
            drawing: 3,
            incarnation: "i".into(),
        };
        let mut context = context();
        context.drawings.push(DrawingChoice {
            selector,
            target: None,
            kind,
            name: "Region fixture".into(),
        });
        let mut surface = AlertSurface {
            open: true,
            ..Default::default()
        };
        surface.create(&context, None);
        surface.draft.as_mut().unwrap().spec.conditions[0].operator = initial;
        assert!(
            confirmation::click(&mut surface, &ctx, "Value")
                .alerts
                .is_none()
        );
        let request = confirmation::click(&mut surface, &ctx, "Region fixture")
            .alerts
            .unwrap();
        assert_eq!(request.resolve, Some(selector));
        surface.settle_target(Ok(target.clone()));
        let condition = &surface.draft.as_ref().unwrap().spec.conditions[0];
        assert_eq!(condition.target, TargetRef::Drawing(target));
        assert_eq!(condition.operator, expected);
        assert!(
            confirmation::click(&mut surface, &ctx, "Region fixture")
                .alerts
                .is_none()
        );
        assert!(
            confirmation::click(&mut surface, &ctx, "Value")
                .alerts
                .is_none()
        );
        let condition = &surface.draft.as_ref().unwrap().spec.conditions[0];
        assert!(matches!(condition.target, TargetRef::Value(_)));
        assert_eq!(
            condition.operator,
            if expected == Operator::Entering {
                Operator::Crossing
            } else {
                expected
            }
        );
        assert!(
            AlertBook::default()
                .validate(&surface.draft.as_ref().unwrap().spec)
                .is_ok()
        );
    }
}

#[cfg(test)]
#[test]
fn alert_storage_error_never_renders_an_empty_manager() {
    let ctx = egui::Context::default();
    let book = AlertBook::default();
    let mut surface = AlertSurface {
        open: true,
        ..Default::default()
    };
    for problem in [
        AlertError::UnsupportedVersion(0),
        AlertError::Invalid {
            field: "document".into(),
            reason: "malformed".into(),
        },
    ] {
        let env = SurfaceEnv {
            alerts: Some(&book),
            alert_error: Some(&problem),
            ..SurfaceEnv::quiet(std::time::Instant::now())
        };
        for _ in 0..2 {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                surface.draw(ctx, &env);
            });
        }
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            assert!(surface.draw(ctx, &env).alerts.is_none());
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text()),
                _ => None,
            })
            .collect();
        assert!(
            labels.contains(&"Alert storage is unavailable"),
            "{labels:?}"
        );
        assert!(!labels.iter().any(|text| text.starts_with("No alerts.")));
        assert!(labels.iter().any(|text| text.contains("restart Quantick")));
    }
    let env = SurfaceEnv {
        alerts: Some(&book),
        ..SurfaceEnv::quiet(std::time::Instant::now())
    };
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        surface.draw(ctx, &env);
    });
    assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text().starts_with("No alerts."))));
}
#[test]
fn alert_editor_workflows() {
    let context = context();
    let book = AlertBook::default();
    let mut surface = AlertSurface::default();
    surface.create(&context, None);
    let draft = surface.draft.as_mut().unwrap();
    draft.spec.message = "Preserve me".into();
    draft.subview = Subview::Notifications;
    draft.spec.notification.sound = Some(sound::AlertSound::Beep.token().into());
    draft.subview = Subview::Conditions;
    draft.spec.frequency = Frequency::PerBarClose;
    assert!(book.validate(&draft.spec).is_ok());
    assert_eq!(draft.spec.message, "Preserve me");
    surface.settle_command(Err("Disk is full".into()));
    assert_eq!(surface.draft.as_ref().unwrap().spec.message, "Preserve me");
    assert_eq!(surface.error.as_deref(), Some("Disk is full"));
    surface.settle_command(Ok(()));
    assert!(surface.draft.is_none());
}
#[test]
fn alert_surface_harness() {
    for dark in [true, false] {
        for height in [360.0, 800.0] {
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let context = context();
            let book = AlertBook::default();
            let env = SurfaceEnv {
                alerts: Some(&book),
                alert_context: Some(&context),
                ..SurfaceEnv::quiet(std::time::Instant::now())
            };
            let mut surface = AlertSurface {
                open: true,
                ..Default::default()
            };
            surface.create(&context, None);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640.0, height),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    assert!(surface.draw(ctx, &env).alerts.is_none());
                },
            );
            assert!(!output.shapes.is_empty());
            assert_eq!(surface.draft.as_ref().unwrap().spec.market, context.market);
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| {
                    surface.draw(ctx, &env);
                },
            );
            assert!(surface.draft.is_none());
            assert!(
                surface.open,
                "Escape returns to the manager before closing it"
            );
        }
    }
}

#[cfg(test)]
#[test]
fn alert_editor_displays_exact_threshold() {
    for value in [100.0485, 0.0000000123456789, 123456789012345.67] {
        let mut context = context();
        context.price = Some(value);
        context.market.source = SourceContext::Replay {
            recording: "C:/recordings/session/trades.csv".into(),
        };
        let book = AlertBook::default();
        let env = SurfaceEnv {
            alerts: Some(&book),
            alert_context: Some(&context),
            ..SurfaceEnv::quiet(std::time::Instant::now())
        };
        let mut surface = AlertSurface::default();
        surface.create(&context, None);
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                surface.draw_editor(ui, &book, &env, &mut SurfaceResponse::default());
            });
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text.galley.text())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            labels.contains(&format!("Value {value}").as_str()),
            "{labels:?}"
        );
        assert!(labels.contains(&"Replay · trades.csv · TEST · tick:100"));
        assert_eq!(
            surface.draft.as_ref().unwrap().spec.conditions[0].target,
            TargetRef::Value(value)
        );
    }
}

#[cfg(test)]
#[test]
fn alert_manager_actions_match_status() {
    let id = AlertId(7);
    let expected_revision = 3;
    for (status, label, expected) in [
        (
            AlertStatus::Monitoring,
            "Pause",
            Some(AlertCommand::Pause {
                id,
                expected_revision,
            }),
        ),
        (
            AlertStatus::Paused,
            "Resume",
            Some(AlertCommand::Resume {
                id,
                expected_revision,
            }),
        ),
        (
            AlertStatus::Completed,
            "Rearm",
            Some(AlertCommand::Rearm {
                id,
                expected_revision,
            }),
        ),
        (AlertStatus::Expired, "Expired", None),
    ] {
        let ctx = egui::Context::default();
        let mut actual = None;
        let mut paint = |ctx: &egui::Context| {
            egui::CentralPanel::default().show(ctx, |ui| {
                actual = manager::status_action(ui, status, id, expected_revision);
            });
        };
        let output = ctx.run(egui::RawInput::default(), &mut paint);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().ends_with(label) => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("status action is rendered");
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            &mut paint,
        );
        assert_eq!(actual, expected, "{status:?}");
    }
}
