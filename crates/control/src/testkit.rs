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
            client_name: "fake assistant".to_owned(),
            connection_id: ConnectionId::from_bytes([2; CONTROL_RUNTIME_ID_BYTES]),
            request_id: RequestId::new("fake-1").expect("literal request id"),
            reason: None,
            requested_at_unix_ms: 0,
        })
    }

    #[must_use]
    pub fn human() -> Self {
        let mut value = Self::agent();
        value.0.actor_kind = ActorKind::HumanUi;
        value.0.client_name = "fake trader".to_owned();
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
    fn actor_variants_preserve_fake_window_contexts() {
        let agent = ActorContextBuilder::agent().build();
        let human = ActorContextBuilder::human().build();
        assert!(agent.validate().is_ok());
        assert!(human.validate().is_ok());
        assert_eq!(agent.actor_kind, ActorKind::Agent);
        assert_eq!(agent.client_name, "fake assistant");
        assert_eq!(agent.request_id.as_str(), "fake-1");
        assert_eq!(human.actor_kind, ActorKind::HumanUi);
        assert_eq!(human.client_name, "fake trader");
        assert_eq!(human.principal_id, agent.principal_id);
        assert_eq!(human.connection_id, agent.connection_id);
        assert_eq!(human.request_id, agent.request_id);
        assert_eq!(human.reason, agent.reason);
        assert_eq!(human.requested_at_unix_ms, agent.requested_at_unix_ms);
    }
}
