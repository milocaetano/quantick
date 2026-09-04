//! The repository's health numbers, as one stable text table.
//!
//! Every mission of the refactor sprint has been graded on the same dozen
//! numbers — how much of the workspace is `app`, which files are still the
//! largest, how wide the widest structs are, what the two ratchets have signed
//! for against what the tree actually weighs — and each grading re-derived
//! them by hand from ad-hoc `wc`, `grep` and `awk` runs. Re-derivation is not
//! free and it is not reproducible: two sessions counting "production lines"
//! with two different `grep` invocations produce two different answers, and
//! neither can be diffed against the other.
//!
//! So the crate that already owns the measurements answers the question
//! itself. [`render`] returns a `label<TAB>value` table with one line per
//! number, in a fixed order, with no timestamps and no absolute paths — which
//! makes `diff` between a report taken before a merge and one taken after
//! *the report of what the merge changed*.
//!
//! # Why the rules here are the size guard's rules
//!
//! Nothing in this module decides for itself what "production" means. It
//! walks with [`size::measure`] and strips test items with
//! [`size::production_source`], because a second definition of production
//! source is the duplicated-constant defect this repository files against its
//! own code — and the first symptom would be a report whose per-crate totals
//! disagree with the ratchet that rations them, with no way to tell which of
//! the two is lying.
//!
//! # Why this measures and enforces nothing
//!
//! Not one of these numbers is ratcheted. The mode exists to make "did it
//! improve?" answerable, and a number that fails a build is a number people
//! negotiate with rather than read. Deciding which of them earns a ceiling is
//! a later decision, taken with a few weeks of reports to look at.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::{GUARDS, graph, headless, size};

/// How many of the largest production files the report names.
///
/// Eight because that is roughly where this repository's tail flattens: the
/// files above it are the extraction targets a sprint argues about, and the
/// ones below are ordinary. A longer list would move more lines per diff
/// without adding a decision to take.
pub const LARGEST_FILES: usize = 8;

/// How many fields a `pub struct` must carry to be called wide.
///
/// A struct this size is the same defect the size ratchet exists for, one
/// level down: a type that absorbed a subsystem instead of docking against
/// it. Thirty is deliberately far above anything ordinary — the report is
/// meant to name four or five types, not audit every record in the tree.
pub const WIDE_STRUCT_FIELDS: usize = 30;

/// One indent of Rust, in spaces. A field of a struct sits at exactly one;
/// anything deeper belongs to a nested type or a generic bound.
const INDENT: usize = 4;

/// The substrings counted per production line, each with the label it is
/// reported under.
///
/// A table rather than three counters because the three questions are the
/// same question — how many sites of a thing the tree still carries — and
/// three hand-written loops is how one of them silently starts counting over
/// a different set of files than the other two.
///
/// Each needle is spelled in two halves so this table cannot match itself.
/// The scan walks every production file in the workspace, this one included,
/// and a whole literal here would add a phantom site to its own count — a
/// number wrong by construction, in the direction that makes the tree look
/// worse than it is, and wrong again the day somebody splits the module.
pub const SITES: [(&str, &str); 2] = [
    ("site.allow", concat!("#[all", "ow(")),
    ("site.process_id", concat!("process:", ":id()")),
];

/// How many times each entry of [`SITES`] occurs in one file's production
/// source, in that order.
///
/// Public so a fixture can pin the one property that is easy to get wrong and
/// impossible to notice: these counts see production source only, so a
/// `process::id()` a test module opens a scratch directory with is *not* a
/// site the repository still carries. Counting it would score the branch that
/// moved tests out of a production file as no improvement at all.
pub fn site_counts(production: &[&str]) -> Vec<usize> {
    SITES
        .iter()
        .map(|(_, needle)| {
            production
                .iter()
                .map(|line| line.matches(needle).count())
                .sum()
        })
        .collect()
}

