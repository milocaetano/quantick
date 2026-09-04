//! Ratchet guard for module cycles inside a crate.
//!
//! `CLAUDE.md` states the dependency rule one level up — `app` → `pine` →
//! `indicators` → `engine`, never a reverse edge — and cargo enforces it,
//! because a cycle between crates does not compile. Inside a crate nothing
//! enforces anything: `paper_report` may import from `paper_trading` while
//! `paper_trading` imports back, and the build is green.
//!
//! It happened twice in three days. PR #285 broke an `app`/`pane`/`tab`
//! cycle by hand. PR #282 — a well-reviewed refactor that split the paper
//! surfaces in two — welded `paper_report` and `paper_trading` together in
//! the same week, and nothing in the repository saw it: not fmt, not
//! clippy, not the build, not the suite, not two rounds of review. It was
//! found by someone drawing the graph months after the fact, which is the
//! most expensive moment to find it and the reason this file exists.
//!
//! # What a cycle costs
//!
//! Two modules in a cycle are one module with a comment between them. They
//! cannot be read apart, moved apart, or tested apart — a fixture for one
//! links the other — and the trunk-shrinking work `size` rations depends
//! on being able to lift a surface out to its own file. A cycle is the
//! edit that quietly makes the next extraction impossible.
//!
//! # What is measured
//!
//! One node per **top-level module** of a crate: `control::gateway` counts
//! as `control`, because that is the granularity a crate root declares its
//! modules at and the granularity the rule is stated at.
//!
//! One edge per **`use crate::` statement** in production code, from the
//! module the file belongs to, to the first path segment after `crate::`.
//! Deliberately not every inline `crate::foo::bar` path: a doc comment
//! linking `[`size`](crate::size)` is not a dependency, and counting those
//! reports this very crate as a three-way cycle on the day it ships. A
//! guard whose first output is a false positive is a guard that gets
//! switched off.
//!
//! Test code is not measured, for [`crate::size`]'s reason and through
//! [`crate::size::production_source`], the same function: a test module
//! reaching for a fixture is not the defect, and a guard that fired on one
//! would teach authors to write fewer tests.
//!
//! Root files (`lib.rs`, `main.rs`) are skipped. A `crate::` path always
//! begins with a top-level module name, so the root has no incoming edges
//! and can never sit inside a cycle.
//!
//! # Why the baseline does not start at zero
//!
//! Because the repository does not. When this guard was written it found
//! three cycles, not the one that prompted it: the `paper` pair that this
//! branch removed, `app`/`surfaces`/`control` (the control plane operates
//! on `QuantickApp`, which draws the surfaces that open the control
//! plane's popup), and `events`/`position` in `quantick-trading`. Starting
//! at zero would have meant rewriting two architectures nobody asked to
//! touch, in the branch that adds the guard.
//!
//! So the honest start is the ratchet's own answer, the one `size` gives
//! for `app.rs`: record what exists, signed, and forbid it from growing.
//! Every crate not listed is capped at zero — the [`THRESHOLD`] — so a new
//! cycle anywhere fails the build, which is the whole point, and the two
//! signed entries can only ever move down.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::Finding;
use crate::ratchet::Policy;
use crate::size::production_source;

/// The number of cycles a crate may have without a signed baseline entry.
///
/// Zero, unlike its sibling ratchets, and the difference is the point: a
/// large file is a cost to weigh, while a cycle is a defect to fix. There
/// is no size below which one is acceptable.
pub const THRESHOLD: usize = 0;

/// How far below its ceiling a crate may sit before the entry must be
/// tightened. Zero: breaking a cycle is a whole event, not a gradual
/// shrink, and an entry left at 2 after one was broken is permission
/// nobody re-earned to weld the pair back together.
pub const SLACK: usize = 0;

/// The recorded ceilings, workspace-relative.
pub const BASELINE_FILE: &str = "crates/guards/cycle-baseline.txt";

/// Both budget tolerances are zero for [`SLACK`]'s reason: this total
/// counts defects, and every movement in it is deliberate.
pub const BUDGET_SLACK: usize = 0;

/// See [`BUDGET_SLACK`].
pub const BUDGET_HEADROOM: usize = 0;

