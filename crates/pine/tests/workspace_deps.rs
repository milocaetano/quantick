//! The dependency direction `CLAUDE.md` states, checked against the
//! manifests.
//!
//! That line is the contract a reviewer cites to call a reverse edge a
//! blocker, and nothing failed when it stopped matching the workspace: it
//! shipped claiming `feed-* → pine`, an edge that must never exist. The same
//! drift habit `dialect_doc` exists to break, applied to the crate graph.

use std::path::{Path, PathBuf};

/// Workspace root, from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/pine sits two levels below the root")
        .to_path_buf()
}

/// The `path = "../x"` dependencies a crate declares, by directory name.
fn path_dependencies(crate_dir: &Path) -> Vec<String> {
    let manifest = std::fs::read_to_string(crate_dir.join("Cargo.toml"))
        .unwrap_or_else(|e| panic!("{} has a manifest: {e}", crate_dir.display()));
    manifest
        .lines()
        .filter_map(|line| {
            let start = line.find("path = \"../")? + "path = \"../".len();
            let rest = &line[start..];
            let end = rest.find('"')?;
            Some(rest[..end].to_owned())
        })
        .collect()
}

#[test]
fn feeds_depend_on_the_domain_crates_only() {
    let root = workspace_root().join("crates");
    for entry in std::fs::read_dir(&root).expect("crates/ is readable") {
        let dir = entry.expect("entry is readable").path();
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !name.starts_with("feed-") {
            continue;
        }
        for dependency in path_dependencies(&dir) {
            assert!(
                matches!(dependency.as_str(), "engine" | "orderbook"),
                "{name} depends on `{dependency}`: a feed produces trades and \
                 has no business linking anything else — and no feed may \
                 depend on another feed"
            );
        }
    }
}

#[test]
fn the_domain_crates_never_depend_upwards() {
    let root = workspace_root().join("crates");
    // Each crate and everything it is allowed to reach.
    let allowed: &[(&str, &[&str])] = &[
        ("engine", &[]),
        ("orderbook", &[]),
        ("replay", &["engine"]),
        ("sim", &["engine"]),
        ("indicators", &["engine"]),
        ("pine", &["indicators"]),
    ];
    for (crate_name, may_depend_on) in allowed {
        let dir = root.join(crate_name);
        for dependency in path_dependencies(&dir) {
            assert!(
                may_depend_on.contains(&dependency.as_str()),
                "{crate_name} depends on `{dependency}`, which reverses the \
                 one-way direction CLAUDE.md states"
            );
        }
    }
}

#[test]
fn claude_md_lists_every_crate() {
    let root = workspace_root();
    let doc = std::fs::read_to_string(root.join("CLAUDE.md")).expect("CLAUDE.md is readable");
    let mut missing = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).expect("crates/ is readable") {
        let dir = entry.expect("entry is readable").path();
        if !dir.join("Cargo.toml").exists() {
            continue;
        }
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !doc.contains(&format!("`{name}`")) {
            missing.push(name);
        }
    }
    assert!(
        missing.is_empty(),
        "crates absent from CLAUDE.md's architecture list: {missing:?}"
    );
}