/// The site counted over the *whole* file rather than over production source.
///
/// `#[cfg(test)]` is exactly what [`size::production_source`] strips, so
/// counting it in production source would score zero by construction and the
/// row would be a lie that never moves. What the sprint wants to know is how
/// many production files still carry their tests inside them, which is a
/// count over the file as written.
///
/// Spelled in halves for the reason [`SITES`] gives.
const TEST_MODULE_SITE: &str = concat!("#[cfg(te", "st)]");

/// The identifier whose absence marks a line as portable out of `app`.
///
/// `app` is the only crate allowed to know about the UI toolkit, so a
/// production line in it that never names `egui` is a line some other crate
/// could hold. The count is headroom, not a defect list: plenty of those
/// lines are genuinely app glue.
const UI_IDENTIFIER: &str = "egui";

/// The crate whose share of the workspace the sprint is trying to shrink.
const TRUNK_CRATE: &str = "app";

/// The whole report, ready to print.
///
/// Deterministic by construction: every section sorts before it prints, the
/// only paths are workspace-relative with forward slashes, and nothing here
/// reads a clock, an environment variable or a random number. Two calls
/// against the same tree return equal strings, which is the property
/// `report_is_byte_identical_across_runs` pins.
pub fn render(root: &Path) -> String {
    let sizes = size::measure(root);
    let scanned = Scan::of(root, &sizes.counts);

    let mut out = String::new();
    crates_section(&mut out, &sizes.counts);
    largest_files_section(&mut out, &sizes.counts);
    wide_structs_section(&mut out, &scanned);
    ratchets_section(&mut out, root);
    sites_section(&mut out, &scanned);
    row(
        &mut out,
        format!("{TRUNK_CRATE}.lines.without_{UI_IDENTIFIER}"),
        scanned.portable_trunk_lines,
    );
    // The two architecture invariants, as numbers a merge can be diffed on:
    // how wide the graph is, and whether the headless rule still holds. Both
    // are measurements here and enforcement elsewhere -- `--report` describes
    // the tree, it does not judge it.
    row(&mut out, "graph.edges", graph::edges());
    row(&mut out, "headless.findings", headless::findings(root));
    row(&mut out, "scan.unreadable", sizes.unreadable.len());
    row(&mut out, "scan.undecodable", sizes.undecodable.len());
    row(&mut out, "scan.blind", sizes.blind.len());
    out
}

/// One `label<TAB>value` line. The single owner of the report's shape, so a
/// section cannot invent a second one.
fn row(out: &mut String, label: impl AsRef<str>, value: impl std::fmt::Display) {
    let _ = writeln!(out, "{}\t{value}", label.as_ref());
}

/// The crate a workspace-relative path belongs to: the directory immediately
/// under `crates/`, which is how both baselines already spell paths.
fn crate_of(path: &str) -> Option<&str> {
    path.strip_prefix("crates/")?.split('/').next()
}

/// Production lines per crate, the workspace total, and the trunk's share.
///
/// The percentage is integer arithmetic on purpose. A fractional percent
/// invites a formatting decision that can differ between float paths, and a
/// report whose last digit wobbles is one nobody can diff.
fn crates_section(out: &mut String, counts: &[(String, usize)]) {
    let mut per_crate: Vec<(&str, usize)> = Vec::new();
    for (path, lines) in counts {
        let Some(name) = crate_of(path) else { continue };
        match per_crate.iter_mut().find(|(known, _)| *known == name) {
            Some((_, total)) => *total += lines,
            None => per_crate.push((name, *lines)),
        }
    }
    per_crate.sort();
    for (name, lines) in &per_crate {
        row(out, format!("crate.lines.{name}"), lines);
    }
    let total: usize = per_crate.iter().map(|(_, lines)| lines).sum();
    let trunk = per_crate
        .iter()
        .find(|(name, _)| *name == TRUNK_CRATE)
        .map_or(0, |(_, lines)| *lines);
    row(out, "crate.lines.total", total);
    // A zero total is an unreadable `crates/`, which `render` also reports as
    // `scan.blind`. Printing 0 rather than dividing keeps the report from
    // panicking on the one tree it most needs to describe.
    let share = (trunk * 100).checked_div(total).unwrap_or(0);
    row(out, format!("crate.lines.{TRUNK_CRATE}_percent"), share);
}

