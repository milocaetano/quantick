//! A unique scratch folder per test, cleaned up on the way out.
//!
//! Test-only. Two modules here touch a real disk — the replay session loader
//! and the MT5 bridge's script resolution — and two spellings of "make me a
//! temp folder" is one more thing to keep in step.
//!
//! Every crate that needs one carries its own copy rather than sharing:
//! `guards` depends on nothing at all and so can share nothing, and once one
//! crate has to have its own the others may as well match it.
//! `crates/app/src/scratch.rs` documents at length the flake this shape
//! exists to end — a process id Windows reuses within minutes, naming a
//! folder a later run inherits already populated — and
//! `crates/guards/src/scratch.rs` is the guard that keeps
//! `std::env::temp_dir()` out of every file but this one.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

/// The prefix every scratch path in this crate carries, so one `ls` finds
/// them all.
const PREFIX: &str = "quantick-feed";

/// A token no other run of this crate can produce: this process's id, and the
/// nanoseconds since the epoch at which it first asked. Read once per
/// process.
///
/// The process id on its own was not enough. A reused id names a folder a
/// later run can inherit populated, and the failure then looks like the change
/// under test. A reused pid cannot reproduce the nanosecond.
fn run_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| {
        // A clock that refuses to answer is not a reason to fail a test run;
        // the pid alone still separates concurrent runs, and only a reused pid
        // on a broken clock could then collide.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        format!("{}-{nanos}", std::process::id())
    })
}

/// A directory of this test's own, removed with everything under it when the
/// value is dropped — including on a panic, which unwinds.
///
/// Keep it alive for as long as the files matter: a `ScratchDir` dropped at
/// the end of the statement that created it takes its files with it.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    /// A folder unique to this run, this label and this call.
    pub(crate) fn new(label: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "{PREFIX}-{}-{}-{label}",
            run_token(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        Self(dir)
    }

    /// The folder itself.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside it. The file need not exist — a test asking what happens
    /// to a missing file wants exactly this.
    pub(crate) fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // Best-effort: a file still held open by something the test spawned is
        // not worth failing a green test over, and the run token means the
        // leftover can never be mistaken for a later run's own directory.
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
    fn a_scratch_directory_is_gone_once_it_is_dropped() {
        let path = {
            let scratch = ScratchDir::new("removed-on-drop");
            std::fs::write(scratch.join("file.csv"), "content").expect("writable");
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
}
