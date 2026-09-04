//! The smallest temporary directory that removes itself, so these tests stay
//! inside the no-dependency rule the whole crate is built on.
//!
//! It was three spellings before: `ratchet`'s own `ScratchDir`, `context`'s
//! process-id-keyed `scratch`, and `size`'s folder named after the *test*
//! rather than after anything unique at all. The last is the interesting one
//! — its comment records why it was written that way, "a reused pid leaves a
//! populated directory behind and the test then fails on the previous run's
//! contents" — which trades one collision for a worse one: two worktrees
//! running the suite at once, the workflow `CLAUDE.md` prescribes, then share
//! a fixture directory and delete each other's files mid-test.
//!
//! [`run_token`] settles both. A pid *and* the nanosecond the process started
//! cannot be reproduced by a later run or a concurrent one, so the name needs
//! no help from the test's own name, and `Drop` means nothing is left to
//! inherit.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{fs, process};

/// A token no other run of this crate can produce. Read once per process.
fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        format!("{}-{nanos}", process::id())
    })
}

/// A directory that removes itself, with everything under it, when dropped.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// A fresh directory, unique to this run and this call.
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "quantick-guards-{}-{}-{label}",
            run_token(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temp dir is creatable");
        Self(path)
    }

    /// The directory itself.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// So a `ScratchDir` stands where the `PathBuf` roots these tests passed around
/// stood: `root.join(..)`, `check(&root)`.
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
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_directories_with_one_label_never_share_a_path() {
        let first = ScratchDir::new("same");
        let second = ScratchDir::new("same");
        assert_ne!(first.path(), second.path());
    }

    #[test]
    fn the_tree_goes_when_the_value_does() {
        let path = {
            let dir = ScratchDir::new("removed");
            fs::create_dir_all(dir.join("nested")).expect("nested dirs are creatable");
            fs::write(dir.join("nested/file.txt"), "content").expect("the file is writable");
            dir.path().to_path_buf()
        };
        assert!(!path.exists(), "the tree went with the value: {path:?}");
    }
}
