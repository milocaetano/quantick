//! Extensible registry identifiers and opaque runtime identities.

use std::{fmt, str::FromStr};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use zeroize::Zeroize;

use crate::limits::{
    CONTROL_ID_MAX_BYTES, CONTROL_ID_MAX_SEGMENTS, CONTROL_IDEMPOTENCY_KEY_MAX_BYTES,
    CONTROL_REQUEST_ID_MAX_BYTES, CONTROL_RUNTIME_ID_BYTES,
};

pub const REGISTRY_ID_PATTERN: &str = r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$";
pub const NAMESPACED_REGISTRY_ID_PATTERN: &str = r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,7}$";
const RUNTIME_ID_BASE64URL_LENGTH: usize = 22;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdErrorKind {
    Empty,
    TooLong,
    TooManySegments,
    NamespaceRequired,
    InvalidSyntax,
    InvalidEncoding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdValidationError {
    kind: IdErrorKind,
    type_name: &'static str,
}

impl IdValidationError {
    const fn new(kind: IdErrorKind, type_name: &'static str) -> Self {
        Self { kind, type_name }
    }

    pub const fn kind(&self) -> IdErrorKind {
        self.kind
    }
}

impl fmt::Display for IdValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {}: {}",
            self.type_name,
            match self.kind {
                IdErrorKind::Empty => "value is empty",
                IdErrorKind::TooLong => "value exceeds its byte limit",
                IdErrorKind::TooManySegments => "value has too many dotted segments",
                IdErrorKind::NamespaceRequired => "value must contain a namespace",
                IdErrorKind::InvalidSyntax => "value has invalid syntax",
                IdErrorKind::InvalidEncoding => "value has invalid encoding or decoded length",
            }
        )
    }
}

impl std::error::Error for IdValidationError {}

fn validate_registry_id(
    value: &str,
    namespaced: bool,
    type_name: &'static str,
) -> Result<(), IdValidationError> {
    if value.is_empty() {
        return Err(IdValidationError::new(IdErrorKind::Empty, type_name));
    }
    if value.len() > CONTROL_ID_MAX_BYTES {
        return Err(IdValidationError::new(IdErrorKind::TooLong, type_name));
    }

    let mut segments = 0usize;
    for segment in value.split('.') {
        segments += 1;
        if segments > CONTROL_ID_MAX_SEGMENTS {
            return Err(IdValidationError::new(
                IdErrorKind::TooManySegments,
                type_name,
            ));
        }

        let mut bytes = segment.bytes();
        if !matches!(bytes.next(), Some(b'a'..=b'z'))
            || !bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(IdValidationError::new(
                IdErrorKind::InvalidSyntax,
                type_name,
            ));
        }
    }

    if namespaced && segments < 2 {
        return Err(IdValidationError::new(
            IdErrorKind::NamespaceRequired,
            type_name,
        ));
    }
    Ok(())
}

macro_rules! registry_id {
    ($name:ident, $namespaced:literal, $pattern:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(
            #[schemars(length(min = 1, max = CONTROL_ID_MAX_BYTES), regex(pattern = $pattern))]
            String,
        );

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdValidationError> {
                let value = value.into();
                validate_registry_id(&value, $namespaced, stringify!($name))?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdValidationError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

registry_id!(
    CapabilityId,
    true,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,7}$",
    "A versioned operation exposed by a module."
);
registry_id!(
    SnapshotScopeId,
    true,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,7}$",
    "A namespaced semantic snapshot projection."
);
registry_id!(
    EventKind,
    true,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,7}$",
    "A namespaced semantic event kind."
);
registry_id!(
    ErrorCode,
    true,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,7}$",
    "A stable machine-readable error code."
);
registry_id!(
    ModuleId,
    false,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$",
    "A module contributing control-plane behavior."
);
registry_id!(
    PermissionId,
    false,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$",
    "A granular authority or data-read grant."
);
registry_id!(
    EffectId,
    false,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$",
    "A remote effect classification."
);
registry_id!(
    ProfileId,
    false,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$",
    "An application-registered authority ceiling."
);
registry_id!(
    RiskFlagId,
    false,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$",
    "A declared side effect or risk dimension."
);
registry_id!(
    ConfirmationClassId,
    false,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$",
    "An application-defined confirmation policy class."
);
registry_id!(
    PreconditionId,
    true,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,7}$",
    "A stable namespaced capability precondition."
);
registry_id!(
    AvailabilityReasonId,
    true,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,7}$",
    "A stable reason why a capability cannot run immediately."
);
registry_id!(
    CostClassId,
    false,
    r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){0,7}$",
    "A declared cold-path execution cost class."
);

