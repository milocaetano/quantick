//! Chart-aware strategy conversation. Provider credentials are never part of this contract.
use crate::readback::{Readback, journal};
use quantick_control::{
    id::{AvailabilityReasonId, ModuleId, PermissionId},
    registry::{
        Availability, AvailabilityStatus, CapabilityDescriptor, EffectPersistence,
        IdempotencyPolicy,
    },
    schema::generated_schema,
    wire::WireU64,
};
use quantick_control_host::authority::{
    ReadSpec, STRATEGY_DESCRIBE_PERMISSION_ID, STRATEGY_MODULE_ID, read_descriptor,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const OPEN_CAPABILITY_ID: &str = "strategy.describe.open";
pub const READ_CAPABILITY_ID: &str = "strategy.describe.read";
pub const SUBMIT_CAPABILITY_ID: &str = "strategy.describe.submit";
pub const CANCEL_CAPABILITY_ID: &str = "strategy.describe.cancel";
pub const SAVE_CAPABILITY_ID: &str = "strategy.describe.save";
pub const EVENT: &str = "strategy.describe.changed";
const UNAVAILABLE_REASON: &str = "strategy.describe_unavailable";
const UNAVAILABLE_NEXT_STEP: &str = "Read strategy.describe.read for conversation status.";

pub fn unavailable_error() -> ControlError {
    let mut error = ControlError::new(
        quantick_control::id::ErrorCode::new(
            quantick_control::error::codes::CAPABILITY_UNAVAILABLE,
        )
        .expect("static error code"),
        "Strategy description is unavailable",
        false,
    );
    error.context.details = Some(serde_json::json!({"reason_code": UNAVAILABLE_REASON}));
    error.context.next_steps.push(UNAVAILABLE_NEXT_STEP.into());
    error
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub revision: WireU64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Submit {
    pub revision: WireU64,
    #[schemars(length(min = 1, max = 4096))]
    pub text: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Save {
    pub revision: WireU64,
    #[schemars(length(min = 1, max = 128))]
    pub id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct Marker {
    pub slot: usize,
    pub close_time: i64,
    pub side: crate::trade::ActionSide,
}
#[derive(Serialize, JsonSchema)]
pub struct Turn {
    pub author: String,
    pub text: String,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Unavailable,
    Closed,
    Ready,
    Waiting,
    Question,
    Preview,
    Saved,
    Cancelled,
    Failed,
}

#[derive(Serialize, JsonSchema)]
pub struct Status {
    pub revision: WireU64,
    pub phase: Phase,
    pub message: String,
    #[schemars(length(max = 16))]
    pub transcript: Vec<Turn>,
    pub summary: Option<String>,
    pub source: Option<String>,
    #[schemars(length(max = 128))]
    pub markers: Vec<Marker>,
    pub saved_id: Option<String>,
}

pub fn descriptor(id: &str, mutations_available: bool) -> CapabilityDescriptor {
    let (title, description, schema, durable) = match id {
        OPEN_CAPABILITY_ID => (
            "Open strategy description",
            "Start a conversation pinned to the focused chart.",
            generated_schema::<Empty>(),
            false,
        ),
        SUBMIT_CAPABILITY_ID => (
            "Submit a strategy description",
            "Send a sentence or clarification and pinned chart context to the configured provider.",
            generated_schema::<Submit>(),
            false,
        ),
        CANCEL_CAPABILITY_ID => (
            "Cancel strategy description",
            "Invalidate pending work for the named conversation revision.",
            generated_schema::<Revision>(),
            false,
        ),
        SAVE_CAPABILITY_ID => (
            "Save described strategy",
            "Create a new strategy source and publish its prepared alarm-only instances.",
            generated_schema::<Save>(),
            true,
        ),
        READ_CAPABILITY_ID => {
            return read_descriptor::<Empty, Status>(&ReadSpec {
                id: READ_CAPABILITY_ID,
                module: STRATEGY_MODULE_ID,
                title: "Read strategy description",
                description: "Read conversation progress, preview markers and saved identity.",
                permissions: &["observe", "observe.user_text"],
                pagination: None,
            });
        }
        _ => unreachable!("unknown strategy description operation"),
    };
    let mut desc = crate::script::script_descriptor(
        id,
        title,
        description,
        schema,
        generated_schema::<Status>(),
    );
    desc.module = ModuleId::new(STRATEGY_MODULE_ID).expect("static module id");
    if !mutations_available {
        desc.availability = Availability {
            status: AvailabilityStatus::Unavailable,
            reason_code: Some(
                AvailabilityReasonId::new(UNAVAILABLE_REASON).expect("static reason"),
            ),
            next_step: Some(UNAVAILABLE_NEXT_STEP.into()),
        };
    }
    desc.persistence = if durable {
        EffectPersistence::Durable
    } else {
        EffectPersistence::Transient
    };
    if id == SUBMIT_CAPABILITY_ID {
        use quantick_control_host::authority::{
            STRATEGY_PROVIDER_EFFECT_ID, STRATEGY_PROVIDER_RISK,
        };
        desc.effect = quantick_control::id::EffectId::new(STRATEGY_PROVIDER_EFFECT_ID)
            .expect("static effect id");
        desc.risk_flags.insert(
            quantick_control::id::RiskFlagId::new(STRATEGY_PROVIDER_RISK)
                .expect("static risk flag"),
        );
        desc.reversible = false;
    }
    desc.idempotency = IdempotencyPolicy::Required;
    desc.expected_cost.max_response_bytes =
        Some(quantick_control::limits::CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES);
    desc.stale_input_safety = Some("A request key prevents duplicate work. Submit, cancel and save also name the pinned revision.".into());
    desc.required_permissions.extend(
        ["observe.user_text", STRATEGY_DESCRIBE_PERMISSION_ID]
            .map(|id| PermissionId::new(id).expect("static permission")),
    );
    desc
}

const fn readback(capability: &'static str) -> Readback {
    journal(
        capability,
        IdempotencyPolicy::Required,
        EVENT,
        "payload.operation, payload.revision",
        "the event after the pre-call cursor names the operation and resulting revision",
        &["registered_contracts_are_inert"],
    )
}
pub const READBACKS: &[Readback] = &[
    readback(OPEN_CAPABILITY_ID),
    readback(SUBMIT_CAPABILITY_ID),
    readback(CANCEL_CAPABILITY_ID),
    readback(SAVE_CAPABILITY_ID),
];

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "strategy-describe-empty-input-v1.schema.json",
            generated_schema::<Empty>(),
        ),
        (
            "strategy-describe-submit-input-v1.schema.json",
            generated_schema::<Submit>(),
        ),
        (
            "strategy-describe-cancel-input-v1.schema.json",
            generated_schema::<Revision>(),
        ),
        (
            "strategy-describe-save-input-v1.schema.json",
            generated_schema::<Save>(),
        ),
        (
            "strategy-describe-result-v1.schema.json",
            generated_schema::<Status>(),
        ),
    ]
}

use quantick_control::error::ControlError;
use serde::de::DeserializeOwned;
use serde_json::Value;
pub const MAX_TEXT_BYTES: usize = 4096;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Open,
    Read,
    Submit { revision: u64, text: String },
    Cancel { revision: u64 },
    Save { revision: u64, id: String },
}

