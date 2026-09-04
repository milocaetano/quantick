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
//!
//! # Why this is a guard and not an integration test
//!
//! It was one, under `crates/pine/tests/`, and none of it imported anything
//! from `quantick-pine`: it reads manifests and compares strings. But an
//! integration test in a crate's `tests/` directory makes cargo build that
//! crate and its whole dependency tree first, so the one check that answers
//! "did I just add a reverse edge?" was only ever asked by a session that
//! happened to be building `pine`. A reverse edge is among the two mistakes an
//! agent is most likely to make by accident, and it was guarded by the most
//! expensive question in the repository.
//!
//! Here it costs what every other guard costs, which is the whole argument of
//! [`crate`]'s own doc comment: a check nobody can afford to ask is a check
//! that runs late, and late is exactly when its finding costs the most.
//!
//! # Where each finding's rule is written
//!
//! Every violation names the sentence it enforces, so the agent that trips one
//! reads that sentence rather than the whole of `CLAUDE.md`. Four of the six
//! checks cite `CLAUDE.md`'s *Architecture* section. The two manifest-hygiene
//! checks cite the root `Cargo.toml` instead, because that is honestly where
//! those two rules are written: `[workspace.dependencies]` and
//! `[workspace.lints]` are the statement, and no prose restates them.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Finding;

/// Each crate under a dependency rule, and everything it is allowed to
/// reach.
///
/// `backtest` is not a domain crate — it is the headless consumer — but it is
/// listed for the same reason the domain crates are: the edge that must never
/// appear is `backtest → app`, and this entry is what fails the build if one
/// is ever added.
///
/// This table is the single source of the graph. `AGENTS.md`'s map draws the
/// same shape in prose and in a diagram, for a reader; this is the copy a
/// review cites, because it is the copy that can fail.
pub const ALLOWED: &[(&str, &[&str])] = &[
    ("control", &[]),
    ("control-local", &["control"]),
    ("mcp", &["control", "control-local"]),
    ("engine", &[]),
    ("orderbook", &[]),
    // The order-flow engine reads bars from `engine` and depth events from
    // `orderbook`, and is told the time by its caller. It sits beside
    // `indicators`: something the chart draws and `backtest` may consume.
    ("orderflow", &["engine", "orderbook"]),
    ("replay", &["engine"]),
    // The feed host: the port every venue implements, and the adapters that
    // run one. It sits above the three `feed-*` venue crates and `replay` —
    // a recorded session is a source like any other — and below `app`. It is
    // the one crate below `app` that owns runtimes, threads and the wall
    // clock; nothing it reaches does.
    (
        "feed",
        &[
            "engine",
            "orderbook",
            "replay",
            "feed-binance",
            "feed-hyperliquid",
            "feed-mt5",
        ],
    ),
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

/// What a `feed-*` venue crate may reach, and nothing else. A feed produces
/// trades; it links the vocabulary of a trade and of a book, and no more —
/// and no feed may depend on another feed.
const FEEDS_MAY_REACH: &[&str] = &["engine", "orderbook"];

/// The rule a dependency-direction finding cites.
const RULE_DIRECTION: &str = "CLAUDE.md: Architecture, dependency direction";

/// The rule a coverage finding cites: `CLAUDE.md` names every crate, and the
/// architecture list is what a reader is pointed at.
const RULE_CRATE_LIST: &str = "CLAUDE.md: Architecture, the crate list";

/// Where a third-party version may be stated. Not prose anywhere: the root
/// manifest's own table is the rule.
const RULE_WORKSPACE_DEPENDENCIES: &str = "Cargo.toml: [workspace.dependencies]";

/// Where a crate's lint strictness comes from, for the same reason.
const RULE_WORKSPACE_LINTS: &str = "Cargo.toml: [workspace.lints]";

/// What the guard asks for when an edge points the wrong way.
pub const REMEDY_DIRECTION: &str = "A crate manifest declares an edge the one-way dependency direction forbids. Remove it and \
     reach the capability the other way — through a port the lower crate defines and the higher \
     one implements. If the edge is genuinely correct, the graph changed, and `ALLOWED` in \
     `crates/guards/src/graph.rs` is where that argument is signed.";

/// What the guard asks for when a crate has no rule at all.
pub const REMEDY_COVERAGE: &str = "A crate is not covered by the dependency table, so the one-way direction is unenforced for \
     it — which looks green and is worse than a failure. Add it to `ALLOWED` in \
     `crates/guards/src/graph.rs` with exactly what it may reach, and name it in `CLAUDE.md`'s \
     architecture section.";

/// What the guard asks for when a manifest pins its own third-party source.
pub const REMEDY_VERSIONS: &str = "A third-party source is stated outside the root manifest. Move the version into \
     `[workspace.dependencies]` in the root `Cargo.toml` and inherit it here with \
     `{ workspace = true }`. A version stated in two places is a version that can disagree with \
     itself, and the second place is the one nobody updates.";

/// What the guard asks for when a crate does not inherit the workspace lints.
pub const REMEDY_LINTS: &str = "A crate does not inherit `[workspace.lints]`. Add `[lints]` with `workspace = true` to it. \
     Without that, the crate is checked at a different strictness from the rest of the \
     workspace, so `cargo clippy -p <crate>` can pass on code CI rejects — the whole failure the \
     lints table was introduced to end.";

/// Every violation of the crate graph and of manifest hygiene.
pub fn check(root: &Path) -> Vec<Finding> {
    let manifests = crate_manifests(root);
    let mut findings = Vec::new();
    findings.extend(feeds_reach_the_domain_only(&manifests));
    findings.extend(nothing_depends_upwards(&manifests));
    findings.extend(every_crate_is_covered(&manifests));
    findings.extend(the_architecture_list_names_every_crate(root, &manifests));
    findings.extend(versions_live_in_the_root_manifest(&manifests));
    findings.extend(every_crate_inherits_the_lints(&manifests));
    findings
}

/// The same checks narrowed to one file, for the edit-time hook.
///
/// A manifest edit is the only way to move an edge, and `CLAUDE.md` is the
/// only other file this guard reads — so those are the two paths that answer.
/// The whole-repo checks run either way: they are six string comparisons over
/// a dozen small files, and narrowing them would risk the hook and the scan
/// disagreeing, which is the one thing a per-file mode must never do.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if !in_scope(relative) {
        return Vec::new();
    }
    check(root)
}

