//! The paper-trading sidecar: in-app choices that must survive a restart.
//!
//! One value set, one home: the shipped base stays in the config
//! (`[paper] trades_dir`), environment variables stay the explicit
//! per-run overrides, and this sidecar records what the user changed
//! *in-app* — the added-symbols pattern: an in-app edit must survive a
//! restart without the app rewriting the user's hand-commented
//! `quantick.toml` (a TOML writer would strip its comments, which is
//! exactly what the config rules forbid). Today that is the journal
//! folder picked with the panel's button and the cmd-trading settings.
//!
//! Same store discipline as the chart layers: a versioned TOML next to the
//! config (override with `QUANTICK_PAPER_STATE`), read once at startup,
//! written when a choice changes, temp-file-and-rename so a crash
//! mid-write cannot leave half a file behind. Anything unreadable is
//! ignored entirely.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Environment override for the paper-state file location.
const STATE_ENV: &str = "QUANTICK_PAPER_STATE";
/// Default file, next to the working directory's config.
const STATE_FILE: &str = "paper-state.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
/// Adding an optional field is not breaking — absent keys stay `None`.
const FORMAT_VERSION: u32 = 1;

/// Everything the sidecar remembers. Every field is optional: `None`
/// always means "never touched in-app", so defaults stay the resolver's
/// business, not this file's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PaperState {
    /// The folder the user picked for the trade journal, when they did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trades_dir: Option<String>,
    /// Cmd trading on/off; `None` has never been toggled (default on).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd_trading_enabled: Option<bool>,
    /// Modifier token (`shift`/`ctrl`/`alt`) for the buy gesture; an
    /// unknown token falls back to the default at read time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd_buy_modifier: Option<String>,
    /// See [`Self::cmd_buy_modifier`]; the sell side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cmd_sell_modifier: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PaperStateFile {
    version: u32,
    #[serde(flatten)]
    state: PaperState,
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

/// The stored state; default (all `None`) when the file is missing,
/// unreadable or from an unknown version. A blank `trades_dir` is no
/// choice.
#[must_use]
pub(crate) fn load(path: &Path) -> PaperState {
    let Ok(text) = std::fs::read_to_string(path) else {
        return PaperState::default();
    };
    match toml::from_str::<PaperStateFile>(&text) {
        Ok(file) if file.version == FORMAT_VERSION => {
            let mut state = file.state;
            state.trades_dir = state.trades_dir.filter(|dir| !dir.trim().is_empty());
            state
        }
        Ok(file) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_STATE_VERSION",
                path = %path.display(),
                version = file.version,
                action = "keeping_defaults",
                "paper state file is from an unknown version"
            );
            PaperState::default()
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_STATE_UNREADABLE",
                path = %path.display(),
                %error,
                action = "keeping_defaults",
                "paper state file is unreadable"
            );
            PaperState::default()
        }
    }
}

/// Write the whole state back. Callers read-modify-write, so one changed
/// choice never erases another.
pub(crate) fn save(path: &Path, state: &PaperState) {
    let file = PaperStateFile {
        version: FORMAT_VERSION,
        state: state.clone(),
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

/// Forget the picked folder, keeping every other choice — the
/// consolidation's closing step: once the legacy pick's files are copied
/// into the documents home, the resolver's own default *is* the home.
pub(crate) fn clear_trades_dir(path: &Path) {
    let mut state = load(path);
    state.trades_dir = None;
    save(path, &state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choices_round_trip_through_disk() {
        let path = scratch_path();
        assert_eq!(
            load(&path),
            PaperState::default(),
            "missing file, no choices"
        );
        let state = PaperState {
            trades_dir: Some("D:/trading/journals".to_owned()),
            cmd_trading_enabled: Some(false),
            cmd_buy_modifier: Some("alt".to_owned()),
            cmd_sell_modifier: Some("ctrl".to_owned()),
        };
        save(&path, &state);
        assert_eq!(load(&path), state);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn clearing_the_folder_keeps_the_other_choices() {
        let path = scratch_path();
        save(
            &path,
            &PaperState {
                trades_dir: Some("D:/trading/journals".to_owned()),
                cmd_trading_enabled: Some(false),
                ..PaperState::default()
            },
        );
        clear_trades_dir(&path);
        let state = load(&path);
        assert_eq!(state.trades_dir, None, "the pick is forgotten");
        assert_eq!(
            state.cmd_trading_enabled,
            Some(false),
            "an unrelated choice survives"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_legacy_file_with_only_the_folder_still_loads() {
        let path = scratch_path();
        std::fs::write(&path, "version = 1\ntrades_dir = \"D:/journals\"\n").unwrap();
        let state = load(&path);
        assert_eq!(state.trades_dir.as_deref(), Some("D:/journals"));
        assert_eq!(state.cmd_trading_enabled, None, "absent keys stay None");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unknown_versions_and_garbage_degrade_to_no_choice() {
        let path = scratch_path();
        std::fs::write(&path, "version = 99\ntrades_dir = \"x\"\n").unwrap();
        assert_eq!(load(&path), PaperState::default(), "unknown version");
        std::fs::write(&path, "not even toml [").unwrap();
        assert_eq!(load(&path), PaperState::default(), "garbage");
        std::fs::write(&path, "version = 1\ntrades_dir = \"  \"\n").unwrap();
        assert_eq!(load(&path).trades_dir, None, "a blank choice is no choice");
        std::fs::remove_file(&path).ok();
    }
}