fn parse<T: DeserializeOwned>(input: &Value) -> Result<T, ControlError> {
    serde_json::from_value(input.clone())
        .map_err(|_| ControlError::invalid_request("Invalid strategy description input"))
}
pub fn command(id: &str, input: &Value) -> Result<Command, ControlError> {
    match id {
        OPEN_CAPABILITY_ID | READ_CAPABILITY_ID => {
            let _: Empty = parse(input)?;
            Ok(if id == OPEN_CAPABILITY_ID {
                Command::Open
            } else {
                Command::Read
            })
        }
        SUBMIT_CAPABILITY_ID => {
            let Submit { revision, text } = parse(input)?;
            if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
                return Err(ControlError::invalid_request(
                    "Message exceeds the size limit or is blank",
                ));
            }
            Ok(Command::Submit {
                revision: revision.get(),
                text,
            })
        }
        SAVE_CAPABILITY_ID => {
            let Save { revision, id } = parse(input)?;
            if id.trim().is_empty() || id.len() > crate::script_strategy::SCRIPT_NAME_MAX_BYTES {
                return Err(ControlError::invalid_request("Invalid strategy file name"));
            }
            Ok(Command::Save {
                revision: revision.get(),
                id,
            })
        }
        CANCEL_CAPABILITY_ID => Ok(Command::Cancel {
            revision: parse::<Revision>(input)?.revision.get(),
        }),
        _ => Err(ControlError::invalid_request(
            "Unknown strategy description operation",
        )),
    }
}
