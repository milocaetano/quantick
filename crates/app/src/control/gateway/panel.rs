//! The access panel: the menu entry, the window, and everything drawn in it.
//!
//! Split from `gateway.rs` because it is the one part of the control host that
//! talks to `egui` at all. The host decides what a client may do; this file
//! only shows the trader what has been decided and offers the two buttons that
//! change it. Nothing here reaches a socket, and nothing on the gateway thread
//! reaches here.

use quantick_control::descriptor::INSTANCE_DESCRIPTOR_HOST;
use quantick_control::limits::CONTROL_UI_BUDGET_US;

use std::collections::BTreeSet;

use quantick_control::id::PermissionId;
use quantick_ui_model::UpdateSession;
use quantick_ui_model::access_panel::{Effects, Facts, Intent, LocalOperation, Model, Phase};

use super::{
    AccessState, ControlAccess, MARK_SHORTCUT, is_analyst_permission, is_annotate_permission,
    is_cockpit_permission, is_trade_permission,
};
use crate::control::contract::ObserverContract;

/// The gap between the panel's sections.
const CONTROL_PANEL_SECTION_SPACING_PX: f32 = 6.0;

/// Paint one borrowed permission section and return only changed checkbox requests.
/// The caller releases these borrows before executing each request.
fn draw_scope_section(
    ui: &mut eframe::egui::Ui,
    contract: &ObserverContract,
    configured_scopes: &BTreeSet<PermissionId>,
    can_edit: bool,
    admits: impl Fn(&PermissionId) -> bool,
) -> Vec<Intent> {
    let mut requests = Vec::new();
    for descriptor in contract
        .selectable_permissions()
        .filter(|descriptor| admits(&descriptor.id))
    {
        let mut selected = configured_scopes.contains(&descriptor.id);
        // The description is the label — a first-week user reads "Chart
        // framing, viewport, and bars", not `observe.chart` — and the ID
        // stays beside it because it is what a client asks for by name.
        let label = if descriptor.sensitive {
            format!("{} · {} (sensitive)", descriptor.description, descriptor.id)
        } else {
            format!("{} · {}", descriptor.description, descriptor.id)
        };
        if can_edit || crate::theme::get(ui.ctx()).resolved.dark {
            ui.add_enabled(can_edit, eframe::egui::Checkbox::new(&mut selected, label));
        } else {
            ui.horizontal_wrapped(|ui| {
                let response =
                    ui.add_enabled(false, eframe::egui::Checkbox::without_text(&mut selected));
                response.widget_info(|| {
                    eframe::egui::WidgetInfo::selected(
                        eframe::egui::WidgetType::Checkbox,
                        false,
                        selected,
                        &label,
                    )
                });
                crate::theme::supporting_label(ui, label);
            });
        }
        if can_edit && selected != configured_scopes.contains(&descriptor.id) {
            requests.push(Intent::RequestPermission(descriptor.id.clone(), selected));
        }
    }
    requests
}

impl ControlAccess {
    pub(crate) fn panel_facts(&self) -> Facts {
        Facts {
            phase: match self.state {
                AccessState::Disabled => Phase::Disabled,
                AccessState::Enabling => Phase::Enabling,
                AccessState::Enabled(_) => Phase::Enabled,
                AccessState::Disabling(_) => Phase::Disabling,
            },
            identity_available: self.identity.is_some(),
            cockpit: self.grants_cockpit(),
            annotate: self.grants_annotate(),
            analyst: self.grants_analyst(),
        }
    }

