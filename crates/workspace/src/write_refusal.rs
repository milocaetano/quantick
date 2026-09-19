//! Why a session writes no store, and the one gate every store write asks
//! (decision DS7). Headless, so the refusal is a value a caller matches on,
//! not a sentence it parses.
//!
//! The launch root decides the refusal once ([`refuse_writes`]); every
//! writer below it — the cockpit stores, the paper sidecar, the strategy
//! bank, the window's own files — asks [`guard_write`] before it touches the
//! disk, so a refused session writes nothing rather than some files.

use std::path::Path;
use std::sync::OnceLock;

/// Why a session writes no store (decision DS7): the `QUANTICK_*` names set
/// at launch that this build does not read. Typed, so a caller tells a
/// refusal from an I/O failure and a reader lists the names rather than
/// parsing a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritesRefused {
    /// The unread names, sorted and unique.
    pub hooks: Vec<String>,
}

impl std::fmt::Display for WritesRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "saving is off: {} set, which this build does not read \
             (capture hooks need a --features harness build)",
            self.hooks.join(", ")
        )
    }
}

impl std::error::Error for WritesRefused {}

impl From<WritesRefused> for String {
    fn from(refused: WritesRefused) -> Self {
        refused.to_string()
    }
}

/// This session's refusal, set once by the launch root.
static WRITES_REFUSED: OnceLock<WritesRefused> = OnceLock::new();

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// The same refusal for one test thread only.
    static THREAD_WRITES_REFUSED: std::cell::RefCell<Option<WritesRefused>> =
        const { std::cell::RefCell::new(None) };
}

/// Refuse every store write for the rest of this session.
pub fn refuse_writes(refused: WritesRefused) {
    let _ = WRITES_REFUSED.set(refused);
}

/// [`refuse_writes`] for the calling test thread only; `None` lifts it.
#[cfg(any(test, feature = "test-support"))]
pub fn refuse_writes_on_this_thread(refused: Option<WritesRefused>) {
    THREAD_WRITES_REFUSED.with(|slot| *slot.borrow_mut() = refused);
}

/// Why store writes are refused this session, if they are.
#[must_use]
pub fn writes_refused() -> Option<WritesRefused> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(reason) = THREAD_WRITES_REFUSED.with(|refused| refused.borrow().clone()) {
        return Some(reason);
    }
    WRITES_REFUSED.get().cloned()
}

/// Ask before writing `path`: `Err` with the reason, logged, when this
/// session writes no store.
///
/// # Errors
///
/// The session's [`WritesRefused`], when it has one.
pub fn guard_write(path: &Path) -> Result<(), WritesRefused> {
    guard_writes(&path.display())
}

/// [`guard_write`] for a write that touches several stores, named by
/// `target` rather than by one path.
///
/// # Errors
///
/// The session's [`WritesRefused`], when it has one.
pub fn guard_writes(target: &dyn std::fmt::Display) -> Result<(), WritesRefused> {
    match writes_refused() {
        None => Ok(()),
        Some(reason) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "STORE_WRITE_REFUSED",
                path = %target,
                reason = %reason,
                action = "nothing_written",
                "this session writes no store"
            );
            Err(reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_thread_writes_nothing_and_a_lifted_one_writes_again() {
        let refused = WritesRefused {
            hooks: vec!["QUANTICK_UI_STATE".to_owned()],
        };
        assert_eq!(guard_write(Path::new("ui-state.toml")), Ok(()));
        refuse_writes_on_this_thread(Some(refused.clone()));
        assert_eq!(guard_write(Path::new("ui-state.toml")), Err(refused));
        refuse_writes_on_this_thread(None);
        assert_eq!(guard_writes(&"workspace import"), Ok(()));
    }

    #[test]
    fn a_refusal_names_every_unread_hook_and_the_build_that_reads_them() {
        let refused = WritesRefused {
            hooks: vec![
                "QUANTICK_LAYOUTS".to_owned(),
                "QUANTICK_UI_STATE".to_owned(),
            ],
        };
        assert_eq!(
            String::from(refused),
            "saving is off: QUANTICK_LAYOUTS, QUANTICK_UI_STATE set, which this build \
             does not read (capture hooks need a --features harness build)"
        );
    }
}