/// The [`LARGEST_FILES`] biggest production files.
///
/// Selected by line count, then printed by path. Printing them in rank order
/// would make one file overtaking another rewrite every line between them;
/// sorting the output by path means a diff shows the files that actually
/// changed and nothing else.
fn largest_files_section(out: &mut String, counts: &[(String, usize)]) {
    let mut ranked: Vec<&(String, usize)> = counts.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut largest: Vec<&(String, usize)> = ranked.into_iter().take(LARGEST_FILES).collect();
    largest.sort();
    for (path, lines) in largest {
        row(out, format!("file.largest.{path}"), lines);
    }
}

/// Every wide struct the scan found, by crate-qualified name.
fn wide_structs_section(out: &mut String, scanned: &Scan) {
    for (name, fields) in &scanned.wide_structs {
        row(out, format!("struct.wide.{name}"), fields);
    }
}

/// Each ratchet's signed `!budget`, the recorded ceilings it caps, and what
/// the files it tracks weigh today.
///
/// Three numbers rather than the two the mission asked for, because the
/// budget caps the *recorded* total and not the measured one. The slack
/// between recorded and measured is the debt a sprint pays down before
/// `--tighten` writes it off, and it is the number that moves first when a
/// refactor lands.
///
/// The registry is the only list. A ratchet added to [`GUARDS`] appears here
/// without an edit, which is the property that stopped the cycle ratchet from
/// being invisible to `--tighten` when it was added.
fn ratchets_section(out: &mut String, root: &Path) {
    for guard in GUARDS {
        let Some(ratchet) = &guard.ratchet else {
            continue;
        };
        let name = guard.name;
        match ratchet.policy.baseline(root) {
            Ok(baseline) => {
                match &baseline.budget {
                    Some(budget) => row(out, format!("ratchet.{name}.budget"), budget.allowed),
                    // Not silently zero. A missing directive is a cap that
                    // stopped existing, and a `0` here would read as a
                    // ratchet holding the line perfectly.
                    None => row(out, format!("ratchet.{name}.budget"), "absent"),
                }
                row(out, format!("ratchet.{name}.recorded"), baseline.recorded());
            }
            // The report describes the tree; it does not fail on it. The
            // guards themselves already turn an unparsable baseline into a
            // finding with a remedy, and duplicating that here would give the
            // sprint two voices on the same problem.
            Err(_) => {
                row(out, format!("ratchet.{name}.budget"), "unparsed");
                row(out, format!("ratchet.{name}.recorded"), "unparsed");
            }
        }
        row(
            out,
            format!("ratchet.{name}.measured"),
            (ratchet.measured)(root),
        );
    }
}

/// The counted sites, in the fixed order [`SITES`] lists them.
fn sites_section(out: &mut String, scanned: &Scan) {
    for ((label, _), count) in SITES.iter().zip(&scanned.sites) {
        row(out, *label, count);
    }
    row(out, "site.cfg_test", scanned.test_modules);
}

/// Everything the report reads out of file *contents*, gathered in one pass.
///
/// The paths come from [`size::measure`], so this never decides for itself
/// which files are in scope; it only re-reads what that walk already found
/// and asks four more questions of each one.
struct Scan {
    /// Wide structs by crate-qualified name, sorted.
    wide_structs: Vec<(String, usize)>,
    /// One count per entry of [`SITES`], in the same order.
    sites: Vec<usize>,
    /// `#[cfg(test)]` occurrences in files that are not themselves test files.
    test_modules: usize,
    /// Production lines in the trunk crate that never name the UI toolkit.
    portable_trunk_lines: usize,
}

