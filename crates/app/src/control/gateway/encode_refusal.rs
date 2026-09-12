//! What a client is told when the answer it is owed cannot cross the wire.
//!
//! The codec validates its own output, so an answer can be refused after the
//! work behind it is done: a result larger than the negotiated limit, or one
//! carrying a value the wire does not allow — a floating-point number, which
//! is how every version-1 layout call that answers with the arrangement
//! fails (see `control::layout::v2`). By then an action has already acted.
//! A bare "capability unavailable" reads as "nothing happened", which is the
//! one reading that sends a client to act a second time, so the refusal says
//! what the client does not know and where to look.
//!
//! Kept out of `server.rs` because that file is at the size ceiling and this
//! is the gateway's word on one situation, not connection handling.

use quantick_control::{
    codec::CodecError,
    error::{ControlError, codes},
};

use super::super::types::known_error;

/// The first step every such refusal carries. Conditional on purpose: the
/// gateway sends this without knowing the capability, and a read that was
/// refused for its size changed nothing.
const MAY_HAVE_ACTED: &str = "If this call changes state, it may already have taken effect: \
     read the state back before sending it again.";

/// The refusal for an answer `error` kept off the wire.
pub(super) fn unencodable(error: &CodecError) -> ControlError {
    let (mut refusal, newer_version) = match error {
        CodecError::PayloadTooLarge { .. }
        | CodecError::StringTooLarge { .. }
        | CodecError::JsonTooDeep { .. } => (
            known_error(
                codes::PAYLOAD_TOO_LARGE,
                "response exceeds the negotiated protocol limit",
                false,
            ),
            false,
        ),
        _ => (
            known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "response could not be encoded under the negotiated protocol rules",
                false,
            ),
            true,
        ),
    };
    refusal.context.next_steps.push(MAY_HAVE_ACTED.to_owned());
    if newer_version {
        refusal.context.next_steps.push(
            "If this capability registers a newer version, that version may answer in a \
             form the wire accepts; `control.describe` lists every registered version."
                .to_owned(),
        );
    }
    refusal
}