/// Whether a workspace-relative path is one this guard reads.
fn in_scope(relative: &str) -> bool {
    relative == "CLAUDE.md"
        || relative == "Cargo.toml"
        || (relative.starts_with("crates/") && relative.ends_with("/Cargo.toml"))
}

/// One crate's manifest: the directory name and the text.
struct Manifest {
    /// The directory under `crates/`, which is how every rule names a crate.
    name: String,
    /// The manifest as written.
    text: String,
}

/// Every crate manifest under `crates/`, sorted by name so the findings come
/// out in the same order on every platform.
///
/// A directory that cannot be listed, or a manifest that cannot be read, is
/// simply absent here — and `every_crate_is_covered` cannot see it either, so
/// nothing claims a crate is clean that nobody opened. The tree this walks is
/// a dozen small files in the repository being checked; a read failure over
/// it is a broken checkout, not a rule violation.
fn crate_manifests(root: &Path) -> Vec<Manifest> {
    let mut manifests = Vec::new();
    let Ok(entries) = fs::read_dir(root.join("crates")) else {
        return manifests;
    };
    for entry in entries.flatten() {
        let dir: PathBuf = entry.path();
        let Ok(text) = fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        manifests.push(Manifest { name, text });
    }
    manifests.sort_by(|a, b| a.name.cmp(&b.name));
    manifests
}

/// The `path = "../x"` dependencies a manifest declares, by directory name.
fn path_dependencies(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let start = line.find("path = \"../")? + "path = \"../".len();
            let rest = &line[start..];
            let end = rest.find('"')?;
            Some(rest[..end].to_owned())
        })
        .collect()
}

