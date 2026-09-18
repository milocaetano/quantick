//! Feed & asset configuration: the document is `quantick_stores::config`;
//! this is where the environment reaches it.
//!
//! Resolution order (see [`load`]): the `QUANTICK_CONFIG` env path, then
//! `quantick.toml` in the working directory, then the built-in default
//! embedded at compile time. The two startup-selection variables are read
//! here too and applied through the store's deterministic selection.

use std::path::PathBuf;

pub use quantick_stores::config::*;

fn optional_env(variable: &'static str) -> Result<Option<String>, StartupSelectionError> {
    match std::env::var(variable) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(StartupSelectionError::NonUnicode { variable })
        }
    }
}

/// Apply [`DEFAULT_FEED_ENV`] and [`DEFAULT_SYMBOL_ENV`] to a loaded config.
///
/// This thin environment adapter delegates all selection behavior to
/// [`apply_startup_selection`], which stays deterministic and can be tested
/// without mutating process-global environment variables.
///
/// # Errors
///
/// Returns [`StartupSelectionError`] for non-Unicode environment values or a
/// feed/symbol pair that does not resolve against `config`.
pub fn apply_startup_selection_from_env(
    config: &mut AppConfig,
) -> Result<(), StartupSelectionError> {
    let feed = optional_env(DEFAULT_FEED_ENV)?;
    let symbol = optional_env(DEFAULT_SYMBOL_ENV)?;
    apply_startup_selection(config, feed.as_deref(), symbol.as_deref())
}

/// Whether either startup-selection env var named the market this run opens
/// on.
///
/// The saved workspace ([`crate::ui_state`]) otherwise decides it, and this is
/// how the two are ordered: an env var is an explicit request for this one
/// run, so a validation run pinned to `QUANTICK_DEFAULT_SYMBOL` must not find
/// itself on yesterday's cockpit instead.
#[must_use]
pub fn startup_selection_came_from_env() -> bool {
    std::env::var_os(DEFAULT_FEED_ENV).is_some() || std::env::var_os(DEFAULT_SYMBOL_ENV).is_some()
}

/// Load the config, following the resolution order documented on this module.
///
/// Returns the config together with where it came from. An external file (env
/// path or working-directory file) that is present but unreadable, unparseable,
/// or invalid is a hard error; the embedded default is only used when no external
/// file exists.
///
/// # Errors
///
/// Returns [`ConfigError`] when a present external file cannot be read, parsed,
/// or validated. The embedded default is validated in tests, so it never errors.
pub fn load() -> Result<(AppConfig, ConfigSource), ConfigError> {
    load_from(
        std::env::var_os(CONFIG_ENV).map(PathBuf::from),
        &crate::symbols_file::default_path(),
    )
}

crate::hooks::declare_hooks![
    "QUANTICK_CONFIG",
    "QUANTICK_DEFAULT_FEED",
    "QUANTICK_DEFAULT_SYMBOL"
];
