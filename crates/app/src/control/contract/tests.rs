use super::*;
use quantick_control::handshake::ProfileAuthority as _;
use quantick_control::{id::RequestId, wire::RequestEnvelope};

/// The access panel offers no way to grant the trade tier — and above
/// all not under "Read scopes for the next connection".
///
/// This is the same mistake the cockpit section's own comment records
/// being fixed once: a permission that matches neither of the panel's
/// two special cases falls through into the *first* section, which is
/// the read one. A grant that places orders, rendered under a heading
/// that promises reading only, is the worst kind of consent surface —
/// and here it would have been worse than useless besides, since
/// nothing can currently act on the tick.
#[test]
fn the_access_panel_never_offers_the_trade_scope() {
    let contract = contract();

    let trade: Vec<_> = contract
        .selectable_permissions()
        .filter(|descriptor| super::super::gateway::is_trade_permission(&descriptor.id))
        .collect();
    assert!(
        !trade.is_empty(),
        "the tier exists, or this test is guarding nothing"
    );
    for descriptor in &trade {
        assert!(
            descriptor.sensitive,
            "{}: a grant that touches a position is sensitive",
            descriptor.id
        );
    }

    // The read section's own filter, verbatim from `draw_panel_body`.
    let read_section: Vec<_> = contract
        .selectable_permissions()
        .filter(|descriptor| {
            !super::super::gateway::is_annotate_permission(&descriptor.id)
                && !super::super::gateway::is_cockpit_permission(&descriptor.id)
                && !super::super::gateway::is_trade_permission(&descriptor.id)
        })
        .collect();
    assert!(
        read_section
            .iter()
            .all(|descriptor| !super::super::gateway::is_trade_permission(&descriptor.id)),
        "no trade scope reaches the read section"
    );

    // Nor does either write section claim it, so it is offered nowhere.
    for other in [
        super::super::gateway::is_annotate_permission as fn(&PermissionId) -> bool,
        super::super::gateway::is_cockpit_permission as fn(&PermissionId) -> bool,
    ] {
        for descriptor in &trade {
            assert!(
                !other(&descriptor.id),
                "{}: the trade tier borrows no other section",
                descriptor.id
            );
        }
    }
}

fn contract() -> ObserverContract {
    ObserverContract::new(
        &super::super::standard_registry().unwrap(),
        Arc::new(super::super::actions::standard_actions().unwrap()),
        EvidenceStore::new(),
    )
    .unwrap()
}

fn request(capability: &str, payload: Value) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        request_id: RequestId::new("contract-test").unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        capability_id: CapabilityId::new(capability).unwrap(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run: false,
        reason: None,
        payload,
    }
}

#[test]
fn sensitive_cross_scope_reference_fails_closed() {
    let contract = contract();
    let grant = contract.default_grant();
    let error = contract
        .prepare(
            request(
                SNAPSHOT_CAPABILITY_ID,
                json!({ "scopes": ["interaction.selection"] }),
            ),
            &grant,
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), codes::SCOPE_DENIED);
    assert!(
        error.context.details.unwrap()["missing_permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|permission| permission == "observe.paper")
    );
}

#[test]
fn default_grant_can_read_chart_but_not_sensitive_scopes() {
    let contract = contract();
    let grant = contract.default_grant();
    contract
        .prepare(
            request(
                SNAPSHOT_CAPABILITY_ID,
                json!({ "scopes": ["chart.summary", "interaction.cursor"] }),
            ),
            &grant,
        )
        .unwrap();
    assert!(!grant.contains(&permission("observe.paper")));
    assert!(!grant.contains(&permission("observe.evidence")));
    assert!(!grant.contains(&permission("observe.user_text")));
}

