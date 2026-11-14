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
mod readability_tests;
