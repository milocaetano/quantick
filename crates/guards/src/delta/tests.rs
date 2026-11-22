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

#[test]
fn fold_then_new_fragment_authorizes_only_its_signed_growth() {
    let base = tree(500);
    let head = tree(501);
    fragment(base.path(), "old.txt", "crates/app +10\n!budget +10\n");
    fs::write(
        head.join(app_lines::BASELINE_FILE),
        "!budget 1010\ncrates/app 1010\n",
    )
    .unwrap();
    fragment(head.path(), "new.txt", "crates/app +1\n!budget +1\n");
    assert!(check(head.path(), base.path(), guard()).is_empty());

    fs::write(
        head.join("crates/app/src/lib.rs"),
        "fn line() {}\n".repeat(502),
    )
    .unwrap();
    assert!(!check(head.path(), base.path(), guard()).is_empty());
}

#[test]
fn fold_rejects_upward_raw_entry_or_budget_even_with_new_cuts() {
    for baseline in [
        "!budget 1010\ncrates/app 1011\n",
        "!budget 1011\ncrates/app 1010\n",
    ] {
        let base = tree(500);
        let head = tree(500);
        fragment(base.path(), "old.txt", "crates/app +10\n!budget +10\n");
        fs::write(head.join(app_lines::BASELINE_FILE), baseline).unwrap();
        fragment(head.path(), "new.txt", "crates/app -1\n!budget -1\n");
        assert!(!check(head.path(), base.path(), guard()).is_empty());
    }
}

#[test]
fn fold_rejects_retained_modified_or_partially_consumed_old_fragments() {
    for old_in_head in [
        vec![
            ("first.txt", "crates/app +5\n!budget +5\n"),
            ("second.txt", "crates/app +5\n!budget +5\n"),
        ],
        vec![("first.txt", "crates/app +4\n!budget +4\n")],
        vec![("second.txt", "crates/app +5\n!budget +5\n")],
    ] {
        let base = tree(500);
        let head = tree(500);
        fragment(base.path(), "first.txt", "crates/app +5\n!budget +5\n");
        fragment(base.path(), "second.txt", "crates/app +5\n!budget +5\n");
        fs::write(
            head.join(app_lines::BASELINE_FILE),
            "!budget 1010\ncrates/app 1010\n",
        )
        .unwrap();
        for (name, content) in old_in_head {
            fragment(head.path(), name, content);
        }
        assert!(!check(head.path(), base.path(), guard()).is_empty());
    }
}

#[test]
fn zero_net_and_coverage_only_fragments_can_be_folded() {
    for content in [
        "crates/app +1\ncrates/app -1\n!budget +1\n!budget -1\n",
        "!measure-change ratchet.app-lines.measured +1\n",
    ] {
        let base = tree(500);
        let head = tree(500);
        fragment(base.path(), "old.txt", content);
        assert!(check(head.path(), base.path(), guard()).is_empty());
    }
}

#[test]
fn empty_and_canonical_head_base_paths_fail_closed() {
    let head = tree(500);
    assert!(
        check(head.path(), Path::new(""), guard())
            .iter()
            .any(|finding| finding.line == "base root must not be empty")
    );
    for base in [head.path().to_owned(), head.join(".")] {
        assert!(
            check(head.path(), &base, guard()).iter().any(
                |finding| finding.line == "base root must be a separate checkout from the head"
            )
        );
    }
}
