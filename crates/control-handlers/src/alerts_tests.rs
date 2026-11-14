//! A second host consumes published wire schemas and the real admission gate.
#[path = "../../stores/src/scratch.rs"]
mod scratch;
use super::*;
use quantick_control::{error::codes, id::*, wire::*};
use quantick_control_host::{
    actions::ActionRegistry, admission::TierPolicy, authority, contract::CapabilityContract,
    idempotency::IdempotencyStore, projection::ProjectionRegistry,
};
use std::{collections::BTreeSet, sync::Arc};

struct Clock;
impl quantick_control_host::clock::HostClock for Clock {
    fn unix_ms(&self) -> i64 {
        100
    }
    fn monotonic(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
struct Host {
    runtime: quantick_stores::alerts::AlertRuntime,
    resolutions: usize,
}
impl AlertDefinitionsPort for Host {
    fn alert_book(&self) -> &quantick_alerts::AlertBook {
        &self.runtime.book
    }
    fn alert_book_at(&mut self) -> (&mut quantick_alerts::AlertBook, i64) {
        (&mut self.runtime.book, 100)
    }
    fn alert_command(&mut self, command: AlertCommand) -> Result<CommandResult, AlertError> {
        self.runtime.command(command, 100)
    }
    fn alert_resolve_target(
        &mut self,
        selector: schema::DrawingSelector,
    ) -> Result<quantick_alerts::DurableDrawingRef, schema::TargetResolveError> {
        if selector.drawing_id != 1 {
            return Err(schema::TargetResolveError::Missing);
        }
        self.resolutions += 1;
        Ok(quantick_alerts::DurableDrawingRef {
            workspace: "workspace".into(),
            origin: "tab".into(),
            pane: "pane".into(),
            drawing: 1,
            incarnation: "incarnation".into(),
        })
    }
}
struct Client {
    contract: CapabilityContract<()>,
    actions: ActionRegistry<Host, crate::test_support::FakeAccess>,
    access: std::cell::RefCell<crate::test_support::FakeAccess>,
    keys: Arc<IdempotencyStore>,
    grant: BTreeSet<PermissionId>,
}
impl Client {
    fn new() -> Self {
        let mut actions = ActionRegistry::new();
        register(&mut actions).unwrap();
        let builder = authority::builder().unwrap();
        let mut contract = builder
            .build(&ProjectionRegistry::<Host>::new(Arc::new(Clock)))
            .unwrap();
        for descriptor in actions.descriptors() {
            contract.register_external(descriptor.clone()).unwrap();
        }
        Self {
            contract,
            actions,
            access: std::cell::RefCell::new(crate::test_support::FakeAccess::with_budget(0)),
            keys: Arc::default(),
            grant: [
                authority::permission(authority::OBSERVE_PERMISSION_ID),
                authority::permission(authority::COCKPIT_PERMISSION_ID),
            ]
            .into(),
        }
    }
    fn request(&self, id: &str, payload: Value, key: Option<&str>) -> RequestEnvelope {
        RequestEnvelope {
            protocol_version: 1,
            request_id: RequestId::new("request").unwrap(),
            instance_id: InstanceId::from_bytes([1; 16]),
            capability_id: CapabilityId::new(id).unwrap(),
            capability_version: 1,
            expected_revisions: vec![],
            idempotency_key: key.map(|key| IdempotencyKey::new(key).unwrap()),
            dry_run: false,
            reason: None,
            payload,
        }
    }
    fn call(
        &self,
        host: &mut Host,
        id: &str,
        payload: Value,
        key: Option<&str>,
    ) -> Result<Value, ControlError> {
        // A serialized client envelope reaches the production admission path.
        let request = self.request(id, payload, key);
        let bytes = serde_json::to_vec(&request).unwrap();
        let request: RequestEnvelope = serde_json::from_slice(&bytes).unwrap();
        self.contract.admit(request.clone(),&self.grant,TierPolicy::STRICT,|id,v|self.actions.schemas(id,v),|_,_,_| -> Result<quantick_control_host::contract::ReadPreparation<()>,ControlError> { unreachable!("external action") })?;
        let principal = PrincipalId::from_bytes([2; 16]);
        let mut ticket = IdempotencyStore::ticket(&request.instance_id, &principal, &request)?;
        if let Some(ticket) = &mut ticket
            && let Some(response) = IdempotencyStore::admit(&self.keys, ticket, &request, 100)
        {
            return outcome(response);
        }
        let action = self.actions.lookup(id, 1).unwrap();
        let actor = ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: principal,
            client_name: "Independent alert consumer".into(),
            connection_id: ConnectionId::from_bytes([3; 16]),
            request_id: request.request_id.clone(),
            reason: None,
            requested_at_unix_ms: 100,
        };
        let response_outcome = match (action.handler)(
            host,
            &mut self.access.borrow_mut(),
            &actor,
            &request.payload,
        ) {
            Ok(result) => {
                assert!(
                    self.contract
                        .validate_output(&request.capability_id, 1, &result, |id, v| self
                            .actions
                            .schemas(id, v)),
                    "{id}: {result}"
                );
                quantick_control::canonical::validate_control_value(&result).unwrap();
                ResponseOutcome::Success { result }
            }
            Err(error) => ResponseOutcome::Failure { error },
        };
        let response = ResponseEnvelope {
            protocol_version: 1,
            request_id: request.request_id,
            instance_id: request.instance_id,
            capture_revision: None,
            module_revisions: vec![],
            outcome: response_outcome,
            warnings: vec![],
        };
        if let Some(ticket) = &ticket {
            self.keys.record(ticket, &response, 100);
        }
        let bytes = serde_json::to_vec(&response).unwrap();
        assert!(bytes.len() < quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES);
        outcome(serde_json::from_slice(&bytes).unwrap())
    }
}
fn outcome(response: ResponseEnvelope) -> Result<Value, ControlError> {
    match response.outcome {
        ResponseOutcome::Success { result } => Ok(result),
        ResponseOutcome::Failure { error } => Err(error),
    }
}
fn draft() -> Value {
    json!({"version":1,"id":0,"name":"Price threshold","market":{"source":{"Live":{"feed":"fixture"}},"symbol":"ABC","aggregation":"tick:2"},"conditions":[{"operator":"GreaterThan","target":{"Value":"100.25"}}],"frequency":"Once","expiration":"Never","message":"Threshold reached","notification":{"popup":true,"toast":true,"sound":"beep","sound_seconds":null}})
}
#[test]
fn alert_crud_and_history() {
    let directory = scratch::thread_dir("alert-admission");
    let path = directory.join("alerts.toml");
    let mut host = Host {
        runtime: quantick_stores::alerts::AlertRuntime::open(path.clone(), 100),
        resolutions: 0,
    };
    let mut client = Client::new();
    let spec = draft();
    client
        .grant
        .remove(&authority::permission(authority::COCKPIT_PERMISSION_ID));
    assert_eq!(
        client
            .call(
                &mut host,
                schema::CREATE,
                json!({"spec":spec}),
                Some("create")
            )
            .unwrap_err()
            .code
            .as_str(),
        codes::PERMISSION_DENIED
    );
    assert!(host.alert_views().unwrap().is_empty());
    client
        .grant
        .insert(authority::permission(authority::COCKPIT_PERMISSION_ID));
    assert!(
        client
            .call(&mut host, schema::CREATE, json!({"spec":spec}), None)
            .is_err()
    );
    assert_eq!(
        client
            .call(&mut host, schema::VALIDATE, json!({"spec":spec}), None)
            .unwrap(),
        json!({"valid":true})
    );
    let mut invalid = spec.clone();
    invalid["conditions"][0]["operator"] = json!("Inside");
    let error = client
        .call(&mut host, schema::VALIDATE, json!({"spec":invalid}), None)
        .unwrap_err();
    assert_eq!(error.context.details.unwrap()["field"], "operator");
    let created = client
        .call(
            &mut host,
            schema::CREATE,
            json!({"spec":spec}),
            Some("create"),
        )
        .unwrap();
    assert_eq!(
        created,
        client
            .call(
                &mut host,
                schema::CREATE,
                json!({"spec":spec}),
                Some("create")
            )
            .unwrap()
    );
    assert_eq!(host.alert_views().unwrap().len(), 1);
    let mut changed = spec.clone();
    changed["name"] = json!("Other");
    assert_eq!(
        client
            .call(
                &mut host,
                schema::CREATE,
                json!({"spec":changed}),
                Some("create")
            )
            .unwrap_err()
            .code
            .as_str(),
        codes::IDEMPOTENCY_CONFLICT
    );
    let id = created["id"].as_u64().unwrap();
    let mut saved = client
        .call(&mut host, schema::READ, json!({"id":id}), None)
        .unwrap()["spec"]
        .clone();
    saved["message"] = json!("Edited threshold");
    let edited = client
        .call(
            &mut host,
            schema::EDIT,
            json!({"spec":saved,"expected_revision":1}),
            Some("edit"),
        )
        .unwrap();
    let error = client
        .call(
            &mut host,
            schema::PAUSE,
            json!({"id":id,"expected_revision":1}),
            Some("stale"),
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), codes::REVISION_CONFLICT);
    assert_eq!(
        error.context.details.unwrap()["actual_revision"],
        edited["revision"]
    );
    let mut revision = edited["revision"].as_u64().unwrap();
    for (operation, key) in [
        (schema::PAUSE, "pause"),
        (schema::RESUME, "resume"),
        (schema::REARM, "rearm"),
    ] {
        revision = client
            .call(
                &mut host,
                operation,
                json!({"id":id,"expected_revision":revision}),
                Some(key),
            )
            .unwrap()["revision"]
            .as_u64()
            .unwrap();
    }
    host.runtime.reload(100).unwrap();
    assert_eq!(
        client
            .call(&mut host, schema::READ, json!({"id":id}), None)
            .unwrap()["revision"],
        revision
    );
    prove_history_and_target(&mut client, &mut host, id, revision);
}
fn prove_history_and_target(client: &mut Client, host: &mut Host, id: u64, revision: u64) {
    // A real core event, through the persisted host, becomes immutable wire history.
    let market = host.alert_views().unwrap()[0].spec.market.clone();
    let input = quantick_alerts::AlertInput {
        market: &market,
        utc_ms: 200,
        session_ms: 200,
        bar: quantick_alerts::BarIdentity {
            epoch: 1,
            sequence: 1,
        },
        kind: quantick_alerts::ObservationKind::Trade,
        coordinate: 1.,
        price: 101.5,
        availability: Ok(()),
        drawings: &[],
    };
    host.runtime.book.evaluate(&input, &mut host.runtime.events);
    // Restoration deliberately seeds rather than replaying historical triggers.
    host.runtime.book.evaluate(
        &quantick_alerts::AlertInput {
            price: 102.5,
            ..input
        },
        &mut host.runtime.events,
    );
    host.runtime.flush().unwrap();
    let history = client.call(host, schema::HISTORY, json!({}), None).unwrap();
    assert_eq!(history["events"].as_array().unwrap().len(), 1);
    assert_eq!(history["events"][0]["spec"]["message"], "Edited threshold");
    assert!(history["events"][0]["price"].is_string());
    assert_eq!(
        host.runtime.events.len(),
        1,
        "mutations and retries never deliver"
    );
    let persisted = std::fs::read(&host.runtime.store.path).unwrap();
    for (operation, payload) in [
        (
            schema::DELETE,
            json!({"id":id,"expected_revision":revision}),
        ),
        (schema::CLEAR_HISTORY, json!({})),
    ] {
        assert_eq!(
            client
                .call(host, operation, payload, Some("denied-removal"))
                .unwrap_err()
                .code
                .as_str(),
            codes::PERMISSION_DENIED,
        );
    }
    assert_eq!(std::fs::read(&host.runtime.store.path).unwrap(), persisted);
    assert_eq!(host.alert_views().unwrap().len(), 1);
    assert_eq!(
        client.call(host, schema::HISTORY, json!({}), None).unwrap(),
        history
    );
    client.grant.insert(authority::permission(
        authority::COCKPIT_ALERT_DELETE_PERMISSION_ID,
    ));
    assert_eq!(
        client
            .call(
                host,
                schema::DELETE,
                json!({"id":id,"expected_revision":revision + 1}),
                Some("stale-delete")
            )
            .unwrap_err()
            .code
            .as_str(),
        codes::REVISION_CONFLICT
    );
    client
        .call(
            host,
            schema::DELETE,
            json!({"id":id,"expected_revision":revision}),
            Some("delete"),
        )
        .unwrap();
    assert!(
        client
            .call(host, schema::READ, json!({"id":id}), None)
            .is_err()
    );
    assert_eq!(
        client.call(host, schema::HISTORY, json!({}), None).unwrap(),
        history
    );
    assert!(
        client
            .call(host, schema::LIST, json!({"limit":33}), None)
            .is_err()
    );
    assert_eq!(
        client.call(host, schema::LIST, json!({}), None).unwrap()["alerts"],
        json!([])
    );
    client
        .call(host, schema::CLEAR_HISTORY, json!({}), Some("clear"))
        .unwrap();
    assert_eq!(
        client.call(host, schema::HISTORY, json!({}), None).unwrap()["events"],
        json!([])
    );
    let selector = json!({"tab_id":1,"pane_id":1,"drawing_id":1});
    let target = client
        .call(
            host,
            schema::TARGET_RESOLVE,
            selector.clone(),
            Some("resolve"),
        )
        .unwrap();
    assert_eq!(
        client
            .call(host, schema::TARGET_RESOLVE, selector, Some("resolve"))
            .unwrap(),
        target
    );
    assert_eq!(host.resolutions, 1);
    let access = client.access.borrow();
    assert_eq!(
        access.events.len(),
        8,
        "only successful distinct mutations journal; retries never duplicate"
    );
    for (event, operation) in access.events.iter().zip([
        schema::CREATE,
        schema::EDIT,
        schema::PAUSE,
        schema::RESUME,
        schema::REARM,
        schema::DELETE,
        schema::CLEAR_HISTORY,
        schema::TARGET_RESOLVE,
    ]) {
        assert_eq!(event.kind.as_str(), schema::CHANGED_EVENT);
        assert_eq!(event.payload["operation"], operation);
        assert_eq!(event.payload["request_id"], "request");
        assert!(event.payload["receipt"].is_object());
        assert!(
            serde_json::to_vec(&event.payload).unwrap().len()
                < quantick_control::limits::CONTROL_EVENT_MAX_BYTES
        );
    }
    assert_eq!(access.events.last().unwrap().payload["receipt"], target);
    drop(access);

    assert!(
        client
            .call(
                host,
                schema::TARGET_RESOLVE,
                json!({"tab_id":1,"pane_id":1,"drawing_id":2}),
                Some("missing")
            )
            .is_err()
    );
}
#[test]
fn alert_published_catalog_matches_domain() {
    let spec = schema::spec_schema();
    let catalog = &spec["x-operator-targets"];
    for operator in quantick_alerts::operators::OPERATORS {
        let name = serde_json::to_value(operator.operator).unwrap();
        assert_eq!(catalog[name.as_str().unwrap()], json!(operator.targets));
    }
    let expected: Vec<_> = std::iter::once(Value::Null)
        .chain(quantick_alerts::sound::AlertSound::all().map(|sound| json!(sound.token())))
        .collect();
    assert_eq!(
        spec["properties"]["notification"]["properties"]["sound"]["enum"],
        json!(expected)
    );
    for id in schema::OPERATIONS {
        let descriptor = schema::descriptor(id);
        quantick_control::schema::CompiledSchema::new(&descriptor.input_schema).unwrap();
        quantick_control::schema::CompiledSchema::new(&descriptor.output_schema).unwrap();
    }
}

