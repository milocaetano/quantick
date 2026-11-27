//! Scratch directories the tests own, rather than leak.
//!
//! Test-side, and the shared copy: every crate whose tests touch a real disk
//! mounts this file with `#[path = "../../stores/src/scratch.rs"]` instead of
//! carrying its own, because a shared crate would be a dependency every crate
//! with tests gains for a hundred lines of `std`. Two keep their own:
//! `crates/app/src/scratch.rs`, since `extension_boundary` refuses `#[path]`
//! modules under `crates/app/src`, and `crates/guards/src/scratch_dir.rs`,
//! since `guards` depends on nothing. The repository guard
//! `crates/guards/src/scratch.rs` names these files and refuses
//! `std::env::temp_dir()` anywhere else.
//!
//! **Unique per run.** Every path carries [`run_token`] — the process id *and*
//! the nanoseconds since the epoch at which this process first asked. Windows
//! reuses a process id within minutes, so a folder named after the id alone is
//! a folder a later run inherits populated, and the failure looks like the
//! change under test. A reused pid cannot reproduce the nanosecond.
//!
//! **Removed by an owner.** A test that can hold a value holds a
//! [`ScratchDir`] or [`ScratchFile`], which removes its tree on `Drop` —
//! including on a panic, which unwinds. A path resolved inside production code
//! under `cfg!(test)` has no such value: it is re-resolved on every call and
//! must answer the same twice. Those take [`thread_dir`], which hands the
//! directory to the test's own thread and removes it when that thread ends.
//! `libtest` runs each test on a thread it spawned, whose thread-locals drop
//! on the way out; under `--test-threads=1` the main thread runs them and the
//! process exits without that, leaving one directory per label behind.
//!
//! Every path opens with the mounting crate's package name, so one `ls` finds
//! a crate's leftovers and two crates never share a folder.

// Each mounting crate uses its own subset of this module.
#![allow(dead_code)]

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// The prefix every scratch path carries: the mounting crate's package name.
const PREFIX: &str = env!("CARGO_PKG_NAME");

/// A token no other run of this crate can produce: this process's id, and the
/// nanoseconds since the epoch at which it first asked.
///
/// Read **once** per process, not once per call: a per-call read would make
/// two resolutions of the same stable [`thread_dir`] path disagree.
pub(crate) fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        // A clock that refuses to answer is not a reason to fail a test run;
        // the pid alone still separates concurrent runs.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        format!("{}-{nanos}", std::process::id())
    })
}

/// A directory of this test's own, removed with everything under it when the
/// value is dropped.
///
/// Keep it alive for as long as the files matter: a `ScratchDir` dropped at
/// the end of the statement that created it takes its files with it.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    /// A fresh directory named `<package>-<pid>-<nanos>-<counter>-<label>`,
    /// created so a caller can write into it at once.
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "{PREFIX}-{}-{}-{label}",
            run_token(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        Self(dir)
    }

    /// The directory itself.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside it. The file need not exist — a test asking what happens
    /// to a missing file wants exactly this.
    pub(crate) fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.0.join(relative)
    }

    /// Write `contents` at `relative`, creating parent folders as needed.
    pub(crate) fn write(&self, relative: impl AsRef<Path>, contents: &str) -> PathBuf {
        let path = self.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a scratch parent is creatable");
        }
        fs::write(&path, contents).expect("a scratch file is writable");
        path
    }
}

impl std::ops::Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for ScratchDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // Best-effort: a file something the test spawned still holds open is
        // not worth failing a green test over, and the run token means no
        // later run can inherit it.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One scratch *file*, in a directory of its own that removes itself. It
/// stands where a `&Path` naming a file is wanted.
pub(crate) struct ScratchFile {
    /// Held only for its `Drop`: removing the directory removes the file.
    _dir: ScratchDir,
    path: PathBuf,
}

impl ScratchFile {
    /// `name` inside a fresh directory labelled `label`.
    pub(crate) fn new(label: &str, name: &str) -> Self {
        let dir = ScratchDir::new(label);
        let path = dir.join(name);
        Self { _dir: dir, path }
    }

    /// The file's path. It does not exist until something writes it.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl std::ops::Deref for ScratchFile {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for ScratchFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

thread_local! {
    /// The per-thread directories this thread's tests share, removed with
    /// the thread.
    static OWNED: RefCell<Vec<(String, ScratchDir)>> = const { RefCell::new(Vec::new()) };
}

/// A directory shared by every call on this thread with the same `label`,
/// created on first use and removed when the thread ends.
///
/// Stable per (run, thread, label), and only that: a *different* thread gets
/// a different directory, which keeps two tests apart. So never hand a
/// `thread_dir` path to anything that outlives the test's thread, and never
/// expect a spawned thread to resolve the same path — a test that needs one
/// directory across threads wants a [`ScratchDir`] it passes by path.
pub(crate) fn thread_dir(label: &str) -> PathBuf {
    OWNED.with(|owned| {
        let mut dirs = owned.borrow_mut();
        if let Some((_, dir)) = dirs.iter().find(|(known, _)| known == label) {
            return dir.path().to_path_buf();
        }
        let dir = ScratchDir::new(label);
        let path = dir.path().to_path_buf();
        dirs.push((label.to_owned(), dir));
        path
    })
}
