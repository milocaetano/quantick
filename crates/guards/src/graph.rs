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
//! reads that sentence rather than the whole of `CLAUDE.md`. Six of the nine
//! checks cite `CLAUDE.md`'s *Architecture* section. The rest cite where their
//! rule is honestly written: the cap on `app` is [`APP_DEPENDENCY_CEILING`],
//! and the two manifest-hygiene checks cite the root `Cargo.toml`, whose
//! `[workspace.dependencies]` and `[workspace.lints]` are the statement, and
//! no prose restates them.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::Finding;

/// Every crate's rank. A crate may depend by path, in any dependency table
/// (dev, build and target ones included), only on a crate of a strictly lower
/// rank, so no edge can point up or sideways and no cycle can form.
///
/// Each rank is the longest chain of edges below the crate today, so the top
/// rank is the graph's depth. Rank 0 links no sibling at all: `guards` has to
/// answer before anything is built, and `platform` keeps domain types and the
/// UI toolkit out of the OS-window calls.
///
/// This table is the single source of the graph. `AGENTS.md`'s map lists crates
/// and what they own and points here for ranks; this is the copy a review
/// cites, because it is the copy that can fail.
pub const RANK: &[(&str, u8)] = &[
    ("app", 8),
    ("ui-model", 7),
    ("control-handlers", 6),
    ("backtest", 5),
    ("session", 5),
    ("drawings", 4),
    ("stores", 4),
    ("chart", 3),
    ("feed", 3),
    ("paper", 3),
    ("strategy", 3),
    ("anchored-studies", 2),
    ("broker-mt5", 2),
    ("chart-interaction", 2),
    ("feed-binance", 2),
    ("feed-hyperliquid", 2),
    ("indicator-session", 2),
    ("mcp", 2),
    ("pine", 2),
    ("sim", 2),
    ("alerts", 1),
    ("control-host", 1),
    ("control-local", 1),
    ("feed-mt5", 1),
    ("indicators", 1),
    ("orderflow", 1),
    ("replay", 1),
    ("sources", 1),
    ("trading", 1),
    ("appearance", 0),
    ("backpressure", 0),
    ("civil", 0),
    ("control", 0),
    ("engine", 0),
    ("guards", 0),
    ("layers", 0),
    ("operability", 0),
    ("orderbook", 0),
    ("platform", 0),
    ("workspace", 0),
];

/// The same-rank edges the graph still carries, as `(from, to)`.
///
/// Shrink-only: an entry whose edge is gone, or whose edge the ranks already
/// allow, is a finding, and the seed test refuses a new one. Removing an edge
/// here is how the graph gets flatter; adding one is how it would get a cycle.
pub const RANK_EXCEPTIONS: &[(&str, &str)] = &[("indicator-session", "pine")];

/// The crate whose shipped sibling dependencies are capped.
const TOP_OF_THE_GRAPH: &str = "app";

/// How many sibling crates `app` may ship with: non-optional path entries of
/// its plain and target tables, never dev or build ones.
///
/// Shrink-only, like the size ratchet: going over is a finding, and so is
/// sitting under it, which asks for the lower number. A crate only a test
/// needs goes to `[dev-dependencies]`; one only a harness needs is optional.
/// 30 since `quantick-sim` stopped re-exporting the order vocabulary: `app`
/// already linked `quantick-trading` through it, now it names the owner.
pub const APP_DEPENDENCY_CEILING: usize = 30;

/// What a `feed-*` venue crate may reach, and nothing else. A feed produces
/// trades; it links the vocabulary of a trade and of a book, and the source
/// vocabulary's symbol rules (a perpetual's `.p`) the config document checks
/// with, and no more — and no feed may depend on another feed.
const FEEDS_MAY_REACH: &[&str] = &["engine", "orderbook", "sources"];

/// The crates only the named crates may link, in any dependency table. Ranks
/// order the graph but let any higher crate reach any lower one; this keeps
/// the leaves unlinked and the window's and the feed host's exceptions where
/// `CLAUDE.md` puts them. A name ending in `*` covers every crate it prefixes.
const CONSUMED_ONLY_BY: &[(&str, &[&str])] = &[
    ("app", &[]),
    ("backtest", &[]),
    ("guards", &[]),
    ("mcp", &[]),
    ("platform", &["app"]),
    ("feed", &["app"]),
    ("broker-mt5", &["app", "feed"]),
    ("feed-*", &["app", "feed"]),
];