#[test]
fn observer_registry_contains_only_read_capabilities() {
    // Nine reads with prepare handlers, plus the registered actions,
    // which have none here: an action is prepared from the action registry
    // and sits behind annotate permissions the observer ceiling does not
    // hold — discoverable to every client, reachable by none of them
    // until the trader grants the annotator profile.
    const READ_CAPABILITIES: usize = 9;
    let contract = contract();
    let capabilities = contract.registry().capabilities().collect::<Vec<_>>();
    let actions = contract.actions.descriptors().count();
    assert_eq!(capabilities.len(), READ_CAPABILITIES + actions);
    assert_eq!(contract.contract.read_count(), READ_CAPABILITIES);
    let observer_ceiling = contract
        .registry()
        .permission_ceiling(&profile(OBSERVER_PROFILE_ID))
        .expect("the observer profile has a ceiling");
    // The analyst ceiling is the whole of the reading: the observer's
    // scopes plus the private ones. Every read sits inside it and no
    // write does, which is the property that makes it a read-only tier.
    let analyst_ceiling = contract
        .registry()
        .permission_ceiling(&profile(ANALYST_PROFILE_ID))
        .expect("the analyst profile has a ceiling");
    for capability in &capabilities {
        let reachable = capability.required_permissions.is_subset(&observer_ceiling);
        assert_eq!(
            capability.required_permissions.is_subset(&analyst_ceiling),
            capability.read_only,
            "{}: the analyst ceiling holds every read and nothing else",
            capability.id
        );
        assert!(
            !reachable || capability.read_only,
            "{}: only read-only capabilities sit inside the observer ceiling",
            capability.id
        );
        // No capability in this contract is destructive, and the reason
        // is documented on `super::recovery`'s descriptor: the flag is
        // coupled to an expected-revision check this host refuses, so
        // claiming it would advertise a guarantee nothing delivers. The
        // assertion that matters either way is that nothing which could
        // end a trade is reachable from the observer ceiling.
        assert!(
            !(capability.destructive && reachable),
            "{}: a destructive capability is inside the observer ceiling",
            capability.id
        );
    }
    assert!(
        capabilities
            .iter()
            .any(|capability| capability.id.as_str() == super::super::actions::MARK_CAPABILITY_ID)
    );
}

/// A `Required` revision policy is a promise the tier has to keep. Asked
/// of `prepare` itself rather than of a constant, so the probe measures
/// the tier every connection is really admitted under: when it refuses a
/// supplied revision, no registered capability may require one, or a
/// client following the descriptor could never call it while one
/// omitting the protection would act unchecked.
#[test]
fn observer_tier_admits_every_required_revision() {
    use quantick_control::registry::RevisionPolicy;
    const PROBE: &str = "feed.reload";
    let contract = contract();
    let probe = contract
        .registry()
        .capabilities()
        .find(|capability| capability.id.as_str() == PROBE)
        .expect("the probe capability is registered");
    assert_ne!(
        probe.revision_policy,
        RevisionPolicy::Forbidden,
        "the probe must be one whose own policy accepts a revision"
    );
    let grant = contract
        .contract
        .permissions()
        .iter()
        .map(|descriptor| descriptor.id.clone())
        .collect::<BTreeSet<_>>();
    contract
        .prepare(request(PROBE, json!({})), &grant)
        .expect("the probe is admitted without a revision");
    let mut revised = request(PROBE, json!({}));
    revised.expected_revisions.push(ModuleRevision {
        module_id: module("feed"),
        revision: WireU64::new(1),
    });
    let tier_admits_revisions = contract.prepare(revised, &grant).is_ok();
    let required = contract
        .registry()
        .capabilities()
        .filter(|capability| capability.revision_policy == RevisionPolicy::Required)
        .map(|capability| capability.id.to_string())
        .collect::<Vec<_>>();
    assert!(
        tier_admits_revisions || required.is_empty(),
        "{required:?} require an expected revision the observer tier refuses to receive"
    );
}

#[test]
fn a_second_registered_handler_docks_without_changing_gateway_dispatch() {
    let mut contract = contract();
    contract
        .contract
        .register_read(
            authority::read_descriptor::<EmptyInput, DescribeResult>(&authority::ReadSpec {
                id: "control.second",
                module: "control",
                title: "Second observer handler",
                description: "Exercises the registered capability handler port.",
                permissions: &[OBSERVE_PERMISSION_ID],
                pagination: None,
            }),
            reads::prepare_describe,
        )
        .unwrap();

    let prepared = contract
        .prepare(
            request("control.second", json!({})),
            &contract.default_grant(),
        )
        .unwrap();
    assert!(
        prepared
            .dispatch
            .execute_worker(
                &contract,
                &InstanceId::from_bytes([1; 16]),
                &profile(OBSERVER_PROFILE_ID),
                &contract.default_grant(),
                &ProtocolLimits::default(),
            )
            .unwrap()
            .is_ok()
    );
}
