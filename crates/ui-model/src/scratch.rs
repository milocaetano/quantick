//! Test scratch directories with exclusive creation and RAII cleanup.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A directory of this test's own, removed with everything under it when the
/// value is dropped — including on a panic, which unwinds.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// Claim a fresh directory without relying on a clock or a reused process ID.
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let dir = std::env::temp_dir().join(format!(
                "quantick-ui-model-{}-{}-{label}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("could not create scratch directory: {error}"),
            }
        }
    }

    /// The directory itself.
    pub fn path(&self) -> &Path {
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
/// for the `cfg!(test)` store paths that have no value to hold a
/// [`ScratchDir`] in. Gone when the thread ends.
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
        // over, and exclusive creation prevents a later run from inheriting it.
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
