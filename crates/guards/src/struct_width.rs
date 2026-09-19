//! How many fields a struct may carry, rationed.
//!
//! A struct with dozens of fields is the size ratchet's defect one level down:
//! a type that absorbed a subsystem instead of docking against one. `--report`
//! has printed every such struct as a `struct.wide.*` row since the refactor
//! sprints began, and nothing failed when one grew — `app::Tab` could gain a
//! field on every branch while every file that declares it stayed under the
//! size threshold, because the fields were the thing growing, not the file.
//!
//! So the rows are a ratchet now. Every struct at or over
//! [`WIDE_STRUCT_FIELDS`] carries a `crate::Name fields` entry in
//! [`BASELINE_FILE`] on the shared [`Policy`]; one over its entry fails, a
//! newly wide one with no entry fails, and a vanished one is stale. The slack
//! is zero: removing a single field is a finding until `--tighten` writes the
//! new count, so the ceiling follows every shrink and a field taken out cannot
//! quietly be put back.
//!
//! # What is counted
//!
//! A line rule rather than a parse: a `struct Name … {` at column zero opens a
//! body, a line at exactly one [`INDENT`] naming a field counts, and a `}`
//! back at column zero ends it. `rustfmt` guarantees that shape over this
//! repository, so the indent carries the structure. Every visibility counts —
//! the report once counted only `pub struct`, and deleting `pub` is not an
//! architectural improvement. Production source only, as [`size`] defines it:
//! a test fixture's struct is not the trunk.
//!
//! A struct is named by its crate and its type name, as the report labels it.
//! Two structs of the same name in one crate share an entry and are held to
//! the wider of the two; renaming a type to dodge that is a diff a reviewer
//! reads.

use std::fs;
use std::path::Path;

use crate::Finding;
use crate::ratchet::{Policy, Unmeasured};
use crate::size;

/// The recorded field counts and their budget.
pub const BASELINE_FILE: &str = "crates/guards/struct-width-baseline.txt";

/// How many fields a struct must carry to be called wide, and so to need an
/// entry.
///
/// Thirty is deliberately far above anything ordinary — the ratchet is meant
/// to name a handful of types, not audit every record in the tree.
pub const WIDE_STRUCT_FIELDS: usize = 30;

/// One indent of Rust, in spaces. A field of a struct sits at exactly one;
/// anything deeper belongs to a nested type or a generic bound.
const INDENT: usize = 4;

/// The tree every struct is taken from.
const SOURCE: &str = "crates/";

/// What the guard asks for when a struct is over, under or missing its entry.
pub const REMEDY: &str = "A struct past its recorded field count absorbed a subsystem instead \
    of docking against one. Group the new state into an owner type the struct holds as one \
    field, or take as many fields out in the same change. A deliberate raise is the struct's \
    entry and the !budget in crates/guards/struct-width-baseline.txt, both raised and signed \
    with a reason in the same change. A struct that lost fields needs no argument: `cargo run \
    -p quantick-guards -- --tighten` writes the new count.";

/// What the guard asks for when the budget sits above the recorded counts.
pub const BUDGET_SLACK_REMEDY: &str = "The recorded field counts fell and the !budget has not \
    caught up. Nothing has to be argued: `cargo run -p quantick-guards -- --tighten` writes the \
    new total, and only ever downward.";

/// What the guard asks for when the baseline itself cannot be read as data.
pub const BASELINE_REMEDY: &str = "crates/guards/struct-width-baseline.txt could not be read as \
    data, so no struct was checked. Every line is blank, a `#` comment, the one \
    `!budget <count>` directive, or `<crate>::<Struct> <fields>`. Fix the line the finding \
    names.";

/// What the guard asks for when the tree could not be scanned whole.
pub const UNMEASURED_REMEDY: &str = "The struct widths could not be taken, so no struct was \
    checked: a path under crates/ could not be listed, read or decoded. Fix the path the finding \
    names. Until then the guard is not reporting a clean tree; it is reporting that it could not \
    look.";

/// This guard's ratchet: the shared mechanism, with this guard's wording.
pub const POLICY: Policy = Policy {
    baseline_file: BASELINE_FILE,
    // A struct *at* the width needs an entry; the policy asks `over`.
    threshold: WIDE_STRUCT_FIELDS - 1,
    // Zero both ways: one field is a design decision, not noise to absorb.
    slack: 0,
    budget_slack: 0,
    budget_headroom: 0,
    unit: "fields",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: BUDGET_SLACK_REMEDY,
    baseline_remedy: BASELINE_REMEDY,
};

/// Every brace-bodied struct in this production source with its field count,
/// in source order, whatever its width.
pub fn widths(production: &[&str]) -> Vec<(String, usize)> {
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
        found.push((name.to_owned(), fields));
    }
    found
}

/// Every struct in this production source with at least
/// [`WIDE_STRUCT_FIELDS`] fields, by name.
pub fn wide_structs(production: &[&str]) -> Vec<(String, usize)> {
    widths(production)
        .into_iter()
        .filter(|(_, fields)| *fields >= WIDE_STRUCT_FIELDS)
        .collect()
}

