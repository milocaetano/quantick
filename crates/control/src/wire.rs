//! Versioned transport-neutral request, response, and actor DTOs.

use std::{collections::BTreeSet, fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;

use crate::{
    canonical::validate_control_value,
    error::ControlError,
    id::{
        CapabilityId, ConnectionId, IdempotencyKey, InstanceId, ModuleId, PrincipalId, RequestId,
    },
    limits::{CONTROL_CLIENT_NAME_MAX_BYTES, CONTROL_REASON_MAX_BYTES},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WireValueError(&'static str);

impl fmt::Display for WireValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for WireValueError {}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, JsonSchema)]
pub struct WireU64(#[schemars(with = "String", regex(pattern = r"^(0|[1-9][0-9]*)$"))] pub u64);

impl WireU64 {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WireU64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl Serialize for WireU64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl FromStr for WireU64 {
    type Err = WireValueError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(WireValueError("invalid canonical unsigned integer"));
        }
        value
            .parse::<u64>()
            .map(Self)
            .map_err(|_| WireValueError("unsigned integer is outside the u64 range"))
    }
}

impl<'de> Deserialize<'de> for WireU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct CanonicalDecimal(
    #[schemars(regex(pattern = r"^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$"))] String,
);

impl CanonicalDecimal {
    pub fn new(value: impl Into<String>) -> Result<Self, WireValueError> {
        let value = value.into();
        validate_decimal(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CanonicalDecimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

fn validate_decimal(value: &str) -> Result<(), WireValueError> {
    if value.is_empty() || value == "-0" || value.starts_with('+') || value.contains(['e', 'E']) {
        return Err(WireValueError("invalid canonical decimal"));
    }
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let (integer, fraction) = match unsigned.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (unsigned, None),
    };
    if integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || (integer.len() > 1 && integer.starts_with('0'))
    {
        return Err(WireValueError("invalid canonical decimal"));
    }
    if let Some(fraction) = fraction
        && (fraction.is_empty()
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            || fraction.ends_with('0'))
    {
        return Err(WireValueError("invalid canonical decimal"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ModuleRevision {
    pub module_id: ModuleId,
    pub revision: WireU64,
}

/// Field names a client may never supply on a request: the actor context is
/// assigned by the gateway after authentication, never accepted from the wire.
/// The codec refuses a request carrying any of them, and the published schema
/// forbids the same names (see [`reject_reserved_actor_fields`]), so a client
/// generated from the contract cannot build a request the host will refuse.
pub const RESERVED_ACTOR_FIELDS: &[&str] = &[
    "actor",
    "actor_context",
    "actor_kind",
    "client_name",
    "connection_id",
    "principal_id",
    "requested_at_unix_ms",
];

/// Make the generated request schema refuse exactly what the codec refuses.
///
/// Denying every unknown field would be the shorter spelling, and it was the
/// first one tried. The contract rules it out: reserved actor fields are
/// rejected, and *other* additive fields follow the compatibility rule, so a
/// host must stay a tolerant reader of an envelope a newer client may extend.
/// A `not`/`anyOf`/`required` clause forbids the reserved names alone and
/// leaves the envelope open to additive evolution.
fn reject_reserved_actor_fields(schema: &mut schemars::Schema) {
    let forbidden = RESERVED_ACTOR_FIELDS
        .iter()
        .map(|field| serde_json::json!({ "required": [field] }))
        .collect::<Vec<_>>();
    schema.insert("not".to_owned(), serde_json::json!({ "anyOf": forbidden }));
}

/// A request as it arrives from a client.
///
/// Unknown fields other than the reserved actor names are ignored, as the
/// contract's tolerant-reader rule requires of every wire DTO.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(transform = reject_reserved_actor_fields)]
pub struct RequestEnvelope {
    #[schemars(range(min = 1))]
    pub protocol_version: u32,
    pub request_id: RequestId,
    pub instance_id: InstanceId,
    pub capability_id: CapabilityId,
    #[schemars(range(min = 1))]
    pub capability_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_revisions: Vec<ModuleRevision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<IdempotencyKey>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = CONTROL_REASON_MAX_BYTES))]
    pub reason: Option<String>,
    /// Capability input. Floating-point numbers are rejected: exact decimals
    /// cross this boundary as strings, so a price never arrives as an f64.
    /// JSON Schema cannot express that over arbitrary nesting, so the schema
    /// states it and `canonical::validate_control_value` enforces it.
    pub payload: Value,
}

const fn is_false(value: &bool) -> bool {
    !*value
}

impl RequestEnvelope {
    pub fn validate(&self) -> Result<(), ControlError> {
        if self.protocol_version == 0 || self.capability_version == 0 {
            return Err(ControlError::invalid_request(
                "protocol and capability versions must be positive",
            ));
        }
        if self
            .reason
            .as_ref()
            .is_some_and(|reason| reason.len() > CONTROL_REASON_MAX_BYTES)
        {
            return Err(ControlError::invalid_request(
                "reason exceeds its byte limit",
            ));
        }
        validate_revisions(&self.expected_revisions)?;
        validate_control_json("request payload", &self.payload)
    }
}

fn validate_revisions(revisions: &[ModuleRevision]) -> Result<(), ControlError> {
    let mut modules = BTreeSet::new();
    for revision in revisions {
        if !modules.insert(&revision.module_id) {
            return Err(ControlError::invalid_request(
                "module revision list contains a duplicate module",
            ));
        }
    }
    Ok(())
}

fn validate_sorted_revisions(revisions: &[ModuleRevision]) -> Result<(), ControlError> {
    validate_revisions(revisions)?;
    if revisions
        .windows(2)
        .any(|pair| pair[0].module_id > pair[1].module_id)
    {
        return Err(ControlError::invalid_request(
            "response module revisions are not deterministically sorted",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ResponseOutcome {
    Success { result: Value },
    Failure { error: ControlError },
}

impl<'de> Deserialize<'de> for ResponseOutcome {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut object = serde_json::Map::<String, Value>::deserialize(deserializer)?;
        let result = object.remove("result");
        let error = object.remove("error");
        match (result, error) {
            (Some(result), None) => Ok(Self::Success { result }),
            (None, Some(error)) => serde_json::from_value(error)
                .map(|error| Self::Failure { error })
                .map_err(de::Error::custom),
            (Some(_), Some(_)) => Err(de::Error::custom(
                "response must contain exactly one of result or error",
            )),
            (None, None) => Err(de::Error::custom(
                "response must contain either result or error",
            )),
        }
    }
}

impl JsonSchema for ResponseOutcome {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ResponseOutcome".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let error_schema = generator.subschema_for::<ControlError>();
        schemars::json_schema!({
            "properties": {
                "result": true,
                "error": error_schema
            },
            "oneOf": [
                {
                    "required": ["result"],
                    "not": {"required": ["error"]}
                },
                {
                    "required": ["error"],
                    "not": {"required": ["result"]}
                }
            ]
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseEnvelope {
    #[schemars(range(min = 1))]
    pub protocol_version: u32,
    pub request_id: RequestId,
    pub instance_id: InstanceId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_revision: Option<WireU64>,
    pub module_revisions: Vec<ModuleRevision>,
    #[serde(flatten)]
    pub outcome: ResponseOutcome,
    pub warnings: Vec<ControlWarning>,
}

impl ResponseEnvelope {
    pub fn validate(&self) -> Result<(), ControlError> {
        if self.protocol_version == 0 {
            return Err(ControlError::invalid_request(
                "protocol version must be positive",
            ));
        }
        validate_sorted_revisions(&self.module_revisions)?;
        match &self.outcome {
            ResponseOutcome::Success { result } => validate_control_json("response result", result),
            ResponseOutcome::Failure { error } => {
                validate_sorted_revisions(&error.context.current_revisions)?;
                if let Some(details) = &error.context.details {
                    validate_control_json("error details", details)?;
                }
                Ok(())
            }
        }
    }

    pub fn validate_for(&self, request: &RequestEnvelope) -> Result<(), ControlError> {
        self.validate()?;
        if self.protocol_version != request.protocol_version
            || self.request_id != request.request_id
            || self.instance_id != request.instance_id
        {
            return Err(ControlError::invalid_request(
                "response does not correlate with its request",
            ));
        }
        Ok(())
    }
}

fn validate_control_json(kind: &str, value: &Value) -> Result<(), ControlError> {
    validate_control_value(value).map_err(|error| {
        ControlError::invalid_request(format!("{kind} is not valid control JSON: {error}"))
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ControlWarning {
    pub code: crate::id::ErrorCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    HumanUi,
    Automation,
    Agent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActorContext {
    pub actor_kind: ActorKind,
    pub principal_id: PrincipalId,
    #[schemars(
        length(min = 1, max = CONTROL_CLIENT_NAME_MAX_BYTES),
        regex(pattern = r"^[^\x00-\x1f\x7f]+$")
    )]
    pub client_name: String,
    pub connection_id: ConnectionId,
    pub request_id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = CONTROL_REASON_MAX_BYTES))]
    pub reason: Option<String>,
    pub requested_at_unix_ms: i64,
}

impl ActorContext {
    pub fn validate(&self) -> Result<(), ControlError> {
        if self.client_name.trim().is_empty()
            || self.client_name.len() > CONTROL_CLIENT_NAME_MAX_BYTES
            || self.client_name.chars().any(char::is_control)
        {
            return Err(ControlError::invalid_request(
                "actor client name is invalid",
            ));
        }
        if self
            .reason
            .as_ref()
            .is_some_and(|reason| reason.len() > CONTROL_REASON_MAX_BYTES)
        {
            return Err(ControlError::invalid_request(
                "actor reason exceeds its byte limit",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuthorizedRequest {
    pub envelope: RequestEnvelope,
    pub actor: ActorContext,
}

impl AuthorizedRequest {
    pub fn validate(&self) -> Result<(), ControlError> {
        self.envelope.validate()?;
        self.actor.validate()?;
        if self.actor.request_id != self.envelope.request_id
            || self.actor.reason != self.envelope.reason
        {
            return Err(ControlError::invalid_request(
                "trusted actor context does not match its request",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_range_u64_is_a_decimal_string_on_the_wire() {
        let value = WireU64::new(u64::MAX);
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            format!("\"{}\"", u64::MAX)
        );
        assert_eq!(
            serde_json::from_str::<WireU64>(&format!("\"{}\"", u64::MAX)).unwrap(),
            value
        );
        assert!(serde_json::from_str::<WireU64>("1").is_err());
    }

    #[test]
    fn decimal_strings_are_canonical() {
        for valid in ["0", "1", "-1", "0.1", "-0.25", "123.456"] {
            assert!(CanonicalDecimal::new(valid).is_ok(), "{valid}");
        }
        for invalid in ["-0", "+1", "01", "1.0", "1.", ".1", "1e3"] {
            assert!(CanonicalDecimal::new(invalid).is_err(), "{invalid}");
        }
    }
}
