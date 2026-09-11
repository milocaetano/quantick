//! Shared test-only released-document inventory, not a compatibility implementation.

use std::{collections::BTreeMap, fs, path::Path};

use quantick_control::{canonical::raw_digest, schema::require_compatible_version};
use serde::Deserialize;
use serde_json::Value;

const SOURCE_COMMIT: &str = "a808b2d87b36d73041027e4d20c053544b454a96";
// Freeze the inventory as well as its files: deleting a manifest row must not
// turn a formerly published schema into an untested document.
const MANIFEST_DIGEST: &str =
    "sha256:fff12d5cc40eb0219ae336693b7aaea2e3f52695964d5da3a99b71c04ee87079";

#[derive(Deserialize)]
struct Manifest {
    source_commit: String,
    source_repository: String,
    documents: Vec<Document>,
}

#[derive(Deserialize)]
struct Document {
    file_name: String,
    owner: String,
    version: u32,
    source_path: String,
    sha256: String,
}

pub struct ReleasedSchemas {
    manifest: Manifest,
    schemas: BTreeMap<String, Value>,
}

impl ReleasedSchemas {
    pub fn load() -> Self {
        // Both consuming crates sit immediately beneath crates/.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schemas/control/released/v1");
        let bytes = fs::read(root.join("manifest.json")).expect("read released v1 manifest");
        assert_eq!(
            raw_digest(&bytes),
            MANIFEST_DIGEST,
            "released inventory changed"
        );
        let manifest: Manifest = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(manifest.source_commit, SOURCE_COMMIT);
        assert_eq!(
            manifest.source_repository,
            "https://github.com/milocaetano/quantick"
        );
        let mut schemas = BTreeMap::new();
        for document in &manifest.documents {
            assert!(matches!(document.owner.as_str(), "core" | "app"));
            assert_eq!(document.version, 1);
            assert!(document.file_name.ends_with("-v1.schema.json"));
            assert_eq!(
                document.source_path,
                format!("schemas/control/{}", document.file_name)
            );
            let bytes = fs::read(root.join(&document.file_name))
                .unwrap_or_else(|error| panic!("missing released {}: {error}", document.file_name));
            assert_eq!(
                raw_digest(&bytes),
                document.sha256,
                "released bytes changed: {}",
                document.file_name
            );
            assert!(
                schemas
                    .insert(
                        document.file_name.clone(),
                        serde_json::from_slice(&bytes).unwrap()
                    )
                    .is_none(),
                "duplicate released identity: {}",
                document.file_name
            );
        }
        let mut actual_files = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        actual_files.sort();
        let mut expected_files = schemas.keys().cloned().collect::<Vec<_>>();
        expected_files.push("manifest.json".to_owned());
        expected_files.sort();
        assert_eq!(
            actual_files, expected_files,
            "released file inventory differs"
        );
        assert_eq!(
            manifest
                .documents
                .iter()
                .filter(|d| d.owner == "core")
                .count(),
            10
        );
        assert_eq!(
            manifest
                .documents
                .iter()
                .filter(|d| d.owner == "app")
                .count(),
            46
        );
        Self { manifest, schemas }
    }

    pub fn schema(&self, name: &str) -> &Value {
        self.schemas.get(name).expect("known released schema")
    }

    pub fn check_generated(
        &self,
        owner: &str,
        generated: impl IntoIterator<Item = (String, Value)>,
    ) -> Result<(), String> {
        if !matches!(owner, "core" | "app") {
            return Err(format!("unknown schema owner: {owner}"));
        }
        let mut current = BTreeMap::new();
        for (name, schema) in generated {
            if current.insert(name.clone(), schema).is_some() {
                return Err(format!("duplicate generated schema: {name}"));
            }
        }
        // Check the complete retained set first. Iterating only today's
        // generated documents would silently accept a removed v1 route.
        for document in self.manifest.documents.iter().filter(|d| d.owner == owner) {
            if !current.contains_key(&document.file_name) {
                return Err(format!("missing published schema: {}", document.file_name));
            }
        }
        for document in self.manifest.documents.iter().filter(|d| d.owner == owner) {
            // The exact filename is the versioned identity. A newly published
            // v2 document cannot replace the retained v1 document in this map.
            require_compatible_version(
                document.version,
                self.schema(&document.file_name),
                document.version,
                &current[&document.file_name],
            )
            .map_err(|error| format!("{}: {error}", document.file_name))?;
        }
        Ok(())
    }
}

/// Exercise complete coverage without recompiling a deliberately broken catalog.
pub fn assert_each_removal_fails(
    released: &ReleasedSchemas,
    owner: &str,
    generated: &[(String, Value)],
) {
    assert!(released.check_generated(owner, []).is_err());
    for document in released
        .manifest
        .documents
        .iter()
        .filter(|d| d.owner == owner)
    {
        let removed = &document.file_name;
        let remaining = generated
            .iter()
            .filter(|(name, _)| name != removed)
            .cloned();
        assert_eq!(
            released.check_generated(owner, remaining),
            Err(format!("missing published schema: {removed}"))
        );
    }
    // A successor can coexist with v1, but cannot replace its published route.
    let mut with_successor = generated.to_vec();
    let original = &released
        .manifest
        .documents
        .iter()
        .find(|document| document.owner == owner)
        .unwrap()
        .file_name;
    let successor = original.replace("-v1.schema.json", "-v2.schema.json");
    let schema = generated
        .iter()
        .find(|(name, _)| name == original)
        .unwrap()
        .1
        .clone();
    with_successor.push((successor, schema));
    released
        .check_generated(owner, with_successor.clone())
        .unwrap();
    with_successor.retain(|(name, _)| name != original);
    assert_eq!(
        released.check_generated(owner, with_successor),
        Err(format!("missing published schema: {original}"))
    );
}
