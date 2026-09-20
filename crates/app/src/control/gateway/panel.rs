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

use super::{
    AccessState, ControlAccess, MARK_SHORTCUT, is_analyst_permission, is_annotate_permission,
    is_cockpit_permission, is_trade_permission,
};
use crate::control::contract::ObserverContract;

/// How wide the access window opens before the trader resizes it.
const CONTROL_PANEL_DEFAULT_WIDTH_PX: f32 = 520.0;
/// The gap between the panel's sections.
const CONTROL_PANEL_SECTION_SPACING_PX: f32 = 6.0;
/// How much of the screen the panel's body may take before it scrolls.
const CONTROL_PANEL_MAX_SCREEN_FRACTION: f32 = 0.8;

/// One section of scope checkboxes: every selectable permission the filter
/// admits, ticked into the grant for the next connection.
///
/// A free function over the two fields it touches rather than a method,
/// because the loop reads the contract while it writes the grant, and the
/// four sections differ only in which permissions they show. One loop means
/// a section added later cannot quietly render its labels differently from
/// the three beside it.
fn draw_scope_section(
    ui: &mut eframe::egui::Ui,
    contract: &ObserverContract,
    configured_scopes: &mut BTreeSet<PermissionId>,
    can_edit: bool,
    admits: impl Fn(&PermissionId) -> bool,
) {
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
        ui.add_enabled(can_edit, eframe::egui::Checkbox::new(&mut selected, label));
        if can_edit {
            if selected {
                configured_scopes.insert(descriptor.id.clone());
            } else {
                configured_scopes.remove(&descriptor.id);
            }
        }
    }
}

impl ControlAccess {
    pub fn menu_label(&self) -> &'static str {
        match self.state {
            AccessState::Disabled => "Local agent access…",
            AccessState::Enabling => "Local agent access: enabling…",
            AccessState::Enabled(_) => "Local agent access: on",
            AccessState::Disabling(_) => "Local agent access: disabling…",
        }
    }

    pub fn draw_panel(&mut self, ctx: &eframe::egui::Context) {
        if !self.show_panel {
            return;
        }
        let mut open = self.show_panel;
        // The body is four consent sections, a button and the client list, and
        // it grew past the window the analyst tier was added to it: on a
        // 1009-point screen the enabled state reached the bottom edge with the
        // client list under it. An auto-sized window does not scroll by
        // itself, so the trader could not reach the list or the disable
        // button at all. The scroll area keeps the panel inside the screen
        // whatever tier is added next.
        // The whole window, not `available_rect`: by the time the panel draws,
        // the chart and the dock have already claimed their space, and a
        // fraction of what is left is a fraction of the wrong rectangle — it
        // made the panel a third of its own content tall.
        let max_height = ctx.screen_rect().height() * CONTROL_PANEL_MAX_SCREEN_FRACTION;
        eframe::egui::Window::new("Local agent access")
            .id(eframe::egui::Id::new("control_access_panel"))
            .open(&mut open)
            .default_width(CONTROL_PANEL_DEFAULT_WIDTH_PX)
            .resizable(true)
            // A scrolling window stops sizing itself to its content, so the
            // default height has to be asked for as well: without it egui
            // opens the panel at its own default and clips the consent text a
            // third of the way in.
            .default_height(max_height)
            .max_height(max_height)
            .vscroll(true)
            .show(ctx, |ui| self.draw_panel_body(ui));
        self.show_panel = open;
    }

    fn draw_panel_body(&mut self, ui: &mut eframe::egui::Ui) {
        ui.label(
            "Allows configured local tools such as Codex or Claude to read this already-open Quantick window. Granted data may be sent to the model provider used by that tool.",
        );
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        let status = match self.state {
            AccessState::Disabled => "Off",
            AccessState::Enabling => "Enabling…",
            AccessState::Enabled(_) if self.grants_annotate() => {
                "On — reading, and answering on the chart"
            }
            AccessState::Enabled(_) if self.grants_analyst() => {
                "On — reading only, including private session data"
            }
            AccessState::Enabled(_) => "On — reading only",
            AccessState::Disabling(_) => "Disabling and revoking clients…",
        };
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
            ui.colored_label(eframe::egui::Color32::LIGHT_RED, error);
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
        let can_edit = matches!(self.state, AccessState::Disabled);
        let Self {
            contract,
            configured_scopes,
            ..
        } = self;
        draw_scope_section(ui, contract, configured_scopes, can_edit, |permission| {
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
        ui.small(
            "Still reading only: nothing here places an object, changes a chart or touches a position. What it adds is the material the rest of the reading leaves out — your paper account, the words you wrote on the chart, redacted diagnostic logs, evidence bundles and pictures of the window. A client asks for this tier by name, and the connected-clients list below says which one it holds.",
        );
        draw_scope_section(
            ui,
            contract,
            configured_scopes,
            can_edit,
            is_analyst_permission,
        );
        // The tier that writes is a separate decision, said in the words a
        // trader would use: everything above lets an assistant *read* the
        // window; everything here lets it put something in it.
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        ui.strong("Let an assistant answer on the chart");
        ui.small(
            "Objects an assistant places are labelled with its name wherever you see them, and \"Remove objects placed for you\" in the object manager takes them all back at once. Nothing here can delete your own drawings or touch a position. Rearranging your charts is the separate grant below.",
        );
        draw_scope_section(
            ui,
            contract,
            configured_scopes,
            can_edit,
            is_annotate_permission,
        );
        // The cockpit tier, in its own section for the reason it is its own
        // tier: it is a *write* grant, and it was rendering under "Read
        // scopes" — a checkbox that rearranges the trader's window, presented
        // as though it only looked at it.
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        ui.strong("Let an assistant rearrange your charts");
        ui.small(
            "Changes which charts are on screen, where they sit and how wide they are — the same things the layout picker does. Nothing here places or removes an object, and nothing here touches a position. A chart put away keeps its drawings, its indicators and its bars, and comes back with them.",
        );
        draw_scope_section(
            ui,
            contract,
            configured_scopes,
            can_edit,
            is_cockpit_permission,
        );
        if !can_edit {
            ui.small("Disable access before changing scopes; re-enabling rotates the token and requires a new handshake.");
        }

        ui.separator();
        match self.state {
            AccessState::Disabled => {
                let label = if self.grants_cockpit() {
                    "Enable access (reading, answering and rearranging)"
                } else if self.grants_annotate() {
                    "Enable access (reading and answering)"
                } else if self.grants_analyst() {
                    "Enable analyst access (reading, including private data)"
                } else {
                    "Enable observer access"
                };
                if ui
                    .add_enabled(self.identity.is_some(), eframe::egui::Button::new(label))
                    .clicked()
                {
                    self.enable(ui.ctx());
                }
            }
            AccessState::Enabling => {
                if ui.button("Cancel and keep access off").clicked() {
                    self.request_disable();
                }
            }
            AccessState::Enabled(_) => {
                if ui.button("Disable and revoke all clients").clicked() {
                    self.request_disable();
                }
            }
            AccessState::Disabling(_) => {
                ui.spinner();
            }
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
                self.revoke(connection_id);
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
