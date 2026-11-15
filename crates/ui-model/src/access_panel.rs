//! Local access panel presentation. The host alone owns grants and runtime authority.
use quantick_control::id::{ConnectionId, PermissionId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Disabled,
    Enabling,
    Enabled,
    Disabling,
}

/// Small display facts supplied by the authoritative host, never authorization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub phase: Phase,
    pub identity_available: bool,
    pub cockpit: bool,
    pub annotate: bool,
    pub analyst: bool,
}

impl Facts {
    pub fn menu_label(self) -> &'static str {
        match self.phase {
            Phase::Disabled => "Local agent access…",
            Phase::Enabling => "Local agent access: enabling…",
            Phase::Enabled => "Local agent access: on",
            Phase::Disabling => "Local agent access: disabling…",
        }
    }

    /// Whether the Settings switch reads on and whether it appears interactive.
    pub fn switch_state(self) -> (bool, bool) {
        match self.phase {
            Phase::Disabled => (false, self.identity_available),
            Phase::Enabling | Phase::Enabled => (true, true),
            Phase::Disabling => (false, false),
        }
    }

    pub fn status(self) -> &'static str {
        match self.phase {
            Phase::Disabled => "Off",
            Phase::Enabling => "Enabling…",
            Phase::Enabled if self.cockpit => "On — reading, answering and rearranging your charts",
            Phase::Enabled if self.annotate => "On — reading, and answering on the chart",
            Phase::Enabled if self.analyst => "On — reading only, including private session data",
            Phase::Enabled => "On — reading only",
            Phase::Disabling => "Disabling and revoking clients…",
        }
    }

    /// The primary control is presentation only; clicking still requests a host check.
    pub fn primary_action(self) -> Option<PrimaryAction> {
        let (label, enabled, intent) = match self.phase {
            Phase::Disabled => (
                self.enable_label(),
                self.identity_available,
                Intent::RequestEnable,
            ),
            Phase::Enabling => ("Cancel and keep access off", true, Intent::RequestDisable),
            Phase::Enabled => (
                "Disable and revoke all clients",
                true,
                Intent::RequestDisable,
            ),
            Phase::Disabling => return None,
        };
        Some(PrimaryAction {
            label,
            enabled,
            intent,
        })
    }

    pub fn enable_label(self) -> &'static str {
        if self.cockpit {
            "Enable access (reading, answering and rearranging)"
        } else if self.annotate {
            "Enable access (reading and answering)"
        } else if self.analyst {
            "Enable analyst access (reading, including private data)"
        } else {
            "Enable observer access"
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct State {
    pub open: bool,
    pub facts: Facts,
}

#[derive(Debug)]
pub enum Intent {
    Open,
    Close,
    Observe(Facts),
    Observed(Facts),
    RequestEnable,
    RequestDisable,
    RequestRevoke(ConnectionId),
    RequestPermission(PermissionId, bool),
}

#[derive(Debug)]
pub struct PrimaryAction {
    pub label: &'static str,
    pub enabled: bool,
    pub intent: Intent,
}

/// Requests must be checked against current host state even if display facts are stale.
#[derive(Debug, PartialEq, Eq)]
pub enum LocalOperation {
    Enable,
    Disable,
    Revoke(ConnectionId),
    Permission(PermissionId, bool),
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub local_operation: Option<LocalOperation>,
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;

    fn update(state: &mut State, intent: Intent) -> Effects {
        let local_operation = match intent {
            Intent::Open => {
                state.open = true;
                None
            }
            Intent::Close => {
                state.open = false;
                None
            }
            Intent::Observe(facts) | Intent::Observed(facts) => {
                state.facts = facts;
                None
            }
            Intent::RequestEnable => Some(LocalOperation::Enable),
            Intent::RequestDisable => Some(LocalOperation::Disable),
            Intent::RequestRevoke(connection) => Some(LocalOperation::Revoke(connection)),
            Intent::RequestPermission(permission, selected) => {
                Some(LocalOperation::Permission(permission, selected))
            }
        };
        Effects { local_operation }
    }
}

#[cfg(test)]
#[path = "access_panel/tests.rs"]
mod tests;