impl Scan {
    fn of(root: &Path, counts: &[(String, usize)]) -> Self {
        let mut scan = Scan {
            wide_structs: Vec::new(),
            sites: vec![0; SITES.len()],
            test_modules: 0,
            portable_trunk_lines: 0,
        };
        let trunk = format!("crates/{TRUNK_CRATE}/src/");
        for (path, _) in counts {
            // A file the walk measured and this pass cannot read is left out
            // rather than guessed at; `size::measure` is the surface that
            // reports it, and `render` prints its count as `scan.unreadable`.
            let Ok(source) = fs::read_to_string(root.join(path)) else {
                continue;
            };
            if !is_test_file(path) {
                scan.test_modules += source.matches(TEST_MODULE_SITE).count();
            }
            let production = size::production_source(&source);
            for (total, found) in scan.sites.iter_mut().zip(site_counts(&production)) {
                *total += found;
            }
            if path.starts_with(&trunk) {
                scan.portable_trunk_lines += production
                    .iter()
                    .filter(|line| !line.contains(UI_IDENTIFIER))
                    .count();
            }
            if let Some(name) = crate_of(path) {
                scan.wide_structs.extend(
                    wide_structs(&production)
                        .into_iter()
                        .map(|(struct_name, fields)| (format!("{name}::{struct_name}"), fields)),
                );
            }
        }
        scan.wide_structs.sort();
        scan
    }
}

/// Whether a path is test code by its own name.
///
/// `size`'s walk already refuses any path with a `tests/` segment, so the
/// only test files left in scope are the `tests.rs` siblings a module split
/// leaves behind. Counting their `#[cfg(test)]` would score the extraction
/// that moved tests *out* of a production file as no improvement at all.
fn is_test_file(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|file| file == "tests.rs")
}

/// Every `pub struct` in this production source with at least
/// [`WIDE_STRUCT_FIELDS`] fields, by name.
///
/// A line rule rather than a parse: a `pub struct Name … {` at column zero
/// opens a body, a line at exactly one [`INDENT`] naming a field closes over
/// it, and a `}` back at column zero ends it. That is the rule the mission
/// stated, and it is the rule `rustfmt` guarantees over this repository —
/// every source here is formatted, so the indent carries the structure.
/// Writing a real parser would be a larger and less predictable thing than
/// the number is worth, and the fixture tests pin the rule rather than a
/// parse tree.
pub fn wide_structs(production: &[&str]) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    let mut index = 0;
    while index < production.len() {
        let Some(name) = struct_name(production[index]) else {
            index += 1;
            continue;
        };
        index += 1;
        let mut fields = 0;
        while index < production.len() && production[index] != "}" {
            if is_field(production[index]) {
                fields += 1;
            }
            index += 1;
        }
        if fields >= WIDE_STRUCT_FIELDS {
            found.push((name.to_owned(), fields));
        }
    }
    found
}

/// The name of the `pub struct` this line opens a body for, if it does.
///
/// Only a brace-bodied struct at column zero qualifies. A tuple struct and a
/// unit struct end on their own line with `;` and have no fields of the shape
/// this counts, and an indented `struct` is nested inside something that is
/// already being counted.
fn struct_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("pub struct ")?;
    if !rest.ends_with('{') {
        return None;
    }
    let name = rest
        .split(|c: char| c == '<' || c == '{' || c == '(' || c.is_whitespace())
        .next()?;
    if name.is_empty() { None } else { Some(name) }
}

/// Whether a line inside a struct body declares a field.
///
/// Exactly one indent, then an optional visibility, then an identifier and a
/// colon. The indent test is what keeps a nested type's own fields, a
/// multi-line generic bound and a `where` clause out of the count; the colon
/// is what keeps attributes, doc comments and blank lines out.
fn is_field(line: &str) -> bool {
    let Some(rest) = line.strip_prefix(&" ".repeat(INDENT)) else {
        return false;
    };
    if rest.starts_with(' ') {
        return false;
    }
    let rest = match rest.strip_prefix("pub") {
        // `pub name:` and `pub(crate) name:` both declare a field; `public: u8`
        // is a field called `public` and must not lose its own prefix.
        Some(after) => after
            .strip_prefix(' ')
            .or_else(|| after.split_once(") ").map(|(_, tail)| tail))
            .unwrap_or(rest),
        None => rest,
    };
    let Some((name, _)) = rest.split_once(':') else {
        return false;
    };
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}