/// A feed produces trades and links nothing else — and no feed depends on
/// another feed.
fn feeds_reach_the_domain_only(manifests: &[Manifest]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for manifest in manifests {
        if !manifest.name.starts_with("feed-") {
            continue;
        }
        for dependency in path_dependencies(&manifest.text) {
            if FEEDS_MAY_REACH.contains(&dependency.as_str()) {
                continue;
            }
            findings.push(Finding::new(
                format!(
                    "crates/{}/Cargo.toml: depends on `{dependency}`: a feed produces trades and \
                     has no business linking anything else — and no feed may depend on another \
                     feed — {RULE_DIRECTION}",
                    manifest.name
                ),
                REMEDY_DIRECTION,
            ));
        }
    }
    findings
}

/// Every edge a listed crate declares is one [`ALLOWED`] permits.
fn nothing_depends_upwards(manifests: &[Manifest]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (crate_name, may_depend_on) in ALLOWED {
        let Some(manifest) = manifests.iter().find(|m| m.name == *crate_name) else {
            continue;
        };
        for dependency in path_dependencies(&manifest.text) {
            if may_depend_on.contains(&dependency.as_str()) {
                continue;
            }
            findings.push(Finding::new(
                format!(
                    "crates/{crate_name}/Cargo.toml: depends on `{dependency}`, which reverses \
                     the one-way direction — {RULE_DIRECTION}"
                ),
                REMEDY_DIRECTION,
            ));
        }
    }
    findings
}

/// A crate nobody listed is not a failure in the check above — it is simply
/// unguarded, which looks green and is worse. This is the reminder.
fn every_crate_is_covered(manifests: &[Manifest]) -> Vec<Finding> {
    manifests
        .iter()
        .filter(|manifest| {
            manifest.name != TOP_OF_THE_GRAPH
                && !manifest.name.starts_with("feed-")
                && !ALLOWED.iter().any(|(listed, _)| *listed == manifest.name)
        })
        .map(|manifest| {
            Finding::new(
                format!(
                    "crates/{}/Cargo.toml: no dependency rule covers this crate, so the one-way \
                     direction is unenforced for it — {RULE_DIRECTION}",
                    manifest.name
                ),
                REMEDY_COVERAGE,
            )
        })
        .collect()
}

/// Every crate is named in `CLAUDE.md`'s architecture section.
fn the_architecture_list_names_every_crate(root: &Path, manifests: &[Manifest]) -> Vec<Finding> {
    let Ok(doc) = fs::read_to_string(root.join("CLAUDE.md")) else {
        return vec![Finding::new(
            format!(
                "CLAUDE.md: could not be read, so no crate could be checked against its \
                     architecture list — {RULE_CRATE_LIST}"
            ),
            REMEDY_COVERAGE,
        )];
    };
    manifests
        .iter()
        .filter(|manifest| !doc.contains(&format!("`{}`", manifest.name)))
        .map(|manifest| {
            Finding::new(
                format!(
                    "CLAUDE.md: the architecture list does not name `{}` — {RULE_CRATE_LIST}",
                    manifest.name
                ),
                REMEDY_COVERAGE,
            )
        })
        .collect()
}

/// The keys by which a dependency pins its own source, rather than inheriting
/// one. `version` is the common case; the others are how a dependency escapes
/// the registry entirely, and a third-party crate pinned to a git revision
/// outside the root manifest is the same drift wearing different clothes.
const SOURCE_PINNING_KEYS: &[&str] = &["version", "git", "tag", "rev", "branch", "path"];

/// A third-party source is stated in the root manifest and inherited here.
fn versions_live_in_the_root_manifest(manifests: &[Manifest]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for manifest in manifests {
        for (name, value) in dependency_entries(&manifest.text) {
            // The `quantick-*` crates depend on each other by path and state
            // no version, which is the point: the checks above read exactly
            // those lines to enforce the one-way direction.
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
            if !pins_its_own_source {
                continue;
            }
            findings.push(Finding::new(
                format!(
                    "crates/{}/Cargo.toml: `{name} = {value}` states a third-party source outside \
                     the root manifest — {RULE_WORKSPACE_DEPENDENCIES}",
                    manifest.name
                ),
                REMEDY_VERSIONS,
            ));
        }
    }
    findings
}

