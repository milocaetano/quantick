//! An indicator written in conversation: compile Quantick Pine, read the
//! diagnostics as data, attach what compiled, detach it again.
//!
//! This is the closed loop the plan calls the highest-value capability on its
//! list (§PR 5b), and it is cheap because both halves already exist:
//! `quantick_pine::compile` returns `Vec<PineError>` with a stable code, a
//! byte span, a message and notes, and the indicator host is headless. What
//! this module adds is refusing to render them into a string first — an agent
//! that has to parse "line 4, column 12: …" back out of prose cannot fix its
//! own script reliably, and a rendered error is exactly the pixels-instead-of-
//! data failure the control plane exists to end.

pub(crate) use quantick_control_schema::script::*;

use quantick_control::{
    error::ControlError,
    id::{EventKind, ModuleId},
    registry::RegistryError,
    wire::ActorContext,
};

use serde::Serialize;

use serde_json::json;

use crate::metrics;

use super::{
    actions::ActionRegistry,
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
};

pub(crate) use super::types::known_error;

/// The module both script capabilities belong to — the same module the
/// indicator scopes will register under, because a capability belongs to the
/// module its ID names (contract §5).
pub(crate) use quantick_control_host::authority::SCRIPT_MODULE_ID;

/// Dock the script actions.
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(
        attach_descriptor(),
        crate::app::indicator_control::attach_script,
    )?;
    registry.register(
        detach_descriptor(),
        crate::app::indicator_control::detach_script,
    )?;
    Ok(())
}

pub(crate) fn journal_script<T: Serialize>(
    access: &mut ControlAccess,
    actor: &ActorContext,
    kind: &str,
    payload: &T,
) -> Result<(), ControlError> {
    let event_actor = EventActor {
        kind: actor.actor_kind,
        client_name: actor.client_name.clone(),
    };
    let payload = serde_json::to_value(payload)
        .map_err(|error| ControlError::invalid_request(format!("script event: {error}")))?;
    access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(SCRIPT_MODULE_ID).expect("static module ID is valid"),
            kind: EventKind::new(kind).expect("static event kind is valid"),
            actor: Some(event_actor),
            payload: json!({ "script": payload }),
        },
        metrics::wall_clock_ms(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A script whose last character is multi-byte and which does not end in a
    /// newline puts the lexer's end-of-line span *inside* that character:
    /// `end_logical_line` emits it at `pos - 1`, and `pos` is the length in
    /// bytes. Reporting that position must answer with a diagnostic, not take
    /// the application thread down with it.
    #[test]
    fn a_span_inside_a_character_reports_a_position() {
        let source = "x = // ré";
        let offset = source.len() - 1;
        assert!(!source.is_char_boundary(offset), "the case under test");
        let error = quantick_pine::PineError::new(
            quantick_pine::ErrorCode::PineSyntax,
            quantick_pine::Span::at(offset),
            "interior character span",
        );
        let reported = compile_error(&[error], source);
        let details = reported.context.details.unwrap();
        let diagnostic = &details["diagnostics"][0];
        assert_eq!(
            (
                diagnostic["line"].as_u64().unwrap(),
                diagnostic["column"].as_u64().unwrap()
            ),
            (1, 9),
        );
    }
}