#[test]
fn alert_wire_prices_reject_precision_loss() {
    for price in ["100.25", "0", "-1.5", "0.0000001", "100000000000000000000"] {
        let mut spec = draft();
        spec["conditions"][0]["target"]["Value"] = json!(price);
        let parsed = read_spec(spec.clone()).unwrap();
        assert_eq!(wire_spec(&parsed), spec);
    }
    for price in ["1.00000000000000000001", "-0", "1.0", "1e3"] {
        let mut spec = draft();
        spec["conditions"][0]["target"]["Value"] = json!(price);
        assert!(read_spec(spec).is_err(), "{price}");
    }
    let mut spec = draft();
    spec["conditions"][0]["target"]["Value"] = json!(format!("0.{}1", "0".repeat(400)));
    assert!(
        read_spec(spec).is_err(),
        "nonzero underflow is never silently zero"
    );
    assert_eq!(wire_price(-0.0), "0");
}
#[test]
fn alert_pages_stop_before_the_envelope_byte_limit() {
    let mut spec = draft();
    let text = format!("x{}", "\u{0001}".repeat(2047));
    spec["name"] = json!(text);
    spec["message"] = json!(text);
    spec["market"]["source"]["Live"]["feed"] = json!(text);
    spec["market"]["symbol"] = json!(text);
    spec["market"]["aggregation"] = json!(text);
    spec["conditions"] = json!(vec![
        json!({"operator":"Crossing", "target":{"Drawing":{"workspace":text,"origin":text,"pane":text,"drawing":1,"incarnation":text}}});
        8
    ]);
    let parsed = read_spec(spec).unwrap();
    let mut book = quantick_alerts::AlertBook::default();
    for _ in 0..32 {
        book.command(AlertCommand::Create(parsed.clone()), 0)
            .unwrap();
    }
    let values: Vec<_> = book.views().unwrap().into_iter().map(view).collect();
    let first = bounded_page(values.clone()).unwrap();
    assert!(!first.is_empty() && first.len() < 32);
    let last = first.last().unwrap()["spec"]["id"].as_u64().unwrap();
    let second = bounded_page(
        values
            .into_iter()
            .filter(|value| value["spec"]["id"].as_u64().unwrap() > last),
    )
    .unwrap();
    assert_eq!(second[0]["spec"]["id"], last + 1);
    let bytes = serde_json::to_vec(&json!({"alerts":first,"next_after":last})).unwrap();
    assert!(bytes.len() < quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES - 512);
}

