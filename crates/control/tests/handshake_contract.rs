use std::collections::BTreeSet;

use quantick_control::{
    error::codes,
    fake::reference_registry,
    handshake::{
        BearerToken, HandshakeGrant, HandshakeRequest, ProfileAuthority, ProtocolLimits,
        ProtocolVersionRange, accept_handshake,
    },
    id::{ConnectionId, InstanceId, PermissionId, PrincipalId, ProcessNonce, ProfileId},
    limits::CONTROL_REQUEST_TIMEOUT_MS,
};

/// Two profiles whose ceilings overlap without either containing the other.
/// The reference registry's profiles are nested, which is the shape that hides
/// the downscoping bug, so this authority states the awkward case directly.
struct OverlappingProfiles;

impl OverlappingProfiles {
    fn permission(name: &str) -> PermissionId {
        PermissionId::new(name).unwrap()
    }
}

impl ProfileAuthority for OverlappingProfiles {
    fn permission_ceiling(&self, profile: &ProfileId) -> Option<BTreeSet<PermissionId>> {
        match profile.as_str() {
            "auditor" => Some(BTreeSet::from([
                Self::permission("read"),
                Self::permission("audit"),
            ])),
            "trader" => Some(BTreeSet::from([
                Self::permission("read"),
                Self::permission("trade"),
            ])),
            _ => None,
        }
    }

    fn is_known_permission(&self, permission: &PermissionId) -> bool {
        matches!(permission.as_str(), "read" | "audit" | "trade")
    }
}

fn overlapping_pair() -> (HandshakeRequest, HandshakeGrant) {
    let instance = InstanceId::from_bytes([1; 16]);
    let token = BearerToken::from_bytes([7; 32]);
    let request = HandshakeRequest {
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        instance_id: instance.clone(),
        client_name: "narrow client".to_owned(),
        client_version: "1.0.0".to_owned(),
        bearer_token: token.clone(),
        // The client asks to be capped at a profile that cannot trade.
        requested_profile: ProfileId::new("auditor").unwrap(),
        requested_scopes: BTreeSet::from([
            PermissionId::new("read").unwrap(),
            PermissionId::new("trade").unwrap(),
        ]),
    };
    let grant = HandshakeGrant {
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        instance_id: instance,
        process_nonce: ProcessNonce::from_bytes([4; 16]),
        bearer_token: token,
        connection_id: ConnectionId::from_bytes([2; 16]),
        principal_id: PrincipalId::from_bytes([3; 16]),
        application_version: "0.1.0".to_owned(),
        application_commit: "abc123".to_owned(),
        profile_ceiling: ProfileId::new("trader").unwrap(),
        granted_scopes: BTreeSet::from([
            PermissionId::new("read").unwrap(),
            PermissionId::new("trade").unwrap(),
        ]),
        limits: ProtocolLimits::default(),
    };
    (request, grant)
}

#[test]
fn overlapping_profile_ceilings_never_grant_what_the_requested_profile_forbids() {
    let (request, grant) = overlapping_pair();

    // Taking either ceiling wholesale hands out `trade`: the client asked to be
    // capped at `auditor`, which forbids it, and the grant's `trader` allows it.
    // Only the intersection honours both caps, and here the two profiles are
    // incomparable, so no profile can honestly name the result.
    let error = accept_handshake(&request, &grant, &OverlappingProfiles).unwrap_err();
    assert_eq!(error.code.as_str(), codes::PERMISSION_DENIED);
    assert!(!error.retryable);
}

#[test]
fn a_narrower_requested_profile_still_downscopes_a_broader_grant() {
    // The nested case has to keep working: asking for less than the grant
    // allows is the ordinary least-privilege path, not the awkward one.
    let (mut request, mut grant) = overlapping_pair();
    request.requested_profile = ProfileId::new("trader").unwrap();
    grant.profile_ceiling = ProfileId::new("trader").unwrap();
    request.requested_scopes = BTreeSet::from([PermissionId::new("read").unwrap()]);

    let response = accept_handshake(&request, &grant, &OverlappingProfiles).unwrap();
    assert_eq!(response.effective_profile.as_str(), "trader");
    assert_eq!(
        response.effective_scopes,
        BTreeSet::from([PermissionId::new("read").unwrap()])
    );
    assert!(
        !response
            .effective_scopes
            .contains(&PermissionId::new("trade").unwrap())
    );
}