pub const REMEDY: &str = "A module cycle is two modules welded into one: neither can be read, moved or tested \
     without the other, and the next extraction out of the trunk is blocked by it. Break it \
     by moving what they share *down* into a module below both — `plot_area` came out of \
     `pane`, `paper_chrome` out of `paper_trading` — and not by re-exporting one from the \
     other, which hides the edge without removing it. If the cycle is genuinely deliberate, \
     raise the crate's entry in crates/guards/cycle-baseline.txt with the reason written \
     beside it, and lower another entry in the same change.";

pub const BUDGET_REMEDY: &str = "The cycle budget is every welded module pair this repository has signed for. It is the \
     one number that says whether the codebase is getting easier or harder to take apart, so \
     a new cycle is paid for by breaking an old one in the same change — not by raising the \
     cap.";

pub const BUDGET_SLACK_REMEDY: &str = "A signed cycle has been broken and the budget still reserves room for it. Good news with \
     the number already computed: run `cargo run -p quantick-guards -- --tighten`, which only \
     ever moves these numbers down.";

pub const BASELINE_REMEDY: &str = "The cycle baseline could not be read as data, so no crate was checked at all — this is a \
     syntax error in a data file, not a design problem. Each line is `path ceiling`, `#` \
     starts a comment, and exactly one `!budget <total>` caps the sum.";

pub const POLICY: Policy = Policy {
    baseline_file: BASELINE_FILE,
    threshold: THRESHOLD,
    slack: SLACK,
    budget_slack: BUDGET_SLACK,
    budget_headroom: BUDGET_HEADROOM,
    unit: "module cycles",
    remedy: REMEDY,
    budget_remedy: BUDGET_REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// One strongly connected component larger than a single module: the
/// modules welded together, and the `use crate::` statements that weld
/// them.
#[derive(Debug, PartialEq, Eq)]
pub struct Cycle {
    /// The modules in the component, sorted.
    pub modules: Vec<String>,
    /// One `from -> to  (file, and N more)` line per edge inside the
    /// component, sorted. The finding names these rather than only the
    /// modules: "app, control and surfaces are a cycle" is a fact, and
    /// "control -> app  (control/actions.rs, and 20 more)" is the line to
    /// open.
    pub edges: Vec<String>,
}

/// What a walk of `crates/` found.
pub struct Measured {
    /// Cycle counts by crate path (`crates/app`), sorted.
    pub counts: Vec<(String, usize)>,
    /// The components behind those counts, by the same crate path.
    pub cycles: BTreeMap<String, Vec<Cycle>>,
    /// Files that exist and could not be read. Reported rather than
    /// skipped, for [`crate::size`]'s reason: a file the guard could not
    /// open is not a file it has cleared, and an edge it never saw is a
    /// cycle it never found.
    pub unreadable: Vec<String>,
}

/// The module a source file belongs to, given its path relative to the
/// crate's `src/`, or `None` for a file this guard does not measure.
///
/// `None` covers the crate root, whose modules cannot be named from
/// `crate::`, and test trees, which are not production code.
pub fn module_of(relative: &str) -> Option<String> {
    if relative == "lib.rs" || relative == "main.rs" {
        return None;
    }
    let mut segments = relative.split('/');
    let head = segments.next()?;
    // A `tests/` directory anywhere below `src/`, and the `*_tests.rs`
    // files this repo splits large suites into. Both are `#[cfg(test)]`
    // modules; neither ships.
    if relative.split('/').any(|segment| segment == "tests") || relative.ends_with("_tests.rs") {
        return None;
    }
    Some(head.strip_suffix(".rs").unwrap_or(head).to_owned())
}

/// The first path segment of every `use crate::…` statement in a source
/// file, in order, with duplicates kept so a caller can count them.
///
/// A statement, not a substring: the scan starts only where a line *begins*
/// a `use` (after an optional visibility), so `crate::` inside a doc
/// comment or a string is never mistaken for an import. From there it reads
/// the use-tree across however many lines it spans, which is why the parse
/// is written by hand rather than by line.
///
/// Grouped imports are the whole reason it is not a regular expression:
/// `use crate::{app::QuantickApp, tab::Tab};` is two edges, and
/// `use crate::pane::{Pane, Split};` is one. The parser tracks, per brace
/// level, whether the next identifier is still the head of its path.
pub fn use_targets(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut targets = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let head = trimmed
            .strip_prefix("pub(crate) ")
            .or_else(|| trimmed.strip_prefix("pub(super) "))
            .or_else(|| trimmed.strip_prefix("pub "))
            .unwrap_or(trimmed);
        let Some(rest) = head.strip_prefix("use crate::") else {
            continue;
        };
        // A use tree may span lines. Rebuilding the tail from the line list
        // rather than indexing into `source` keeps the parse correct on a
        // file with CRLF endings, where `str::lines` has already dropped a
        // byte the offsets would still have counted.
        let mut tail = String::from(rest);
        let mut next = index + 1;
        while !tail.contains(';') && next < lines.len() {
            tail.push('\n');
            tail.push_str(lines[next]);
            next += 1;
        }
        read_use_tree(&tail, &mut targets);
    }
    targets
}

