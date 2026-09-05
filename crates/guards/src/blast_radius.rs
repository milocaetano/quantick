//! Where a change deposited its lines, read from a unified diff.
//!
//! The size ratchet rations lines per file, so a deposit into a file that
//! still has headroom is free. PR #306 put 216 production lines of one bar
//! kind's series into `crates/app/src/state.rs` — the state every bar kind
//! shares — and no guard fired, because `state.rs` had room. Nothing was
//! broken; the change was simply larger in one place than anybody had a
//! number for.
//!
//! This mode is that number. It does not judge and cannot fail a build: it
//! reads a diff and says where the lines went, so a reviewer arguing about
//! blast radius argues from a figure rather than an impression.
//!
//! # Why the diff is not enough on its own
//!
//! "Production lines" is [`size::production_lines`]'s question, and its answer
//! depends on the whole file: a line is test code when a column-0
//! `#[cfg(test)]` above it opened an item that has not closed yet. That
//! attribute is usually hundreds of lines above the hunk that added the line,
//! so it is not in the diff. Classifying from the diff alone scores every
//! added line as production — measured against #306's `state.rs`, 564 instead
//! of 216, which is the whole finding inverted.
//!
//! So each pre-existing file the size guard tracks is read from the working
//! tree, and the diff's post-image line numbers index into its production
//! flags. That is the post-image the diff describes whenever the mode is used
//! as intended, `git diff origin/main...HEAD | ... --blast-radius` from the
//! branch. A file the diff names and the tree does not hold is reported as
//! such rather than guessed at, under `blast.files_unreadable`.
//!
//! # What is counted, and what is only totalled
//!
//! A per-file row is a *deposit*: a pre-existing file the size ratchet tracks
//! — `crates/**/*.rs`, [`size::tracked`] — that gained production lines.
//! Everything else is honest about being outside the count rather than
//! silently dropped: a new file has no ceiling to deposit into, and there is
//! no definition of "production" for Markdown or TOML that is not a second
//! definition of production, which is the duplicated-constant defect this
//! repository files against its own code. Those files still appear in the
//! three totals, and in `blast.files_unmeasured`.

use std::path::Path;

/// One pre-existing tracked file and the production lines it gained.
#[derive(Debug, PartialEq, Eq)]
pub struct Deposit {
    /// Workspace-relative path with forward slashes, as the size guard spells
    /// it, so a row here and a baseline entry are the same string.
    pub path: String,
    /// Production lines added less production lines removed. Only positive
    /// values become rows: the label says lines *added*.
    pub production_added: i64,
}

/// What a diff did, in the shape the report renders.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BlastRadius {
    /// Deposits, largest first, ties broken by path so two runs over one diff
    /// print the same table.
    pub deposits: Vec<Deposit>,
    /// Every file the diff names, new and pre-existing, added and deleted.
    pub files_touched: usize,
    /// Those of them that existed before the diff.
    pub pre_existing_files_touched: usize,
    /// Added lines across the whole diff, whatever file they landed in.
    pub insertions: usize,
    /// Touched files the size guard does not track, so no deposit could be
    /// computed for them. Reported rather than omitted: a file missing from
    /// the table should never be indistinguishable from a file that gained
    /// nothing.
    pub files_unmeasured: usize,
    /// Pre-existing tracked files the diff names that the working tree does
    /// not hold, or does not decode as UTF-8. Same reason as the field above.
    pub files_unreadable: usize,
}

/// Measure a unified diff against the working tree at `root`.
///
/// Never fails and never panics: a diff this cannot parse yields zeroes, which
/// is a report-only mode's correct answer to input it does not understand.
pub fn measure(_root: &Path, _diff: &str) -> BlastRadius {
    BlastRadius::default()
}

