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
