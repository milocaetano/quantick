//! Consolidate signed fragments without changing their effective ceilings.

use std::fs;
use std::path::Path;

use crate::GUARDS;
use crate::ratchet::Policy;

/// Validate all baselines before the first write, then consume their fragments.
/// Call after tightening; no value is inferred or raised here.
pub fn run(root: &Path) -> Result<(), String> {
    let mut pending = Vec::new();
    for guard in GUARDS {
        let Some(ratchet) = &guard.ratchet else {
            continue;
        };
        pending.push(prepare(root, ratchet.policy)?);
    }
    for (baseline, text, fragments) in pending {
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
    let original =
        fs::read_to_string(root.join(policy.baseline_file)).map_err(|error| error.to_string())?;
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

    #[test]
    fn fold_preserves_effective_ceilings() {
        let root = ScratchDir::new("fold");
        fs::create_dir_all(root.join("crates/guards")).unwrap();
        for guard in GUARDS {
            let Some(ratchet) = &guard.ratchet else {
                continue;
            };
            let policy = ratchet.policy;
            fs::write(
                root.join(policy.baseline_file),
                "# Signed cap\n!budget 100\nfixture 100\n",
            )
            .unwrap();
            fs::create_dir_all(root.join(policy.raises_dir())).unwrap();
            for index in 0..20 {
                fs::write(
                    root.join(policy.raises_dir())
                        .join(format!("branch-{index}.txt")),
                    "fixture -1\n!budget -1\n",
                )
                .unwrap();
            }
        }
        run(root.path()).unwrap();
        for guard in GUARDS {
            let Some(ratchet) = &guard.ratchet else {
                continue;
            };
            let baseline = ratchet.policy.baseline(root.path()).unwrap();
            assert_eq!(baseline.entry("fixture").unwrap().ceiling, 80);
            assert_eq!(baseline.budget.unwrap().allowed, 80);
            assert!(baseline.raise_files.len() <= 15);
        }
    }
}
