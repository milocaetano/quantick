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
            let options = connect_options("cockpit", requested, false, false);
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
                granted_scopes: connect_options("cockpit", granted, false, false).requested_scopes,
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
            !connect_options(profile, false, false, false)
                .requested_scopes
                .contains(&recovery)
        );
        if *profile != "cockpit" {
            assert!(
                !connect_options(profile, true, false, false)
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

#[test]
fn alert_delete_scope_opt_in() {
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
    let deletion = PermissionId::new("cockpit.alert.delete").unwrap();
    for requested in [false, true] {
        for granted in [false, true] {
            let options = connect_options("cockpit", false, requested, false);
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
                granted_scopes: connect_options("cockpit", false, granted, false).requested_scopes,
                limits: ProtocolLimits::default(),
            };
            let accepted = accept_handshake(&request, &grant, &registry).unwrap();
            assert_eq!(
                accepted.effective_scopes.contains(&deletion),
                requested && granted
            );
            for id in ["alerts.delete", "alerts.clear_history"] {
                let capability = catalog["capabilities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["id"] == id)
                    .unwrap();
                let authorized = capability["required_permissions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|permission| {
                        accepted
                            .effective_scopes
                            .contains(&PermissionId::new(permission.as_str().unwrap()).unwrap())
                    });
                assert_eq!(authorized, requested && granted, "{id}");
            }
            assert!(
                !accepted
                    .effective_scopes
                    .contains(&PermissionId::new("trade").unwrap())
            );
        }
    }
    for profile in AVAILABLE_PROFILES {
        assert!(
            !connect_options(profile, false, false, false)
                .requested_scopes
                .contains(&deletion)
        );
        if *profile != "cockpit" {
            assert!(
                !connect_options(profile, false, true, false)
                    .requested_scopes
                    .contains(&deletion)
            );
        }
    }
    for mode in [vec![], vec!["setup", "--client", "codex"]] {
        for profile in AVAILABLE_PROFILES {
            let args: Vec<String> = mode
                .iter()
                .copied()
                .chain(["--profile", profile, "--allow-alert-delete"])
                .map(str::to_owned)
                .collect();
            let parsed = parse_args(&args);
            if *profile == "cockpit" {
                match parsed.unwrap() {
                    Command::Serve {
                        allow_alert_delete,
                        allow_recovery,
                        ..
                    }
                    | Command::Setup {
                        allow_alert_delete,
                        allow_recovery,
                        ..
                    } => {
                        assert!(allow_alert_delete);
                        assert!(!allow_recovery, "alert deletion does not enable recovery");
                    }
                    Command::Help => panic!("the opt-in starts the requested mode"),
                }
            } else {
                assert!(parsed.is_err());
            }
        }
    }
    assert!(parse_args(&["--allow-alert-delete".into()]).is_err());
    for client in [SetupClient::Codex, SetupClient::Claude] {
        let text = walkthrough(
            client,
            std::path::Path::new("quantick-mcp"),
            "cockpit",
            false,
            true,
            false,
        );
        assert!(text.contains("--allow-alert-delete"));
        assert!(text.contains("cockpit.alert.delete"));
        assert!(!text.contains("--allow-recovery"));
    }
}

#[test]
fn indicators_require_both_explicit_cockpit_opt_in_and_the_window_grant() {
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
    let recovery = PermissionId::new("cockpit.indicators").unwrap();
    for requested in [false, true] {
        for granted in [false, true] {
            let options = connect_options("cockpit", false, false, requested);
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
                granted_scopes: connect_options("cockpit", false, false, granted).requested_scopes,
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
            !connect_options(profile, false, false, false)
                .requested_scopes
                .contains(&recovery)
        );
        if *profile != "cockpit" {
            assert!(
                !connect_options(profile, false, false, true)
                    .requested_scopes
                    .contains(&recovery)
            );
        }
    }
}

#[test]
fn parsing_indicators_is_cockpit_only_for_serving_and_setup() {
    for mode in [vec![], vec!["setup", "--client", "codex"]] {
        for profile in AVAILABLE_PROFILES {
            let args: Vec<String> = mode
                .iter()
                .copied()
                .chain(["--profile", profile, "--allow-indicators"])
                .map(str::to_owned)
                .collect();
            let parsed = parse_args(&args);
            if *profile == "cockpit" {
                match parsed.unwrap() {
                    Command::Serve {
                        allow_indicators, ..
                    }
                    | Command::Setup {
                        allow_indicators, ..
                    } => assert!(allow_indicators),
                    Command::Help => panic!("indicator opt-in starts the requested mode"),
                }
            } else {
                assert!(parsed.is_err());
            }
        }
    }
    assert!(parse_args(&["--allow-indicators".into()]).is_err());
}

#[test]
fn indicator_opt_in_advertises_sensitive_effects_without_raising_other_profiles() {
    use quantick_mcp::tools::{INVOKE, tools_with_sensitive_edits};
    for profile in AVAILABLE_PROFILES {
        for allow in [false, true] {
            let tools = tools_with_sensitive_edits(profile, allow);
            let invoke = tools.iter().find(|tool| tool.name == INVOKE).unwrap();
            assert_eq!(
                invoke.annotations.destructive_hint,
                *profile == "cockpit" && allow
            );
        }
    }
}

#[test]
fn alerts_and_indicator_permissions_survive_sync() {
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
    let deletion = PermissionId::new("cockpit.alert.delete").unwrap();
    let indicators = PermissionId::new("cockpit.indicators").unwrap();
    for requested in [false, true] {
        for requested_indicators in [false, true] {
            for granted in [false, true] {
                for granted_indicators in [false, true] {
                    let options =
                        connect_options("cockpit", false, requested, requested_indicators);
                    let versions = ProtocolVersionRange::new(
                        CURRENT_PROTOCOL_VERSION,
                        CURRENT_PROTOCOL_VERSION,
                    )
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
                        granted_scopes: connect_options(
                            "cockpit",
                            false,
                            granted,
                            granted_indicators,
                        )
                        .requested_scopes,
                        limits: ProtocolLimits::default(),
                    };
                    let accepted = accept_handshake(&request, &grant, &registry).unwrap();
                    assert_eq!(
                        accepted.effective_scopes.contains(&deletion),
                        requested && granted
                    );
                    assert_eq!(
                        accepted.effective_scopes.contains(&indicators),
                        requested_indicators && granted_indicators
                    );
                    for (id, authorized) in [
                        ("alerts.delete", requested && granted),
                        ("alerts.clear_history", requested && granted),
                        (
                            "indicator.native.edit",
                            requested_indicators && granted_indicators,
                        ),
                    ] {
                        let capability = catalog["capabilities"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|row| row["id"] == id)
                            .unwrap();
                        assert_eq!(
                            capability["required_permissions"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .all(|permission| accepted.effective_scopes.contains(
                                    &PermissionId::new(permission.as_str().unwrap()).unwrap()
                                )),
                            authorized,
                            "{id}"
                        );
                    }
                    assert!(
                        !accepted
                            .effective_scopes
                            .contains(&PermissionId::new("trade").unwrap())
                    );
                }
            }
        }
    }
    for deletion in [false, true] {
        for indicators in [false, true] {
            for client in [SetupClient::Codex, SetupClient::Claude] {
                let text = walkthrough(
                    client,
                    std::path::Path::new("quantick-mcp"),
                    "cockpit",
                    false,
                    deletion,
                    indicators,
                );
                assert_eq!(text.contains("--allow-alert-delete"), deletion);
                assert_eq!(text.contains("--allow-indicators"), indicators);
                assert!(!text.contains("--allow-recovery"));
                for setup in [false, true] {
                    let mut args = vec!["--profile".into(), "cockpit".into()];
                    if setup {
                        args.splice(
                            0..0,
                            [
                                "setup".into(),
                                "--client".into(),
                                match client {
                                    SetupClient::Codex => "codex",
                                    SetupClient::Claude => "claude",
                                }
                                .into(),
                            ],
                        );
                    }
                    if deletion {
                        args.push("--allow-alert-delete".into());
                    }
                    if indicators {
                        args.push("--allow-indicators".into());
                    }
                    match parse_args(&args).unwrap() {
                        Command::Serve {
                            allow_alert_delete,
                            allow_indicators,
                            allow_recovery,
                            ..
                        }
                        | Command::Setup {
                            allow_alert_delete,
                            allow_indicators,
                            allow_recovery,
                            ..
                        } => {
                            assert_eq!(allow_alert_delete, deletion);
                            assert_eq!(allow_indicators, indicators);
                            assert!(!allow_recovery);
                        }
                        _ => panic!("serving preserves independent opt-ins"),
                    }
                }
            }
        }
    }
}
