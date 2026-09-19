//! Scratch directories these tests own, rather than leak.
//!
//! Test-only. The same helper `crates/control-local/src/scratch.rs` carries,
//! in this crate's own copy because a crate that may not depend on another
//! cannot share one: a temporary path keyed on `std::process::id()` alone is
//! a path a later run can inherit populated, because the operating system
//! reuses process ids. The repository guard `crates/guards/src/scratch.rs`
//! names each copy and refuses the call anywhere else.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// A token no other run of this crate can produce: this process's id, and the
/// nanoseconds since the epoch at which it first asked. Read once per
/// process, so two resolutions inside one test agree.
fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        format!("{}-{nanos}", std::process::id())
    })
}

/// A directory of this test's own, removed with everything under it when the
/// value is dropped — including on a panic, which unwinds.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    /// A fresh directory named `quantick-paper-<pid>-<nanos>-<counter>-<label>`.
    /// Created, so a caller can write into it at once.
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "quantick-paper-{}-{}-{label}",
            run_token(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        Self(dir)
    }

    /// The directory itself.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

thread_local! {
    /// The per-thread directories this thread's tests share, removed with
    /// the thread: `libtest` runs each test on its own thread, so a path
    /// minted here outlives no test and leaks past no run.
    static OWNED: RefCell<Vec<(String, ScratchDir)>> = const { RefCell::new(Vec::new()) };
}

/// A directory shared by every call on this thread with the same `label`,
/// for the store tests that have no value to hold a [`ScratchDir`] in.
/// Gone when the thread ends.
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
        // Best-effort, like every other copy of this: a file something the
        // test spawned still holds open is not worth failing a green test
        // over, and the run token means no later run can inherit it.
        let _ = std::fs::remove_dir_all(&self.0);
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
    fn the_tree_goes_when_the_value_does() {
        let path = {
            let dir = ScratchDir::new("removed-on-drop");
            std::fs::write(dir.join("file.txt"), "content").expect("the file is writable");
            dir.path().to_path_buf()
        };
        assert!(!path.exists(), "the tree went with the value: {path:?}");
    }
}