    pub fn menu_label(&self) -> &'static str {
        self.panel_facts().menu_label()
    }

    pub fn switch_state(&self) -> (bool, bool) {
        self.panel_facts().switch_state()
    }

    /// A checkbox requests precisely one membership edit; launch configuration
    /// keeps its separate normalization rules and permission floors.
    fn request_permission(&mut self, permission: PermissionId, selected: bool) {
        if !matches!(self.state, AccessState::Disabled)
            || !self
                .contract
                .selectable_permissions()
                .any(|item| item.id == permission)
        {
            return;
        }
        if selected {
            self.configured_scopes.insert(permission);
        } else {
            self.configured_scopes.remove(&permission);
        }
    }

    pub(crate) fn panel_request(
        &mut self,
        ctx: &eframe::egui::Context,
        session: &mut UpdateSession<'_, Model>,
        intent: Intent,
    ) {
        session.with_local_effects(
            &mut |mut effects: Effects| {
                let receipt = effects.local_operation.take().map(|operation| {
                    match operation {
                        LocalOperation::Enable => self.set_switch(ctx, true),
                        LocalOperation::Disable => self.set_switch(ctx, false),
                        LocalOperation::Revoke(id) => self.revoke(id),
                        LocalOperation::Permission(id, selected) => {
                            self.request_permission(id, selected)
                        }
                    }
                    Intent::Observed(self.panel_facts())
                });
                (effects, receipt)
            },
            |session| session.update(intent),
        );
    }

    fn scope_section(
        &mut self,
        ui: &mut eframe::egui::Ui,
        session: &mut UpdateSession<'_, Model>,
        admits: impl Fn(&PermissionId) -> bool,
    ) {
        let requests = draw_scope_section(
            ui,
            &self.contract,
            &self.configured_scopes,
            session.state().facts.phase == Phase::Disabled,
            admits,
        );
        for request in requests {
            self.panel_request(ui.ctx(), session, request);
        }
    }

    /// Why access cannot start, which the Settings row shows as a problem.
    pub fn start_error(&self) -> Option<&str> {
        self.initialization_error.as_deref()
    }

    /// What the last transition reported, which the Settings row shows plainly,
    /// since the panel that also shows it may be closed.
    pub fn status_note(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The same two acts as the panel's own buttons; the scopes stay the ones
    /// the panel shows.
    pub fn set_switch(&mut self, ctx: &eframe::egui::Context, on: bool) {
        match (on, &self.state) {
            (true, AccessState::Disabled) if self.identity.is_some() => self.enable(ctx),
            (false, AccessState::Enabling | AccessState::Enabled(_)) => self.request_disable(),
            _ => {}
        }
    }

    pub(crate) fn draw_panel_body(
        &mut self,
        ui: &mut eframe::egui::Ui,
        session: &mut UpdateSession<'_, Model>,
    ) {
        ui.label(
            "Allows configured local tools such as Codex or Claude to read this already-open Quantick window. Granted data may be sent to the model provider used by that tool.",
        );
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        let status = session.state().facts.status();
        ui.horizontal(|ui| {
            ui.strong("Status:");
            ui.label(status);
        });
        // The one trader gesture this surface owns, said where the trader
        // configures it: a newcomer finds it here, not in a manual.
        ui.small(format!(
            "{} marks what is under the pointer; clients read marks through events.",
            ui.ctx().format_shortcut(&MARK_SHORTCUT)
        ));

        if let Some(error) = &self.initialization_error {
            ui.colored_label(crate::theme::get(ui.ctx()).error_text(), error);
        }
        if let Some(notice) = &self.notice {
            ui.label(notice);
        }
        if let AccessState::Enabled(runtime) = &self.state {
            ui.monospace(format!(
                "{}:{} · instance {}",
                INSTANCE_DESCRIPTOR_HOST, runtime.public.port, runtime.public.instance_id
            ));
            ui.small(format!(
                "Published at {} · descriptor {}",
                runtime.public.published_at_unix_ms,
                runtime.public.descriptor_path.display()
            ));
        }

        ui.separator();
        ui.strong("Read scopes for the next connection");
        let can_edit = session.state().facts.phase == Phase::Disabled;
        if !can_edit {
            crate::theme::supporting_label(
                ui,
                "Current grants are read-only. Disable access before changing scopes.",
            );
        }
        self.scope_section(ui, session, |permission| {
            !is_annotate_permission(permission)
                && !is_cockpit_permission(permission)
                // The private reads have their own section below, for the
                // reason the write tiers do: this heading promises the
                // ordinary reading of a chart, and the paper account is not
                // that.
                && !is_analyst_permission(permission)
                // The trade tier is not offered here at all. It is not a
                // read scope — this heading promises reading only, and the
                // cockpit section exists because that same mistake was made
                // once already — and it is not offered anywhere else either,
                // because nothing can currently grant it: no profile the
                // handshake reaches holds it, so a checkbox would take the
                // trader's tick and change nothing. An honest absence beats
                // a control that lies about what it does. The section that
                // grants it belongs to the change that decides some
                // connection may trade.
                && !is_trade_permission(permission)
        });
        // The analyst tier: still reading, and a different decision. Every
        // scope here is private in a way the ones above are not — what the
        // trader wrote, what their account is doing, what the window looked
        // like — so it is asked for in its own words rather than mixed into
        // a list where a tick on "Chart framing" sits beside a tick on the
        // paper position.
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        ui.strong("Let an assistant read your private session data");
        crate::theme::supporting_label(
            ui,
            "Still reading only: nothing here places an object, changes a chart or touches a position. What it adds is the material the rest of the reading leaves out — your paper account, the words you wrote on the chart, redacted diagnostic logs, evidence bundles and pictures of the window. A client asks for this tier by name, and the connected-clients list below says which one it holds.",
        );
        self.scope_section(ui, session, is_analyst_permission);
        // The tier that writes is a separate decision, said in the words a
        // trader would use: everything above lets an assistant *read* the
        // window; everything here lets it put something in it.
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        ui.strong("Let an assistant answer on the chart");
        crate::theme::supporting_label(
            ui,
            "Objects an assistant places are labelled with its name wherever you see them, and \"Remove objects placed for you\" in the object manager takes them all back at once. Nothing here can delete your own drawings or touch a position. Rearranging your charts is the separate grant below.",
        );
        self.scope_section(ui, session, is_annotate_permission);
        // The cockpit tier, in its own section for the reason it is its own
        // tier: it is a *write* grant, and it was rendering under "Read
        // scopes" — a checkbox that rearranges the trader's window, presented
        // as though it only looked at it.
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        ui.strong("Let an assistant rearrange your charts");
        crate::theme::supporting_label(
            ui,
            "Changes which charts are on screen, where they sit and how wide they are — the same things the layout picker does. Nothing here places or removes an object, and nothing here touches a position. A chart put away keeps its drawings, its indicators and its bars, and comes back with them.",
        );
        self.scope_section(ui, session, is_cockpit_permission);
        if !can_edit {
            crate::theme::supporting_label(
                ui,
                "Disable access before changing scopes; re-enabling rotates the token and requires a new handshake.",
            );
        }

        ui.separator();
        if let Some(action) = session.state().facts.primary_action() {
            if ui
                .add_enabled(action.enabled, eframe::egui::Button::new(action.label))
                .clicked()
            {
                self.panel_request(ui.ctx(), session, action.intent);
            }
        } else {
            ui.spinner();
        }

        ui.separator();
        ui.strong(format!("Connected clients ({})", self.connections.len()));
        if self.connections.is_empty() {
            ui.label("No authenticated clients.");
        } else {
            let mut revoke = None;
            for client in self.connections.values() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&client.client_name);
                        if ui.button("Revoke").clicked() {
                            revoke = Some(client.connection_id.clone());
                        }
                    });
                    ui.small(format!(
                        "requested {} · effective {} · connected {} · last request {}",
                        client.requested_profile,
                        client.effective_profile,
                        client.connected_at_unix_ms,
                        client
                            .last_request_at_unix_ms
                            .map_or_else(|| "none".to_owned(), |at| at.to_string())
                    ));
                    ui.small(format!(
                        "scopes: {}",
                        client
                            .effective_scopes
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                });
            }
            if let Some(connection_id) = revoke {
                self.panel_request(ui.ctx(), session, Intent::RequestRevoke(connection_id));
            }
        }

        ui.separator();
        ui.small(format!(
            "Last UI drain: {} request(s), {} µs, budget {} µs{}",
            self.last_drain.processed,
            self.last_drain.elapsed_us,
            CONTROL_UI_BUDGET_US,
            match (self.last_drain.budget_exceeded, self.last_drain.processed) {
                (true, 0) => " (budget spent before any request ran)",
                (true, _) => " (budget exceeded by one non-preemptible capture)",
                (false, _) => "",
            }
        ));
        let projection = self.projections.performance();
        ui.small(format!(
            "Projection captures: {} · last {} µs · worst {} µs · budget violations {}",
            projection.captures,
            projection.last_capture_us,
            projection.worst_capture_us,
            projection.budget_violations
        ));
    }
}

#[cfg(test)]
mod readability_tests {
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
            use super::super::{
                ConnectedClient, GatewayCommand, GatewayPublicInfo, GatewayRuntime,
            };
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
}
