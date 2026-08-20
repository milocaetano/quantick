use quantick_control::canonical::{canonical_digest, canonical_json};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct GoldenFile {
    version: u32,
    vectors: Vec<GoldenVector>,
}

#[derive(Deserialize)]
struct GoldenVector {
    name: String,
    input: Value,
    canonical: String,
    digest: String,
}

#[test]
fn canonical_json_v1_matches_committed_cross_language_vectors() {
    let golden: GoldenFile = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/control/canonical-json-v1-vectors.json"
    )))
    .unwrap();
    assert_eq!(golden.version, 1);
    for vector in golden.vectors {
        assert_eq!(
            canonical_json(&vector.input).unwrap(),
            vector.canonical,
            "{}",
            vector.name
        );
        assert_eq!(
            canonical_digest(&vector.input).unwrap(),
            vector.digest,
            "{}",
            vector.name
        );
    }
}

#[test]
fn object_order_is_irrelevant_but_array_and_utf8_identity_are_not() {
    let left: Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
    let right: Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
    assert_eq!(canonical_digest(&left), canonical_digest(&right));

    let arrays = (serde_json::json!([1, 2]), serde_json::json!([2, 1]));
    assert_ne!(canonical_digest(&arrays.0), canonical_digest(&arrays.1));

    let composed = serde_json::json!("é");
    let decomposed = serde_json::json!("é");
    assert_ne!(canonical_digest(&composed), canonical_digest(&decomposed));
}
