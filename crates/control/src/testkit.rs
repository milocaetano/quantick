//! Valid deterministic actor contexts for control-plane tests.

use crate::id::{ConnectionId, PrincipalId, RequestId};
use crate::limits::CONTROL_RUNTIME_ID_BYTES;
use crate::wire::{ActorContext, ActorKind};

#[derive(Clone)]
pub struct ActorContextBuilder(ActorContext);

impl ActorContextBuilder {
    #[must_use]
    pub fn agent() -> Self {
        Self(ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: PrincipalId::from_bytes([1; CONTROL_RUNTIME_ID_BYTES]),
            client_name: "test agent".to_owned(),
            connection_id: ConnectionId::from_bytes([2; CONTROL_RUNTIME_ID_BYTES]),
            request_id: RequestId::new("test-1").expect("literal request id"),
            reason: None,
            requested_at_unix_ms: 0,
        })
    }

    #[must_use]
    pub fn human() -> Self {
        let mut value = Self::agent();
        value.0.actor_kind = ActorKind::HumanUi;
        value.0.client_name = "test trader".to_owned();
        value
    }

    #[must_use]
    pub fn build(&self) -> ActorContext {
        self.0.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_variants_are_valid_and_independent() {
        let builder = ActorContextBuilder::agent();
        assert!(builder.build().validate().is_ok());
        assert_eq!(
            ActorContextBuilder::human().build().actor_kind,
            ActorKind::HumanUi
        );
    }
}
