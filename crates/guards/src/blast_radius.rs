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

use std::fs;
use std::path::Path;

use crate::size;

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
pub fn measure(root: &Path, diff: &str) -> BlastRadius {
    let mut radius = BlastRadius::default();

    for file in parse(diff) {
        radius.files_touched += 1;
        radius.insertions += file.added.len();
        if !file.new_file {
            radius.pre_existing_files_touched += 1;
        }

        if file.new_file || file.deleted || !size::tracked(&file.path) {
            radius.files_unmeasured += 1;
            continue;
        }

        let Ok(source) = fs::read_to_string(root.join(&file.path)) else {
            radius.files_unreadable += 1;
            continue;
        };

        let flags = size::production_flags(&source);
        let production = |line: usize| flags.get(line.saturating_sub(1)).copied().unwrap_or(false);
        let added = file.added.iter().filter(|line| production(**line)).count() as i64;
        // A removed line has no post-image number of its own; it sat where the
        // cursor stood when the diff walked past it, so that is the position
        // whose flag governs it. Clamped, because a line removed from the end
        // of a file leaves the cursor one past it.
        let removed = file
            .removed
            .iter()
            .filter(|line| production((**line).clamp(1, flags.len().max(1))))
            .count() as i64;

        if added - removed > 0 {
            radius.deposits.push(Deposit {
                path: file.path,
                production_added: added - removed,
            });
        }
    }

    // Descending by size, then by path. The second key is not decoration: two
    // files that gained the same number of lines would otherwise print in
    // whatever order the diff happened to list them, and a report whose rows
    // move without the tree moving cannot be diffed against yesterday's.
    radius.deposits.sort_by(|left, right| {
        right
            .production_added
            .cmp(&left.production_added)
            .then_with(|| left.path.cmp(&right.path))
    });
    radius
}

/// The report's `label<TAB>value` table, deposits first.
pub fn render(radius: &BlastRadius) -> String {
    let mut out = String::new();
    for deposit in &radius.deposits {
        out.push_str(&format!(
            "blast.file.{}\t{}\n",
            deposit.path, deposit.production_added
        ));
    }
    out.push_str(&format!("blast.files_touched\t{}\n", radius.files_touched));
    out.push_str(&format!(
        "blast.pre_existing_files_touched\t{}\n",
        radius.pre_existing_files_touched
    ));
    out.push_str(&format!("blast.insertions\t{}\n", radius.insertions));
    out.push_str(&format!(
        "blast.files_unmeasured\t{}\n",
        radius.files_unmeasured
    ));
    out.push_str(&format!(
        "blast.files_unreadable\t{}\n",
        radius.files_unreadable
    ));
    out
}

/// One file's section of the diff, reduced to what a deposit needs.
struct FileDiff {
    path: String,
    new_file: bool,
    deleted: bool,
    /// Post-image line numbers of the added lines.
    added: Vec<usize>,
    /// Post-image cursor position at each removed line.
    removed: Vec<usize>,
}

/// Split a unified diff into its files.
///
/// Deliberately a small reader rather than a parser. It follows `diff --git`
/// for the section boundary and `+++`/`---` for the path and the new-file
/// flag, and inside a hunk it trusts the first character of the line. The one
/// thing it must not get wrong is mistaking a `+++` of file content for a
/// header, so header lines are only read before the section's first `@@`.
fn parse(diff: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut in_hunk = false;
    let mut cursor = 0usize;

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.push(FileDiff {
                // The `+++` line below is the authority on the path; this is
                // the fallback for a section that has none, which is what a
                // pure mode change looks like.
                path: git_path(rest.split(" b/").last().unwrap_or_default()),
                new_file: false,
                deleted: false,
                added: Vec::new(),
                removed: Vec::new(),
            });
            in_hunk = false;
            continue;
        }

        let Some(file) = files.last_mut() else {
            continue;
        };

        if let Some(range) = line.strip_prefix("@@ ") {
            in_hunk = true;
            cursor = post_image_start(range);
            continue;
        }

        if !in_hunk {
            if line.starts_with("new file mode") {
                file.new_file = true;
            } else if line.starts_with("deleted file mode") {
                file.deleted = true;
            } else if let Some(path) = line.strip_prefix("--- ") {
                if path == "/dev/null" {
                    file.new_file = true;
                }
            } else if let Some(path) = line.strip_prefix("+++ ") {
                if path == "/dev/null" {
                    file.deleted = true;
                } else {
                    file.path = git_path(path.trim_start_matches("b/"));
                }
            }
            continue;
        }

        match line.as_bytes().first() {
            Some(b'+') => {
                file.added.push(cursor);
                cursor += 1;
            }
            Some(b'-') => file.removed.push(cursor),
            // `\ No newline at end of file` annotates the line above and
            // occupies no position of its own.
            Some(b'\\') => {}
            // A context line, including the empty one git writes for a blank
            // line of context.
            _ => cursor += 1,
        }
    }

    files
}

/// The `+<start>` of an `@@ -a,b +c,d @@` range, 1-based, or 1 when the header
/// does not read as one.
fn post_image_start(range: &str) -> usize {
    range
        .split('+')
        .nth(1)
        .and_then(|plus| plus.split([',', ' ']).next())
        .and_then(|start| start.parse().ok())
        .unwrap_or(1)
}

/// A path as the size guard spells it: forward slashes, no surrounding quotes.
///
/// git quotes a path holding a byte outside the printable ASCII range and
/// escapes what is inside. Unquoting that properly is a parser; what this does
/// instead is leave such a path alone, so it fails to match a file on disk and
/// is counted under `blast.files_unreadable` rather than silently attributed
/// to the wrong file.
fn git_path(raw: &str) -> String {
    raw.trim().replace('\\', "/")
}

#[cfg(test)]
mod tests {
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
