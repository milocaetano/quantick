use super::*;
use eframe::egui;

fn native_frame(
    access: &mut ControlAccess,
    panel: &mut crate::surfaces::access_panel::AccessPanelSurface,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    facts: Option<Facts>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 3000.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            panel.with_updates(
                |response| assert!(response.model_effects.is_empty()),
                |_, session| {
                    session.update(Intent::Observe(
                        facts.unwrap_or_else(|| access.panel_facts()),
                    ));
                    crate::surfaces::access_panel::paint_window(access, ctx, session);
                },
            )
        },
    )
}

fn native_text(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text)
                if text.galley.text().contains(label)
                    && clipped.clip_rect.contains_rect(text.visual_bounding_rect()) =>
            {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
}

fn native_pointer(
    access: &mut ControlAccess,
    panel: &mut crate::surfaces::access_panel::AccessPanelSurface,
    ctx: &egui::Context,
    at: egui::Pos2,
    facts: Option<Facts>,
) -> egui::FullOutput {
    let events = |pressed| {
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    };
    native_frame(access, panel, ctx, events(true), facts);
    native_frame(access, panel, ctx, events(false), facts)
}

fn native_click(
    access: &mut ControlAccess,
    panel: &mut crate::surfaces::access_panel::AccessPanelSurface,
    ctx: &egui::Context,
    label: &str,
    facts: Option<Facts>,
) -> egui::FullOutput {
    let output = native_frame(access, panel, ctx, vec![], facts);
    let at =
        native_text(&output, label).unwrap_or_else(|| panic!("visible native control {label}"));
    native_pointer(access, panel, ctx, at, facts)
}

impl ControlAccess {
    /// C34's real widget path, with only the external gateway replaced by a command channel.
    pub(crate) fn assert_native_panel_widgets() {
        use super::super::{ConnectedClient, GatewayCommand, GatewayPublicInfo, GatewayRuntime};
        use quantick_control::id::{ConnectionId, InstanceId, ProfileId};
        let ctx = egui::Context::default();
        let mut access = ControlAccess::new();
        access.identity = None; // No worker or real discovery publication can start in this fixture.
        let mut panel = crate::surfaces::Surfaces::default().access_panel;
        panel.update(Intent::Open);
        native_frame(&mut access, &mut panel, &ctx, vec![], None);
        let scope = PermissionId::new("observe.chart").unwrap();
        assert!(access.configured_scopes.contains(&scope));
        native_click(&mut access, &mut panel, &ctx, "observe.chart", None);
        assert!(
            !access.configured_scopes.contains(&scope),
            "native checkbox reaches the host membership edit"
        );

        access.configured_scopes.clear();
        let generation = access.grant_generation;
        native_click(
            &mut access,
            &mut panel,
            &ctx,
            "Enable observer access",
            Some(Facts {
                identity_available: true,
                ..Facts::default()
            }),
        );
        assert!(
            !panel.model_state.facts.identity_available,
            "enable click receives the authoritative refusal"
        );
        assert!(matches!(access.state, AccessState::Disabled));
        assert_eq!(access.grant_generation, generation);

        access.state = AccessState::Enabling;
        native_click(
            &mut access,
            &mut panel,
            &ctx,
            "Cancel and keep access off",
            None,
        );
        assert_eq!(access.grant_generation, generation + 1);
        assert_eq!(panel.model_state.facts.phase, Phase::Disabling);

        let (_, requests) = crossbeam_channel::bounded(1);
        let (_, statuses) = crossbeam_channel::bounded(1);
        let (commands, received) = crossbeam_channel::bounded(2);
        access.state = AccessState::Enabled(GatewayRuntime {
            grant_generation: access.grant_generation,
            requests,
            statuses,
            commands,
            cancellation: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            public: GatewayPublicInfo {
                instance_id: InstanceId::from_bytes([1; 16]),
                port: 0,
                descriptor_path: std::path::PathBuf::new(),
                published_at_unix_ms: 0,
            },
        });
        let client = |id: ConnectionId| ConnectedClient {
            connection_id: id,
            client_name: "Native fixture".to_owned(),
            connected_at_unix_ms: 0,
            requested_profile: ProfileId::new("observer").unwrap(),
            effective_profile: ProfileId::new("observer").unwrap(),
            effective_scopes: BTreeSet::new(),
            last_request_at_unix_ms: None,
        };
        let id = ConnectionId::from_bytes([7; 16]);
        access.connections.insert(id.clone(), client(id.clone()));
        native_click(&mut access, &mut panel, &ctx, "Revoke", None);
        assert!(access.connections.is_empty());
        assert!(access.revoked_connections.contains(&id));
        assert!(
            matches!(received.try_recv().unwrap(), GatewayCommand::Revoke(found) if found == id)
        );
        assert!(
            received.try_recv().is_err(),
            "one native revoke sends one command"
        );

        let id = ConnectionId::from_bytes([8; 16]);
        access.connections.insert(id.clone(), client(id.clone()));
        let output = native_click(
            &mut access,
            &mut panel,
            &ctx,
            "Disable and revoke all clients",
            None,
        );
        assert_eq!(access.grant_generation, generation + 2);
        assert!(access.connections.is_empty());
        assert!(access.revoked_connections.contains(&id));
        assert!(matches!(
            received.try_recv().unwrap(),
            GatewayCommand::Shutdown
        ));
        assert!(received.try_recv().is_err());
        assert_eq!(panel.model_state.facts.phase, Phase::Disabling);
        assert!(
            native_text(&output, "No authenticated clients.").is_some(),
            "disable settles before the following client section paints"
        );

        let output = native_frame(&mut access, &mut panel, &ctx, vec![], None);
        let title = native_text(&output, "Local agent access").unwrap();
        let rect = ctx.memory(|memory| {
            memory
                .area_rect(egui::Id::new("control_access_panel"))
                .unwrap()
        });
        // egui's native titlebar puts the square close control on this diagonal.
        let close = egui::pos2(rect.right() - (title.y - rect.top()), title.y);
        native_pointer(&mut access, &mut panel, &ctx, close, None);
        assert!(
            !panel.model_state.open,
            "native window close updates the registered model"
        );
        assert!(
            matches!(access.state, AccessState::Disabling(_)),
            "closing presentation leaves host lifecycle alone"
        );
    }
}