/// The name of the struct this line opens a body for, if it does.
///
/// Only a brace-bodied struct at column zero qualifies, at any visibility. A
/// tuple struct and a unit struct end on their own line with `;` and have no
/// fields of the shape this counts, and an indented `struct` is nested inside
/// something that is already being counted.
fn struct_name(line: &str) -> Option<&str> {
    let rest = strip_visibility(line).strip_prefix("struct ")?;
    if !rest.ends_with('{') {
        return None;
    }
    let name = rest
        .split(|c: char| c == '<' || c == '{' || c == '(' || c.is_whitespace())
        .next()?;
    if name.is_empty() { None } else { Some(name) }
}

/// A line with any leading `pub`, `pub(crate)`, `pub(super)` or
/// `pub(in path)` taken off. `public: u8` is not a visibility and keeps its
/// prefix.
fn strip_visibility(line: &str) -> &str {
    let Some(after) = line.strip_prefix("pub") else {
        return line;
    };
    after
        .strip_prefix(' ')
        .or_else(|| {
            after
                .strip_prefix('(')
                .and_then(|inner| inner.split_once(") ").map(|(_, tail)| tail))
        })
        .unwrap_or(line)
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
    let Some((name, _)) = strip_visibility(rest).split_once(':') else {
        return false;
    };
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// The crate-qualified label a struct is recorded under: the directory under
/// `crates/`, then the type name.
fn label(path: &str, name: &str) -> Option<String> {
    let krate = path.strip_prefix(SOURCE)?.split('/').next()?;
    Some(format!("{krate}::{name}"))
}

/// Every struct of one file, labelled, or why the file could not be read.
fn file_widths(root: &Path, path: &str) -> Result<Vec<(String, usize)>, String> {
    let source = fs::read_to_string(root.join(path))
        .map_err(|e| format!("  {path}: could not be read: {e}"))?;
    Ok(widths(&size::production_source(&source))
        .into_iter()
        .filter_map(|(name, fields)| Some((label(path, &name)?, fields)))
        .collect())
}

/// Every struct in the workspace by label, sorted, each label once at the
/// widest struct carrying it. An error when anything under `crates/` could
/// not be measured, never a shorter list: a struct the scan missed is a
/// struct that could grow unseen.
pub fn counts(root: &Path) -> Result<Vec<(String, usize)>, Unmeasured> {
    let mut missed = Vec::new();
    let mut all: Vec<(String, usize)> = Vec::new();
    for (path, _) in size::measure_under(root, SOURCE)? {
        match file_widths(root, &path) {
            Ok(found) => all.extend(found),
            Err(problem) => missed.push(problem),
        }
    }
    if !missed.is_empty() {
        return Err(Unmeasured { missed });
    }
    all.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    all.dedup_by(|later, first| later.0 == first.0);
    Ok(all)
}

/// The fields of every struct at or over [`WIDE_STRUCT_FIELDS`], summed.
pub fn measured(root: &Path) -> Result<usize, Unmeasured> {
    Ok(counts(root)?
        .iter()
        .map(|(_, fields)| fields)
        .filter(|fields| **fields >= WIDE_STRUCT_FIELDS)
        .sum())
}

/// Every way the tree and the recorded counts disagree: each struct against
/// its entry, a wide struct with none, the budget, and stale entries.
pub fn check(root: &Path) -> Vec<Finding> {
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    let counts = match counts(root) {
        Ok(counts) => counts,
        Err(unmeasured) => {
            return vec![Finding::new(format!("  {unmeasured}"), UNMEASURED_REMEDY)];
        }
    };
    let seen = |label: &str| counts.iter().any(|(known, _)| known == label);
    POLICY.against(&recorded, &counts, 0, &seen)
}

/// The edit-time question. A source file is answered from its own structs
/// alone — a widened struct is caught in the file that widened it — and an
/// edit to the baseline re-runs the whole check, stale entries and budget
/// included.
pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if relative == BASELINE_FILE {
        return check(root);
    }
    if !size::tracked(relative) {
        return Vec::new();
    }
    let recorded = match POLICY.baseline(root) {
        Ok(recorded) => recorded,
        Err(problem) => return vec![POLICY.unparsed(&problem)],
    };
    match file_widths(root, relative) {
        Ok(found) => found
            .iter()
            .filter_map(|(label, fields)| POLICY.verdict(recorded.entry(label), label, *fields))
            .collect(),
        // A file that does not decode is the encoding guard's finding, and
        // the whole-tree check refuses to pass over it.
        Err(_) => Vec::new(),
    }
}

/// Lower every entry whose struct lost a field, and the budget with them.
/// Refused when the tree could not be scanned whole.
pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    let counts = counts(root).map_err(|unmeasured| format!("struct-width: {unmeasured}"))?;
    POLICY.tighten(root, &counts, 0)
}

#[cfg(test)]
mod tests;
