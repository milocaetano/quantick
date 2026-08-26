//! A second implementation of [`ControlLink`], for tests.
//!
//! It applies the same routing rules as the real link — no instance is
//! `control.instance_gone`, several without a choice is
//! `control.instance_ambiguous` — and answers the observer capabilities with
//! small canned documents shaped like the real ones. Anything that is not a
//! registered read is refused with `control.permission_denied`, which is what
//! the gateway does under the observer ceiling. Tests that want the real
//! transport use a fake *gateway* instead (see the crate's integration tests).

use std::collections::BTreeSet;

use quantick_control::{
    error::{ControlError, codes},
    id::{ErrorCode, InstanceId},
    wire::{ResponseEnvelope, ResponseOutcome, WireU64},
};
use serde_json::{Value, json};

use crate::{
    link::{ControlLink, InstanceSummary, Instances},
    tools::{
        CHART_WINDOW_CAPABILITY, DESCRIBE_CAPABILITY, DIAGNOSTICS_CAPABILITY,
        EVENTS_READ_CAPABILITY, EVENTS_WAIT_CAPABILITY, SCENE_CAPABILITY, SNAPSHOT_CAPABILITY,
    },
};

/// One recorded call, so a test can assert what reached the "instance".
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedCall {
    pub instance: Option<InstanceId>,
    pub capability_id: String,
    pub capability_version: u32,
    pub payload: Value,
}

#[derive(Debug, Default)]
pub struct FakeLink {
    instances: Vec<InstanceId>,
    pub calls: Vec<RecordedCall>,
}

impl FakeLink {
    pub fn add_instance(&mut self, instance_id: InstanceId) {
        self.instances.push(instance_id);
    }

    fn route(&self, instance: Option<&InstanceId>) -> Result<InstanceId, ControlError> {
        match instance {
            Some(wanted) => self
                .instances
                .iter()
                .find(|id| *id == wanted)
                .cloned()
                .ok_or_else(|| known(codes::INSTANCE_GONE, "no such live instance", true)),
            None => match self.instances.len() {
                0 => {
                    let mut error = known(
                        codes::INSTANCE_GONE,
                        "no Quantick instance is running",
                        true,
                    );
                    error.context.next_steps =
                        vec!["Start Quantick and enable local agent access.".to_owned()];
                    Err(error)
                }
                1 => Ok(self.instances[0].clone()),
                _ => {
                    let mut error = known(
                        codes::INSTANCE_AMBIGUOUS,
                        "more than one live Quantick instance is available",
                        false,
                    );
                    error.context.details = Some(json!({
                        "instance_ids": self.instances.iter().map(ToString::to_string).collect::<Vec<_>>()
                    }));
                    Err(error)
                }
            },
        }
    }
}

impl ControlLink for FakeLink {
    fn instances(&mut self) -> Result<Instances, ControlError> {
        let instances = self
            .instances
            .iter()
            .enumerate()
            .map(|(index, id)| InstanceSummary {
                instance_id: id.clone(),
                application_version: "0.1.0-fake".to_owned(),
                application_commit: "fake".to_owned(),
                process_id: 1_000 + u32::try_from(index).unwrap_or(0),
                published_at_unix_ms: 1_700_000_000_000 + i64::try_from(index).unwrap_or(0),
            })
            .collect::<Vec<_>>();
        let next_steps = if instances.is_empty() {
            vec!["Start Quantick and enable local agent access.".to_owned()]
        } else {
            Vec::new()
        };
        Ok(Instances {
            instances,
            issues: Vec::new(),
            next_steps,
        })
    }

    fn invoke(
        &mut self,
        instance: Option<&InstanceId>,
        capability_id: &str,
        capability_version: u32,
        payload: Value,
    ) -> Result<ResponseEnvelope, ControlError> {
        let instance_id = self.route(instance)?;
        self.calls.push(RecordedCall {
            instance: instance.cloned(),
            capability_id: capability_id.to_owned(),
            capability_version,
            payload: payload.clone(),
        });
        let outcome = match capability_id {
            DESCRIBE_CAPABILITY => ResponseOutcome::Success {
                result: describe_document(&instance_id),
            },
            SNAPSHOT_CAPABILITY
            | DIAGNOSTICS_CAPABILITY
            | CHART_WINDOW_CAPABILITY
            | SCENE_CAPABILITY
            | EVENTS_READ_CAPABILITY
            | EVENTS_WAIT_CAPABILITY => ResponseOutcome::Success {
                result: json!({ "echo": payload, "capability": capability_id }),
            },
            _ => ResponseOutcome::Failure {
                error: known(
                    codes::PERMISSION_DENIED,
                    "capability is not available to the observer profile",
                    false,
                ),
            },
        };
        Ok(ResponseEnvelope {
            protocol_version: 1,
            request_id: quantick_control::id::RequestId::new("fake").expect("static id is valid"),
            instance_id,
            capture_revision: Some(WireU64::new(1)),
            module_revisions: Vec::new(),
            outcome,
            warnings: Vec::new(),
        })
    }
}

/// A describe document shaped like the real one, with two capabilities and
/// two scopes so searches have something to distinguish.
fn describe_document(instance_id: &InstanceId) -> Value {
    json!({
        "instance_id": instance_id,
        "application_version": "0.1.0-fake",
        "application_commit": "fake",
        "protocol_version": 1,
        "effective_profile": "observer",
        "effective_scopes": ["observe", "observe.chart"],
        "effective_limits": {},
        "modules": [
            { "id": "control", "title": "Control", "description": "The control plane itself" },
            { "id": "chart", "title": "Chart", "description": "Charts and bars" }
        ],
        "profiles": [],
        "permissions": [],
        "capabilities": [
            {
                "id": DESCRIBE_CAPABILITY,
                "version": 1,
                "title": "Describe",
                "description": "Report the instance",
                "module": "control",
                "effect": "observe",
                "read_only": true,
                "availability": { "status": "available" },
                "required_permissions": ["observe"]
            },
            {
                "id": CHART_WINDOW_CAPABILITY,
                "version": 1,
                "title": "Chart window",
                "description": "Read closed bars",
                "module": "chart",
                "effect": "observe",
                "read_only": true,
                "availability": { "status": "available" },
                "required_permissions": ["observe", "observe.chart"]
            }
        ],
        "snapshot_scopes": [
            { "id": "system.info", "module_id": "system", "title": "System", "description": "Build identity", "schema_version": 1, "required_permissions": ["observe"], "schema": { "type": "object" } },
            { "id": "chart.summary", "module_id": "chart", "title": "Chart summary", "description": "Panes and bars", "schema_version": 1, "required_permissions": ["observe", "observe.chart"], "schema": { "type": "object" } }
        ]
    })
}

fn known(code: &str, message: &str, retryable: bool) -> ControlError {
    ControlError::new(
        ErrorCode::new(code).expect("static error code is valid"),
        message,
        retryable,
    )
}

/// The scopes a fake observer connection would hold; handy for tests that
/// build connect options.
pub fn observer_scopes() -> BTreeSet<quantick_control::id::PermissionId> {
    ["observe", "observe.chart"]
        .into_iter()
        .map(|id| quantick_control::id::PermissionId::new(id).expect("static permission is valid"))
        .collect()
}
