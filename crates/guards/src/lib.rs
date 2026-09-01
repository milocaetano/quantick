//! Repository guards for the things the compiler cannot see.
//!
//! Three rules hold in this repo that no amount of `cargo build` can check: a
//! file may not silently absorb a crate ([`size`]), everything written into a
//! tracked file is English ([`language`]), and sources are UTF-8 without a BOM
//! and without welded doc comments ([`encoding`]). Each is a rule
//! `CLAUDE.md` states and each fails invisibly — fmt, clippy, build and the
//! whole suite stay green while it is broken.
//!
//! # Why this is a crate rather than three test files
//!
//! It used to be three test files under `crates/app/tests/`. None of them
//! imported anything from `quantick-app` — they read files and count lines,
//! and `std` is the whole dependency — but an integration test living in a
//! crate's `tests/` directory makes cargo build that crate first. So asking
//! the cheapest question in the repo meant building the largest crate in it:
//! four minutes of link for five seconds of work, on a warm `target/`.
//!
//! That cost was not paid once. The size ratchet is *designed* to fire while
//! you work — that is what a ratchet is for — and each firing sent the author
//! back through a full build to read one number. A guard that expensive to
//! consult is one that gets consulted late, which is exactly when its finding
//! costs the most to act on.
//!
//! So the guards moved into a crate with no dependencies at all. `cargo test
//! --workspace` still runs them and CI is unchanged; what changed is that
//! `cargo test -p quantick-guards` is now a question you can afford to ask
//! after every edit, and the [`bin`](../quantick_guards/index.html) beside
//! this library answers it for a single file in milliseconds, which is what
//! the edit-time hook in `.claude/hooks/` calls.
//!
//! The rules did not soften. Every threshold, every keyword and every
//! grandfathered path came across unchanged; the only guard whose reach grew
//! is [`encoding`], which now sees every crate rather than only the one it
//! was born inside.

pub mod encoding;
pub mod language;
pub mod size;

use std::path::{Path, PathBuf};

/// The workspace root, from this crate's manifest directory.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/guards sits two levels below the workspace root")
        .to_path_buf()
}

/// One guard, so the binary and the tests name the same three things in the
/// same order.
pub struct Guard {
    /// How the guard is named in output.
    pub name: &'static str,
    /// Every violation across the whole repository.
    pub check: fn(&Path) -> Vec<String>,
    /// Every violation in one file, for the edit-time hook.
    pub check_file: fn(&Path, &str) -> Vec<String>,
    /// What to do about a violation.
    pub remedy: &'static str,
}

/// Every guard this crate runs.
pub const GUARDS: &[Guard] = &[
    Guard {
        name: "size",
        check: size::check,
        check_file: size::check_file,
        remedy: size::REMEDY,
    },
    Guard {
        name: "language",
        check: language::check,
        check_file: language::check_file,
        remedy: language::REMEDY,
    },
    Guard {
        name: "encoding",
        check: encoding::check,
        check_file: encoding::check_file,
        remedy: encoding::REMEDY,
    },
];
