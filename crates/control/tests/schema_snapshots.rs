use quantick_control::{
    schema::validate_schema,
    schema_catalog::{PublicContractDocument, public_contract_documents},
};
use serde_json::Value;

#[test]
fn generated_public_contracts_match_committed_files() {
    for document in public_contract_documents() {
        if document.is_json_schema {
            validate_schema(&document.document).unwrap();
        }
        let committed: Value = serde_json::from_str(committed_schema(&document)).unwrap();
        assert_eq!(
            document.document, committed,
            "regenerate and review {}",
            document.file_name
        );
    }
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
        "handshake-response-v1.schema.json" => include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../schemas/control/handshake-response-v1.schema.json"
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
