use super::*;
use crate::authority;
use serde_json::json;

fn builder() -> ContractBuilder {
    let mut builder = authority::builder().unwrap();
    builder
        .register_module(ModuleDescriptor {
            id: authority::module("fixture"),
            title: "Fixture".into(),
            description: "Fixture module".into(),
        })
        .unwrap();
    builder
}

fn scope(name: &str) -> SnapshotScopeDescriptor {
    SnapshotScopeDescriptor {
        id: SnapshotScopeId::new(name).unwrap(),
        module_id: authority::module("fixture"),
        schema_version: 1,
        title: "Fixture".into(),
        description: "Fixture scope".into(),
        required_permissions: [authority::permission("observe")].into(),
        schema: json!({"type": "object", "additionalProperties": false}),
    }
}

#[test]
fn metadata_scopes_are_validated_and_sorted_without_a_host() {
    let contract = builder()
        .build_catalog(vec![scope("fixture.z"), scope("fixture.a")])
        .unwrap();
    let ids: Vec<_> = contract
        .snapshot_scopes()
        .iter()
        .map(|scope| scope.id.as_str())
        .collect();
    assert_eq!(ids, ["fixture.a", "fixture.z"]);
    assert_eq!(
        contract
            .readable_scopes(&[authority::permission("observe")].into())
            .len(),
        2
    );
    assert_eq!(contract.read_count(), 0);
}

#[test]
fn raw_metadata_cannot_bypass_live_scope_validation() {
    let changes: [fn(&mut SnapshotScopeDescriptor); 8] = [
        |scope| scope.schema_version = 0,
        |scope| scope.module_id = authority::module("missing"),
        |scope| scope.id = SnapshotScopeId::new("another.scope").unwrap(),
        |scope| scope.title = " ".into(),
        |scope| scope.description.clear(),
        |scope| scope.required_permissions.clear(),
        |scope| {
            scope
                .required_permissions
                .insert(authority::permission("unknown"));
        },
        |scope| scope.schema = json!({"type": "invalid-type"}),
    ];
    for (index, change) in changes.into_iter().enumerate() {
        let mut value = scope("fixture.scope");
        change(&mut value);
        assert!(
            builder().build_catalog(vec![value]).is_err(),
            "invalid metadata case {index} was accepted"
        );
    }
    assert!(
        builder()
            .build_catalog(vec![scope("fixture.scope"), scope("fixture.scope")])
            .is_err()
    );
}