/// Read one use-tree, starting immediately after `crate::`, pushing the
/// head segment of every path it names.
fn read_use_tree(rest: &str, targets: &mut Vec<String>) {
    // Per brace level: whether identifiers at this level are path heads
    // (`crate::{a, b}` — yes; `a::{X, Y}` — no), and whether the next
    // identifier is still the first of its path.
    let mut heads = vec![true];
    let mut pending = vec![true];
    let bytes = rest.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let depth = heads.len() - 1;
        match bytes[index] {
            b'{' => {
                heads.push(heads[depth] && pending[depth]);
                pending.push(true);
                pending[depth] = false;
                index += 1;
            }
            b'}' => {
                if depth == 0 {
                    break;
                }
                heads.pop();
                pending.pop();
                index += 1;
            }
            b',' => {
                pending[depth] = true;
                index += 1;
            }
            b';' => break,
            byte if byte.is_ascii_alphanumeric() || byte == b'_' => {
                let from = index;
                while index < bytes.len()
                    && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
                {
                    index += 1;
                }
                if pending[depth] {
                    pending[depth] = false;
                    if heads[depth] {
                        targets.push(rest[from..index].to_owned());
                    }
                }
            }
            _ => index += 1,
        }
    }
}

/// Every strongly connected component larger than one module, sorted.
///
/// `edges` is `module -> (target, file)`, already restricted to modules the
/// crate actually has.
///
/// The components come from a reachability closure rather than from
/// Tarjan's algorithm, and that is a deliberate trade. A crate here has at
/// most a hundred modules, so the closure is a few million bit operations —
/// unmeasurable beside reading the files — and in exchange the grouping is
/// three obvious loops that a reviewer can check by reading, in a guard
/// whose whole value is that its findings are believed.
pub fn components(edges: &BTreeMap<String, BTreeSet<(String, String)>>) -> Vec<Cycle> {
    let modules: Vec<&String> = edges.keys().collect();
    let mut reaches: BTreeMap<&String, BTreeSet<&String>> = modules
        .iter()
        .map(|module| {
            let direct = edges[*module]
                .iter()
                .filter_map(|(target, _)| edges.get_key_value(target).map(|(key, _)| key))
                .collect();
            (*module, direct)
        })
        .collect();
    // Warshall: `via` is the module a path is allowed to pass through.
    for via in &modules {
        for from in &modules {
            if !reaches[*from].contains(*via) {
                continue;
            }
            let through: Vec<&String> = reaches[*via].iter().copied().collect();
            reaches.get_mut(*from).expect("module").extend(through);
        }
    }

    let mut grouped: Vec<Cycle> = Vec::new();
    let mut placed: BTreeSet<&String> = BTreeSet::new();
    for module in &modules {
        if placed.contains(*module) {
            continue;
        }
        let mutual: Vec<&String> = modules
            .iter()
            .copied()
            .filter(|other| {
                other == module
                    || (reaches[*module].contains(*other) && reaches[*other].contains(*module))
            })
            .collect();
        if mutual.len() < 2 {
            continue;
        }
        let names: BTreeSet<&String> = mutual.iter().copied().collect();
        placed.extend(names.iter().copied());
        // One line per *edge of the graph*, not per import statement. The
        // `app`/`surfaces`/`control` component has twenty-one files naming
        // `crate::app`, and printing all of them buries the two edges that
        // actually close the loop under a list of the obvious one. The
        // representative file is the first in path order, with the rest
        // counted so nobody reads the line as "this is the only place".
        let mut pairs: BTreeMap<(&String, &String), Vec<&String>> = BTreeMap::new();
        for member in &names {
            for (target, file) in &edges[*member] {
                if let Some(target) = names.get(target) {
                    pairs.entry((*member, *target)).or_default().push(file);
                }
            }
        }
        let inner: Vec<String> = pairs
            .into_iter()
            .map(|((from, to), mut files)| {
                files.sort();
                let more = match files.len() {
                    1 => String::new(),
                    n => format!(", and {} more", n - 1),
                };
                format!("{from} -> {to}  ({}{more})", files[0])
            })
            .collect();
        grouped.push(Cycle {
            modules: names.iter().map(|name| (*name).to_owned()).collect(),
            edges: inner,
        });
    }
    grouped
}

