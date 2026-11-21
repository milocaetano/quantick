use super::{Artifact, BuildIdentity};
use std::{collections::BTreeSet, path::Path};
const BUILD: BuildIdentity = BuildIdentity {
    application_version: "test",
    git_commit: Some("fixture"),
};

#[test]
fn headless_artifacts_match_compatibility() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let first = super::artifacts(BUILD).unwrap();
    assert_eq!(
        first,
        super::artifacts(BUILD).unwrap(),
        "two generations differ"
    );
    let actual: BTreeSet<_> = first.iter().map(|artifact| artifact.path.clone()).collect();
    let mut expected: BTreeSet<_> = std::fs::read_dir(root.join("schemas/control"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.is_file() && path.file_name().unwrap() != "canonical-json-v1-vectors.json"
        })
        .map(|path| {
            format!(
                "schemas/control/{}",
                path.file_name().unwrap().to_str().unwrap()
            )
        })
        .collect();
    expected.extend([
        "docs/control-plane/capability-inventory.md".into(),
        "docs/control-plane/retry-matrix.md".into(),
    ]);
    assert_eq!(actual, expected, "generated artifact path set changed");
    for artifact in first {
        assert_eq!(
            std::fs::read(root.join(&artifact.path)).unwrap(),
            artifact.contents.as_bytes(),
            "{} changed bytes",
            artifact.path
        );
    }
}

#[test]
fn duplicate_artifact_paths_are_rejected() {
    let mut artifacts = [
        Artifact {
            path: "same".into(),
            contents: "one".into(),
        },
        Artifact {
            path: "same".into(),
            contents: "two".into(),
        },
    ];
    assert!(
        super::validate_artifacts(&mut artifacts)
            .unwrap_err()
            .contains("duplicate generated path: same")
    );
}

#[test]
fn caller_supplied_build_identity_is_preserved() {
    let description = super::describe(BUILD).unwrap();
    assert_eq!(description.application_version, "test");
    assert_eq!(description.application_commit, "fixture");
}

#[test]
fn family_consumers_match_manifest() {
    let families = quantick_control::families::FAMILIES;
    assert!(families.windows(2).all(|pair| pair[0].id < pair[1].id));
    let contract = super::composition::contract().unwrap();
    let mut expected = crate::observer::read_descriptors();
    macro_rules! manifest_actions {
        ($($id:literal {
            schemas: [$($schema:ident),*], actions: [$($action:ident),*],
            projections: [$($projection:ident),*], authority: [$($authority:ident),*],
            readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
            samples: [$($sample:ident),*],
        })*) => { $($(expected.extend(crate::$action::action_descriptors());)*)* };
    }
    quantick_control::control_families!(manifest_actions);
    expected.sort_by(|left, right| (&left.id, left.version).cmp(&(&right.id, right.version)));
    let actual: Vec<_> = contract.registry().capabilities().cloned().collect();
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(
        crate::retry_matrix::drift(quantick_control_schema::retry_matrix::READBACKS, &contract)
            .is_empty()
    );
    assert_eq!(
        families
            .iter()
            .map(|family| family.readbacks.len())
            .sum::<usize>(),
        quantick_control_schema::retry_matrix::FAMILIES.len()
    );
    let documents = super::documents();
    let names: BTreeSet<_> = documents
        .iter()
        .map(|document| document.file_name)
        .collect();
    assert_eq!(names.len(), documents.len());
    // Full descriptor/schema/retry byte parity guards semantics, including v1 documents.
    headless_artifacts_match_compatibility();
}