#[test]
fn alert_removal_contract_is_irreversible_and_separately_granted() {
    let permission = authority::permissions()
        .into_iter()
        .find(|p| p.id.as_str() == authority::COCKPIT_ALERT_DELETE_PERMISSION_ID)
        .unwrap();
    assert!(permission.sensitive);
    assert_eq!(
        permission.default_grant,
        quantick_control::registry::DefaultGrant::Prompt
    );
    assert_eq!(
        permission.profile_ceilings,
        [ProfileId::new(authority::COCKPIT_PROFILE_ID).unwrap()].into()
    );
    let effect = authority::effects()
        .into_iter()
        .find(|e| e.id.as_str() == authority::ALERT_DELETE_EFFECT_ID)
        .unwrap();
    assert_eq!(effect.permission_floor, permission.id);
    assert!(!effect.constraints.durable_requires_reversible);
    for operation in schema::OPERATIONS {
        let descriptor = schema::descriptor(operation);
        let removal = matches!(*operation, schema::DELETE | schema::CLEAR_HISTORY);
        assert_eq!(
            descriptor.effect.as_str() == authority::ALERT_DELETE_EFFECT_ID,
            removal
        );
        assert_eq!(
            descriptor.required_permissions.contains(&permission.id),
            removal
        );
        if removal {
            assert!(!descriptor.reversible);
            assert_eq!(descriptor.risk_flags, effect.required_risk_flags);
            assert_eq!(
                descriptor.persistence,
                quantick_control::registry::EffectPersistence::Durable
            );
            assert_eq!(
                descriptor.idempotency,
                quantick_control::registry::IdempotencyPolicy::Required
            );
            assert_eq!(
                descriptor.revision_policy,
                quantick_control::registry::RevisionPolicy::OptionalForAdditive
            );
        }
    }
}