/// The two tables a fixture workspace may replace; the ranks are always
/// [`RANK`].
struct Rules<'a> {
    exceptions: &'a [(&'a str, &'a str)],
    app_ceiling: usize,
}

/// The tables this repository is held to.
const WORKSPACE_RULES: Rules<'static> = Rules {
    exceptions: RANK_EXCEPTIONS,
    app_ceiling: APP_DEPENDENCY_CEILING,
};

impl Rules<'_> {
    fn excepts(&self, from: &str, to: &str) -> bool {
        self.exceptions.contains(&(from, to))
    }
}

/// A crate's rank, if [`RANK`] names it.
fn rank_of(krate: &str) -> Option<u8> {
    RANK.iter()
        .find(|(name, _)| *name == krate)
        .map(|(_, rank)| *rank)
}

/// The only crates that may link `krate`, if [`CONSUMED_ONLY_BY`] restricts it.
fn allowed_consumers(krate: &str) -> Option<&'static [&'static str]> {
    CONSUMED_ONLY_BY
        .iter()
        .find(|(name, _)| match name.strip_suffix('*') {
            Some(prefix) => krate.starts_with(prefix),
            None => *name == krate,
        })
        .map(|(_, consumers)| *consumers)
}

/// The rule a dependency-direction finding cites.
const RULE_DIRECTION: &str = "CLAUDE.md: Architecture, dependency direction";

/// The rule a coverage finding cites: `CLAUDE.md` names every crate, and the
/// architecture list is what a reader is pointed at.
const RULE_CRATE_LIST: &str = "CLAUDE.md: Architecture, the crate list";

/// The rule a restricted-consumer finding cites.
const RULE_CONSUMERS: &str =
    "CLAUDE.md: Architecture, leaves stay leaves and the `feed` and `platform` exceptions";

/// Where the cap on `app` is written: the constant is the rule.
const RULE_APP_CEILING: &str = "crates/guards/src/graph.rs: APP_DEPENDENCY_CEILING";

/// Where a third-party version may be stated. Not prose anywhere: the root
/// manifest's own table is the rule.
const RULE_WORKSPACE_DEPENDENCIES: &str = "Cargo.toml: [workspace.dependencies]";

/// Where a crate's lint strictness comes from, for the same reason.
const RULE_WORKSPACE_LINTS: &str = "Cargo.toml: [workspace.lints]";

/// What the guard asks for when an edge points the wrong way.
pub const REMEDY_DIRECTION: &str = "A crate manifest declares an edge to a crate whose rank is not below its own. Remove it and \
     reach the capability the other way — through a port the lower crate defines and the higher \
     one implements. If the edge is genuinely correct, the graph changed: re-rank the crates in \
     `RANK` in `crates/guards/src/graph.rs` so every edge points down, which is where that \
     argument is signed. `RANK_EXCEPTIONS` only shrinks.";

/// What the guard asks for when a feed reaches past what a feed may link.
pub const REMEDY_FEEDS: &str = "A `feed-*` venue may link only `engine`, `orderbook` and `sources`, and never another \
     venue. Remove the edge: what the venue needed belongs in the feed host (`feed`), which \
     runs the venues and links what they report into.";

/// What the guard asks for when a crate links one it is not allowed to.
pub const REMEDY_CONSUMERS: &str = "The crate this manifest links is one only named crates may link: a leaf nothing links, \
     `platform` for the window alone, or the feed host and its venues, the one place below \
     `app` with runtimes, threads and network. Remove the edge and reach the capability through \
     a port the lower crate defines. `CONSUMED_ONLY_BY` in `crates/guards/src/graph.rs` is the \
     list.";

/// What the guard asks for when a signed exception outlived its edge.
pub const REMEDY_EXCEPTIONS: &str = "An entry in `RANK_EXCEPTIONS` in `crates/guards/src/graph.rs` no longer excuses anything: \
     its edge is gone, or the ranks already allow it. Delete the entry; the list only shrinks.";

