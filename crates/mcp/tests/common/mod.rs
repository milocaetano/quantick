//! Scratch directories these integration tests own, rather than leak.
//!
//! Both test binaries here spelled their own temporary path, and both keyed
//! it on `std::process::id()` alone: a reused process id — Windows hands one
//! back within minutes — pointed a later run at an earlier run's descriptor
//! directory, and nothing ever removed either. This module is the `mcp` half
//! of the same fix `crates/app/src/scratch.rs` makes for the application.
//!
//! It lives under `tests/common/` rather than beside its callers because
//! cargo compiles every file directly in `tests/` as a test binary of its
//! own; a subdirectory module is shared support instead.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// A token no other run can produce: this process's id, and the nanoseconds
/// since the epoch at which it first asked. Read once per process.
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
///
/// Hold it for as long as the files matter: the gateway and the adapter both
/// keep writing into the directory for as long as they are alive.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    /// A fresh directory named `quantick-mcp-<pid>-<nanos>-<counter>-<label>`.
    ///
    /// Not created: both callers hand the path to discovery as the instances
    /// directory, and "no such directory" is the state they are testing — an
    /// empty one that exists is a different answer. Whatever the adapter or
    /// the gateway creates there is still removed on the way out.
    pub fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "quantick-mcp-{}-{}-{label}",
            run_token(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self(dir)
    }

    /// The directory itself.
    pub fn path(&self) -> &Path {
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
        // Best-effort: a file a spawned adapter still holds open is not worth
        // failing a green test over, and the run token means no later run can
        // mistake the leftover for its own.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
