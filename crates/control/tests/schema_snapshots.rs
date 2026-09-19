use quantick_control::{
    schema::validate_schema,
    schema_catalog::{PublicContractDocument, public_contract_documents},
};
use serde_json::Value;

/// Where the committed contracts live, resolved from the crate this test is
/// compiled in so the regeneration path below writes to the real files.
const SCHEMA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/control");

const REGENERATE: &str = "cargo test -p quantick-control --test schema_snapshots -- --ignored regenerate_public_contracts";

#[test]
fn generated_public_contracts_match_committed_files() {
    check_or_rewrite(false);
}

/// Rewrites the committed files from the generated documents. Regenerating by
/// hand is how a schema and its validator drift apart: the diff is what gets
/// reviewed, and it still has to be reviewed, but producing it should not be
/// transcription work. Asked for by name, never read from the environment.
#[test]
#[ignore = "rewrites schemas/control; run by name to regenerate"]
fn regenerate_public_contracts() {
    check_or_rewrite(true);
}

fn check_or_rewrite(update: bool) {
    let mut rewritten = Vec::new();

    for document in public_contract_documents() {
        if document.is_json_schema {
            validate_schema(&document.document).unwrap();
        }
        let committed: Value = serde_json::from_str(committed_schema(&document)).unwrap();
        if update {
            if document.document != committed {
                let path = std::path::Path::new(SCHEMA_DIR).join(document.file_name);
                let mut rendered = serde_json::to_string_pretty(&document.document).unwrap();
                rendered.push('\n');
                std::fs::write(&path, rendered).unwrap();
                rewritten.push(document.file_name);
            }
            continue;
        }
        assert_eq!(
            document.document, committed,
            "{} is stale — regenerate with `{REGENERATE}` and review the diff",
            document.file_name
        );
    }

    assert!(
        rewritten.is_empty(),
        "rewrote {rewritten:?}; rerun the check to confirm and review the diff"
    );
}

fn committed_schema(document: &PublicContractDocument) -> &'static str {
    match document.file_name {
        "capability-descriptor-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/capability-descriptor-v1.schema.json"
        )),
        "control-error-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/control-error-v1.schema.json"
        )),
        "event-cursor-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/event-cursor-v1.schema.json"
        )),
        "handshake-request-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/handshake-request-v1.schema.json"
        )),
        "handshake-reply-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/handshake-reply-v1.schema.json"
        )),
        "handshake-response-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/handshake-response-v1.schema.json"
        )),
        "instance-descriptor-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/instance-descriptor-v1.schema.json"
        )),
        "page-cursor-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/page-cursor-v1.schema.json"
        )),
        "request-envelope-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/request-envelope-v1.schema.json"
        )),
        "response-envelope-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/response-envelope-v1.schema.json"
        )),
        "reference-capabilities-v1.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/reference-capabilities-v1.json"
        )),
        name => panic!("public schema `{name}` has no committed snapshot route"),
    }
}