/// The cycles in one crate, given its directory.
fn measure_crate(crate_dir: &Path, crate_path: &str, unreadable: &mut Vec<String>) -> Vec<Cycle> {
    let src = crate_dir.join("src");
    let mut edges: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
    let mut files: Vec<(String, String)> = Vec::new();
    collect_sources(&src, &src, &mut files, crate_path, unreadable);
    for (relative, source) in &files {
        let Some(module) = module_of(relative) else {
            continue;
        };
        edges.entry(module.clone()).or_default();
        let production = production_source(source).join("\n");
        for target in use_targets(&production) {
            if target == module {
                continue;
            }
            edges
                .entry(module.clone())
                .or_default()
                .insert((target, format!("{crate_path}/src/{relative}")));
        }
    }
    components(&edges)
}

/// Every `.rs` file under `src`, as `(path relative to src, contents)`.
fn collect_sources(
    src: &Path,
    dir: &Path,
    out: &mut Vec<(String, String)>,
    crate_path: &str,
    unreadable: &mut Vec<String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        // A directory the guard cannot list is edges it never saw. Named
        // rather than skipped: silence here is indistinguishable from clean.
        unreadable.push(format!(
            "  {crate_path}: {} could not be listed — edges inside it were not measured",
            dir.display()
        ));
        return;
    };
    let mut paths: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_sources(src, &path, out, crate_path, unreadable);
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let Ok(relative) = path.strip_prefix(src) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        match fs::read_to_string(&path) {
            Ok(source) => out.push((relative, source)),
            // Not valid UTF-8 is the encoding guard's finding, not this
            // one's; unreadable for any other reason is still an unseen
            // edge, and both are named here rather than passed over.
            Err(e) => unreadable.push(format!("  {crate_path}/src/{relative}: unreadable: {e}")),
        }
    }
}

/// Cycle counts for every crate in the workspace, sorted by crate path.
pub fn measure(root: &Path) -> Measured {
    let mut found = Measured {
        counts: Vec::new(),
        cycles: BTreeMap::new(),
        unreadable: Vec::new(),
    };
    let Ok(entries) = fs::read_dir(root.join("crates")) else {
        return found;
    };
    let mut dirs: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    dirs.sort();
    for dir in dirs {
        if !dir.join("src").is_dir() {
            continue;
        }
        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        let crate_path = format!("crates/{name}");
        let cycles = measure_crate(&dir, &crate_path, &mut found.unreadable);
        found.counts.push((crate_path.clone(), cycles.len()));
        found.cycles.insert(crate_path, cycles);
    }
    found.counts.sort();
    found
}

/// The findings for one crate: the ratchet's verdict, and — whenever there
/// is one — the components behind the number.
///
/// The detail lines carry the same remedy as the verdict, so
/// [`crate::remedies`] prints the instruction once however many cycles a
/// crate has.
fn detailed(found: &Measured, path: &str, verdict: Option<Finding>) -> Vec<Finding> {
    let Some(verdict) = verdict else {
        return Vec::new();
    };
    let remedy = verdict.remedy;
    let mut out = vec![verdict];
    for cycle in found.cycles.get(path).into_iter().flatten() {
        out.push(Finding::new(
            format!("    {}", cycle.modules.join(" <-> ")),
            remedy,
        ));
        out.extend(
            cycle
                .edges
                .iter()
                .map(|edge| Finding::new(format!("      {edge}"), remedy)),
        );
    }
    out
}