#[test]
fn access_panel_permission_requests_cannot_bypass_host_state() {
    let ctx = egui::Context::default();
    let mut access = ControlAccess::new();
    let mut panel = crate::surfaces::Surfaces::default().access_panel;
    let permission = PermissionId::new("observe.chart").unwrap();
    let initial = access.configured_scopes.clone();
    let (_, requests) = crossbeam_channel::bounded(1);
    let (_, statuses) = crossbeam_channel::bounded(1);
    let (commands, received) = crossbeam_channel::bounded(2);
    let runtime = super::super::GatewayRuntime {
        grant_generation: 0,
        requests,
        statuses,
        commands,
        cancellation: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        public: super::super::GatewayPublicInfo {
            instance_id: quantick_control::id::InstanceId::from_bytes([1; 16]),
            port: 0,
            descriptor_path: std::path::PathBuf::new(),
            published_at_unix_ms: 0,
        },
    };
    panel.with_updates(
            |response| assert!(response.model_effects.is_empty()),
            |_, session| {
                for phase in [AccessState::Enabling, AccessState::Disabling(None), AccessState::Enabled(runtime)] {
                    access.state = phase;
                    // Stale presentation cannot widen or remove the host's grant.
                    session.update(Intent::Observe(Facts::default()));
                    access.panel_request(
                        &ctx,
                        session,
                        Intent::RequestPermission(permission.clone(), false),
                    );
                    assert_eq!(access.configured_scopes, initial);
                    assert_eq!(session.state().facts, access.panel_facts());
                }
                let connection = quantick_control::id::ConnectionId::from_bytes([7; 16]);
                access.panel_request(&ctx, session, Intent::RequestRevoke(connection.clone()));
                assert!(matches!(received.try_recv().unwrap(), super::super::GatewayCommand::Revoke(id) if id == connection));
                assert!(received.try_recv().is_err(), "one revoke sends one command");
                assert!(access.revoked_connections.contains(&connection));
                access.state = AccessState::Disabled;
                access.configured_scopes.clear();
                access.panel_request(
                    &ctx,
                    session,
                    Intent::RequestPermission(permission.clone(), true),
                );
                assert_eq!(
                    access.configured_scopes,
                    BTreeSet::from([permission.clone()]),
                    "checkbox edits insert no floors"
                );
                for invalid in ["observe", "unknown.permission"] {
                    access.panel_request(
                        &ctx,
                        session,
                        Intent::RequestPermission(PermissionId::new(invalid).unwrap(), true),
                    );
                    assert_eq!(
                        access.configured_scopes,
                        BTreeSet::from([permission.clone()])
                    );
                }
                access.panel_request(
                    &ctx,
                    session,
                    Intent::RequestPermission(permission.clone(), false),
                );
                assert!(access.configured_scopes.is_empty());
                access.identity = None;
                session.update(Intent::Observe(Facts {
                    identity_available: true,
                    ..Facts::default()
                }));
                access.panel_request(&ctx, session, Intent::RequestEnable);
                assert!(matches!(access.state, AccessState::Disabled));
                assert!(!session.state().facts.identity_available);
                access.state = AccessState::Enabling;
                let generation = access.grant_generation;
                access.panel_request(&ctx, session, Intent::RequestDisable);
                assert_eq!(
                    access.grant_generation,
                    generation + 1,
                    "one request executes once"
                );
                assert_eq!(session.state().facts.phase, Phase::Disabling);
                access.panel_request(&ctx, session, Intent::RequestDisable);
                assert_eq!(
                    access.grant_generation,
                    generation + 1,
                    "transition readiness remains host-owned"
                );
                let connection = quantick_control::id::ConnectionId::from_bytes([7; 16]);
                access.panel_request(&ctx, session, Intent::RequestRevoke(connection.clone()));
                assert!(access.revoked_connections.contains(&connection));
            },
        );
}

