//! UI-free code in the UI crate, rationed. Skeleton: the tests come first.

use std::path::Path;

use crate::Finding;
use crate::ratchet::{Policy, Unmeasured};

pub const BASELINE_FILE: &str = "crates/guards/ui-free-baseline.txt";
pub const EXEMPTIONS_FILE: &str = "crates/guards/ui-free-exemptions.txt";
pub const ENTRY: &str = "crates/app";
pub const SLACK: usize = 200;
pub const REMEDY: &str = "";
pub const EXEMPTION_REMEDY: &str = "";

pub const POLICY: Policy = Policy {
    baseline_file: BASELINE_FILE,
    threshold: 0,
    slack: SLACK,
    budget_slack: SLACK,
    budget_headroom: 0,
    unit: "UI-free production lines",
    remedy: REMEDY,
    budget_remedy: REMEDY,
    budget_slack_remedy: REMEDY,
    baseline_remedy: REMEDY,
};

pub fn names_ui(_production: &[&str]) -> bool {
    false
}

pub fn measured(_root: &Path) -> Result<usize, Unmeasured> {
    Ok(0)
}

pub fn check(_root: &Path) -> Vec<Finding> {
    Vec::new()
}

pub fn check_file(_root: &Path, _relative: &str) -> Vec<Finding> {
    Vec::new()
}

pub fn tighten(_root: &Path) -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests;
