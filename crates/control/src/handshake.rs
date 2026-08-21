//! Version negotiation and authenticated authority downscoping.

use std::{collections::BTreeSet, fmt};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use crate::{
    error::{ControlError, codes},
    id::{ConnectionId, InstanceId, PermissionId, PrincipalId, ProcessNonce, ProfileId},
    limits::{
        CONTROL_CLIENT_NAME_MAX_BYTES, CONTROL_DEFAULT_PAGE_ITEMS, CONTROL_ID_MAX_BYTES,
        CONTROL_MAX_JSON_DEPTH, CONTROL_MAX_PAGE_ITEMS, CONTROL_MAX_REQUEST_BYTES,
        CONTROL_MAX_RESPONSE_BYTES, CONTROL_MAX_STRING_BYTES, CONTROL_PROTOCOL_MAX_FRAME_BYTES,
        CONTROL_REQUEST_TIMEOUT_MS, CONTROL_TOKEN_BYTES, CONTROL_WAIT_TIMEOUT_MAX_MS,
    },
};

const TOKEN_BASE64URL_LENGTH: usize = 43;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProtocolVersionRange {
    #[serde(rename = "protocol_min")]
    #[schemars(rename = "protocol_min")]
    #[schemars(range(min = 1))]
    pub minimum: u32,
    #[serde(rename = "protocol_max")]
    #[schemars(rename = "protocol_max")]
    #[schemars(range(min = 1))]
    pub maximum: u32,
}

impl ProtocolVersionRange {
    pub const fn new(minimum: u32, maximum: u32) -> Result<Self, VersionRangeError> {
        if minimum == 0 || minimum > maximum {
            return Err(VersionRangeError);
        }
        Ok(Self { minimum, maximum })
    }

    pub const fn negotiate(self, peer: Self) -> Option<u32> {
        let lower = if self.minimum > peer.minimum {
            self.minimum
        } else {
            peer.minimum
        };
        let upper = if self.maximum < peer.maximum {
            self.maximum
        } else {
            peer.maximum
        };
        if lower <= upper { Some(upper) } else { None }
    }

    pub const fn is_valid(self) -> bool {
        self.minimum > 0 && self.minimum <= self.maximum
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionRangeError;

impl fmt::Display for VersionRangeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("protocol version range must be positive and ordered")
    }
}

impl std::error::Error for VersionRangeError {}

#[derive(Clone, JsonSchema)]
pub struct BearerToken(
    #[schemars(
        with = "String",
        length(equal = TOKEN_BASE64URL_LENGTH),
        regex(pattern = r"^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$")
    )]
    [u8; CONTROL_TOKEN_BYTES],
);

impl BearerToken {
    pub const fn from_bytes(bytes: [u8; CONTROL_TOKEN_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn from_base64url(encoded: &str) -> Result<Self, BearerTokenError> {
        if encoded.len() != TOKEN_BASE64URL_LENGTH {
            return Err(BearerTokenError);
        }
        // Every intermediate copy of the secret is wrapped, because `Drop` on
        // the token itself only clears the copy the token owns. The decoded
        // `Vec` and the re-encoded comparison string are both full plaintext
        // copies, and both used to be dropped in the clear.
        let bytes = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| BearerTokenError)?,
        );
        let bytes: [u8; CONTROL_TOKEN_BYTES] =
            bytes.as_slice().try_into().map_err(|_| BearerTokenError)?;
        let token = Self(bytes);
        // Reject a non-canonical encoding that decodes to the same bytes. The
        // comparison runs over the wrapped re-encoding so the check costs no
        // lingering copy of its own.
        if *token.to_base64url() != *encoded {
            return Err(BearerTokenError);
        }
        Ok(token)
    }

    /// The canonical encoding, wrapped so the caller cannot accidentally keep
    /// a plaintext copy of the secret alive.
    pub fn to_base64url(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(self.0))
    }

    fn constant_time_eq(&self, other: &Self) -> bool {
        bool::from(self.0.ct_eq(&other.0))
    }
}

impl PartialEq for BearerToken {
    fn eq(&self, other: &Self) -> bool {
        self.constant_time_eq(other)
    }
}

