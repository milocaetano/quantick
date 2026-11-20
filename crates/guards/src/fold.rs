//! Consolidate signed fragments without changing their effective ceilings.

use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::Path;

use crate::GUARDS;
use crate::ratchet::{FOLD_STAGE, Policy};

/// Validate all baselines before the first write, then consume their fragments.
/// Call after tightening; no value is inferred or raised here.
pub fn run(root: &Path) -> Result<(), String> {
    run_after(root, || true).map(|_| ())
}

/// Hold the recovery lock across the caller's tightening and the fold itself.
pub fn run_after(root: &Path, tighten: impl FnOnce() -> bool) -> Result<bool, String> {
    let _lock = lock(root)?;
    recover_locked(root)?;
    if !tighten() {
        return Ok(false);
    }
    stage(root)?;
    recover_locked(root)?;
    Ok(true)
}

fn stage(root: &Path) -> Result<(), String> {
    let mut pending = Vec::new();
    for guard in GUARDS {
        let Some(ratchet) = &guard.ratchet else {
            continue;
        };
        pending.push(prepare(root, ratchet.policy)?);
    }
    let stage = root.join(FOLD_STAGE).join("data");
    fs::create_dir(&stage).map_err(|error| error.to_string())?;
    for (baseline, text, fragments) in &pending {
        let path = stage.join(baseline);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, text).map_err(|error| format!("{baseline}: {error}"))?;
        for fragment in fragments {
            let path = stage.join(fragment);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            fs::write(path, []).map_err(|error| format!("{fragment}: {error}"))?;
        }
    }
    mark_pending(root, &checksum(&pending))
}

fn mark_pending(root: &Path, signature: &str) -> Result<(), String> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(FOLD_STAGE).join("pending"))
        .and_then(|mut file| file.write_all(signature.as_bytes()))
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Finish a staged fold before measuring or tightening its partly consumed inputs.
/// The pending marker denies every baseline read until all policies are consistent.
pub fn recover(root: &Path) -> Result<(), String> {
    let _lock = lock(root)?;
    recover_locked(root)
}

fn lock(root: &Path) -> Result<fs::File, String> {
    let stage = root.join(FOLD_STAGE);
    fs::create_dir_all(&stage).map_err(|error| error.to_string())?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(stage.join("lock"))
        .map_err(|error| error.to_string())?;
    file.try_lock()
        .map_err(|error| format!("another ratchet fold holds the recovery lock: {error}"))?;
    Ok(file)
}

