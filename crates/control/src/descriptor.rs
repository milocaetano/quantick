//! Validated descriptor for one explicitly enabled local application gateway.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    handshake::{BearerToken, ProtocolVersionRange},
    id::{InstanceId, ProcessNonce},
    limits::{CONTROL_DESCRIPTOR_MAX_BYTES, CONTROL_ID_MAX_BYTES},
};

pub const INSTANCE_DESCRIPTOR_VERSION: u32 = 1;
pub const INSTANCE_DESCRIPTOR_TRANSPORT: &str = "tcp";
pub const INSTANCE_DESCRIPTOR_HOST: &str = "127.0.0.1";

/// The private rendezvous record published only while local access is enabled.
///
/// This type deliberately contains no URL or command string. Consumers must
/// validate it before opening a socket and must never log or return the bearer
/// token to an external caller.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstanceDescriptor {
    #[schemars(range(min = 1, max = 1))]
    pub descriptor_version: u32,
    pub instance_id: InstanceId,
    pub process_nonce: ProcessNonce,
    #[schemars(range(min = 1))]
    pub process_id: u32,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub process_started_at_unix_ms: i64,
    #[schemars(length(min = 1, max = CONTROL_ID_MAX_BYTES), regex(pattern = r"^[\x20-\x7e]+$"))]
    pub application_version: String,
    #[schemars(length(min = 1, max = CONTROL_ID_MAX_BYTES), regex(pattern = r"^[\x20-\x7e]+$"))]
    pub application_commit: String,
    #[serde(flatten)]
    pub protocol_versions: ProtocolVersionRange,
    #[schemars(regex(pattern = r"^tcp$"))]
    pub transport: String,
    #[schemars(regex(pattern = r"^127\.0\.0\.1$"))]
    pub host: String,
    #[schemars(range(min = 1, max = 65535))]
    pub port: u16,
    pub bearer_token: BearerToken,
    #[schemars(extend("x-unit" = "unix_milliseconds"))]
    pub published_at_unix_ms: i64,
}

impl InstanceDescriptor {
    pub fn validate(&self) -> Result<(), DescriptorValidationError> {
        if self.descriptor_version != INSTANCE_DESCRIPTOR_VERSION {
            return Err(DescriptorValidationError::new(
                "unsupported instance descriptor version",
            ));
        }
        if self.process_id == 0 {
            return Err(DescriptorValidationError::new(
                "descriptor process ID must be positive",
            ));
        }
        if self.process_started_at_unix_ms <= 0 || self.published_at_unix_ms <= 0 {
            return Err(DescriptorValidationError::new(
                "descriptor timestamps must be positive Unix milliseconds",
            ));
        }
        validate_label("application version", &self.application_version)?;
        validate_label("application commit", &self.application_commit)?;
        if !self.protocol_versions.is_valid() {
            return Err(DescriptorValidationError::new(
                "descriptor protocol range is invalid",
            ));
        }
        if self.transport != INSTANCE_DESCRIPTOR_TRANSPORT {
            return Err(DescriptorValidationError::new(
                "descriptor transport must be literal tcp",
            ));
        }
        if self.host != INSTANCE_DESCRIPTOR_HOST {
            return Err(DescriptorValidationError::new(
                "descriptor host must be literal IPv4 loopback",
            ));
        }
        if self.port == 0 {
            return Err(DescriptorValidationError::new(
                "descriptor TCP port must be positive",
            ));
        }
        let encoded = serde_json::to_vec(self).map_err(|_| {
            DescriptorValidationError::new("descriptor cannot be encoded as bounded JSON")
        })?;
        if encoded.len() > CONTROL_DESCRIPTOR_MAX_BYTES {
            return Err(DescriptorValidationError::new(
                "descriptor exceeds its reviewed byte limit",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.json", self.instance_id)
    }
}

fn validate_label(kind: &str, value: &str) -> Result<(), DescriptorValidationError> {
    if value.trim().is_empty()
        || value.len() > CONTROL_ID_MAX_BYTES
        || !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err(DescriptorValidationError::owned(format!(
            "{kind} must be non-empty bounded printable ASCII"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DescriptorValidationError(String);

impl DescriptorValidationError {
    fn new(message: &'static str) -> Self {
        Self(message.to_owned())
    }

    fn owned(message: String) -> Self {
        Self(message)
    }
}

impl fmt::Display for DescriptorValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DescriptorValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_descriptor() -> InstanceDescriptor {
        InstanceDescriptor {
            descriptor_version: INSTANCE_DESCRIPTOR_VERSION,
            instance_id: InstanceId::from_bytes([1; 16]),
            process_nonce: ProcessNonce::from_bytes([2; 16]),
            process_id: 42,
            process_started_at_unix_ms: 1_700_000_000_000,
            application_version: "0.1.0".to_owned(),
            application_commit: "abc123".to_owned(),
            protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
            transport: INSTANCE_DESCRIPTOR_TRANSPORT.to_owned(),
            host: INSTANCE_DESCRIPTOR_HOST.to_owned(),
            port: 31_337,
            bearer_token: BearerToken::from_bytes([3; 32]),
            published_at_unix_ms: 1_700_000_000_100,
        }
    }

    #[test]
    fn descriptor_is_strict_loopback_and_bounded() {
        let descriptor = valid_descriptor();
        descriptor.validate().unwrap();
        assert_eq!(
            descriptor.file_name(),
            format!("{}.json", descriptor.instance_id)
        );
        assert!(serde_json::to_vec(&descriptor).unwrap().len() < CONTROL_DESCRIPTOR_MAX_BYTES);
    }

    #[test]
    fn descriptor_rejects_endpoint_and_version_substitution() {
        let mut descriptor = valid_descriptor();
        descriptor.host = "localhost".to_owned();
        assert!(descriptor.validate().is_err());
        descriptor.host = INSTANCE_DESCRIPTOR_HOST.to_owned();
        descriptor.transport = "http".to_owned();
        assert!(descriptor.validate().is_err());
        descriptor.transport = INSTANCE_DESCRIPTOR_TRANSPORT.to_owned();
        descriptor.descriptor_version += 1;
        assert!(descriptor.validate().is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let mut value = serde_json::to_value(valid_descriptor()).unwrap();
        value["command"] = serde_json::json!("quantick --start");
        assert!(serde_json::from_value::<InstanceDescriptor>(value).is_err());
    }
}