/// What the guard asks for when a crate has no rank at all.
pub const REMEDY_COVERAGE: &str = "A crate is not covered by the dependency table, so the one-way direction is unenforced for \
     it — which looks green and is worse than a failure. Give it a rank in `RANK` in \
     `crates/guards/src/graph.rs`, above everything it depends on and below everything that \
     depends on it, and name it in `CLAUDE.md`'s architecture section.";

/// What the guard asks for when `app` ships with more siblings than its cap.
pub const REMEDY_CEILING_OVER: &str = "`app` ships with more sibling crates than `APP_DEPENDENCY_CEILING` in \
     `crates/guards/src/graph.rs` allows. Move a crate only tests use to `[dev-dependencies]`, \
     make one only a harness uses optional behind its feature, or move the code that needs it \
     into a crate below `app`. The ceiling only shrinks.";

/// What the guard asks for when `app` ships with fewer siblings than its cap.
pub const REMEDY_CEILING_UNDER: &str = "`app` ships with fewer sibling crates than `APP_DEPENDENCY_CEILING` in \
     `crates/guards/src/graph.rs`. Lower the constant to the measured number, so the crate \
     that left cannot come back unnoticed.";

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
    check_against(root, &WORKSPACE_RULES)
}

fn check_against(root: &Path, rules: &Rules<'_>) -> Vec<Finding> {
    let manifests = crate_manifests(root);
    let mut findings = Vec::new();
    findings.extend(feeds_reach_the_domain_only(&manifests));
    findings.extend(restricted_crates_keep_their_consumers(&manifests));
    findings.extend(nothing_depends_upwards(&manifests, rules));
    findings.extend(every_exception_still_excuses_an_edge(&manifests, rules));
    findings.extend(every_crate_is_ranked(&manifests));
    findings.extend(the_top_stays_under_its_ceiling(&manifests, rules));
    findings.extend(the_architecture_list_names_every_crate(root, &manifests));
    findings.extend(versions_live_in_the_root_manifest(&manifests));
    findings.extend(every_crate_inherits_the_lints(&manifests));
    findings
}

/// The same checks narrowed to one file, for the edit-time hook.
///
/// A manifest edit is the only way to move an edge, and `CLAUDE.md` is the
/// only other file this guard reads — so those are the two paths that answer.
/// The whole-repo checks run either way: they are a few string comparisons over
/// a dozen small files, and narrowing them would risk the hook and the scan
/// disagreeing, which is the one thing a per-file mode must never do.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if !in_scope(relative) {
        return Vec::new();
    }
    check(root)
}

/// Whether a workspace-relative path is one this guard reads.
///
/// The root `Cargo.toml` is here although no check reads its tables, and that
/// is deliberate rather than an oversight: moving a version into
/// `[workspace.dependencies]` is how a caller *fixes* a
/// [`RULE_WORKSPACE_DEPENDENCIES`] finding, and an author who edits the root
/// and is told nothing cannot tell a fix from a no-op. The edit re-runs the
/// checks over the crates, where the finding they were fixing lives, and
/// watching it disappear is the answer they came for.
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
/// simply absent here — and `every_crate_is_ranked` cannot see it either, so
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

/// The sibling crates a manifest depends on by path, by directory name.
///
/// Read through [`dependency_entries`] rather than by scanning raw lines for
/// the literal `path = "../`. That literal is the exact defect the version
/// rule below already had to fix once: a perfectly valid `rust_decimal="1"`
/// has no spaces around its `=`, and a `path="../pine"` written the same way
/// would have yielded no dependency at all — a reverse edge passing green,
/// which is the silent pass [`REMEDY_COVERAGE`] itself warns about. One
/// parser also means an inline table spread over several lines is understood
/// here exactly as it is there.
///
/// Sorted and deduplicated: a crate a manifest names in two tables (a plain
/// entry and a dev entry asking for a feature) is one edge.
fn path_dependencies(text: &str) -> Vec<String> {
    let mut siblings: Vec<String> = dependency_entries(text)
        .iter()
        .filter_map(|(_, value)| path_target(value))
        .collect();
    siblings.sort();
    siblings.dedup();
    siblings
}

