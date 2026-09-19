//! Signed exemption lists: one `<key> <reason>` per line.
//!
//! Several guards let a reviewed line in a data file stand a rule down for one
//! file or one crate, and every one of them has to refuse the same three
//! mistakes the same way: an exemption with no reason, a key the guard cannot
//! exempt, and a key listed twice. One reader, so a typo can never widen one
//! list while it would be refused in another.

use std::fs;
use std::path::Path;

use crate::Finding;

/// One signed exemption.
pub struct Exemption {
    /// What is exempt: a workspace-relative path, a crate name.
    pub key: String,
    /// One-based line in the exemption file.
    pub line: usize,
}

/// Read `file` under `root`. Blank lines and `#` comments are skipped; every
/// other line is `<key> <reason>`. A malformed line is a finding carrying
/// `remedy` and no exemption. `invalid` names what is wrong with a key this
/// guard cannot exempt, or `None` for one it can.
///
/// An error only when the file itself cannot be read.
pub fn read(
    root: &Path,
    file: &str,
    remedy: &'static str,
    invalid: &dyn Fn(&str) -> Option<String>,
) -> Result<(Vec<Exemption>, Vec<Finding>), String> {
    let text =
        fs::read_to_string(root.join(file)).map_err(|e| format!("  {file} is unreadable: {e}"))?;
    let mut found: Vec<Exemption> = Vec::new();
    let mut findings = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.trim();
        if content.is_empty() || content.starts_with('#') {
            continue;
        }
        let (key, reason) = content
            .split_once(char::is_whitespace)
            .unwrap_or((content, ""));
        let problem = if reason.trim().is_empty() {
            Some(format!("exempts {key} with no reason"))
        } else if let Some(problem) = invalid(key) {
            Some(problem)
        } else {
            found
                .iter()
                .find(|known| known.key == key)
                .map(|first| format!("{key} is already exempt on line {}", first.line))
        };
        match problem {
            Some(problem) => {
                findings.push(Finding::new(format!("  {file}:{line}: {problem}"), remedy))
            }
            None => found.push(Exemption {
                key: key.to_owned(),
                line,
            }),
        }
    }
    Ok((found, findings))
}
