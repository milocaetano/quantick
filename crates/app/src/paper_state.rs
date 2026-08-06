//! Where the paper-trading journal lives, when the user picked it in-app.
//!
//! One value, one home: the shipped base stays `[paper] trades_dir` in the
//! config, the `QUANTICK_TRADES_DIR` environment variable stays the
//! explicit per-run override, and this sidecar records the choice made
//! with the panel's folder picker — the added-symbols pattern: an in-app
//! edit must survive a restart without the app rewriting the user's
//! hand-commented `quantick.toml` (a TOML writer would strip its comments,
//! which is exactly what the config rules forbid).
//!
//! Same store discipline as the chart layers: a versioned TOML next to the
//! config (override with `QUANTICK_PAPER_STATE`), read once at startup,
//! written when the choice changes, temp-file-and-rename so a crash
//! mid-write cannot leave half a file behind. Anything unreadable is
//! ignored entirely.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Environment override for the paper-state file location.
const STATE_ENV: &str = "QUANTICK_PAPER_STATE";
/// Default file, next to the working directory's config.
const STATE_FILE: &str = "paper-state.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct PaperStateFile {
    version: u32,
    /// The folder the user picked for the trade journal, when they did.
    #[serde(default)]
    trades_dir: Option<String>,
}

/// The paper-state file the app opens with and writes back to. Under test
/// it is a scratch file of its own per process, for the same reason the
/// chart layers do it: tests must not restore one another's choice, nor
/// rewrite the repo's copy.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return scratch_path();
    }
    std::env::var_os(STATE_ENV).map_or_else(|| PathBuf::from(STATE_FILE), PathBuf::from)
}

/// A store of its own, for tests. See [`default_path`].
fn scratch_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "quantick-paper-state-{}-{}.toml",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The stored folder choice; `None` when the file is missing, unreadable,
/// from an unknown version, or holds no choice.
#[must_use]
pub(crate) fn load(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    match toml::from_str::<PaperStateFile>(&text) {
        Ok(file) if file.version == FORMAT_VERSION => {
            file.trades_dir.filter(|dir| !dir.trim().is_empty())
        }
        Ok(file) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_STATE_VERSION",
                path = %path.display(),
                version = file.version,
                action = "keeping_configured_trades_dir",
                "paper state file is from an unknown version"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_STATE_UNREADABLE",
                path = %path.display(),
                %error,
                action = "keeping_configured_trades_dir",
                "paper state file is unreadable"
            );
            None
        }
    }
}

/// Remember the picked folder.
pub(crate) fn save(path: &Path, trades_dir: &str) {
    let file = PaperStateFile {
        version: FORMAT_VERSION,
        trades_dir: Some(trades_dir.to_owned()),
    };
    let Ok(text) = toml::to_string_pretty(&file) else {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "PAPER_STATE_WRITE_FAILED",
            action = "choice_not_saved",
            "could not serialize the paper state"
        );
        return;
    };
    let temp = path.with_extension("toml.tmp");
    if let Err(error) = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)) {
        let _ = std::fs::remove_file(&temp);
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "PAPER_STATE_WRITE_FAILED",
            path = %path.display(),
            %error,
            action = "choice_not_saved",
            "could not save the paper state"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_round_trips_through_disk() {
        let path = scratch_path();
        assert_eq!(load(&path), None, "a missing file holds no choice");
        save(&path, "D:/trading/journals");
        assert_eq!(load(&path), Some("D:/trading/journals".to_owned()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unknown_versions_and_garbage_degrade_to_no_choice() {
        let path = scratch_path();
        std::fs::write(&path, "version = 99\ntrades_dir = \"x\"\n").unwrap();
        assert_eq!(load(&path), None, "unknown version changes nothing");
        std::fs::write(&path, "not even toml [").unwrap();
        assert_eq!(load(&path), None, "garbage changes nothing");
        std::fs::write(&path, "version = 1\ntrades_dir = \"  \"\n").unwrap();
        assert_eq!(load(&path), None, "a blank choice is no choice");
        std::fs::remove_file(&path).ok();
    }
}