/// The sibling crates a manifest ships with: path entries of its plain and
/// target tables that are not optional, each once. What
/// [`APP_DEPENDENCY_CEILING`] counts.
fn shipped_path_dependencies(text: &str) -> Vec<String> {
    let mut siblings: Vec<String> = dependency_entries_in(text, is_production_section)
        .iter()
        .filter(|(_, value)| !is_optional(value))
        .filter_map(|(_, value)| path_target(value))
        .collect();
    siblings.sort();
    siblings.dedup();
    siblings
}

/// Whether a dependency's value marks it `optional = true`, spaced or not.
fn is_optional(value: &str) -> bool {
    value.split("optional").skip(1).any(|after_key| {
        after_key
            .trim_start()
            .strip_prefix('=')
            .is_some_and(|after_equals| after_equals.trim_start().starts_with("true"))
    })
}

/// The sibling crate a dependency's value points at, if it points at one.
///
/// Whitespace-tolerant on both sides of the `=`, which is the whole reason
/// this exists as its own function rather than as a `find` on a literal.
fn path_target(value: &str) -> Option<String> {
    let after_key = value.split_once("path")?.1;
    let after_equals = after_key.trim_start().strip_prefix('=')?;
    let inside = after_equals.trim_start().strip_prefix('"')?;
    let sibling = inside.strip_prefix("../")?;
    Some(sibling.split('"').next()?.to_owned())
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
                REMEDY_FEEDS,
            ));
        }
    }
    findings
}

/// Every crate [`CONSUMED_ONLY_BY`] restricts is linked, in any table, only
/// by the crates it names.
fn restricted_crates_keep_their_consumers(manifests: &[Manifest]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for manifest in manifests {
        for dependency in path_dependencies(&manifest.text) {
            let Some(consumers) = allowed_consumers(&dependency) else {
                continue;
            };
            if consumers.contains(&manifest.name.as_str()) {
                continue;
            }
            let who = if consumers.is_empty() {
                "no crate".to_owned()
            } else {
                format!("only `{}`", consumers.join("`, `"))
            };
            findings.push(Finding::new(
                format!(
                    "crates/{}/Cargo.toml: depends on `{dependency}`, which {who} may link — \
                     {RULE_CONSUMERS}",
                    manifest.name
                ),
                REMEDY_CONSUMERS,
            ));
        }
    }
    findings
}

/// Every edge a ranked crate declares, in any dependency table, points at a
/// crate of a strictly lower rank, unless [`RANK_EXCEPTIONS`] signs it.
fn nothing_depends_upwards(manifests: &[Manifest], rules: &Rules<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    for manifest in manifests {
        // An unranked crate is `every_crate_is_ranked`'s finding, on either end.
        let Some(own) = rank_of(&manifest.name) else {
            continue;
        };
        for dependency in path_dependencies(&manifest.text) {
            let Some(theirs) = rank_of(&dependency) else {
                // A crate under `crates/` without a rank is reported once, as
                // that crate; a path that leads to no crate here is this one's.
                if !manifests.iter().any(|m| m.name == dependency) {
                    findings.push(Finding::new(
                        format!(
                            "crates/{}/Cargo.toml: depends by path on `{dependency}`, which is no \
                             ranked crate under `crates/` — {RULE_DIRECTION}",
                            manifest.name
                        ),
                        REMEDY_COVERAGE,
                    ));
                }
                continue;
            };
            if theirs < own || rules.excepts(&manifest.name, &dependency) {
                continue;
            }
            findings.push(Finding::new(
                format!(
                    "crates/{}/Cargo.toml: depends on `{dependency}` (rank {theirs}), which is not \
                     below its own rank {own}, so the edge reverses the one-way direction — \
                     {RULE_DIRECTION}",
                    manifest.name
                ),
                REMEDY_DIRECTION,
            ));
        }
    }
    findings
}

