//! A unique scratch folder per test, cleaned up on the way out.
//!
//! Test-only. Shared by every module here that has to touch a real disk —
//! the library scan and the session loader both do, and two spellings of
//! "make me a temp folder" is one more thing to keep in step.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A temporary folder that removes itself when dropped.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// A folder unique to this process, this test name and this call.
    pub(crate) fn new(name: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "quantick-replay-{name}-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// Write `contents` at `relative`, creating parent folders as needed.
    pub(crate) fn write(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// The folder itself.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
