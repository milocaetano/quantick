use super::*;
use crate::Model as _;

#[test]
fn access_panel_lifecycle_and_switch_presentation_follow_supplied_facts() {
    let mut state = State::default();
    assert!(!state.open);
    assert_eq!(Model::update(&mut state, Intent::Open), Effects::default());
    assert!(state.open);
    for (phase, identity_available, switch, menu, status) in [
        (
            Phase::Disabled,
            false,
            (false, false),
            "Local agent access…",
            "Off",
        ),
        (
            Phase::Disabled,
            true,
            (false, true),
            "Local agent access…",
            "Off",
        ),
        (
            Phase::Enabling,
            false,
            (true, true),
            "Local agent access: enabling…",
            "Enabling…",
        ),
        (
            Phase::Enabled,
            false,
            (true, true),
            "Local agent access: on",
            "On — reading only",
        ),
        (
            Phase::Disabling,
            true,
            (false, false),
            "Local agent access: disabling…",
            "Disabling and revoking clients…",
        ),
    ] {
        let facts = Facts {
            phase,
            identity_available,
            ..Facts::default()
        };
        assert_eq!(
            Model::update(&mut state, Intent::Observe(facts)),
            Effects::default()
        );
        assert!(state.open);
        assert_eq!(state.facts, facts);
        assert_eq!(state.facts.switch_state(), switch);
        assert_eq!(state.facts.menu_label(), menu);
        assert_eq!(state.facts.status(), status);
    }
    assert_eq!(Model::update(&mut state, Intent::Close), Effects::default());
    assert!(!state.open);
    let facts = Facts {
        phase: Phase::Enabled,
        ..Facts::default()
    };
    assert_eq!(
        Model::update(&mut state, Intent::Observed(facts)),
        Effects::default()
    );
    assert!(!state.open, "a terminal receipt cannot reopen the panel");
    assert_eq!(state.facts, facts);
}

#[test]
fn supplied_tiers_use_the_same_precedence_for_status_and_enable_label() {
    for cockpit in [false, true] {
        for annotate in [false, true] {
            for analyst in [false, true] {
                let facts = Facts {
                    phase: Phase::Enabled,
                    cockpit,
                    annotate,
                    analyst,
                    identity_available: true,
                };
                let (status, enable) = if cockpit {
                    (
                        "On — reading, answering and rearranging your charts",
                        "Enable access (reading, answering and rearranging)",
                    )
                } else if annotate {
                    (
                        "On — reading, and answering on the chart",
                        "Enable access (reading and answering)",
                    )
                } else if analyst {
                    (
                        "On — reading only, including private session data",
                        "Enable analyst access (reading, including private data)",
                    )
                } else {
                    ("On — reading only", "Enable observer access")
                };
                assert_eq!(facts.status(), status);
                assert_eq!(facts.enable_label(), enable);
            }
        }
    }
}

#[test]
fn requests_forward_typed_identity_without_predicting_authoritative_outcomes() {
    let connection =
        ConnectionId::from_bytes([9; quantick_control::limits::CONTROL_RUNTIME_ID_BYTES]);
    let permission = PermissionId::new("observe.chart").unwrap();
    for phase in [
        Phase::Disabled,
        Phase::Enabling,
        Phase::Enabled,
        Phase::Disabling,
    ] {
        for open in [false, true] {
            let mut state = State {
                open,
                facts: Facts {
                    phase,
                    ..Facts::default()
                },
            };
            for (intent, operation) in [
                (Intent::RequestEnable, LocalOperation::Enable),
                (Intent::RequestDisable, LocalOperation::Disable),
                (
                    Intent::RequestRevoke(connection.clone()),
                    LocalOperation::Revoke(connection.clone()),
                ),
                (
                    Intent::RequestPermission(permission.clone(), true),
                    LocalOperation::Permission(permission.clone(), true),
                ),
                (
                    Intent::RequestPermission(permission.clone(), false),
                    LocalOperation::Permission(permission.clone(), false),
                ),
            ] {
                assert_eq!(
                    Model::update(&mut state, intent).local_operation,
                    Some(operation)
                );
                assert_eq!(state.open, open);
                assert_eq!(
                    state.facts,
                    Facts {
                        phase,
                        ..Facts::default()
                    }
                );
            }
        }
    }
}

#[test]
fn primary_action_follows_host_supplied_phase_identity_and_tiers() {
    for identity_available in [false, true] {
        for phase in [
            Phase::Disabled,
            Phase::Enabling,
            Phase::Enabled,
            Phase::Disabling,
        ] {
            let facts = Facts {
                phase,
                identity_available,
                ..Facts::default()
            };
            let action = facts.primary_action();
            if phase == Phase::Disabling {
                assert!(action.is_none(), "the transition paints a spinner");
                continue;
            }
            let action = action.unwrap();
            assert_eq!(
                action.enabled,
                phase != Phase::Disabled || identity_available
            );
            assert_eq!(
                action.label,
                match phase {
                    Phase::Disabled => facts.enable_label(),
                    Phase::Enabling => "Cancel and keep access off",
                    Phase::Enabled => "Disable and revoke all clients",
                    Phase::Disabling => unreachable!(),
                }
            );
            assert!(matches!(
                (phase, action.intent),
                (Phase::Disabled, Intent::RequestEnable)
                    | (Phase::Enabling | Phase::Enabled, Intent::RequestDisable)
            ));
        }
    }
    for (cockpit, annotate, analyst) in [
        (true, true, true),
        (false, true, true),
        (false, false, true),
    ] {
        let facts = Facts {
            cockpit,
            annotate,
            analyst,
            ..Facts::default()
        };
        assert_eq!(facts.primary_action().unwrap().label, facts.enable_label());
    }
}