/// Every signed exception still excuses an edge: the edge is declared, and
/// the ranks alone would refuse it. Anything else is an entry to delete.
fn every_exception_still_excuses_an_edge(
    manifests: &[Manifest],
    rules: &Rules<'_>,
) -> Vec<Finding> {
    let mut findings = Vec::new();
    for (from, to) in rules.exceptions {
        let declared = manifests
            .iter()
            .find(|manifest| manifest.name == *from)
            .is_some_and(|manifest| {
                path_dependencies(&manifest.text)
                    .iter()
                    .any(|sibling| sibling == to)
            });
        let ranks_allow = matches!(
            (rank_of(from), rank_of(to)),
            (Some(own), Some(theirs)) if theirs < own
        );
        let why = if !declared {
            "no manifest declares that edge"
        } else if ranks_allow {
            "the ranks already allow that edge"
        } else {
            continue;
        };
        findings.push(Finding::new(
            format!(
                "crates/guards/src/graph.rs: `RANK_EXCEPTIONS` signs `{from} -> {to}`, but {why} — \
                 {RULE_DIRECTION}"
            ),
            REMEDY_EXCEPTIONS,
        ));
    }
    findings
}

/// A crate without a rank is not a failure in the checks above — it is simply
/// unguarded, which looks green and is worse. This is the reminder.
fn every_crate_is_ranked(manifests: &[Manifest]) -> Vec<Finding> {
    manifests
        .iter()
        .filter(|manifest| rank_of(&manifest.name).is_none())
        .map(|manifest| {
            Finding::new(
                format!(
                    "crates/{}/Cargo.toml: no rank covers this crate, so the one-way direction is \
                     unenforced for it — {RULE_DIRECTION}",
                    manifest.name
                ),
                REMEDY_COVERAGE,
            )
        })
        .collect()
}

