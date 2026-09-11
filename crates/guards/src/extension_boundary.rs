//! Exact canonical root/port shapes and per-root implementation line caps.
//!
//! This protects explicit source authority, not arbitrary semantic coupling.
//! See `docs/quality/native-indicator-boundary-evidence.md` for the grammar,
//! origin contributions and macro/alias/free-helper limits.

mod lex;
mod scan;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::Finding;
use crate::ratchet::Policy;

pub const SHAPES_FILE: &str = "crates/guards/extension-shapes-baseline.txt";
pub const BUDGET_FILE: &str = "crates/guards/extension-roots-baseline.txt";
const SOURCE: &str = "crates/app/src";
const REMEDY: &str = "Keep focused operation code behind its existing port. Restore the protected \
    shape or explicitly review a contract amendment. Root caps are separate; do not transfer \
    growth between roots. Unsupported scope must be diagnosed, not exempted.";

pub const POLICY: Policy = Policy {
    baseline_file: BUDGET_FILE,
    threshold: 0,
    slack: 0,
    budget_slack: 0,
    budget_headroom: 0,
    unit: "root production lines",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: REMEDY,
    baseline_remedy: REMEDY,
};

/// Source evidence, in deterministic path/target order. Lines are one-based.
#[derive(Default, Debug)]
pub struct Inventory {
    pub shapes: BTreeMap<String, (String, String)>,
    pub counts: Vec<(String, usize)>,
    pub contributions: Vec<(String, String, usize, usize, usize)>,
}

/// Measure the current source independently of baseline values.
pub fn inventory(root: &Path) -> Result<Inventory, String> {
    let mut paths = Vec::new();
    walk(root, SOURCE, &mut paths)?;
    paths.sort();
    let mut result = Inventory::default();
    let mut totals: BTreeMap<String, usize> =
        scan::ROOTS.iter().map(|name| ((*name).into(), 0)).collect();
    for relative in paths {
        let source = fs::read_to_string(root.join(&relative))
            .map_err(|error| format!("{relative}: unreadable source: {error}"))?;
        let file = scan::scan(&source).map_err(|error| format!("{relative}: {error}"))?;
        for (name, shape) in file.shapes {
            if let Some((first, _)) = result
                .shapes
                .insert(name.clone(), (relative.clone(), shape))
            {
                return Err(format!(
                    "{relative}: duplicate target {name}, first in {first}"
                ));
            }
        }
        for (name, lines) in &file.lines {
            *totals.get_mut(name).unwrap() += lines.len();
        }
        // A contribution owns only lines not already credited to an enclosing
        // protected item in this file; the sum is the union, never double-counted.
        let mut credited: BTreeMap<String, std::collections::BTreeSet<usize>> = BTreeMap::new();
        let flags = crate::size::production_flags(&source);
        for (name, start, end) in file.spans {
            let seen = credited.entry(name.clone()).or_default();
            let count = (start - 1..end)
                .filter(|line| flags[*line] && seen.insert(*line))
                .count();
            result
                .contributions
                .push((name, relative.clone(), start, end, count));
        }
    }
    for name in scan::TARGETS {
        if !result.shapes.contains_key(name) {
            return Err(format!("{SOURCE}: missing target {name}"));
        }
    }
    result.counts = totals.into_iter().collect();
    Ok(result)
}

fn walk(root: &Path, relative: &str, paths: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(root.join(relative))
        .map_err(|error| format!("{relative}: unreadable source directory: {error}"))?;
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("{relative}: unreadable directory entry: {error}"))?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| format!("{relative}: non-UTF-8 source path"))?
            .to_owned();
        let path = format!("{relative}/{name}");
        let kind = entry
            .file_type()
            .map_err(|error| format!("{path}: unreadable file type: {error}"))?;
        if kind.is_symlink() {
            return Err(format!("{path}: unsupported source symlink"));
        }
        if kind.is_dir() && name != "tests" && name != "target" {
            walk(root, &path, paths)?;
        } else if kind.is_file() && name.ends_with(".rs") {
            paths.push(path);
        }
    }
    Ok(())
}

fn shapes(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = fs::read_to_string(root.join(SHAPES_FILE))
        .map_err(|error| format!("{SHAPES_FILE}: unreadable baseline: {error}"))?;
    let mut result = BTreeMap::new();
    for (line, content) in text.lines().enumerate() {
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let (name, shape) = content.split_once('\t').ok_or_else(|| {
            format!(
                "{SHAPES_FILE}:{}: expected target TAB normalized shape",
                line + 1
            )
        })?;
        if !scan::TARGETS.contains(&name)
            || shape.is_empty()
            || shape.contains('\t')
            || result.insert(name.into(), shape.into()).is_some()
        {
            return Err(format!(
                "{SHAPES_FILE}:{}: corrupt/duplicate target {name}",
                line + 1
            ));
        }
    }
    for target in scan::TARGETS {
        if !result.contains_key(target) {
            return Err(format!("{SHAPES_FILE}: missing target {target}"));
        }
    }
    Ok(result)
}

fn baseline(root: &Path) -> Result<crate::ratchet::Baseline, String> {
    let baseline = POLICY.baseline(root)?;
    if baseline.entries.len() != scan::ROOTS.len()
        || scan::ROOTS
            .iter()
            .any(|name| baseline.entry(name).is_none())
        || baseline.budget.is_none()
    {
        return Err(format!(
            "{BUDGET_FILE}: exactly two root entries and one budget are required"
        ));
    }
    Ok(baseline)
}

pub fn check(root: &Path) -> Vec<Finding> {
    let (actual, expected, baseline) = match (inventory(root), shapes(root), baseline(root)) {
        (Ok(actual), Ok(expected), Ok(baseline)) => (actual, expected, baseline),
        (actual, expected, baseline) => {
            return [actual.err(), expected.err(), baseline.err()]
                .into_iter()
                .flatten()
                .map(|error| Finding::new(error, REMEDY))
                .collect();
        }
    };
    let mut findings = Vec::new();
    for (name, (path, shape)) in &actual.shapes {
        if expected.get(name) != Some(shape) {
            findings.push(Finding::new(
                format!(
                    "{path}: {name} shape differs\n  expected: {}\n  actual: {shape}",
                    expected[name]
                ),
                REMEDY,
            ));
        }
    }
    for (name, count) in &actual.counts {
        if let Some(mut finding) = POLICY.verdict(baseline.entry(name), name, *count) {
            for (_, path, start, end, contribution) in actual
                .contributions
                .iter()
                .filter(|(target, ..)| target == name)
            {
                finding
                    .line
                    .push_str(&format!("\n    {path}:{start}-{end}: {contribution}"));
            }
            findings.push(finding);
        }
    }
    findings.extend(POLICY.budget_verdict(&baseline, 0));
    findings
}

pub fn check_file(root: &Path, relative: &str) -> Vec<Finding> {
    if relative.starts_with(&format!("{SOURCE}/")) || [SHAPES_FILE, BUDGET_FILE].contains(&relative)
    {
        check(root)
    } else {
        Vec::new()
    }
}

pub fn measured(root: &Path) -> usize {
    inventory(root)
        .map(|inventory| inventory.counts.iter().map(|(_, count)| count).sum())
        .unwrap_or(0)
}

pub fn tighten(root: &Path) -> Result<Vec<String>, String> {
    baseline(root)?;
    let inventory = inventory(root)?;
    POLICY.tighten(root, &inventory.counts, 0)
}

#[cfg(test)]
mod tests;