macro_rules! runtime_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(
            #[schemars(
                                                length(equal = RUNTIME_ID_BASE64URL_LENGTH),
                                                regex(pattern = r"^[A-Za-z0-9_-]{21}[AQgw]$")
                                            )]
            String,
        );

        impl $name {
            pub fn from_bytes(bytes: [u8; CONTROL_RUNTIME_ID_BYTES]) -> Self {
                Self(URL_SAFE_NO_PAD.encode(bytes))
            }

            pub fn new(encoded: impl Into<String>) -> Result<Self, IdValidationError> {
                let encoded = encoded.into();
                if encoded.len() != RUNTIME_ID_BASE64URL_LENGTH {
                    return Err(IdValidationError::new(
                        IdErrorKind::InvalidEncoding,
                        stringify!($name),
                    ));
                }
                let decoded = URL_SAFE_NO_PAD.decode(encoded.as_bytes()).map_err(|_| {
                    IdValidationError::new(IdErrorKind::InvalidEncoding, stringify!($name))
                })?;
                if decoded.len() != CONTROL_RUNTIME_ID_BYTES
                    || URL_SAFE_NO_PAD.encode(&decoded) != encoded
                {
                    return Err(IdValidationError::new(
                        IdErrorKind::InvalidEncoding,
                        stringify!($name),
                    ));
                }
                Ok(Self(encoded))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdValidationError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(value).map_err(de::Error::custom)
            }
        }
    };
}

runtime_id!(InstanceId, "The identity of one running Quantick instance.");
runtime_id!(
    ProcessNonce,
    "A nonce distinguishing one application process."
);
runtime_id!(ConnectionId, "A server-assigned local connection identity.");
runtime_id!(
    PrincipalId,
    "A server-assigned authenticated principal identity."
);
runtime_id!(EvidenceId, "A retained evidence-bundle identity.");
runtime_id!(ResourceId, "A retained immutable resource identity.");

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct RequestId(
    #[schemars(
        length(min = 1, max = CONTROL_REQUEST_ID_MAX_BYTES),
        regex(pattern = r"^[\x20-\x7e]+$")
    )]
    String,
);

impl RequestId {
    pub fn new(value: impl Into<String>) -> Result<Self, IdValidationError> {
        let value = value.into();
        validate_printable_ascii(&value, CONTROL_REQUEST_ID_MAX_BYTES, "RequestId")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RequestId {
    type Err = IdValidationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for RequestId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct IdempotencyKey(
    #[schemars(
        length(min = 1, max = CONTROL_IDEMPOTENCY_KEY_MAX_BYTES),
        regex(pattern = r"^[\x20-\x7e]+$")
    )]
    String,
);

impl IdempotencyKey {
    pub fn new(value: impl Into<String>) -> Result<Self, IdValidationError> {
        let value = value.into();
        validate_printable_ascii(&value, CONTROL_IDEMPOTENCY_KEY_MAX_BYTES, "IdempotencyKey")?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for IdempotencyKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("IdempotencyKey(<redacted>)")
    }
}

impl Drop for IdempotencyKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl<'de> Deserialize<'de> for IdempotencyKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

fn validate_printable_ascii(
    value: &str,
    max_bytes: usize,
    type_name: &'static str,
) -> Result<(), IdValidationError> {
    if value.is_empty() {
        return Err(IdValidationError::new(IdErrorKind::Empty, type_name));
    }
    if value.len() > max_bytes {
        return Err(IdValidationError::new(IdErrorKind::TooLong, type_name));
    }
    if !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte)) {
        return Err(IdValidationError::new(
            IdErrorKind::InvalidSyntax,
            type_name,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_enforce_namespace_and_shape() {
        assert!(CapabilityId::new("extension.operation").is_ok());
        assert!(CapabilityId::new("operation").is_err());
        assert!(ModuleId::new("extension").is_ok());
        assert!(ModuleId::new("Extension").is_err());
        assert!(ModuleId::new("extension..operation").is_err());
    }

    #[test]
    fn runtime_id_requires_canonical_128_bit_base64url() {
        let id = InstanceId::from_bytes([7; CONTROL_RUNTIME_ID_BYTES]);
        assert_eq!(id.as_str().len(), 22);
        assert_eq!(InstanceId::new(id.to_string()).unwrap(), id);
        assert!(InstanceId::new(format!("{}=", id.as_str())).is_err());
    }
}