fn recover_locked(root: &Path) -> Result<(), String> {
    let stage = root.join(FOLD_STAGE).join("data");
    let marker = root.join(FOLD_STAGE).join("pending");
    if marker.try_exists().map_err(|error| error.to_string())? {
        let pending = GUARDS
            .iter()
            .filter_map(|guard| guard.ratchet.as_ref())
            .map(|ratchet| prepare(&stage, ratchet.policy))
            .collect::<Result<Vec<_>, _>>()?;
        if fs::read_to_string(&marker).map_err(|error| error.to_string())? != checksum(&pending) {
            return Err(
                "ratchet fold recovery data is incomplete or corrupt; preserve it for repair"
                    .to_owned(),
            );
        }
        for (baseline, text, fragments) in pending {
            for fragment in fragments {
                match fs::remove_file(root.join(&fragment)) {
                    Ok(()) => (),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => return Err(format!("{fragment}: {error}")),
                }
            }
            fs::write(root.join(&baseline), text)
                .map_err(|error| format!("{baseline}: {error}"))?;
        }
        fs::remove_file(marker).map_err(|error| error.to_string())?;
    }
    if stage.try_exists().map_err(|error| error.to_string())? {
        fs::remove_dir_all(stage).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn checksum(pending: &[(String, String, Vec<String>)]) -> String {
    let mut hash = DefaultHasher::new();
    pending.hash(&mut hash);
    format!("{:016x}", hash.finish())
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

    fn mixed_tree() -> ScratchDir {
        let root = ScratchDir::new("fold-recovery");
        fs::create_dir_all(root.join("crates/guards")).unwrap();
        for policy in GUARDS
            .iter()
            .filter_map(|guard| guard.ratchet.as_ref())
            .map(|r| r.policy)
        {
            fs::write(
                root.join(policy.baseline_file),
                "!budget 100\nfixture 100\n",
            )
            .unwrap();
            fs::create_dir_all(root.join(policy.raises_dir())).unwrap();
            fs::write(
                root.join(policy.raises_dir()).join("a-positive.txt"),
                "!budget +7\nfixture +7\n",
            )
            .unwrap();
            fs::write(
                root.join(policy.raises_dir()).join("b-negative.txt"),
                "!budget -3\nfixture -3\n",
            )
            .unwrap();
        }
        // A staging failure may leave this directory; it never authorizes writes.
        fs::create_dir_all(root.join(FOLD_STAGE)).unwrap();
        root
    }

    fn assert_effective(root: &Path, pending: bool) {
        for ratchet in GUARDS.iter().filter_map(|guard| guard.ratchet.as_ref()) {
            let result = ratchet.policy.baseline(root);
            if pending {
                assert!(result.unwrap_err().contains("fold is pending"));
            } else {
                let baseline = result.unwrap();
                assert_eq!(baseline.entry("fixture").unwrap().ceiling, 104);
                assert_eq!(baseline.budget.unwrap().allowed, 104);
            }
        }
    }

    #[test]
    fn interrupted_preparation_and_competing_folds_leave_inputs_intact() {
        let root = mixed_tree();
        let held = lock(root.path()).unwrap();
        assert!(run(root.path()).unwrap_err().contains("recovery lock"));
        assert!(recover(root.path()).unwrap_err().contains("recovery lock"));
        assert_effective(root.path(), false);
        drop(held);
        let data = root.join(FOLD_STAGE).join("data");
        fs::create_dir(&data).unwrap();
        fs::write(data.join("partial"), "interrupted before pending marker").unwrap();
        run(root.path()).unwrap();
        assert_effective(root.path(), false);
        recover(root.path()).unwrap();
    }

    #[test]
    fn failed_baseline_write_remains_closed_and_retries_exactly_once() {
        let root = mixed_tree();
        stage(root.path()).unwrap();
        let baseline = root.join(crate::app_lines::BASELINE_FILE);
        fs::remove_file(&baseline).unwrap();
        fs::create_dir(&baseline).unwrap();
        assert!(recover(root.path()).is_err());
        assert_effective(root.path(), true);
        fs::remove_dir(&baseline).unwrap();
        recover(root.path()).unwrap();
        recover(root.path()).unwrap();
        run(root.path()).unwrap();
        assert_effective(root.path(), false);
    }

    #[test]
    fn partial_fragment_consumption_is_recoverable_for_mixed_signs() {
        let root = mixed_tree();
        stage(root.path()).unwrap();
        let fragment = root
            .join(crate::app_lines::POLICY.raises_dir())
            .join("b-negative.txt");
        fs::remove_file(&fragment).unwrap();
        fs::create_dir(&fragment).unwrap();
        assert!(recover(root.path()).is_err());
        assert!(!fragment.with_file_name("a-positive.txt").exists());
        assert_effective(root.path(), true);
        fs::remove_dir(&fragment).unwrap();
        recover(root.path()).unwrap();
        run(root.path()).unwrap();
        assert_effective(root.path(), false);
    }

    #[test]
    fn incomplete_or_corrupt_recovery_never_mutates_original_policies() {
        for missing in [false, true] {
            let root = mixed_tree();
            stage(root.path()).unwrap();
            let saved = root
                .join(FOLD_STAGE)
                .join("data")
                .join(crate::app_lines::BASELINE_FILE);
            let original = fs::read(&saved).unwrap();
            if missing {
                fs::remove_file(&saved).unwrap();
            } else {
                fs::write(&saved, "corrupt").unwrap();
            }
            assert!(recover(root.path()).is_err());
            assert_effective(root.path(), true);
            assert_eq!(
                fs::read_to_string(root.join(crate::app_lines::BASELINE_FILE)).unwrap(),
                "!budget 100\nfixture 100\n"
            );
            fs::write(saved, original).unwrap();
            recover(root.path()).unwrap();
            assert_effective(root.path(), false);
        }
    }

    #[test]
    fn recovery_manifest_detects_missing_fragments_and_valid_but_changed_data() {
        for corruption in [
            "missing fragment",
            "changed fragment",
            "changed baseline",
            "truncated marker",
        ] {
            let root = mixed_tree();
            stage(root.path()).unwrap();
            let data = root.join(FOLD_STAGE).join("data");
            let path = match corruption {
                "missing fragment" | "changed fragment" => data
                    .join(crate::app_lines::POLICY.raises_dir())
                    .join("a-positive.txt"),
                "changed baseline" => data.join(crate::app_lines::BASELINE_FILE),
                _ => root.join(FOLD_STAGE).join("pending"),
            };
            let original = fs::read(&path).unwrap();
            match corruption {
                "missing fragment" => fs::remove_file(&path).unwrap(),
                "changed baseline" => fs::write(&path, "!budget 105\nfixture 105\n").unwrap(),
                "changed fragment" => fs::write(&path, "!budget +1\nfixture +1\n").unwrap(),
                _ => fs::write(&path, "partial").unwrap(),
            }
            assert!(recover(root.path()).unwrap_err().contains("corrupt"));
            assert_effective(root.path(), true);
            assert_eq!(
                fs::read_to_string(root.join(crate::app_lines::BASELINE_FILE)).unwrap(),
                "!budget 100\nfixture 100\n"
            );
            fs::write(path, original).unwrap();
            recover(root.path()).unwrap();
            assert_effective(root.path(), false);
        }
    }

    #[test]
    fn tightening_is_called_after_recovery_while_the_fold_lock_is_held() {
        let root = mixed_tree();
        stage(root.path()).unwrap();
        let completed = run_after(root.path(), || {
            assert_effective(root.path(), false);
            assert!(run(root.path()).unwrap_err().contains("recovery lock"));
            false
        })
        .unwrap();
        assert!(!completed);
        assert_effective(root.path(), false);
    }

    #[test]
    fn pending_marker_creation_failure_does_not_consume_inputs() {
        let root = mixed_tree();
        stage(root.path()).unwrap();
        let marker = root.join(FOLD_STAGE).join("pending");
        let signature = fs::read_to_string(&marker).unwrap();
        fs::remove_file(&marker).unwrap();
        fs::create_dir(&marker).unwrap();
        assert!(mark_pending(root.path(), &signature).is_err());
        assert_effective(root.path(), true);
        assert_eq!(
            fs::read_to_string(root.join(crate::app_lines::BASELINE_FILE)).unwrap(),
            "!budget 100\nfixture 100\n"
        );
        fs::remove_dir(&marker).unwrap();
        mark_pending(root.path(), &signature).unwrap();
        run(root.path()).unwrap();
        assert_effective(root.path(), false);
    }

    #[cfg(windows)]
    #[test]
    fn windows_read_locks_cannot_weaken_a_fold_or_its_retry() {
        use std::os::windows::fs::OpenOptionsExt;
        for locked in ["fragment", "baseline", "marker", "cleanup"] {
            let root = mixed_tree();
            stage(root.path()).unwrap();
            let path = match locked {
                "fragment" => root
                    .join(crate::app_lines::POLICY.raises_dir())
                    .join("b-negative.txt"),
                "baseline" => root.join(crate::app_lines::BASELINE_FILE),
                "marker" => root.join(FOLD_STAGE).join("pending"),
                _ => root
                    .join(FOLD_STAGE)
                    .join("data")
                    .join(crate::app_lines::BASELINE_FILE),
            };
            let held = fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(path)
                .unwrap();
            assert!(recover(root.path()).is_err(), "{locked}");
            assert_effective(root.path(), locked != "cleanup");
            drop(held);
            recover(root.path()).unwrap();
            run(root.path()).unwrap();
            assert_effective(root.path(), false);
        }
    }

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