#[test]
fn permission_guidance_and_locked_endpoint_are_reachable_by_wheel() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for width in [1000.0, 1280.0] {
            let ctx = egui::Context::default();
            let mut appearance = quantick_appearance::Appearance::default();
            appearance.apply(source).unwrap();
            crate::theme::sync(&ctx, &appearance);
            let mut access = ControlAccess::new();
            let mut panel = crate::surfaces::Surfaces::default().access_panel;
            panel.update(Intent::Open);
            access.state = AccessState::Enabling;
            let original = access.configured_scopes.clone();
            let expected = [
                "Current grants are read-only",
                "Still reading only:",
                "Objects an assistant",
                "Changes which charts",
                "Disable access before changing scopes;",
                "Cancel and keep access off",
            ];
            let mut visible = [false; 6];
            for frame in 0..120 {
                let events = if frame > 5 && frame % 6 == 0 {
                    vec![
                        // The panel opens centred (`crate::popup`).
                        egui::Event::PointerMoved(egui::pos2(width / 2.0, 390.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -120.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]
                } else {
                    vec![]
                };
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 780.0),
                        )),
                        time: Some(f64::from(frame) / 60.0),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        panel.with_updates(
                            |_| {},
                            |_, session| {
                                session.update(Intent::Observe(access.panel_facts()));
                                crate::surfaces::access_panel::paint_window(
                                    &mut access,
                                    ctx,
                                    session,
                                );
                            },
                        )
                    },
                );
                for clipped in output.shapes {
                    if let egui::Shape::Text(text) = clipped.shape {
                        let bounds = text.galley.rect.translate(text.pos.to_vec2());
                        for (index, prefix) in expected.iter().enumerate() {
                            if text.galley.job.text.starts_with(prefix)
                                && clipped.clip_rect.contains_rect(bounds)
                            {
                                visible[index] = true;
                            }
                        }
                    }
                }
            }
            assert_eq!(
                visible, [true; 6],
                "every full paragraph and endpoint: {expected:?}"
            );
            assert_eq!(access.configured_scopes, original);
            assert!(matches!(access.state, AccessState::Enabling));
        }
    }
}

#[test]
fn supporting_guidance_fits_narrow_surfaces() {
    for source in [quantick_appearance::LIGHT, quantick_appearance::DARK] {
        for width in [320.0, 520.0] {
            let ctx = egui::Context::default();
            let mut appearance = quantick_appearance::Appearance::default();
            appearance.apply(source).unwrap();
            crate::theme::sync(&ctx, &appearance);
            let mut access = ControlAccess::new();
            let original = access.configured_scopes.clone();
            let mut panel = crate::surfaces::Surfaces::default().access_panel;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 3000.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    panel.with_updates(
                        |_| {},
                        |_, session| {
                            session.update(Intent::Observe(access.panel_facts()));
                            egui::CentralPanel::default()
                                .show(ctx, |ui| access.draw_panel_body(ui, session));
                        },
                    );
                },
            );
            assert_eq!(access.configured_scopes, original);
            let mut paragraphs = 0;
            for shape in output.shapes {
                if let egui::Shape::Text(text) = shape.shape
                    && [
                        "Still reading only:",
                        "Objects an assistant",
                        "Changes which charts",
                    ]
                    .iter()
                    .any(|prefix| text.galley.job.text.starts_with(prefix))
                {
                    paragraphs += 1;
                    assert!(text.pos.x + text.galley.rect.right() <= width + 0.5);
                    assert!(text.galley.job.sections[0].format.font_id.size >= 12.0);
                }
            }
            assert_eq!(paragraphs, 3);
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw_scope_section(
                        ui,
                        &access.contract,
                        &access.configured_scopes,
                        false,
                        |_| true,
                    );
                });
            });
            assert_eq!(
                access.configured_scopes, original,
                "read-only grants are immutable"
            );
        }
    }
}
