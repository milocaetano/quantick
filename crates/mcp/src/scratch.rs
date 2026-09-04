//! Scratch directories these tests own, rather than leak.
//!
//! Test-only. The same fix `crates/app/src/scratch.rs` documents at length,
//! in the smallest form this crate needs: a temporary path keyed on
//! `std::process::id()` alone is a path a later run can inherit populated,
//! because the operating system reuses process ids — and nothing here ever
//! removed one.
//!
//! Each crate carries its own copy rather than sharing one. A shared helper
//! would be a new workspace crate that every crate with tests depends on,
//! which this repository's dependency rule and its "no new dependency"
//! constraint both argue against for eighty lines of `std`. The repository
//! guard `crates/guards/src/scratch.rs` is what keeps the copies honest: it
//! names each of them, and refuses the call anywhere else.

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
    /// A fresh directory named `quantick-mcp-unit-<pid>-<nanos>-<counter>-<label>`.
    /// Created, so a caller can write into it at once.
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "quantick-mcp-unit-{}-{}-{label}",
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