#[test]
fn a_negotiated_request_timeout_cannot_exceed_the_reviewed_ceiling() {
    let registry = reference_registry().unwrap();
    let instance = InstanceId::from_bytes([1; 16]);
    let token = BearerToken::from_bytes([7; 32]);
    let request = HandshakeRequest {
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        instance_id: instance.clone(),
        client_name: "patient client".to_owned(),
        client_version: "1.0.0".to_owned(),
        bearer_token: token.clone(),
        requested_profile: ProfileId::new("observer").unwrap(),
        requested_scopes: BTreeSet::new(),
    };
    let mut grant = HandshakeGrant {
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        instance_id: instance,
        process_nonce: ProcessNonce::from_bytes([4; 16]),
        bearer_token: token,
        connection_id: ConnectionId::from_bytes([2; 16]),
        principal_id: PrincipalId::from_bytes([3; 16]),
        application_version: "0.1.0".to_owned(),
        application_commit: "abc123".to_owned(),
        profile_ceiling: ProfileId::new("observer").unwrap(),
        granted_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
        limits: ProtocolLimits::default(),
    };

    // Its sibling `wait_timeout_max_ms` was already bounded; an unbounded
    // request timeout lets a peer announce one a client would wait out forever.
    grant.limits.request_timeout_ms = u64::MAX;
    assert_eq!(
        accept_handshake(&request, &grant, &registry)
            .unwrap_err()
            .code
            .as_str(),
        codes::INVALID_REQUEST
    );

    grant.limits.request_timeout_ms = CONTROL_REQUEST_TIMEOUT_MS;
    assert!(accept_handshake(&request, &grant, &registry).is_ok());
}

#[test]
fn accepted_handshake_selects_overlap_and_downscopes_to_app_grant() {
    let registry = reference_registry().unwrap();
    let instance = InstanceId::from_bytes([1; 16]);
    let token = BearerToken::from_bytes([7; 32]);
    let token_text = token.to_base64url();
    let request = HandshakeRequest {
        protocol_versions: ProtocolVersionRange::new(1, 3).unwrap(),
        instance_id: instance.clone(),
        client_name: "integration client".to_owned(),
        client_version: "1.2.3".to_owned(),
        bearer_token: token.clone(),
        requested_profile: ProfileId::new("developer").unwrap(),
        requested_scopes: BTreeSet::from([
            PermissionId::new("observe").unwrap(),
            PermissionId::new("fake.write").unwrap(),
        ]),
    };
    let grant = HandshakeGrant {
        protocol_versions: ProtocolVersionRange::new(2, 4).unwrap(),
        instance_id: instance,
        process_nonce: ProcessNonce::from_bytes([4; 16]),
        bearer_token: token,
        connection_id: ConnectionId::from_bytes([2; 16]),
        principal_id: PrincipalId::from_bytes([3; 16]),
        application_version: "0.1.0".to_owned(),
        application_commit: "abc123".to_owned(),
        profile_ceiling: ProfileId::new("observer").unwrap(),
        granted_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
        limits: ProtocolLimits::default(),
    };

    let response = accept_handshake(&request, &grant, &registry).unwrap();
    response
        .validate_for(&request, &ProcessNonce::from_bytes([4; 16]))
        .unwrap();
    assert_eq!(response.protocol_version, 3);
    assert_eq!(response.effective_profile.as_str(), "observer");
    assert_eq!(
        response.effective_scopes,
        BTreeSet::from([PermissionId::new("observe").unwrap()])
    );
    assert_eq!(
        response.effective_limits.max_request_bytes,
        quantick_control::limits::CONTROL_MAX_REQUEST_BYTES
    );
    assert_eq!(response.process_nonce, ProcessNonce::from_bytes([4; 16]));
    assert_eq!(response.application_version, "0.1.0");
    assert_eq!(response.application_commit, "abc123");
    let encoded = serde_json::to_string(&response).unwrap();
    assert!(!encoded.contains(token_text.as_str()));
    assert!(!encoded.contains("bearer_token"));

    let mut mismatched_process = response;
    mismatched_process.process_nonce = ProcessNonce::from_bytes([9; 16]);
    assert_eq!(
        mismatched_process
            .validate_for(&request, &ProcessNonce::from_bytes([4; 16]))
            .unwrap_err()
            .code
            .as_str(),
        codes::AUTH_FAILED
    );
}