impl Eq for BearerToken {}

impl fmt::Debug for BearerToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BearerToken(<redacted>)")
    }
}

impl Drop for BearerToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Serialize for BearerToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = self.to_base64url();
        serializer.serialize_str(encoded.as_str())
    }
}

impl<'de> Deserialize<'de> for BearerToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = Zeroizing::new(String::deserialize(deserializer)?);
        Self::from_base64url(encoded.as_str()).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BearerTokenError;

impl fmt::Display for BearerTokenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bearer token must be canonical unpadded base64url for 32 bytes")
    }
}

impl std::error::Error for BearerTokenError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProtocolLimits {
    #[schemars(range(min = 1, max = CONTROL_PROTOCOL_MAX_FRAME_BYTES))]
    pub max_frame_bytes: usize,
    #[schemars(range(min = 1, max = CONTROL_MAX_REQUEST_BYTES))]
    pub max_request_bytes: usize,
    #[schemars(range(min = 1, max = CONTROL_MAX_RESPONSE_BYTES))]
    pub max_response_bytes: usize,
    #[schemars(range(min = 1, max = CONTROL_MAX_JSON_DEPTH))]
    pub max_json_depth: usize,
    #[schemars(range(min = 1, max = CONTROL_MAX_STRING_BYTES))]
    pub max_string_bytes: usize,
    #[schemars(range(min = 1, max = CONTROL_MAX_PAGE_ITEMS))]
    pub default_page_items: usize,
    #[schemars(range(min = 1, max = CONTROL_MAX_PAGE_ITEMS))]
    pub max_page_items: usize,
    #[schemars(range(min = 1, max = CONTROL_REQUEST_TIMEOUT_MS))]
    pub request_timeout_ms: u64,
    #[schemars(range(min = 1, max = CONTROL_WAIT_TIMEOUT_MAX_MS))]
    pub wait_timeout_max_ms: u64,
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: CONTROL_PROTOCOL_MAX_FRAME_BYTES,
            max_request_bytes: CONTROL_MAX_REQUEST_BYTES,
            max_response_bytes: CONTROL_MAX_RESPONSE_BYTES,
            max_json_depth: CONTROL_MAX_JSON_DEPTH,
            max_string_bytes: CONTROL_MAX_STRING_BYTES,
            default_page_items: CONTROL_DEFAULT_PAGE_ITEMS,
            max_page_items: CONTROL_MAX_PAGE_ITEMS,
            request_timeout_ms: CONTROL_REQUEST_TIMEOUT_MS,
            wait_timeout_max_ms: CONTROL_WAIT_TIMEOUT_MAX_MS,
        }
    }
}

