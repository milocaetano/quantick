//! What the crate manifests are required to say, checked against them.
//!
//! Two rules live here, and both guard against drift nobody notices.
//!
//! The dependency direction `CLAUDE.md` states. That line is the contract a
//! reviewer cites to call a reverse edge a blocker, and nothing failed when it
//! stopped matching the workspace: it shipped claiming `feed-* → pine`, an
//! edge that must never exist. The same drift habit `dialect_doc` exists to
//! break, applied to the crate graph.
//!
//! And where a third-party version may be written down — in
//! `[workspace.dependencies]` in the root manifest, inherited from there, so
//! that a version and its feature set are stated once. Nothing failed when
//! `tokio` was declared in seven places with four different feature sets
//! either. It simply cost a reader all seven openings to learn whether they
//! agreed.

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

/// Every crate manifest, paired with its directory name.
fn crate_manifests() -> Vec<(String, String)> {
    let root = workspace_root().join("crates");
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(&root).expect("crates/ is readable") {
        let dir = entry.expect("entry is readable").path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("{} is readable: {e}", manifest.display()));
        manifests.push((name, text));
    }
    assert!(!manifests.is_empty(), "crates/ holds at least one manifest");
    manifests
}

/// Whether a section header opens a table of dependencies — plain, dev, build,
/// or any of the `[target.'cfg(…)'.…]` variants.
fn is_dependency_section(header: &str) -> bool {
    header.ends_with("dependencies]")
}

/// Whether every bracket a value opened has been closed.
///
/// Counting characters is enough here and would not be in general: it would
/// miscount a bracket inside a quoted string. No dependency value in this
/// workspace contains one, and a guard that over-reports a manifest is a guard
/// somebody will read — it fails loudly rather than passing quietly, which is
/// the direction an approximation is allowed to be wrong in.
fn brackets_balance(value: &str) -> bool {
    let opened = value.chars().filter(|c| *c == '{' || *c == '[').count();
    let closed = value.chars().filter(|c| *c == '}' || *c == ']').count();
    opened == closed
}

/// Every dependency entry in a manifest, as `(name, value)`, with a value
/// spanning several lines joined into one.
///
/// Reading a manifest a physical line at a time is what an earlier version of
/// this guard did, and it let two things through. A perfectly valid
/// `rust_decimal="1"` has no spaces around its `=` and was skipped outright,
/// and an inline table written across lines hid everything below its first one.
/// Both are exactly the shape the guard exists to catch, so it now splits on
/// the first `=` wherever it falls and keeps taking lines until the brackets
/// close.
fn dependency_entries(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut in_dependencies = false;
    let mut unclosed: Option<(String, String)> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        // A value still waiting for its closing bracket swallows this line,
        // comments and all, rather than letting it be read as a new key.
        if let Some((name, value)) = unclosed.take() {
            let value = format!("{value} {trimmed}");
            if brackets_balance(&value) {
                entries.push((name, value));
            } else {
                unclosed = Some((name, value));
            }
            continue;
        }

        if trimmed.starts_with('[') {
            in_dependencies = is_dependency_section(trimmed);
            continue;
        }
        if !in_dependencies || trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let entry = (name.trim().to_owned(), value.trim().to_owned());
        if brackets_balance(&entry.1) {
            entries.push(entry);
        } else {
            unclosed = Some(entry);
        }
    }
    // A manifest whose last entry never closed is malformed; report what was
    // read rather than dropping it, so the assertion below can say so.
    entries.extend(unclosed);
    entries
}

/// The keys by which a dependency pins its own source, rather than inheriting
/// one. `version` is the common case; the others are how a dependency escapes
/// the registry entirely, and a third-party crate pinned to a git revision
/// outside the root manifest is the same drift wearing different clothes.
const SOURCE_PINNING_KEYS: &[&str] = &["version", "git", "tag", "rev", "branch", "path"];

/// Both of the tests below iterate `crates/` rather than a list, for the same
/// reason `every_crate_is_covered_by_a_dependency_rule` exists: a new crate
/// nobody remembered to add to a whitelist is not a failure, it is simply
/// unguarded, which looks green and is worse.
#[test]
fn third_party_versions_are_stated_only_in_the_root_manifest() {
    let mut offenders = Vec::new();
    for (crate_name, text) in crate_manifests() {
        for (name, value) in dependency_entries(&text) {
            // The `quantick-*` crates depend on each other by path and state no
            // version, which is the point: the tests above read exactly those
            // lines to enforce the one-way direction.
            if name.starts_with("quantick-") {
                continue;
            }
            // A bare `foo = "1"`, or an inline table carrying any key by which
            // a dependency pins its own source, states outside the root what
            // only the root may state.
            let pins_its_own_source = value.starts_with('"')
                || SOURCE_PINNING_KEYS.iter().any(|key| {
                    value.contains(&format!("{key} =")) || value.contains(&format!("{key}="))
                });
            if pins_its_own_source {
                offenders.push(format!("{crate_name}: {name} = {value}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these entries state a third-party source outside the root manifest: \
         {offenders:#?}\n\nMove the version into `[workspace.dependencies]` in \
         the root `Cargo.toml` and inherit it here with `{{ workspace = true }}`. \
         A version stated in two places is a version that can disagree with \
         itself, and the second place is the one nobody updates."
    );
}

#[test]
fn every_crate_inherits_the_workspace_lints() {
    let mut missing = Vec::new();
    for (crate_name, text) in crate_manifests() {
        let inherits = text
            .split("[lints]")
            .skip(1)
            .any(|rest| rest.lines().any(|line| line.trim() == "workspace = true"));
        if !inherits {
            missing.push(crate_name);
        }
    }
    assert!(
        missing.is_empty(),
        "these crates do not inherit `[workspace.lints]`: {missing:?}\n\nAdd \
         `[lints]` with `workspace = true` to each. Without it a crate is \
         checked at a different strictness from the rest of the workspace, so \
         `cargo clippy -p <crate>` can pass on code CI rejects — which is the \
         whole failure the lints table was introduced to end."
    );
}