#[test]
fn handshake_fails_closed_for_nonoverlap_and_unknown_permissions() {
    let registry = reference_registry().unwrap();
    let instance = InstanceId::from_bytes([1; 16]);
    let token = BearerToken::from_bytes([7; 32]);
    let mut request = HandshakeRequest {
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        instance_id: instance.clone(),
        client_name: "integration client".to_owned(),
        client_version: "1.2.3".to_owned(),
        bearer_token: token.clone(),
        requested_profile: ProfileId::new("observer").unwrap(),
        requested_scopes: BTreeSet::new(),
    };
    let grant = HandshakeGrant {
        protocol_versions: ProtocolVersionRange::new(2, 2).unwrap(),
        instance_id: instance,
        process_nonce: ProcessNonce::from_bytes([4; 16]),
        bearer_token: token,
        connection_id: ConnectionId::from_bytes([2; 16]),
        principal_id: PrincipalId::from_bytes([3; 16]),
        application_version: "0.1.0".to_owned(),
        application_commit: "abc123".to_owned(),
        profile_ceiling: ProfileId::new("observer").unwrap(),
        granted_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
        limits: ProtocolLimits::default(),
    };
    assert_eq!(
        accept_handshake(&request, &grant, &registry)
            .unwrap_err()
            .code
            .as_str(),
        codes::VERSION_UNSUPPORTED
    );

    request.protocol_versions = ProtocolVersionRange::new(2, 2).unwrap();
    request.requested_scopes = BTreeSet::from([PermissionId::new("plugin.undeclared").unwrap()]);
    assert_eq!(
        accept_handshake(&request, &grant, &registry)
            .unwrap_err()
            .code
            .as_str(),
        codes::PERMISSION_DENIED
    );

    request.requested_scopes = BTreeSet::from([PermissionId::new("observe").unwrap()]);
    let mut invalid_grant = grant.clone();
    invalid_grant.granted_scopes =
        BTreeSet::from([PermissionId::new("plugin.undeclared").unwrap()]);
    assert_eq!(
        accept_handshake(&request, &invalid_grant, &registry)
            .unwrap_err()
            .code
            .as_str(),
        codes::PERMISSION_DENIED
    );

    let mut invalid_client = request.clone();
    invalid_client.client_name = "   ".to_owned();
    assert_eq!(
        accept_handshake(&invalid_client, &grant, &registry)
            .unwrap_err()
            .code
            .as_str(),
        codes::INVALID_REQUEST
    );

    let mut wrong_token = request.clone();
    wrong_token.bearer_token = BearerToken::from_bytes([8; 32]);
    assert_eq!(
        accept_handshake(&wrong_token, &grant, &registry)
            .unwrap_err()
            .code
            .as_str(),
        codes::AUTH_FAILED
    );

    let mut invalid_limits = grant;
    invalid_limits.limits.max_frame_bytes =
        quantick_control::limits::CONTROL_PROTOCOL_MAX_FRAME_BYTES + 1;
    assert_eq!(
        accept_handshake(&request, &invalid_limits, &registry)
            .unwrap_err()
            .code
            .as_str(),
        codes::INVALID_REQUEST
    );
}

#[test]
fn handshake_wire_shape_matches_the_discovery_contract_and_empty_scopes_grant_nothing() {
    let registry = reference_registry().unwrap();
    let instance = InstanceId::from_bytes([1; 16]);
    let token = BearerToken::from_bytes([7; 32]);
    let request = HandshakeRequest {
        protocol_versions: ProtocolVersionRange::new(1, 2).unwrap(),
        instance_id: instance.clone(),
        client_name: "least-privilege client".to_owned(),
        client_version: "2.0.0".to_owned(),
        bearer_token: token.clone(),
        requested_profile: ProfileId::new("observer").unwrap(),
        requested_scopes: BTreeSet::new(),
    };
    let request_json = serde_json::to_value(&request).unwrap();
    assert_eq!(request_json["protocol_min"], 1);
    assert_eq!(request_json["protocol_max"], 2);
    assert!(request_json.get("protocol_versions").is_none());

    let grant = HandshakeGrant {
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        instance_id: instance,
        process_nonce: ProcessNonce::from_bytes([4; 16]),
        bearer_token: token,
        connection_id: ConnectionId::from_bytes([2; 16]),
        principal_id: PrincipalId::from_bytes([3; 16]),
        application_version: "0.1.0".to_owned(),
        application_commit: "abc123".to_owned(),
        profile_ceiling: ProfileId::new("observer").unwrap(),
        granted_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
        limits: ProtocolLimits::default(),
    };
    let response = accept_handshake(&request, &grant, &registry).unwrap();
    assert!(response.effective_scopes.is_empty());
    let response_json = serde_json::to_value(response).unwrap();
    for field in [
        "protocol_version",
        "instance_id",
        "process_nonce",
        "connection_id",
        "application_version",
        "application_commit",
        "effective_profile",
        "effective_scopes",
        "effective_limits",
    ] {
        assert!(response_json.get(field).is_some(), "missing {field}");
    }
    assert!(response_json.get("principal_id").is_none());
}