/// [`TOP_OF_THE_GRAPH`] ships with exactly as many sibling crates as its
/// ceiling allows: more fails, and fewer asks for the lower number.
fn the_top_stays_under_its_ceiling(manifests: &[Manifest], rules: &Rules<'_>) -> Vec<Finding> {
    let Some(top) = manifests.iter().find(|m| m.name == TOP_OF_THE_GRAPH) else {
        return Vec::new();
    };
    let shipped = shipped_path_dependencies(&top.text).len();
    let ceiling = rules.app_ceiling;
    let (remedy, line) = match shipped.cmp(&ceiling) {
        Ordering::Equal => return Vec::new(),
        Ordering::Greater => (
            REMEDY_CEILING_OVER,
            format!(
                "crates/{TOP_OF_THE_GRAPH}/Cargo.toml: ships with {shipped} sibling crates, over the \
             ceiling of {ceiling} — {RULE_APP_CEILING}"
            ),
        ),
        Ordering::Less => (
            REMEDY_CEILING_UNDER,
            format!(
                "crates/{TOP_OF_THE_GRAPH}/Cargo.toml: ships with {shipped} sibling crates, under the \
             ceiling of {ceiling}: lower `APP_DEPENDENCY_CEILING` to {shipped} — \
             {RULE_APP_CEILING}"
            ),
        ),
    };
    vec![Finding::new(line, remedy)]
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

/// Whether a section header opens a table of dependencies the crate *ships*
/// with — plain or target-specific, never dev or build. A crate a test links
/// is not a crate the production code consumes.
fn is_production_section(header: &str) -> bool {
    is_dependency_section(header)
        && !header.ends_with("dev-dependencies]")
        && !header.ends_with("build-dependencies]")
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
    dependency_entries_in(text, is_dependency_section)
}

/// [`dependency_entries`], over only the sections `section` accepts.
fn dependency_entries_in(text: &str, section: fn(&str) -> bool) -> Vec<(String, String)> {
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
            in_dependencies = section(trimmed);
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

/// Every crate under `crates/`, by directory name, with the other crates
/// whose shipped dependencies name it by path — sorted both ways.
///
/// Read from the manifests rather than from [`RANK`]: the table says what a
/// crate *may* reach, and a crate with one consumer is a fact about what the
/// workspace actually links. Dev and build dependencies are not consumers
/// (see [`is_production_section`]), and a crate never consumes itself.
pub fn consumers(root: &Path) -> Vec<(String, Vec<String>)> {
    let manifests = crate_manifests(root);
    manifests
        .iter()
        .map(|target| {
            let by: Vec<String> = manifests
                .iter()
                .filter(|consumer| consumer.name != target.name)
                .filter(|consumer| {
                    dependency_entries_in(&consumer.text, is_production_section)
                        .iter()
                        .filter_map(|(_, value)| path_target(value))
                        .any(|sibling| sibling == target.name)
                })
                .map(|consumer| consumer.name.clone())
                .collect();
            (target.name.clone(), by)
        })
        .collect()
}

/// Every path edge the manifests declare, in any table, each once.
/// [`crate::report`] prints it, so a merge that widened the graph shows up as a
/// moved number.
pub fn edges(root: &Path) -> usize {
    crate_manifests(root)
        .iter()
        .map(|manifest| path_dependencies(&manifest.text).len())
        .sum()
}

/// The longest chain of path edges the manifests declare, signed exceptions
/// aside: the depth [`RANK`] has to fit. [`crate::report`] prints it.
///
/// A cycle, which `nothing_depends_upwards` already reports, is cut where the
/// walk meets it again rather than followed forever.
pub fn max_depth(root: &Path) -> usize {
    let graph: BTreeMap<String, Vec<String>> = crate_manifests(root)
        .into_iter()
        .map(|manifest| {
            let below = path_dependencies(&manifest.text)
                .into_iter()
                .filter(|to| !WORKSPACE_RULES.excepts(&manifest.name, to))
                .collect();
            (manifest.name, below)
        })
        .collect();
    let mut depths = BTreeMap::new();
    graph
        .keys()
        .map(|krate| depth_below(krate, &graph, &mut depths, &mut BTreeSet::new()))
        .max()
        .unwrap_or(0)
}

/// The longest chain below `krate`, memoised in `depths`; `walking` holds the
/// chain being walked, so a cycle ends the walk instead of recursing.
fn depth_below(
    krate: &str,
    graph: &BTreeMap<String, Vec<String>>,
    depths: &mut BTreeMap<String, usize>,
    walking: &mut BTreeSet<String>,
) -> usize {
    if let Some(depth) = depths.get(krate) {
        return *depth;
    }
    if !walking.insert(krate.to_owned()) {
        return 0;
    }
    let depth = graph
        .get(krate)
        .into_iter()
        .flatten()
        .map(|below| 1 + depth_below(below, graph, depths, walking))
        .max()
        .unwrap_or(0);
    walking.remove(krate);
    depths.insert(krate.to_owned(), depth);
    depth
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

    /// Check a fixture workspace against the repository's ranks, no signed
    /// exceptions, and the `app` ceiling the test states.
    fn check_fixture(root: &Path, app_ceiling: usize) -> Vec<Finding> {
        check_against(
            root,
            &Rules {
                exceptions: &[],
                app_ceiling,
            },
        )
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
        assert_eq!(check_fixture(root.path(), 1), Vec::new());
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
        let findings = check_fixture(root.path(), 0);
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
        let findings = feeds_reach_the_domain_only(&crate_manifests(root.path()));
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
        let findings = check_fixture(root.path(), 0);
        assert_eq!(findings.len(), 1, "one crate has no rank: {findings:#?}");
        assert!(
            findings[0].line.contains("crates/newcomer/Cargo.toml")
                && findings[0].line.contains("no rank covers this crate"),
            "the finding names the unguarded crate: {}",
            findings[0].line
        );
    }

    #[test]
    fn a_crate_absent_from_the_architecture_list_is_a_finding() {
        let root = workspace("graph-unlisted", &[("engine", ""), ("app", "")]);
        fs::write(root.join("CLAUDE.md"), "# fixture\n\n- `app`\n").expect("CLAUDE.md is writable");
        let findings = check_fixture(root.path(), 0);
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
        let findings = check_fixture(root.path(), 0);
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
        let findings = check_fixture(root.path(), 0);
        assert_eq!(findings.len(), 1, "the pin is found: {findings:#?}");
        assert!(
            findings[0].line.contains("tokio"),
            "the finding names the dependency: {}",
            findings[0].line
        );
    }

    /// The spelling that would have walked past the old line scan: no spaces
    /// around the `=`, which this workspace already writes for versions.
    #[test]
    fn a_reverse_edge_written_without_spaces_is_still_a_finding() {
        let root = workspace(
            "graph-tight-spelling",
            &[
                (
                    "engine",
                    "quantick-pine={path=\"../pine\"}
",
                ),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                ("app", ""),
            ],
        );
        let findings = check_fixture(root.path(), 0);
        assert_eq!(
            findings.len(),
            1,
            "the tight spelling is read: {findings:#?}"
        );
        assert!(
            findings[0]
                .line
                .starts_with("crates/engine/Cargo.toml: depends on `pine`"),
            "the finding names the edge: {}",
            findings[0].line
        );
    }

    /// An edge in an inline table spread over several lines is one edge, and
    /// the shared parser is what makes it visible here as it is to the
    /// version rule.
    #[test]
    fn a_multi_line_path_dependency_is_still_an_edge() {
        let root = workspace(
            "graph-multiline-edge",
            &[
                (
                    "engine",
                    "quantick-pine = {
    path = \"../pine\",
    optional = true,
}
",
                ),
                ("app", ""),
            ],
        );
        let findings = check_fixture(root.path(), 0);
        assert!(
            findings
                .iter()
                .any(|f| f.line.contains("depends on `pine`")),
            "the edge is seen across the line break: {findings:#?}"
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
        let findings = check_fixture(root.path(), 0);
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

    /// A sideways edge is refused as firmly as an upward one: two crates of
    /// one rank could otherwise reach each other.
    #[test]
    fn an_edge_to_a_crate_of_the_same_rank_is_a_finding() {
        let root = workspace(
            "graph-same-rank",
            &[
                ("engine", ""),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                ("sim", &edge("pine")),
                ("app", ""),
            ],
        );
        let findings = check_fixture(root.path(), 0);
        assert_eq!(findings.len(), 1, "the sideways edge: {findings:#?}");
        assert!(
            findings[0]
                .line
                .starts_with("crates/sim/Cargo.toml: depends on `pine` (rank 2)")
                && findings[0].line.contains("its own rank 2"),
            "the finding names both ranks: {}",
            findings[0].line
        );
    }

    /// A test links what it names, so a dev edge counts like a shipped one.
    #[test]
    fn an_upward_dev_dependency_is_a_finding() {
        let root = workspace(
            "graph-dev-upward",
            &[
                ("engine", &format!("\n[dev-dependencies]\n{}", edge("pine"))),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                ("app", ""),
            ],
        );
        let findings = check_fixture(root.path(), 0);
        assert_eq!(findings.len(), 1, "the dev edge: {findings:#?}");
        assert!(
            findings[0]
                .line
                .starts_with("crates/engine/Cargo.toml: depends on `pine`"),
            "the finding names the dev edge: {}",
            findings[0].line
        );
    }

    /// A signed exception excuses its edge; one whose edge is gone, or that
    /// the ranks no longer need, is an entry to delete.
    #[test]
    fn a_signed_exception_passes_and_a_stale_one_is_a_finding() {
        let root = workspace(
            "graph-exceptions",
            &[
                ("engine", ""),
                ("orderbook", ""),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                ("sim", &edge("pine")),
                ("app", ""),
            ],
        );
        let rules = Rules {
            exceptions: &[
                ("sim", "pine"),
                ("engine", "orderbook"),
                ("indicators", "engine"),
            ],
            app_ceiling: 0,
        };
        let findings = check_against(root.path(), &rules);
        let lines: Vec<&str> = findings.iter().map(|f| f.line.as_str()).collect();
        assert_eq!(lines.len(), 2, "the two stale entries: {lines:#?}");
        assert!(
            lines[0].contains("`engine -> orderbook`, but no manifest declares that edge")
                && lines[1].contains("`indicators -> engine`, but the ranks already allow"),
            "each stale entry says why: {lines:#?}"
        );
        assert!(
            findings.iter().all(|f| f.remedy == REMEDY_EXCEPTIONS),
            "both ask for the entry to go: {findings:#?}"
        );
    }

    /// `app`'s cap counts shipped, non-optional sibling entries once each:
    /// a dev entry, an optional one and a crate named twice add nothing.
    #[test]
    fn the_app_ceiling_fails_above_and_asks_to_shrink_below() {
        let app = format!(
            "{}{}quantick-sim = {{ path = \"../sim\", optional = true }}\n\
             [dev-dependencies]\n{}{}",
            edge("engine"),
            edge("pine"),
            edge("pine"),
            edge("indicators"),
        );
        let root = workspace(
            "graph-ceiling",
            &[
                ("engine", ""),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                ("sim", ""),
                ("app", &app),
            ],
        );
        assert_eq!(check_fixture(root.path(), 2), Vec::new());
        let over = check_fixture(root.path(), 1);
        assert!(
            over.len() == 1
                && over[0]
                    .line
                    .contains("ships with 2 sibling crates, over the ceiling of 1"),
            "one over the cap: {over:#?}"
        );
        let under = check_fixture(root.path(), 3);
        assert!(
            under.len() == 1
                && under[0]
                    .line
                    .contains("under the ceiling of 3: lower `APP_DEPENDENCY_CEILING` to 2")
                && under[0].line.ends_with(RULE_APP_CEILING),
            "one under the cap asks for the lower number: {under:#?}"
        );
    }

    /// Shrink-only, pinned: a new exception or a higher ceiling has to edit
    /// this test, which a review sees.
    #[test]
    fn the_exceptions_and_the_ceiling_only_shrink() {
        const SEEDED: &[(&str, &str)] = &[("indicator-session", "pine")];
        assert!(
            RANK_EXCEPTIONS.iter().all(|entry| SEEDED.contains(entry)),
            "RANK_EXCEPTIONS only loses entries: {RANK_EXCEPTIONS:?}"
        );
        const { assert!(APP_DEPENDENCY_CEILING <= 30, "the app ceiling only falls") };
    }

    #[test]
    fn no_crate_is_ranked_twice() {
        let mut names: Vec<&str> = RANK.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        let ranked = names.len();
        names.dedup();
        assert_eq!(names.len(), ranked, "no crate is ranked twice");
    }

    /// The numbers [`crate::report`] prints, on a fixture: a crate named in
    /// two tables is one edge, and the depth is the longest chain.
    #[test]
    fn the_report_counts_each_edge_once_and_the_longest_chain() {
        let root = workspace(
            "graph-report",
            &[
                ("engine", ""),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
                (
                    "app",
                    &format!("{}\n[dev-dependencies]\n{}", edge("pine"), edge("pine")),
                ),
            ],
        );
        assert_eq!(edges(root.path()), 3);
        assert_eq!(max_depth(root.path()), 3);
    }

    /// A cycle, which the direction check reports, ends the depth walk
    /// rather than recursing forever.
    #[test]
    fn the_depth_walk_survives_a_cycle() {
        let root = workspace(
            "graph-cycle",
            &[
                ("engine", &edge("indicators")),
                ("indicators", &edge("engine")),
                ("pine", &edge("indicators")),
            ],
        );
        assert_eq!(max_depth(root.path()), 2);
    }

    /// A lower rank is not enough: a leaf links to nothing above it, and the
    /// broker venue is the feed host's and the window's alone.
    #[test]
    fn a_restricted_crate_linked_by_another_is_a_finding() {
        let root = workspace(
            "graph-consumers",
            &[
                ("mcp", ""),
                ("broker-mt5", ""),
                ("chart", &edge("mcp")),
                ("paper", &edge("broker-mt5")),
                ("feed", &edge("broker-mt5")),
                ("app", ""),
            ],
        );
        let findings = check_fixture(root.path(), 0);
        let lines: Vec<&str> = findings.iter().map(|f| f.line.as_str()).collect();
        assert_eq!(
            lines.len(),
            2,
            "chart and paper offend, feed does not: {lines:#?}"
        );
        assert!(
            lines[0].starts_with("crates/chart/Cargo.toml: depends on `mcp`, which no crate may link")
                && lines[1].starts_with(
                    "crates/paper/Cargo.toml: depends on `broker-mt5`, which only `app`, `feed` may link"
                )
                && lines.iter().all(|line| line.ends_with(RULE_CONSUMERS)),
            "each names the edge and the rule: {lines:#?}"
        );
    }

    /// A path that leads out of `crates/` has no rank to compare and is
    /// reported rather than passing unread.
    #[test]
    fn a_path_dependency_outside_the_workspace_crates_is_a_finding() {
        let root = workspace(
            "graph-outside",
            &[
                ("engine", "vendored = { path = \"../../vendor/x\" }\n"),
                ("app", ""),
            ],
        );
        let findings = check_fixture(root.path(), 0);
        assert!(
            findings.iter().any(|f| f
                .line
                .contains("depends by path on `../vendor/x`, which is no ranked crate")),
            "the stray path is named: {findings:#?}"
        );
    }
}