/// What the budget counts outside the baseline: nothing.
///
/// [`crate::context`] passes a real sum here because a tracked file can be
/// split into sub-threshold pieces whose weight the ceilings stop seeing.
/// Nothing can hide below a threshold of zero, so this ratchet passes `0`
/// like [`crate::size`] does, and the budget stays a pure statement of
/// signed permissions.
///
/// Written as a named constant rather than a literal at the two call sites
/// because the first version summed the unrecorded counts, and every new
/// cycle in an unlisted crate then reported *twice* — once as a crate over
/// its threshold, once as a repository over budget, each with a different
/// remedy. Two instructions for one defect is how an author ends up
/// following the wrong one.
const UNRECORDED: usize = 0;

/// Every way the recorded baseline and the module graph disagree.
pub fn check(root: &Path) -> Vec<Finding> {
    let sources = root.join("crates");
    if !sources.is_dir() {
        // For [`crate::size::check`]'s reason: an unreadable `crates/`
        // measures as empty, which reports every entry stale — and the
        // stated remedy for a stale entry is to delete it.
        return vec![Finding::new(
            format!(
                "  {} is not a readable directory — there is nothing to measure, and every \
                 baseline entry would otherwise be reported stale",
                sources.display()
            ),
            BASELINE_REMEDY,
        )];
    }
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let found = measure(root);
    let mut violations: Vec<Finding> = found
        .unreadable
        .iter()
        .map(|line| Finding::new(line.clone(), BASELINE_REMEDY))
        .collect();
    for (path, count) in &found.counts {
        violations.extend(detailed(
            &found,
            path,
            POLICY.verdict(recorded.entry(path), path, *count),
        ));
    }
    violations.extend(POLICY.budget_verdict(&recorded, UNRECORDED));
    violations.extend(
        recorded
            .entries
            .iter()
            .filter(|entry| !found.counts.iter().any(|(path, _)| path == &entry.path))
            .map(|entry| POLICY.stale(&entry.path)),
    );
    violations
}

/// The same verdict for the crate one edited file belongs to — what the
/// edit-time hook calls after a write.
///
/// A single file cannot answer this question the way it answers a line
/// count: a cycle is a property of the graph, and the edge an edit adds
/// closes a loop through modules the file never mentions. So the unit here
/// is the crate, which is a few dozen small reads rather than a walk of the
/// workspace.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    // The whole scan, not just the budget: with a threshold of zero every
    // entry in this baseline is load-bearing, so *lowering* one below its
    // crate's real count is the edit that matters here — and a hook that
    // answered only the budget would have called that edit clean and left
    // the suite to find it. The scan is a few hundred small reads.
    if relative == BASELINE_FILE {
        return check(root);
    }
    let Some(crate_name) = crate_of(relative) else {
        return Vec::new();
    };
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let crate_path = format!("crates/{crate_name}");
    let mut found = Measured {
        counts: Vec::new(),
        cycles: BTreeMap::new(),
        unreadable: Vec::new(),
    };
    let cycles = measure_crate(
        &root.join("crates").join(&crate_name),
        &crate_path,
        &mut found.unreadable,
    );
    let count = cycles.len();
    found.cycles.insert(crate_path.clone(), cycles);
    let mut violations: Vec<Finding> = found
        .unreadable
        .iter()
        .map(|line| Finding::new(line.clone(), BASELINE_REMEDY))
        .collect();
    violations.extend(detailed(
        &found,
        &crate_path,
        POLICY.verdict(recorded.entry(&crate_path), &crate_path, count),
    ));
    violations
}

/// The crate a workspace-relative path belongs to, if it is Rust source
/// this guard measures.
fn crate_of(relative: &str) -> Option<String> {
    let rest = relative.strip_prefix("crates/")?;
    let (name, inside) = rest.split_once('/')?;
    let inside = inside.strip_prefix("src/")?;
    if !inside.ends_with(".rs") {
        return None;
    }
    Some(name.to_owned())
}