impl ProtocolLimits {
    pub fn validate(&self) -> Result<(), ControlError> {
        let valid = self.max_frame_bytes > 0
            && self.max_frame_bytes <= CONTROL_PROTOCOL_MAX_FRAME_BYTES
            && self.max_request_bytes > 0
            && self.max_request_bytes <= CONTROL_MAX_REQUEST_BYTES
            && self.max_request_bytes <= self.max_frame_bytes
            && self.max_response_bytes > 0
            && self.max_response_bytes <= CONTROL_MAX_RESPONSE_BYTES
            && self.max_response_bytes <= self.max_frame_bytes
            && self.max_json_depth > 0
            && self.max_json_depth <= CONTROL_MAX_JSON_DEPTH
            && self.max_string_bytes > 0
            && self.max_string_bytes <= CONTROL_MAX_STRING_BYTES
            && self.default_page_items > 0
            && self.default_page_items <= self.max_page_items
            && self.max_page_items <= CONTROL_MAX_PAGE_ITEMS
            && self.request_timeout_ms > 0
            && self.request_timeout_ms <= CONTROL_REQUEST_TIMEOUT_MS
            && self.wait_timeout_max_ms > 0
            && self.wait_timeout_max_ms <= CONTROL_WAIT_TIMEOUT_MAX_MS;
        if !valid {
            return Err(ControlError::invalid_request(
                "selected protocol limits exceed the reviewed contract",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HandshakeRequest {
    #[serde(flatten)]
    pub protocol_versions: ProtocolVersionRange,
    pub instance_id: InstanceId,
    #[schemars(
        length(min = 1, max = CONTROL_CLIENT_NAME_MAX_BYTES),
        regex(pattern = r"^[^\x00-\x1f\x7f]+$")
    )]
    pub client_name: String,
    #[schemars(length(min = 1, max = CONTROL_ID_MAX_BYTES), regex(pattern = r"^[\x20-\x7e]+$"))]
    pub client_version: String,
    pub bearer_token: BearerToken,
    pub requested_profile: ProfileId,
    pub requested_scopes: BTreeSet<PermissionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HandshakeResponse {
    #[schemars(range(min = 1))]
    pub protocol_version: u32,
    pub instance_id: InstanceId,
    pub process_nonce: ProcessNonce,
    pub connection_id: ConnectionId,
    #[schemars(length(min = 1, max = CONTROL_ID_MAX_BYTES), regex(pattern = r"^[\x20-\x7e]+$"))]
    pub application_version: String,
    #[schemars(length(min = 1, max = CONTROL_ID_MAX_BYTES), regex(pattern = r"^[\x20-\x7e]+$"))]
    pub application_commit: String,
    pub effective_profile: ProfileId,
    pub effective_scopes: BTreeSet<PermissionId>,
    pub effective_limits: ProtocolLimits,
}

impl HandshakeResponse {
    pub fn validate_for(
        &self,
        request: &HandshakeRequest,
        expected_process_nonce: &ProcessNonce,
    ) -> Result<(), ControlError> {
        if self.instance_id != request.instance_id || &self.process_nonce != expected_process_nonce
        {
            return Err(ControlError::known(
                codes::AUTH_FAILED,
                "handshake response identifies a different application process",
                false,
            ));
        }
        if self.protocol_version < request.protocol_versions.minimum
            || self.protocol_version > request.protocol_versions.maximum
        {
            return Err(ControlError::known(
                codes::VERSION_UNSUPPORTED,
                "server selected a protocol version outside the requested range",
                false,
            ));
        }
        if !self.effective_scopes.is_subset(&request.requested_scopes) {
            return Err(ControlError::known(
                codes::PERMISSION_DENIED,
                "handshake response granted an unrequested scope",
                false,
            ));
        }
        validate_version_label("application version", &self.application_version)?;
        validate_version_label("application commit", &self.application_commit)?;
        self.effective_limits.validate()
    }
}

#[derive(Clone, Debug)]
pub struct HandshakeGrant {
    pub protocol_versions: ProtocolVersionRange,
    pub instance_id: InstanceId,
    pub process_nonce: ProcessNonce,
    pub bearer_token: BearerToken,
    pub connection_id: ConnectionId,
    pub principal_id: PrincipalId,
    pub application_version: String,
    pub application_commit: String,
    pub profile_ceiling: ProfileId,
    pub granted_scopes: BTreeSet<PermissionId>,
    pub limits: ProtocolLimits,
}

pub trait ProfileAuthority {
    fn permission_ceiling(&self, profile: &ProfileId) -> Option<BTreeSet<PermissionId>>;
    fn is_known_permission(&self, permission: &PermissionId) -> bool;
}

pub fn accept_handshake(
    request: &HandshakeRequest,
    grant: &HandshakeGrant,
    authority: &impl ProfileAuthority,
) -> Result<HandshakeResponse, ControlError> {
    if !request.protocol_versions.is_valid() || !grant.protocol_versions.is_valid() {
        return Err(ControlError::known(
            codes::VERSION_UNSUPPORTED,
            "invalid protocol version range",
            false,
        ));
    }
    grant.limits.validate()?;
    let protocol_version = request
        .protocol_versions
        .negotiate(grant.protocol_versions)
        .ok_or_else(|| {
            ControlError::known(
                codes::VERSION_UNSUPPORTED,
                "client and server protocol ranges do not overlap",
                false,
            )
        })?;
    if request.instance_id != grant.instance_id
        || !request.bearer_token.constant_time_eq(&grant.bearer_token)
    {
        return Err(ControlError::known(
            codes::AUTH_FAILED,
            "instance authentication failed",
            false,
        ));
    }
    validate_client_name(&request.client_name)?;
    validate_version_label("client version", &request.client_version)?;
    validate_version_label("application version", &grant.application_version)?;
    validate_version_label("application commit", &grant.application_commit)?;

    let requested_ceiling = authority
        .permission_ceiling(&request.requested_profile)
        .ok_or_else(|| ControlError::known(codes::PERMISSION_DENIED, "unknown profile", false))?;
    let grant_ceiling = authority
        .permission_ceiling(&grant.profile_ceiling)
        .ok_or_else(|| {
            ControlError::known(codes::PERMISSION_DENIED, "unknown grant profile", false)
        })?;
    if request
        .requested_scopes
        .iter()
        .any(|permission| !authority.is_known_permission(permission))
    {
        return Err(ControlError::known(
            codes::PERMISSION_DENIED,
            "the handshake requested an undeclared permission",
            false,
        ));
    }
    if grant
        .granted_scopes
        .iter()
        .any(|permission| !authority.is_known_permission(permission))
    {
        return Err(ControlError::known(
            codes::PERMISSION_DENIED,
            "the application grant contains an undeclared permission",
            false,
        ));
    }

    // Neither side may widen the other. Taking one ceiling wholesale hands the
    // client a permission the discarded ceiling forbids: a client asking to be
    // capped at a profile without `trade`, against a grant that has it, would
    // be handed `trade` precisely because the two only overlap. The
    // intersection is the only combination that honours both caps.
    let effective_ceiling: BTreeSet<PermissionId> = requested_ceiling
        .intersection(&grant_ceiling)
        .cloned()
        .collect();
    // The reported profile has to name the authority the connection actually
    // holds, because it is what the connected-clients panel shows a human.
    // Profiles are built nested, so one of the two always equals the
    // intersection; an incomparable pair is a configuration error rather than
    // something to resolve silently under a label that overstates it.
    let effective_profile = if requested_ceiling == effective_ceiling {
        request.requested_profile.clone()
    } else if grant_ceiling == effective_ceiling {
        grant.profile_ceiling.clone()
    } else {
        return Err(ControlError::known(
            codes::PERMISSION_DENIED,
            "requested and granted profiles are incomparable, so no profile names the effective authority",
            false,
        ));
    };
    let effective_scopes = request
        .requested_scopes
        .intersection(&grant.granted_scopes)
        .filter(|permission| effective_ceiling.contains(*permission))
        .cloned()
        .collect();

    Ok(HandshakeResponse {
        protocol_version,
        instance_id: grant.instance_id.clone(),
        process_nonce: grant.process_nonce.clone(),
        connection_id: grant.connection_id.clone(),
        application_version: grant.application_version.clone(),
        application_commit: grant.application_commit.clone(),
        effective_profile,
        effective_scopes,
        effective_limits: grant.limits.clone(),
    })
}

fn validate_client_name(name: &str) -> Result<(), ControlError> {
    if name.trim().is_empty()
        || name.len() > CONTROL_CLIENT_NAME_MAX_BYTES
        || name.chars().any(char::is_control)
    {
        return Err(ControlError::invalid_request(
            "client name must be non-empty, bounded UTF-8 without control characters",
        ));
    }
    Ok(())
}

fn validate_version_label(kind: &str, value: &str) -> Result<(), ControlError> {
    if value.trim().is_empty()
        || value.len() > CONTROL_ID_MAX_BYTES
        || !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err(ControlError::invalid_request(format!(
            "{kind} must be non-empty bounded printable ASCII"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_selects_highest_overlap() {
        let client = ProtocolVersionRange::new(1, 4).unwrap();
        let server = ProtocolVersionRange::new(3, 5).unwrap();
        assert_eq!(client.negotiate(server), Some(4));
        assert_eq!(
            client.negotiate(ProtocolVersionRange::new(5, 6).unwrap()),
            None
        );
    }

    #[test]
    fn token_debug_is_redacted() {
        let token = BearerToken::from_bytes([9; CONTROL_TOKEN_BYTES]);
        assert_eq!(format!("{token:?}"), "BearerToken(<redacted>)");
    }
}
