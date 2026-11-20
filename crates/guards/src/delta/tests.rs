use super::*;
use crate::scratch_dir::ScratchDir;

fn tree(lines: usize) -> ScratchDir {
    let root = ScratchDir::new("delta");
    fs::create_dir_all(root.join("crates/app/src")).unwrap();
    fs::create_dir_all(root.join("crates/guards")).unwrap();
    fs::write(
        root.join("crates/app/src/lib.rs"),
        "fn line() {}\n".repeat(lines),
    )
    .unwrap();
    fs::write(
        root.join(app_lines::BASELINE_FILE),
        "!budget 1000\ncrates/app 1000\n",
    )
    .unwrap();
    root
}

fn guard() -> &'static Guard {
    crate::GUARDS
        .iter()
        .find(|guard| guard.name == "app-lines")
        .unwrap()
}

fn fragment(root: &Path, name: &str, content: &str) {
    let dir = root.join(app_lines::POLICY.raises_dir());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(name), content).unwrap();
}

#[test]
fn delta_branches_require_own_raises() {
    let base = tree(500);
    let first = tree(501);
    let second = tree(502);
    let shrink = tree(10);
    assert!(!check(first.path(), base.path(), guard()).is_empty());
    assert!(!check(second.path(), base.path(), guard()).is_empty());
    assert!(check(shrink.path(), base.path(), guard()).is_empty());
    fragment(
        first.path(),
        "first.txt",
        "# Explicit growth\ncrates/app +1\n!budget +1\n",
    );
    assert!(check(first.path(), base.path(), guard()).is_empty());
    assert!(!check(second.path(), base.path(), guard()).is_empty());
    // Main compares the merged shrink against its first parent, so no fold
    // commit is needed before this merge can pass the same guard.
    assert!(!check(shrink.path(), first.path(), guard()).is_empty());
    fragment(
        shrink.path(),
        "first.txt",
        "# Explicit growth\ncrates/app +1\n!budget +1\n",
    );
    assert!(check(shrink.path(), first.path(), guard()).is_empty());
}

#[test]
fn missing_corrupt_and_rewritten_bases_fail_closed() {
    let base = tree(10);
    let head = tree(10);
    fragment(base.path(), "existing.txt", "crates/app +1\n!budget +1\n");
    assert!(!check(head.path(), base.path(), guard()).is_empty());
    fragment(head.path(), "existing.txt", "crates/app +2\n!budget +2\n");
    assert!(!check(head.path(), base.path(), guard()).is_empty());
    fs::write(base.join(app_lines::BASELINE_FILE), "corrupt").unwrap();
    assert!(!check(head.path(), base.path(), guard()).is_empty());
    fs::remove_file(base.join(app_lines::BASELINE_FILE)).unwrap();
    assert!(!check(head.path(), base.path(), guard()).is_empty());
}

#[test]
fn changed_baseline_cannot_forge_a_raise() {
    let base = tree(500);
    let head = tree(501);
    fs::write(
        head.join(app_lines::BASELINE_FILE),
        "!budget 1001\ncrates/app 1001\n",
    )
    .unwrap();
    assert!(!check(head.path(), base.path(), guard()).is_empty());
}

#[test]
fn forged_entry_and_overstated_cuts_fail() {
    let base = tree(500);
    let head = tree(499);
    fragment(head.path(), "branch.txt", "crates/app -2\n!budget -2\n");
    assert!(!check(head.path(), base.path(), guard()).is_empty());
    fragment(head.path(), "branch.txt", "forged +2\n!budget +2\n");
    assert!(!check(head.path(), base.path(), guard()).is_empty());
}

#[test]
fn fold_never_authorizes_new_growth() {
    let base = tree(500);
    let head = tree(501);
    fragment(base.path(), "old.txt", "crates/app +10\n!budget +10\n");
    fs::write(
        head.join(app_lines::BASELINE_FILE),
        "!budget 1010\ncrates/app 1010\n",
    )
    .unwrap();
    assert!(!check(head.path(), base.path(), guard()).is_empty());
    fs::write(
        head.join("crates/app/src/lib.rs"),
        "fn line() {}\n".repeat(500),
    )
    .unwrap();
    assert!(check(head.path(), base.path(), guard()).is_empty());
}