/// Lower any entry whose crate now has fewer cycles than it is signed for.
/// Down only, like every other ratchet here.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let found = measure(root);
    POLICY.tighten(root, &found.counts, UNRECORDED)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(pairs: &[(&str, &str)]) -> BTreeMap<String, BTreeSet<(String, String)>> {
        let mut map: BTreeMap<String, BTreeSet<(String, String)>> = BTreeMap::new();
        for (from, to) in pairs {
            map.entry((*from).to_owned()).or_default();
            map.entry((*to).to_owned()).or_default();
            map.entry((*from).to_owned())
                .or_default()
                .insert(((*to).to_owned(), format!("{from}.rs")));
        }
        map
    }

    /// The defect PR #282 landed, in miniature: two modules importing each
    /// other. This is the case the guard exists for, and it must be a
    /// finding rather than a pass.
    #[test]
    fn a_two_module_cycle_is_found() {
        let found = components(&edges(&[
            ("paper_report", "paper_trading"),
            ("paper_trading", "paper_report"),
        ]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].modules, vec!["paper_report", "paper_trading"]);
        assert_eq!(
            found[0].edges,
            vec![
                "paper_report -> paper_trading  (paper_report.rs)",
                "paper_trading -> paper_report  (paper_trading.rs)",
            ]
        );
    }

    /// Twenty-one files in the `app` crate name `crate::app`, and they are
    /// one edge, not twenty-one findings. The count still says so, because
    /// a line reading like the only site would send an author to fix one
    /// file and call the cycle broken.
    #[test]
    fn many_files_on_one_edge_collapse_to_one_line() {
        let mut map = edges(&[("control", "app"), ("app", "control")]);
        for file in ["control/gateway.rs", "control/trade.rs"] {
            map.get_mut("control")
                .expect("control")
                .insert(("app".to_owned(), file.to_owned()));
        }
        let found = components(&map);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].edges,
            vec![
                "app -> control  (app.rs)",
                "control -> app  (control.rs, and 2 more)",
            ]
        );
    }

    /// The shape this branch left behind: one module below two others,
    /// imported by both, importing neither. It must be clean, or the fix
    /// the guard recommends would itself be a finding.
    #[test]
    fn a_shared_module_below_two_others_is_not_a_cycle() {
        let found = components(&edges(&[
            ("paper_report", "paper_chrome"),
            ("paper_trading", "paper_chrome"),
            ("paper_trading", "paper_report"),
        ]));
        assert!(found.is_empty(), "{found:?}");
    }

    /// Three modules closing a loop the long way round — the
    /// `app`/`surfaces`/`control` shape. A guard that only looked at pairs
    /// would call this clean.
    #[test]
    fn a_cycle_through_a_third_module_is_found() {
        let found = components(&edges(&[
            ("app", "surfaces"),
            ("surfaces", "control"),
            ("control", "app"),
        ]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].modules, vec!["app", "control", "surfaces"]);
    }

    /// Two independent cycles in one crate count as two, not one: the
    /// baseline number is what a reviewer reads to know how much is left
    /// to untangle.
    #[test]
    fn independent_cycles_are_counted_separately() {
        let found = components(&edges(&[
            ("a", "b"),
            ("b", "a"),
            ("c", "d"),
            ("d", "c"),
            ("a", "c"),
        ]));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].modules, vec!["a", "b"]);
        assert_eq!(found[1].modules, vec!["c", "d"]);
    }

    /// An edge to something the crate has no module for — a re-export of a
    /// dependency, say — is not an edge inside the crate.
    #[test]
    fn an_edge_to_an_unknown_module_is_dropped() {
        let mut map = edges(&[("a", "b"), ("b", "a")]);
        map.get_mut("a")
            .expect("a")
            .insert(("nowhere".to_owned(), "a.rs".to_owned()));
        let found = components(&map);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].modules, vec!["a", "b"]);
    }

    #[test]
    fn a_plain_import_names_one_module() {
        assert_eq!(use_targets("use crate::pane::Pane;\n"), vec!["pane"]);
    }

    /// The grouped form `control/session.rs` uses. A scan that only read
    /// the first segment after `crate::` would see no edge here at all,
    /// and the `app`/`surfaces`/`control` cycle would go unreported.
    #[test]
    fn a_grouped_import_names_every_module_in_the_group() {
        assert_eq!(
            use_targets(
                "use crate::{app::QuantickApp, paper_chrome::PositionSummary, tab::Tab};\n"
            ),
            vec!["app", "paper_chrome", "tab"]
        );
    }

    /// A nested group names items, not modules: only the head counts.
    #[test]
    fn a_nested_group_does_not_promote_items_to_modules() {
        assert_eq!(
            use_targets("use crate::{drawings::{Drawing, Kind}, theme};\n"),
            vec!["drawings", "theme"]
        );
    }

    #[test]
    fn a_use_tree_may_span_lines() {
        assert_eq!(
            use_targets("use crate::paper_chrome::{\n    caption,\n    pill_toggle,\n};\n"),
            vec!["paper_chrome"]
        );
    }

    #[test]
    fn a_re_export_is_an_edge_like_any_other() {
        assert_eq!(
            use_targets("pub(crate) use crate::paper_report::{HistoryRow, LedgerScope};\n"),
            vec!["paper_report"]
        );
    }

    /// The false positive that would have made the guard untrustworthy on
    /// its first run: this crate's own doc comments link their siblings
    /// with `crate::` paths, and none of them is a dependency.
    #[test]
    fn a_crate_path_in_prose_is_not_an_edge() {
        let source = "//! See [`size`](crate::size) — it does not `use crate::size` here.\n\
                      /// The remedy mentions crate::ratchet too.\n\
                      let name = \"use crate::pane\";\n";
        assert!(use_targets(source).is_empty(), "{:?}", use_targets(source));
    }

    #[test]
    fn a_renamed_import_still_names_its_module() {
        assert_eq!(use_targets("use crate::theme as colours;\n"), vec!["theme"]);
    }

    #[test]
    fn the_crate_root_is_not_a_module() {
        assert_eq!(module_of("lib.rs"), None);
        assert_eq!(module_of("main.rs"), None);
    }

    #[test]
    fn a_nested_file_belongs_to_its_top_level_module() {
        assert_eq!(module_of("control/gateway.rs").as_deref(), Some("control"));
        assert_eq!(module_of("pane.rs").as_deref(), Some("pane"));
        assert_eq!(
            module_of("surfaces/drawing_chrome/mod.rs").as_deref(),
            Some("surfaces")
        );
    }

    /// Test trees are not measured, in both spellings this repo uses.
    #[test]
    fn test_trees_are_not_measured() {
        assert_eq!(module_of("app/tests/mod.rs"), None);
        assert_eq!(module_of("app/tests/paper_trading_tests.rs"), None);
        assert_eq!(module_of("resample_tests.rs"), None);
    }

    /// The hook and the suite must say the same thing about the same
    /// crate. Both sibling ratchets pin this, and the failure it forbids is
    /// the worst one a guard has: an author edits, is told nothing, and
    /// finds the violation in CI.
    #[test]
    fn check_file_agrees_with_the_whole_repo_scan() {
        let root = crate::workspace_root();
        let scan = check(&root);
        let whole: Vec<&str> = scan.iter().map(|f| f.line.as_str()).collect();
        for (crate_path, _) in measure(&root).counts {
            let source = ["src/lib.rs", "src/main.rs"]
                .into_iter()
                .map(|tail| format!("{crate_path}/{tail}"))
                .find(|candidate| root.join(candidate).is_file())
                .unwrap_or_else(|| panic!("{crate_path} has a crate root"));
            let single = check_file(&root, &source);
            let prefix = format!("  {crate_path}: ");
            assert_eq!(
                single
                    .iter()
                    .filter(|f| f.line.starts_with(&prefix))
                    .count(),
                whole.iter().filter(|l| l.starts_with(&prefix)).count(),
                "the two surfaces disagree about whether {crate_path} has a verdict"
            );
            for finding in &single {
                assert!(
                    whole.contains(&finding.line.as_str()),
                    "the hook reports `{}` for {crate_path} and the scan does not",
                    finding.line
                );
            }
        }
    }

    #[test]
    fn only_rust_sources_inside_a_crate_are_routed_to_a_crate() {
        assert_eq!(crate_of("crates/app/src/pane.rs").as_deref(), Some("app"));
        assert_eq!(
            crate_of("crates/guards/src/cycle.rs").as_deref(),
            Some("guards")
        );
        assert_eq!(crate_of("crates/app/Cargo.toml"), None);
        assert_eq!(crate_of("crates/app/tests/guards.rs"), None);
        assert_eq!(crate_of("docs/README.md"), None);
    }
}
