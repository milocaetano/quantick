//! Scratch directories the tests own, rather than leak.
//!
//! Every test in this crate that touches a real disk used to spell its own
//! temporary path, and every spelling keyed on `std::process::id()` alone.
//! Two things follow from that, and both cost review rounds.
//!
//! A process id is *reused*. Windows hands one back within minutes, so a test
//! whose folder is named after it opens the folder a previous run left behind
//! and asserts on that run's files — the failure looks like the change under
//! test and is not. Three `paper_trading` tests failed that way, and CI drops
//! one app test at random for the same reason.
//!
//! And nothing removed any of it: the host that runs this suite daily held
//! 450,227 `quantick-*` entries in `%TEMP%` on 2026-09-04.
//!
//! # The fix, in two halves
//!
//! **Unique per run.** Every path here carries [`run_token`] — the process id
//! *and* the nanoseconds since the epoch at which this process first asked.
//! A reused pid cannot reproduce the nanosecond, so no run can ever see
//! another's leftovers, whatever the operating system does with pids.
//!
//! **Removed by an owner.** Two owners, because there are two shapes of
//! caller. A test that can hold a value holds a [`ScratchDir`], which removes
//! its tree on `Drop` — including when the test panics, since that unwinds.
//! A path resolved *inside* production code under `cfg!(test)` —
//! `store_home::test_path`, `paper_home::startup_home`,
//! `paper_state::scratch_path`, `paper_trading`'s journal folder — has no such
//! value: it is re-resolved on every call and must answer the same twice.
//! Those take [`thread_dir`], which hands the directory to the test's own
//! thread and removes it when that thread ends.
//!
//! The thread is the right owner because `libtest` runs each test on a thread
//! it spawned, and a spawned thread runs its thread-local destructors on the
//! way out. The exception is `--test-threads=1`, where libtest uses the main
//! thread and the process exits without running its destructors; a serialized
//! run therefore leaves one directory per label behind. That is the only mode
//! that leaks, it leaks a bounded handful rather than one per test, and it is
//! not the mode CI or an agent runs.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

// Only [`ScratchDir`] needs these, and it exists only in a test build.
#[cfg(test)]
use std::path::Path;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

/// The prefix every scratch path in this crate carries, so one `ls` finds
/// them all and the guard has one string to look for.
const PREFIX: &str = "quantick-app";

/// A token no other run of this crate can produce: this process's id, and the
/// nanoseconds since the epoch at which it first asked.
///
/// Read **once** per process, not once per call. A per-call read would make
/// two resolutions of the same stable path disagree, which is precisely what
/// `store_home::test_path` exists to prevent — the bundle would then write to
/// a different file than the app is reading.
pub(crate) fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        // A clock that refuses to answer is not a reason to fail a test run;
        // the pid alone still separates concurrent runs, and only a reused
        // pid on a broken clock could then collide.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        format!("{}-{nanos}", std::process::id())
    })
}

/// This thread, as a filename-safe tag. `ThreadId`'s `Debug` is the only way
/// to name a thread on stable Rust, and it renders as `ThreadId(3)`, whose
/// parentheses do not belong in a path.
fn thread_tag() -> String {
    let id = format!("{:?}", std::thread::current().id());
    let digits: String = id.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() {
        // No `ThreadId` has ever rendered without a number, but a tag that
        // silently collapses to the empty string would make two threads share
        // a directory, which is the failure this module exists to end.
        format!("t{id}").replace(['(', ')'], "-")
    } else {
        format!("t{digits}")
    }
}

thread_local! {
    /// Directories this thread created that no value owns. Removed when the
    /// thread ends — see the module docs for why the thread is the owner.
    static OWNED: OwnedDirs = const { OwnedDirs(RefCell::new(Vec::new())) };
}

/// The thread's own directories, and their removal.
struct OwnedDirs(RefCell<Vec<PathBuf>>);

