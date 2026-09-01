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

/// Each crate under a dependency rule, and everything it is allowed to
/// reach.
///
/// `backtest` is not a domain crate — it is the headless consumer — but it is
/// listed for the same reason the domain crates are: the edge that must never
/// appear is `backtest → app`, and this entry is what fails the build if one
/// is ever added.
const ALLOWED: &[(&str, &[&str])] = &[
    ("control", &[]),
    ("control-local", &["control"]),
    ("mcp", &["control", "control-local"]),
    ("engine", &[]),
    ("orderbook", &[]),
    ("replay", &["engine"]),
    // The venue-neutral trading vocabulary sits beside `engine`, not above
    // it: it is what `sim` and any future broker adapter both speak.
    ("trading", &["engine"]),
    ("sim", &["engine", "trading"]),
    ("strategy", &["engine", "sim"]),
    ("indicators", &["engine"]),
    ("pine", &["indicators"]),
    (
        "backtest",
        &["engine", "indicators", "pine", "replay", "sim", "strategy"],
    ),
    // The repository guards. Empty for the same reason `control` is, but
    // load-bearing in a way the others are not: these read files and count
    // lines, and the whole point of giving them a crate was that nothing
    // needs building before they can answer. A dependency here would put a
    // compile back in front of the cheapest checks in the repo.
    ("guards", &[]),
];

/// The one crate above the graph: everything may be linked from it, so there
/// is no upward edge for it to take.
const TOP_OF_THE_GRAPH: &str = "app";

#[test]
fn the_domain_crates_never_depend_upwards() {
    let root = workspace_root().join("crates");
    for (crate_name, may_depend_on) in ALLOWED {
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
fn every_crate_is_covered_by_a_dependency_rule() {
    // `the_domain_crates_never_depend_upwards` iterates the whitelist, not
    // the directory, so a new crate that nobody remembers to list is not a
    // failure there — it is simply unguarded, which is worse than a failure
    // because it looks green. This test is the reminder.
    let root = workspace_root().join("crates");
    let mut unguarded = Vec::new();
    for entry in std::fs::read_dir(&root).expect("crates/ is readable") {
        let dir = entry.expect("entry is readable").path();
        if !dir.join("Cargo.toml").exists() {
            continue;
        }
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let covered = name == TOP_OF_THE_GRAPH
            || name.starts_with("feed-")
            || ALLOWED.iter().any(|(listed, _)| *listed == name);
        if !covered {
            unguarded.push(name);
        }
    }
    assert!(
        unguarded.is_empty(),
        "crates with no dependency rule: {unguarded:?} — add each to ALLOWED with \
         exactly what it may reach, or the one-way direction is unenforced for it"
    );
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
