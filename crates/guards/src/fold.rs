//! Consolidate signed fragments into their baselines without raising any ceiling.

use std::fs;
use std::path::Path;

use crate::GUARDS;
use crate::ratchet::Policy;

/// Validate every baseline before rewriting any, then delete consumed fragments.
/// This is a plain rewrite: after an I/O failure, restore affected files from Git
/// before retrying. It deliberately provides no crash recovery or concurrent writer.
pub fn run(root: &Path) -> Result<(), String> {
    let pending = GUARDS
        .iter()
        .filter_map(|guard| guard.ratchet.as_ref())
        .map(|ratchet| prepare(root, ratchet.policy))
        .collect::<Result<Vec<_>, _>>()?;
    for (baseline, text, fragments) in pending {
        if fragments.is_empty() {
            continue;
        }
        fs::write(root.join(&baseline), text).map_err(|error| format!("{baseline}: {error}"))?;
        for fragment in fragments {
            fs::remove_file(root.join(&fragment))
                .map_err(|error| format!("{fragment}: {error}"))?;
        }
    }
    Ok(())
}

fn prepare(root: &Path, policy: &Policy) -> Result<(String, String, Vec<String>), String> {
    let mut baseline = policy.baseline(root)?;
    let budget = baseline
        .budget
        .as_ref()
        .ok_or_else(|| format!("{}: missing !budget", policy.baseline_file))?;
    let original = fs::read_to_string(root.join(policy.baseline_file))
        .map_err(|error| format!("{}: {error}", policy.baseline_file))?;
    let mut text = String::new();
    for line in original
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
    {
        text.push_str(&format!("{line}\n"));
    }
    text.push_str(&format!("!budget {}\n", budget.allowed));
    baseline.entries.sort_by(|a, b| a.path.cmp(&b.path));
    for entry in &baseline.entries {
        text.push_str(&format!("{} {}\n", entry.path, entry.ceiling));
    }
    Ok((policy.baseline_file.to_owned(), text, baseline.raise_files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch_dir::ScratchDir;

    fn tree() -> ScratchDir {
        let root = ScratchDir::new("fold");
        fs::create_dir_all(root.join("crates/guards")).unwrap();
        for policy in GUARDS
            .iter()
            .filter_map(|g| g.ratchet.as_ref())
            .map(|r| r.policy)
        {
            fs::write(
                root.join(policy.baseline_file),
                "# Signed cap\n!budget 100\nfixture 100\n",
            )
            .unwrap();
            fs::create_dir_all(root.join(policy.raises_dir())).unwrap();
            fs::write(
                root.join(policy.raises_dir()).join("a-positive.txt"),
                "fixture +10\nnew +2\n!budget +12\n",
            )
            .unwrap();
            fs::write(
                root.join(policy.raises_dir()).join("b-negative.txt"),
                "fixture -20\n!budget -20\n",
            )
            .unwrap();
        }
        root
    }

    #[test]
    fn fold_preserves_effective_ceilings_and_retries_are_inert() {
        let root = tree();
        run(root.path()).unwrap();
        for policy in GUARDS
            .iter()
            .filter_map(|g| g.ratchet.as_ref())
            .map(|r| r.policy)
        {
            let baseline = policy.baseline(root.path()).unwrap();
            assert_eq!(baseline.entry("fixture").unwrap().ceiling, 90);
            assert_eq!(baseline.entry("new").unwrap().ceiling, 2);
            assert_eq!(baseline.budget.unwrap().allowed, 92);
            assert!(baseline.raise_files.is_empty());
            let first = fs::read(root.join(policy.baseline_file)).unwrap();
            assert!(first.starts_with(b"# Signed cap\n"));
            run(root.path()).unwrap();
            assert_eq!(fs::read(root.join(policy.baseline_file)).unwrap(), first);
        }
    }

    #[test]
    fn invalid_policy_prevents_any_fold_writes() {
        let root = tree();
        fs::write(root.join(crate::cycle::BASELINE_FILE), "corrupt\n").unwrap();
        assert!(run(root.path()).is_err());
        assert_eq!(
            fs::read_to_string(root.join(crate::app_lines::BASELINE_FILE)).unwrap(),
            "# Signed cap\n!budget 100\nfixture 100\n"
        );
        assert_eq!(
            crate::app_lines::POLICY
                .baseline(root.path())
                .unwrap()
                .raise_files
                .len(),
            2
        );
    }
}
