use quantick_control::id::{ModuleId, PermissionId, SnapshotScopeId};
use quantick_control::limits::CONTROL_MAX_SNAPSHOT_SCOPES;
use quantick_control_host::catalogue::{SnapshotScopeDescriptor, readable_scopes};
use serde_json::json;
use std::collections::BTreeSet;

fn grant(names: &[&str]) -> BTreeSet<PermissionId> {
    names
        .iter()
        .map(|name| PermissionId::new(*name).unwrap())
        .collect()
}

fn scope(name: &str, permissions: &[&str]) -> SnapshotScopeDescriptor {
    SnapshotScopeDescriptor {
        id: SnapshotScopeId::new(name).unwrap(),
        module_id: ModuleId::new("paper").unwrap(),
        schema_version: 1,
        title: "Paper account".to_owned(),
        description: "Account observation requires every declared grant".to_owned(),
        required_permissions: grant(permissions),
        schema: json!({"type": "object", "properties": {}, "additionalProperties": false}),
    }
}

#[test]
fn a_scope_needing_two_permissions_is_hidden_until_both_are_granted() {
    let scopes = [scope("paper.account", &["observe", "observe.paper"])];
    assert!(readable_scopes(&scopes, &grant(&[])).is_empty());
    assert!(readable_scopes(&scopes, &grant(&["observe"])).is_empty());
    assert!(readable_scopes(&scopes, &grant(&["observe.paper"])).is_empty());
    assert_eq!(
        readable_scopes(&scopes, &grant(&["observe", "observe.paper"])),
        [scopes[0].id.clone()]
    );
    assert_eq!(
        readable_scopes(&scopes, &grant(&["observe", "observe.paper", "notify"])),
        [scopes[0].id.clone()]
    );
}

#[test]
fn denied_scopes_do_not_consume_the_limit_of_readable_scopes() {
    let mut scopes = Vec::new();
    for index in 0..CONTROL_MAX_SNAPSHOT_SCOPES {
        scopes.push(scope(&format!("paper.private{index}"), &["observe.paper"]));
    }
    let mut expected = Vec::new();
    for index in 0..=CONTROL_MAX_SNAPSHOT_SCOPES {
        let descriptor = scope(&format!("paper.public{index}"), &["observe"]);
        if index < CONTROL_MAX_SNAPSHOT_SCOPES {
            expected.push(descriptor.id.clone());
        }
        scopes.push(descriptor);
    }
    let readable = readable_scopes(&scopes, &grant(&["observe"]));
    assert_eq!(readable, expected);
    assert_eq!(readable.len(), CONTROL_MAX_SNAPSHOT_SCOPES);
    assert!(!readable.contains(
        &SnapshotScopeId::new(format!("paper.public{CONTROL_MAX_SNAPSHOT_SCOPES}")).unwrap()
    ));
}

#[test]
fn revoking_a_permission_immediately_removes_sensitive_scopes_from_the_next_read() {
    let scopes = [
        scope("paper.account", &["observe", "observe.paper"]),
        scope("paper.status", &["observe"]),
    ];
    let mut granted = grant(&["observe", "observe.paper"]);
    assert_eq!(
        readable_scopes(&scopes, &granted),
        [scopes[0].id.clone(), scopes[1].id.clone()]
    );
    granted.remove(&PermissionId::new("observe.paper").unwrap());
    assert_eq!(readable_scopes(&scopes, &granted), [scopes[1].id.clone()]);
    granted.clear();
    assert!(readable_scopes(&scopes, &granted).is_empty());
    assert_eq!(
        scopes[0].required_permissions,
        grant(&["observe", "observe.paper"])
    );
}