impl Drop for OwnedDirs {
    fn drop(&mut self) {
        for dir in self.0.borrow().iter() {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

/// A directory belonging to this test's thread, created if it is not there
/// and removed when the thread ends.
///
/// Stable: the same `label` on the same thread answers the same path every
/// time, which is what the `cfg!(test)` paths in production modules need.
/// Callers that can hold a value should hold a [`ScratchDir`] instead — its
/// removal happens at the end of the test rather than the end of the thread.
pub(crate) fn thread_dir(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("{PREFIX}-{}-{label}-{}", run_token(), thread_tag()));
    // Created here rather than by the caller: every caller wants it to exist,
    // and a `create_dir_all` per resolution is a stat on a warm path.
    let _ = fs::create_dir_all(&dir);
    OWNED.with(|owned| {
        let mut dirs = owned.0.borrow_mut();
        if !dirs.contains(&dir) {
            dirs.push(dir.clone());
        }
    });
    dir
}

/// A directory of this test's own, removed with everything under it when the
/// value is dropped — including on a panic, which unwinds.
///
/// Keep it alive for as long as the files matter. A test that hands the path
/// to something that writes later must hold the `ScratchDir` until its last
/// assertion, or the directory is gone before the assertion reads it.
/// Gated on `test` because every caller is: the `cfg!(test)` paths in
/// production modules take [`thread_dir`] instead, so nothing outside a test
/// build has a use for this type and a release build should not compile it.
#[cfg(test)]
pub(crate) struct ScratchDir(PathBuf);

#[cfg(test)]
impl ScratchDir {
    /// A fresh directory named `{PREFIX}-<pid>-<nanos>-<counter>-<label>`.
    ///
    /// The counter separates two directories the same test asks for; the run
    /// token separates every run from every other.
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

/// So a `ScratchDir` can stand where a `&Path` is wanted, which is how the
/// per-module `scratch()` helpers this replaced were used: `dir.join(..)`,
/// `fs::write(&dir, ..)`, `load(&dir)`. Holding the value is still the
/// caller's job — a `ScratchDir` dropped at the end of the statement that
/// created it takes its files with it, and the test fails on the missing
/// file, loudly, which is the right way for that mistake to surface.
#[cfg(test)]
impl std::ops::Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl AsRef<Path> for ScratchDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for ScratchDir {
    fn drop(&mut self) {
        // Best-effort, like `replay`'s. A file still held open by something
        // the test spawned is not worth failing a green test over, and the
        // run token means the leftover can never be mistaken for a later
        // run's own directory.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One scratch *file*, in a directory of its own that removes itself.
///
/// The shape most of this crate's per-module helpers had before: they handed
/// back a `PathBuf` naming a `.toml` and nothing ever removed it. A
/// `ScratchFile` stands where that `&Path` stood — `fs::write(&file, ..)`,
/// `load_from(&file)` — and takes its directory with it when the test ends.
#[cfg(test)]
pub(crate) struct ScratchFile {
    /// Held only for its `Drop`: removing the directory removes the file.
    _dir: ScratchDir,
    path: PathBuf,
}

#[cfg(test)]
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

#[cfg(test)]
impl std::ops::Deref for ScratchFile {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
impl AsRef<Path> for ScratchFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_scratch_directories_never_share_a_path() {
        let first = ScratchDir::new("same-label");
        let second = ScratchDir::new("same-label");
        assert_ne!(first.path(), second.path());
    }

    #[test]
    fn a_scratch_directory_is_gone_once_it_is_dropped() {
        let path = {
            let scratch = ScratchDir::new("removed-on-drop");
            scratch.write("nested/file.toml", "content");
            assert!(scratch.path().exists(), "the directory was created");
            scratch.path().to_path_buf()
        };
        assert!(!path.exists(), "the tree went with the value: {path:?}");
    }

    #[test]
    fn the_run_token_carries_the_pid_and_is_stable_within_a_process() {
        let token = run_token();
        assert!(
            token.starts_with(&format!("{}-", std::process::id())),
            "the token opens with this process's id: {token}"
        );
        assert_eq!(token, run_token(), "the token is read once, not per call");
    }

    #[test]
    fn a_scratch_file_goes_with_its_directory() {
        let path = {
            let file = ScratchFile::new("file-drop", "state.toml");
            fs::write(&file, "x").expect("the scratch file is writable");
            assert!(file.path().exists(), "the file was written");
            file.path().to_path_buf()
        };
        assert!(!path.exists(), "the file went with its directory: {path:?}");
    }

    #[test]
    fn a_thread_directory_answers_the_same_path_twice() {
        let first = thread_dir("stable-label");
        let second = thread_dir("stable-label");
        assert_eq!(first, second);
        assert!(first.exists(), "it was created");
    }

    #[test]
    fn a_thread_directory_goes_when_its_thread_ends() {
        let path = std::thread::spawn(|| thread_dir("ends-with-the-thread"))
            .join()
            .expect("the thread finished");
        assert!(
            !path.exists(),
            "the thread's own directory went with it: {path:?}"
        );
    }
}
