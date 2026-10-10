//! Frozen A1C app-contract admission/output evidence; never executes UI/actions.
use super::*;
mod fixtures;
mod observations;
use fixtures::{Corpus, request};

fn outcome(result: Result<PreparedRequest, ControlError>) -> Value {
    match result {
        Err(error) => json!({"error": error}),
        Ok(prepared) => {
            let dispatch = match prepared.dispatch {
                PreparedDispatch::Worker(_) => json!({"route": "worker"}),
                PreparedDispatch::Ui(_) => json!({"route": "ui"}),
                PreparedDispatch::Parked(wait) => json!({"route": "parked", "input": wait.input}),
                PreparedDispatch::Action(action) => {
                    json!({"route": "action", "capability_id": action.capability_id, "capability_version": action.capability_version, "input": action.input})
                }
            };
            json!({"envelope": prepared.envelope, "required_permissions": prepared.required_permissions, "dispatch": dispatch})
        }
    }
}

#[test]
fn complete_contract_and_admission_signatures() {
    let corpus = Corpus::new();
    corpus.assert_cases();
    let mut signatures = serde_json::Map::new();
    for case in 0..5 {
        signatures.insert(fixtures::CASES[case].to_owned(), json!({
            "request": corpus.requests[case], "grant": corpus.grants[case],
            "outcome": outcome(corpus.contract.prepare(corpus.requests[case].clone(), &corpus.grants[case]))
        }));
    }
    let grant = corpus.contract.default_grant();
    let mut extra = vec![
        (
            "dynamic_sensitive_denial",
            request(
                "snapshot.read",
                json!({"scopes": ["interaction.selection"]}),
            ),
        ),
        (
            "duplicate_scope",
            request(
                "snapshot.read",
                json!({"scopes": ["system.info", "system.info"]}),
            ),
        ),
        (
            "unknown_scope",
            request("snapshot.read", json!({"scopes": ["fixture.unknown"]})),
        ),
    ];
    let standard = || request("snapshot.read", json!({"scopes": ["system.info"]}));
    let mut keyed = standard();
    keyed.idempotency_key = Some(quantick_control::id::IdempotencyKey::new("a1c-key").unwrap());
    extra.push(("forbidden_key", keyed));
    let mut dry_run = standard();
    dry_run.dry_run = true;
    extra.push(("strict_dry_run", dry_run));
    let mut revision = standard();
    revision.expected_revisions.push(ModuleRevision {
        module_id: module("chart"),
        revision: WireU64::new(1),
    });
    extra.push(("strict_supplied_revision", revision));
    let mut malformed = standard();
    malformed.capability_version = 0;
    extra.push(("malformed_envelope", malformed));
    let mut version = standard();
    version.capability_version = 2;
    extra.push(("version_mismatch", version));
    for (name, envelope) in extra {
        let result = corpus.contract.prepare(envelope.clone(), &grant);
        assert!(result.is_err(), "{name} must fail closed");
        signatures.insert(
            name.to_owned(),
            json!({"request": envelope, "grant": grant, "outcome": outcome(result)}),
        );
    }
    let mut malformed_denied = standard();
    malformed_denied.protocol_version = 0;
    let result = corpus
        .contract
        .prepare(malformed_denied.clone(), &BTreeSet::new());
    assert_eq!(
        result.as_ref().unwrap_err().code.as_str(),
        codes::INVALID_REQUEST
    );
    signatures.insert(
        "envelope_before_static_denial".to_owned(),
        json!({"request": malformed_denied, "grant": [], "outcome": outcome(result)}),
    );
    let mark_schema = corpus
        .contract
        .registry()
        .capability(&corpus.mark_id, 1)
        .unwrap();
    signatures.insert("external_output_selection".to_owned(), json!({
        "descriptor": mark_schema, "valid_output": corpus.mark, "invalid_output": corpus.invalid_output,
        "valid": corpus.contract.validate_output(&corpus.mark_id, 1, &corpus.mark),
        "invalid": corpus.contract.validate_output(&corpus.mark_id, 1, &corpus.invalid_output),
        "wrong_version": corpus.contract.validate_output(&corpus.mark_id, 2, &corpus.mark)
    }));
    signatures.insert("read_output_selection".to_owned(), json!({
        "valid": corpus.contract.validate_output(&corpus.describe_id, 1, &corpus.describe),
        "invalid": corpus.contract.validate_output(&corpus.describe_id, 1, &corpus.invalid_output),
        "wrong_version": corpus.contract.validate_output(&corpus.describe_id, 2, &corpus.describe)
    }));
    let raw = serde_json::to_vec_pretty(&corpus.describe).unwrap();
    let mut normalized = corpus.describe.clone();
    let commit = normalized["application_commit"].as_str().unwrap();
    assert_eq!(
        commit,
        option_env!("QUANTICK_GIT_COMMIT").unwrap_or("unknown")
    );
    if commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        normalized["application_commit"] = json!("<actual-build-commit>");
    }
    let outputs = [
        ("describe.raw.json", raw),
        (
            "describe.normalized.json",
            serde_json::to_vec_pretty(&normalized).unwrap(),
        ),
        (
            "admission-outcomes.json",
            serde_json::to_vec_pretty(&signatures).unwrap(),
        ),
        (
            "permissions.json",
            serde_json::to_vec_pretty(&json!({"default_grant": grant,
            "selectable": corpus.contract.selectable_permissions().collect::<Vec<_>>(),
            "readable": corpus.contract.readable_scopes(&grant)}))
            .unwrap(),
        ),
    ];
    for (name, bytes) in outputs {
        eprintln!("A1C_SIGNATURE {name} bytes={}", bytes.len());
        if let Some(directory) = std::env::var_os("A1C_SIGNATURE_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join(name), &bytes).unwrap();
        }
        if name != "describe.raw.json"
            && let Some(directory) = std::env::var_os("A1C_COMPARE_DIR")
        {
            assert_eq!(
                std::fs::read(std::path::PathBuf::from(directory).join(name)).unwrap(),
                bytes,
                "complete {name} bytes changed"
            );
        }
    }
}