/// The report's `label<TAB>value` table, deposits first.
pub fn render(_radius: &BlastRadius) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::scratch_dir::ScratchDir;

    /// A file whose test module sits where every real one does — at the
    /// bottom, hundreds of lines below the code a hunk touches.
    fn file_with_a_test_module(production: usize, test: usize) -> String {
        let mut source = String::new();
        for line in 0..production {
            source.push_str(&format!("const PRODUCTION_{line}: usize = {line};\n"));
        }
        source.push_str("#[cfg(test)]\nmod tests {\n");
        for line in 0..test {
            source.push_str(&format!("    const TEST_{line}: usize = {line};\n"));
        }
        source.push_str("}\n");
        source
    }

    fn root_with(path: &str, source: &str) -> ScratchDir {
        let root = ScratchDir::new("blast");
        let file = root.join(path);
        fs::create_dir_all(file.parent().expect("the fixture path has a parent"))
            .expect("the fixture directories are creatable");
        fs::write(&file, source).expect("the fixture file is writable");
        root
    }

    #[test]
    fn lines_added_inside_a_test_module_are_not_a_deposit() {
        // Ten production lines, then the test module; the diff adds three of
        // each. Reading the diff alone would score six.
        let root = root_with(
            "crates/example/src/thing.rs",
            &file_with_a_test_module(10, 5),
        );
        let diff = "\
diff --git a/crates/example/src/thing.rs b/crates/example/src/thing.rs
--- a/crates/example/src/thing.rs
+++ b/crates/example/src/thing.rs
@@ -1,3 +1,6 @@
+const PRODUCTION_0: usize = 0;
+const PRODUCTION_1: usize = 1;
+const PRODUCTION_2: usize = 2;
 const PRODUCTION_3: usize = 3;
 const PRODUCTION_4: usize = 4;
 const PRODUCTION_5: usize = 5;
@@ -12,3 +15,6 @@
+    const TEST_0: usize = 0;
+    const TEST_1: usize = 1;
+    const TEST_2: usize = 2;
     const TEST_3: usize = 3;
";
        let radius = measure(&root, diff);
        assert_eq!(
            radius.deposits,
            vec![Deposit {
                path: "crates/example/src/thing.rs".to_owned(),
                production_added: 3,
            }]
        );
        assert_eq!(
            radius.insertions, 6,
            "every added line is still an insertion"
        );
    }

    #[test]
    fn removed_production_lines_offset_the_deposit() {
        let root = root_with(
            "crates/example/src/thing.rs",
            &file_with_a_test_module(10, 2),
        );
        let diff = "\
diff --git a/crates/example/src/thing.rs b/crates/example/src/thing.rs
--- a/crates/example/src/thing.rs
+++ b/crates/example/src/thing.rs
@@ -1,4 +1,5 @@
+const PRODUCTION_0: usize = 0;
+const PRODUCTION_1: usize = 1;
+const PRODUCTION_2: usize = 2;
-const GONE: usize = 0;
 const PRODUCTION_3: usize = 3;
";
        assert_eq!(
            measure(&root, diff).deposits,
            vec![Deposit {
                path: "crates/example/src/thing.rs".to_owned(),
                production_added: 2,
            }]
        );
    }

    #[test]
    fn a_new_file_is_never_a_deposit() {
        let root = root_with("crates/example/src/new.rs", &file_with_a_test_module(3, 0));
        let diff = "\
diff --git a/crates/example/src/new.rs b/crates/example/src/new.rs
new file mode 100644
--- /dev/null
+++ b/crates/example/src/new.rs
@@ -0,0 +1,3 @@
+const PRODUCTION_0: usize = 0;
+const PRODUCTION_1: usize = 1;
+const PRODUCTION_2: usize = 2;
";
        let radius = measure(&root, diff);
        assert!(
            radius.deposits.is_empty(),
            "a new file has no ceiling to deposit into"
        );
        assert_eq!(radius.files_touched, 1);
        assert_eq!(radius.pre_existing_files_touched, 0);
        assert_eq!(radius.insertions, 3);
        assert_eq!(radius.files_unmeasured, 1);
    }

    #[test]
    fn a_file_outside_the_size_guard_counts_only_in_the_totals() {
        let root = ScratchDir::new("blast");
        let diff = "\
diff --git a/docs/README.md b/docs/README.md
--- a/docs/README.md
+++ b/docs/README.md
@@ -1,1 +1,3 @@
+a line
+another line
 unchanged
";
        let radius = measure(&root, diff);
        assert!(radius.deposits.is_empty());
        assert_eq!(radius.pre_existing_files_touched, 1);
        assert_eq!(radius.insertions, 2);
        assert_eq!(radius.files_unmeasured, 1);
        assert_eq!(
            radius.files_unreadable, 0,
            "a file the guard never tracks is not a file it failed to read"
        );
    }

    #[test]
    fn a_tracked_file_the_tree_does_not_hold_is_reported_not_guessed() {
        let root = ScratchDir::new("blast");
        let diff = "\
diff --git a/crates/example/src/absent.rs b/crates/example/src/absent.rs
--- a/crates/example/src/absent.rs
+++ b/crates/example/src/absent.rs
@@ -1,1 +1,2 @@
+const ADDED: usize = 0;
 const KEPT: usize = 1;
";
        let radius = measure(&root, diff);
        assert!(radius.deposits.is_empty());
        assert_eq!(radius.files_unreadable, 1);
        assert_eq!(radius.files_unmeasured, 0);
    }

    #[test]
    fn rows_are_ordered_by_size_then_path() {
        let root = ScratchDir::new("blast");
        for path in [
            "crates/e/src/b.rs",
            "crates/e/src/a.rs",
            "crates/e/src/big.rs",
        ] {
            let file = root.join(path);
            fs::create_dir_all(file.parent().expect("the fixture path has a parent"))
                .expect("the fixture directories are creatable");
            fs::write(&file, file_with_a_test_module(10, 0)).expect("the fixture file is writable");
        }
        let mut diff = String::new();
        for (path, added) in [
            ("crates/e/src/b.rs", 2),
            ("crates/e/src/big.rs", 5),
            ("crates/e/src/a.rs", 2),
        ] {
            diff.push_str(&format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1,0 +1,{added} @@\n"
            ));
            for line in 0..added {
                diff.push_str(&format!("+const PRODUCTION_{line}: usize = {line};\n"));
            }
        }
        let radius = measure(&root, &diff);
        let paths: Vec<&str> = radius
            .deposits
            .iter()
            .map(|deposit| deposit.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec![
                "crates/e/src/big.rs",
                "crates/e/src/a.rs",
                "crates/e/src/b.rs"
            ]
        );
    }

    #[test]
    fn the_table_is_the_report_label_value_shape() {
        let radius = BlastRadius {
            deposits: vec![Deposit {
                path: "crates/app/src/state.rs".to_owned(),
                production_added: 216,
            }],
            files_touched: 80,
            pre_existing_files_touched: 61,
            insertions: 2_318,
            files_unmeasured: 40,
            files_unreadable: 0,
        };
        assert_eq!(
            render(&radius),
            "blast.file.crates/app/src/state.rs\t216\n\
             blast.files_touched\t80\n\
             blast.pre_existing_files_touched\t61\n\
             blast.insertions\t2318\n\
             blast.files_unmeasured\t40\n\
             blast.files_unreadable\t0\n"
        );
    }

    #[test]
    fn input_that_is_not_a_diff_measures_as_nothing_rather_than_panicking() {
        let root = ScratchDir::new("blast");
        for input in ["", "not a diff at all", "@@ -1 +1 @@\n+orphan hunk\n"] {
            let radius = measure(&root, input);
            assert_eq!(
                radius.files_touched, 0,
                "no `diff --git` header, so no file: {input:?}"
            );
        }
    }

    #[test]
    fn a_content_line_beginning_with_a_header_prefix_is_not_a_header() {
        let root = root_with(
            "crates/example/src/thing.rs",
            "const PRODUCTION_0: usize = 0;\n// +++ b/elsewhere\n",
        );
        let diff = "\
diff --git a/crates/example/src/thing.rs b/crates/example/src/thing.rs
--- a/crates/example/src/thing.rs
+++ b/crates/example/src/thing.rs
@@ -1,1 +1,2 @@
 const PRODUCTION_0: usize = 0;
+// +++ b/elsewhere
";
        assert_eq!(
            measure(&root, diff).deposits,
            vec![Deposit {
                path: "crates/example/src/thing.rs".to_owned(),
                production_added: 1,
            }]
        );
    }
}
