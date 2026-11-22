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
    if base.as_os_str().is_empty() {
        return Err("base root must not be empty".to_owned());
    }
    let head_path = root
        .canonicalize()
        .map_err(|error| format!("head root: {error}"))?;
    let base_path = base
        .canonicalize()
        .map_err(|error| format!("base root: {error}"))?;
    if head_path == base_path {
        return Err("base root must be a separate checkout from the head".to_owned());
    }
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
        let permission = folded.as_ref().map_or_else(
            || ceiling(&after, key) - ceiling(&before, key),
            |raw| ceiling(&after, key) - raw.get(key).copied().unwrap_or(0),
        );
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

fn raw_entries(root: &Path, policy: &Policy) -> Result<(BTreeMap<String, i128>, i128), String> {
    let text = fs::read_to_string(root.join(policy.baseline_file))
        .map_err(|error| format!("{}: {error}", policy.baseline_file))?;
    let mut entries = BTreeMap::new();
    let mut budget = None;
    for raw in text.lines() {
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let (label, value) = content
            .rsplit_once(char::is_whitespace)
            .ok_or_else(|| format!("{}: invalid baseline row", policy.baseline_file))?;
        let value = value
            .parse::<i128>()
            .map_err(|error| format!("{}: {error}", policy.baseline_file))?;
        let label = label.trim();
        if label == "!budget" {
            budget = Some(value);
        } else {
            entries.insert(label.to_owned(), value);
        }
    }
    Ok((
        entries,
        budget.ok_or_else(|| format!("{}: no !budget", policy.baseline_file))?,
    ))
}

fn validate_fragments(
    root: &Path,
    base: &Path,
    policy: &Policy,
    before: &Baseline,
    after: &Baseline,
) -> Result<Option<BTreeMap<String, i128>>, String> {
    let old_budget = before.budget.as_ref().ok_or("base has no !budget")?.allowed;
    after.budget.as_ref().ok_or("head has no !budget")?;
    let baseline_changed = read(root, policy.baseline_file)? != read(base, policy.baseline_file)?;
    let all_old_consumed = before
        .raise_files
        .iter()
        .all(|file| !root.join(file).exists());
    let remaining_are_new = after
        .raise_files
        .iter()
        .all(|file| !base.join(file).exists());
    if !before.raise_files.is_empty() && all_old_consumed && remaining_are_new {
        let (raw, raw_budget) = raw_entries(root, policy)?;
        let old_keys: BTreeSet<_> = before
            .entries
            .iter()
            .map(|entry| entry.path.as_str())
            .collect();
        let raw_keys: BTreeSet<_> = raw.keys().map(String::as_str).collect();
        let downward = old_keys == raw_keys
            && raw_budget <= old_budget as i128
            && raw
                .iter()
                .all(|(key, value)| *value <= ceiling(before, key));
        let unchanged_fold = raw_budget == old_budget as i128
            && raw
                .iter()
                .all(|(key, value)| *value == ceiling(before, key));
        if downward && (baseline_changed || unchanged_fold) {
            return Ok(Some(raw));
        }
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
    Ok(None)
}

#[cfg(test)]
mod tests;
