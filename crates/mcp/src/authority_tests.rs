//! The actual CLI request ceiling, intersected by the production handshake.
use super::*;
use quantick_control::{
    handshake::{
        BearerToken, CURRENT_PROTOCOL_VERSION, HandshakeGrant, HandshakeRequest, ProtocolLimits,
        ProtocolVersionRange, accept_handshake,
    },
    id::{ConnectionId, PrincipalId, ProcessNonce, ProfileId},
    registry::ControlRegistry,
};

#[test]
fn recovery_requires_both_explicit_cockpit_opt_in_and_the_window_grant() {
    let catalog: serde_json::Value = serde_json::from_str(super::tests::CATALOG).unwrap();
    let mut registry = ControlRegistry::default();
    for permission in catalog["permissions"].as_array().unwrap() {
        registry
            .register_permission(serde_json::from_value(permission.clone()).unwrap())
            .unwrap();
    }
    for profile in catalog["profiles"].as_array().unwrap() {
        registry
            .register_profile(serde_json::from_value(profile.clone()).unwrap())
            .unwrap();
    }
    registry.finalize_authority().unwrap();
    let recovery = PermissionId::new("cockpit.recover").unwrap();
    for requested in [false, true] {
        for granted in [false, true] {
            let options = connect_options("cockpit", requested);
            let versions =
                ProtocolVersionRange::new(CURRENT_PROTOCOL_VERSION, CURRENT_PROTOCOL_VERSION)
                    .unwrap();
            let token = BearerToken::from_bytes([1; 32]);
            let instance = InstanceId::from_bytes([2; 16]);
            let request = HandshakeRequest {
                protocol_versions: versions,
                instance_id: instance.clone(),
                client_name: options.client_name,
                client_version: options.client_version,
                bearer_token: token.clone(),
                requested_profile: options.requested_profile,
                requested_scopes: options.requested_scopes,
            };
            let grant = HandshakeGrant {
                protocol_versions: versions,
                instance_id: instance,
                process_nonce: ProcessNonce::from_bytes([3; 16]),
                bearer_token: token,
                connection_id: ConnectionId::from_bytes([4; 16]),
                principal_id: PrincipalId::from_bytes([5; 16]),
                application_version: "test".into(),
                application_commit: "test".into(),
                profile_ceiling: ProfileId::new("cockpit").unwrap(),
                granted_scopes: connect_options("cockpit", granted).requested_scopes,
                limits: ProtocolLimits::default(),
            };
            let accepted = accept_handshake(&request, &grant, &registry).unwrap();
            assert_eq!(
                accepted.effective_scopes.contains(&recovery),
                requested && granted
            );
            assert!(
                !accepted
                    .effective_scopes
                    .contains(&PermissionId::new("trade").unwrap())
            );
        }
    }
    for profile in AVAILABLE_PROFILES {
        assert!(
            !connect_options(profile, false)
                .requested_scopes
                .contains(&recovery)
        );
        if *profile != "cockpit" {
            assert!(
                !connect_options(profile, true)
                    .requested_scopes
                    .contains(&recovery)
            );
        }
    }
}

#[test]
fn parsing_recovery_is_cockpit_only_for_serving_and_setup() {
    for mode in [vec![], vec!["setup", "--client", "codex"]] {
        for profile in AVAILABLE_PROFILES {
            let args: Vec<String> = mode
                .iter()
                .copied()
                .chain(["--profile", profile, "--allow-recovery"])
                .map(str::to_owned)
                .collect();
            let parsed = parse_args(&args);
            if *profile == "cockpit" {
                match parsed.unwrap() {
                    Command::Serve { allow_recovery, .. }
                    | Command::Setup { allow_recovery, .. } => assert!(allow_recovery),
                    Command::Help => panic!("recovery starts the requested mode"),
                }
            } else {
                assert!(parsed.is_err());
            }
        }
    }
    assert!(parse_args(&["--allow-recovery".into()]).is_err());
}
