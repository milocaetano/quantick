//! Merge-base comparisons for aggregate ratchets. All Git work belongs to the caller.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use crate::ratchet::{Baseline, Policy};
use crate::{
    Finding, Guard, app_lines, extension_boundary, impl_spread, struct_width, ui_free, window_reach,
};

const REMEDY: &str = "Compare with the checked-out merge base and add this branch's signed \
    growth to a new baseline fragment. Existing fragments and base data must remain intact; \
    use --tighten --fold for a downward-only consolidation.";

/// Only these six aggregate guards replace cleanup findings with a delta check.
pub fn selected(name: &str) -> bool {
    matches!(
        name,
        "app-lines"
            | "app-ui-free"
            | "window-reach"
            | "extension-boundary"
            | "struct-width"
            | "impl-spread"
    )
}

fn counts(root: &Path, name: &str) -> Result<BTreeMap<String, usize>, String> {
    let scalar = |label: &str, value: Result<usize, crate::ratchet::Unmeasured>| {
        value
            .map(|value| BTreeMap::from([(label.to_owned(), value)]))
            .map_err(|error| error.to_string())
    };
    match name {
        "app-lines" => scalar(app_lines::ENTRY, app_lines::measured(root)),
        "app-ui-free" => scalar(ui_free::ENTRY, ui_free::measured(root)),
        "window-reach" => scalar(window_reach::LABEL, window_reach::measured(root)),
        "impl-spread" => scalar(impl_spread::LABEL, impl_spread::measured(root)),
        "extension-boundary" => Ok(extension_boundary::inventory(root)?
            .counts
            .into_iter()
            .collect()),
        "struct-width" => struct_width::counts(root)
            .map(|counts| counts.into_iter().collect())
            .map_err(|error| error.to_string()),
        _ => Err(format!("{name}: not an aggregate delta ratchet")),
    }
}

/// Existing absolute and structural checks remain authoritative. Cleanup alone
/// does not make an otherwise valid shrinking aggregate branch fail.
pub fn check(root: &Path, base: &Path, guard: &Guard) -> Vec<Finding> {
    let mut findings = (guard.check)(root);
    if !selected(guard.name) {
        return findings;
    }
    findings.retain(|finding| !finding.cleanup);
    if let Err(error) = compare(root, base, guard) {
        findings.push(Finding::new(error, REMEDY));
    }
    findings
}

fn compare(root: &Path, base: &Path, guard: &Guard) -> Result<(), String> {
    let policy = guard
        .ratchet
        .as_ref()
        .ok_or("delta guard has no ratchet")?
        .policy;
    let before = policy.baseline(base)?;
    let after = policy.baseline(root)?;
    let old_counts = counts(base, guard.name)?;
    let new_counts = counts(root, guard.name)?;
    let folded = validate_fragments(root, base, policy, &before, &after)?;
    for entry in &after.entries {
        if before.entry(&entry.path).is_none()
            && !old_counts.contains_key(&entry.path)
            && !new_counts.contains_key(&entry.path)
        {
            return Err(format!(
                "{}: fragment names an unmeasured entry {}",
                guard.name, entry.path
            ));
        }
    }
    let keys: BTreeSet<_> = old_counts.keys().chain(new_counts.keys()).collect();
    let mut problems = Vec::new();
    for key in keys {
        let old = old_counts.get(key).copied().unwrap_or(0);
        let new = new_counts.get(key).copied().unwrap_or(0);
        // Small structs remain outside the existing width policy.
        if old <= policy.threshold && new <= policy.threshold {
            continue;
        }
        let growth = new as i128 - old as i128;
        let permission = if folded {
            0
        } else {
            ceiling(&after, key) - ceiling(&before, key)
        };
        if growth > permission {
            problems.push(format!(
                "{}: {key} grew by {growth}, this branch signed {permission}",
                guard.name
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

fn ceiling(baseline: &Baseline, key: &str) -> i128 {
    baseline.entry(key).map_or(0, |entry| entry.ceiling as i128)
}

fn read(root: &Path, relative: &str) -> Result<Vec<u8>, String> {
    fs::read(root.join(relative)).map_err(|error| format!("{relative}: {error}"))
}

fn validate_fragments(
    root: &Path,
    base: &Path,
    policy: &Policy,
    before: &Baseline,
    after: &Baseline,
) -> Result<bool, String> {
    let old_budget = before.budget.as_ref().ok_or("base has no !budget")?.allowed;
    let new_budget = after.budget.as_ref().ok_or("head has no !budget")?.allowed;
    let baseline_changed = read(root, policy.baseline_file)? != read(base, policy.baseline_file)?;
    // A fold consumes all fragments into one baseline, preserving or lowering
    // every effective ceiling. It cannot authorize any growth.
    let fold = baseline_changed
        && after.raise_files.is_empty()
        && new_budget <= old_budget
        && after
            .entries
            .iter()
            .all(|entry| entry.ceiling as i128 <= ceiling(before, &entry.path));
    if fold {
        return Ok(true);
    }
    if baseline_changed {
        return Err(format!(
            "{}: baseline changed outside a downward fold",
            policy.baseline_file
        ));
    }
    for file in &before.raise_files {
        if read(root, file)? != read(base, file)? {
            return Err(format!("{file}: existing fragment was changed"));
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
