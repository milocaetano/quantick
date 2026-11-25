use serde_json::{Value, json};

use super::fold_schema_refs;

fn catalog(input: &Value, output: &Value) -> Value {
    json!({ "capabilities": [{ "id": "a.call", "input_schema": input, "output_schema": output }] })
}

#[test]
fn a_schema_with_a_published_file_becomes_a_reference_and_the_rest_stay_inline() {
    let published = json!({ "type": "object", "properties": {} });
    let unpublished = json!({ "type": "string" });
    let mut folded = catalog(&published, &unpublished);
    fold_schema_refs(&mut folded, [("a-call-input-v1.schema.json", &published)]);
    assert_eq!(
        folded,
        catalog(
            &json!({ "$ref": "a-call-input-v1.schema.json" }),
            &unpublished
        )
    );
}

#[test]
fn identical_documents_resolve_to_the_lowest_file_name_whatever_their_order() {
    let schema = json!({ "type": "object" });
    for order in [
        ["b.schema.json", "a.schema.json"],
        ["a.schema.json", "b.schema.json"],
    ] {
        let mut folded = catalog(&schema, &schema);
        fold_schema_refs(&mut folded, order.map(|name| (name, &schema)));
        let reference = json!({ "$ref": "a.schema.json" });
        assert_eq!(folded, catalog(&reference, &reference), "order {order:?}");
    }
}

#[test]
fn a_catalog_without_capabilities_is_left_alone() {
    let schema = json!({ "type": "object" });
    let mut catalog = json!({ "profiles": [] });
    fold_schema_refs(&mut catalog, [("a.schema.json", &schema)]);
    assert_eq!(catalog, json!({ "profiles": [] }));
}
