use std::collections::BTreeSet;

#[test]
fn generation_contract_preserves_documents() {
    let documents = super::documents();
    let previous = crate::schema_documents::documents();
    assert!(!documents.is_empty());
    assert_eq!(documents.len(), previous.len());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/control");
    let mut names = BTreeSet::new();
    for (document, old) in documents.iter().zip(previous) {
        assert_eq!(document.file_name, old.file_name);
        assert_eq!(document.schema, old.schema);
        assert!(names.insert(document.file_name), "duplicate document name");
        let generated = format!(
            "{}\n",
            serde_json::to_string_pretty(&document.schema).unwrap()
        );
        let committed = std::fs::read_to_string(root.join(document.file_name)).unwrap();
        assert_eq!(generated, committed, "{} changed bytes", document.file_name);
    }
}