/// Every crate is checked at the workspace's strictness.
fn every_crate_inherits_the_lints(manifests: &[Manifest]) -> Vec<Finding> {
    manifests
        .iter()
        .filter(|manifest| {
            !manifest
                .text
                .split("[lints]")
                .skip(1)
                .any(|rest| rest.lines().any(|line| line.trim() == "workspace = true"))
        })
        .map(|manifest| {
            Finding::new(
                format!(
                    "crates/{}/Cargo.toml: does not inherit the workspace lints — \
                     {RULE_WORKSPACE_LINTS}",
                    manifest.name
                ),
                REMEDY_LINTS,
            )
        })
        .collect()
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
    // read rather than dropping it, so the check above can say so.
    entries.extend(unclosed);
    entries
}

/// Every edge the graph permits, counted. [`crate::report`] prints it, so a
/// merge that widened what a crate may reach shows up as a moved number.
pub fn edges() -> usize {
    ALLOWED.iter().map(|(_, reaches)| reaches.len()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch_dir::ScratchDir;

    /// A manifest body that satisfies everything this guard checks except the
    /// dependencies a caller adds.
    fn manifest(dependencies: &str) -> String {
        format!(
            "[package]\nname = \"x\"\n\n[dependencies]\n{dependencies}\n[lints]\nworkspace = true\n"
        )
    }

    /// Build a scratch workspace: one manifest per named crate, and a
    /// `CLAUDE.md` naming each of them.
    fn workspace(test: &str, crates: &[(&str, &str)]) -> ScratchDir {
        let root = ScratchDir::new(test);
        let mut doc = String::from("# fixture\n\n");
        for (name, dependencies) in crates {
            let dir = root.join(format!("crates/{name}"));
            fs::create_dir_all(&dir).expect("scratch dirs are creatable");
            fs::write(dir.join("Cargo.toml"), manifest(dependencies))
                .expect("the manifest is writable");
            doc.push_str(&format!("- `{name}`\n"));
        }
        fs::write(root.join("CLAUDE.md"), doc).expect("CLAUDE.md is writable");
        root
    }

    /// A dependency line of the shape every `quantick-*` crate uses.
    fn edge(name: &str) -> String {
        format!("quantick-{name} = {{ path = \"../{name}\" }}\n")
    }

    #[test]
    fn a_workspace_that_obeys_every_rule_is_clean() {
        let root = workspace(
            "graph-clean",
            &[
                ("engine", ""),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                ("app", &edge("pine")),
            ],
        );
        assert_eq!(check(root.path()), Vec::new());
    }

    #[test]
    fn a_reverse_edge_is_a_finding_that_names_the_rule() {
        let root = workspace(
            "graph-reverse",
            &[
                ("engine", &edge("pine")),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                ("app", ""),
            ],
        );
        let findings = check(root.path());
        assert_eq!(
            findings.len(),
            1,
            "exactly one edge reverses: {findings:#?}"
        );
        assert!(
            findings[0]
                .line
                .starts_with("crates/engine/Cargo.toml: depends on `pine`"),
            "the finding names the manifest and the edge: {}",
            findings[0].line
        );
        assert!(
            findings[0].line.ends_with(RULE_DIRECTION),
            "the finding names the rule it enforces: {}",
            findings[0].line
        );
    }

    #[test]
    fn a_feed_may_not_reach_past_the_domain_or_another_feed() {
        let root = workspace(
            "graph-feeds",
            &[
                ("engine", ""),
                ("orderbook", ""),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                (
                    "feed-binance",
                    &format!("{}{}", edge("engine"), edge("orderbook")),
                ),
                (
                    "feed-mt5",
                    &format!("{}{}", edge("pine"), edge("feed-binance")),
                ),
                ("app", ""),
            ],
        );
        let findings = check(root.path());
        let lines: Vec<&str> = findings.iter().map(|f| f.line.as_str()).collect();
        assert_eq!(
            lines.len(),
            2,
            "the venue that reaches the script language and the sibling feed: {lines:#?}"
        );
        assert!(
            lines
                .iter()
                .all(|line| line.starts_with("crates/feed-mt5/")),
            "both findings are the one offending venue: {lines:#?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("`pine`"))
                && lines.iter().any(|line| line.contains("`feed-binance`")),
            "each forbidden edge is named: {lines:#?}"
        );
    }

    #[test]
    fn a_crate_with_no_rule_is_reported_rather_than_left_unguarded() {
        let root = workspace(
            "graph-uncovered",
            &[("engine", ""), ("app", ""), ("newcomer", "")],
        );
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "one crate has no rule: {findings:#?}");
        assert!(
            findings[0].line.contains("crates/newcomer/Cargo.toml")
                && findings[0].line.contains("no dependency rule"),
            "the finding names the unguarded crate: {}",
            findings[0].line
        );
    }

    #[test]
    fn a_crate_absent_from_the_architecture_list_is_a_finding() {
        let root = workspace("graph-unlisted", &[("engine", ""), ("app", "")]);
        fs::write(root.join("CLAUDE.md"), "# fixture\n\n- `app`\n").expect("CLAUDE.md is writable");
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "one crate is unnamed: {findings:#?}");
        assert!(
            findings[0].line.contains("does not name `engine`")
                && findings[0].line.ends_with(RULE_CRATE_LIST),
            "the finding names the crate and the rule: {}",
            findings[0].line
        );
    }

    #[test]
    fn a_third_party_version_outside_the_root_manifest_is_a_finding() {
        let root = workspace(
            "graph-versions",
            &[
                (
                    "engine",
                    "serde = { workspace = true }\nrust_decimal=\"1\"\n",
                ),
                ("app", ""),
            ],
        );
        let findings = check(root.path());
        assert_eq!(
            findings.len(),
            1,
            "only the pinned entry offends: {findings:#?}"
        );
        assert!(
            findings[0].line.contains("rust_decimal")
                && findings[0].line.ends_with(RULE_WORKSPACE_DEPENDENCIES),
            "the finding names the entry and the rule: {}",
            findings[0].line
        );
    }

    /// An inline table written across lines hid everything below its first
    /// line from an earlier version of this rule. The pin is on the second.
    #[test]
    fn a_multi_line_inline_table_cannot_hide_its_pin() {
        let root = workspace(
            "graph-multiline",
            &[
                (
                    "engine",
                    "tokio = {\n    features = [\"rt\"],\n    version = \"1\",\n}\n",
                ),
                ("app", ""),
            ],
        );
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "the pin is found: {findings:#?}");
        assert!(
            findings[0].line.contains("tokio"),
            "the finding names the dependency: {}",
            findings[0].line
        );
    }

    #[test]
    fn a_crate_that_does_not_inherit_the_lints_is_a_finding() {
        let root = workspace("graph-lints", &[("engine", ""), ("app", "")]);
        fs::write(
            root.join("crates/engine/Cargo.toml"),
            "[package]\nname = \"x\"\n\n[dependencies]\n",
        )
        .expect("the manifest is writable");
        let findings = check(root.path());
        assert_eq!(findings.len(), 1, "one crate opts out: {findings:#?}");
        assert!(
            findings[0]
                .line
                .contains("does not inherit the workspace lints")
                && findings[0].line.ends_with(RULE_WORKSPACE_LINTS),
            "the finding names the rule: {}",
            findings[0].line
        );
    }

    /// The repository this crate ships in obeys its own graph. The check that
    /// would have failed on the `feed-* → pine` edge the prose once claimed.
    #[test]
    fn this_workspace_obeys_the_graph() {
        assert_eq!(check(&crate::workspace_root()), Vec::new());
    }

    #[test]
    fn the_hook_reads_manifests_and_the_architecture_list_and_nothing_else() {
        assert!(in_scope("crates/engine/Cargo.toml"));
        assert!(in_scope("CLAUDE.md"));
        assert!(in_scope("Cargo.toml"));
        assert!(!in_scope("crates/engine/src/lib.rs"));
        assert!(!in_scope("AGENTS.md"));
    }

    /// The number [`crate::report`] prints. Pinned so a widened `ALLOWED`
    /// moves a reported line rather than passing unnoticed.
    #[test]
    fn the_edge_count_is_the_sum_of_what_every_crate_may_reach() {
        assert_eq!(
            edges(),
            ALLOWED
                .iter()
                .map(|(_, reaches)| reaches.len())
                .sum::<usize>()
        );
        assert!(edges() > 0, "the graph has edges");
    }
}
