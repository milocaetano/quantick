use serde_json::{Value, json};

use super::{CallSchema, FoldError, fold_schema_refs};

fn catalog(input: &Value, output: &Value) -> Value {
    json!({ "capabilities": [{
        "id": "a.call_name", "version": 1, "input_schema": input, "output_schema": output
    }] })
}

fn reference(file_name: &str) -> Value {
    json!({ "$ref": file_name })
}

#[test]
fn a_schema_with_a_published_file_references_it_and_the_rest_get_per_call_files() {
    let published = json!({ "type": "object", "properties": {} });
    let unpublished = json!({ "type": "string" });
    let mut folded = catalog(&published, &unpublished);
    let calls = fold_schema_refs(&mut folded, [("shared-input-v1.schema.json", &published)]);
    assert_eq!(
        folded,
        catalog(
            &reference("shared-input-v1.schema.json"),
            &reference("a-call-name-result-v1.schema.json")
        )
    );
    assert_eq!(
        calls,
        Ok(vec![CallSchema {
            file_name: "a-call-name-result-v1.schema.json".to_owned(),
            schema: unpublished,
        }])
    );
}

#[test]
fn a_call_with_no_published_schema_gets_an_input_and_a_result_file_named_by_version() {
    let input = json!({ "type": "object" });
    let output = json!({ "type": "string" });
    let mut folded = json!({ "capabilities": [
        { "id": "a.call", "version": 1, "input_schema": input, "output_schema": output },
        { "id": "a.call", "version": 2, "input_schema": input },
    ] });
    let calls = fold_schema_refs(&mut folded, []).unwrap();
    assert_eq!(
        folded,
        json!({ "capabilities": [
            {
                "id": "a.call", "version": 1,
                "input_schema": reference("a-call-input-v1.schema.json"),
                "output_schema": reference("a-call-result-v1.schema.json"),
            },
            {
                "id": "a.call", "version": 2,
                "input_schema": reference("a-call-input-v2.schema.json"),
            },
        ] })
    );
    let names: Vec<_> = calls.iter().map(|call| call.file_name.as_str()).collect();
    assert_eq!(
        names,
        [
            "a-call-input-v1.schema.json",
            "a-call-result-v1.schema.json",
            "a-call-input-v2.schema.json"
        ]
    );
    assert_eq!(calls[0].schema, input);
    assert_eq!(calls[1].schema, output);
}

#[test]
fn identical_documents_resolve_to_the_lowest_file_name_whatever_their_order() {
    let schema = json!({ "type": "object" });
    for order in [
        ["b.schema.json", "a.schema.json"],
        ["a.schema.json", "b.schema.json"],
    ] {
        let mut folded = catalog(&schema, &schema);
        let calls = fold_schema_refs(&mut folded, order.map(|name| (name, &schema)));
        let reference = reference("a.schema.json");
        assert_eq!(folded, catalog(&reference, &reference), "order {order:?}");
        assert_eq!(calls, Ok(Vec::new()), "order {order:?}");
    }
}

#[test]
fn a_catalog_without_capabilities_is_left_alone() {
    let schema = json!({ "type": "object" });
    let mut catalog = json!({ "profiles": [] });
    let calls = fold_schema_refs(&mut catalog, [("a.schema.json", &schema)]);
    assert_eq!(catalog, json!({ "profiles": [] }));
    assert_eq!(calls, Ok(Vec::new()));
}

#[test]
fn a_capability_without_an_input_schema_gains_none() {
    let schema = json!({ "type": "object" });
    let mut folded = json!({ "capabilities": [{ "id": "a.read", "output_schema": schema }] });
    fold_schema_refs(&mut folded, [("a.schema.json", &schema)]).unwrap();
    assert_eq!(
        folded,
        json!({ "capabilities": [{ "id": "a.read", "output_schema": reference("a.schema.json") }] })
    );
}

#[test]
fn a_schema_that_needs_its_own_file_but_has_no_name_is_refused() {
    let schema = json!({ "type": "object" });
    for capability in [
        json!({ "version": 1, "input_schema": schema }),
        json!({ "id": "a.call", "input_schema": schema }),
        json!({ "id": "a.call", "version": "1", "input_schema": schema }),
    ] {
        let mut folded = json!({ "capabilities": [{ "id": "b.call" }, capability] });
        assert_eq!(
            fold_schema_refs(&mut folded, []),
            Err(FoldError::UnnamedCapability { index: 1 }),
            "{folded}"
        );
    }
}

#[test]
fn two_schemas_meeting_at_one_file_name_are_refused() {
    let one = json!({ "type": "object" });
    let other = json!({ "type": "string" });
    // `.` and `_` both become `-`, so these two calls would share a file.
    let mut folded = json!({ "capabilities": [
        { "id": "a.b_c", "version": 1, "input_schema": one },
        { "id": "a.b.c", "version": 1, "input_schema": other },
    ] });
    assert_eq!(
        fold_schema_refs(&mut folded, []),
        Err(FoldError::DuplicateFileName(
            "a-b-c-input-v1.schema.json".to_owned()
        ))
    );
    // A published document already holds the name a call's own file would
    // take, with a schema that differs from the call's.
    let mut folded = json!({ "capabilities": [
        { "id": "a.call", "version": 1, "input_schema": one },
    ] });
    assert_eq!(
        fold_schema_refs(&mut folded, [("a-call-input-v1.schema.json", &other)]),
        Err(FoldError::DuplicateFileName(
            "a-call-input-v1.schema.json".to_owned()
        ))
    );
}
