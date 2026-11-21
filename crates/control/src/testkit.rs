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
    pub fn request_id(mut self, value: &str) -> Self {
        self.0.request_id = RequestId::new(value).expect("test request id");
        self
    }
    #[must_use]
    pub fn requested_at(mut self, value: i64) -> Self {
        self.0.requested_at_unix_ms = value;
        self
    }
    #[must_use]
    pub fn reason(mut self, value: &str) -> Self {
        self.0.reason = Some(value.to_owned());
        self
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
    fn actor_defaults_overrides_and_repeated_builds_are_valid() {
        let builder = ActorContextBuilder::agent()
            .request_id("again-1")
            .requested_at(42);
        assert!(builder.build().validate().is_ok());
        assert_eq!(builder.build().requested_at_unix_ms, 42);
        assert_eq!(
            ActorContextBuilder::human().build().actor_kind,
            ActorKind::HumanUi
        );
    }
}
