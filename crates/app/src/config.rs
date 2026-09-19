//! Feed & asset configuration: the document is `quantick_stores::config`.
//!
//! Resolution order (see [`load`]): the `QUANTICK_CONFIG` path the launch
//! composition hands in ([`crate::launch::LaunchConfig`]), then
//! `quantick.toml` in the working directory, then the built-in default
//! embedded at compile time. This module reads no environment.

use std::path::Path;

pub use quantick_stores::config::*;

/// Load the config, following the resolution order documented on this module.
///
/// Returns the config together with where it came from. An external file
/// (the explicit path or the working-directory file) that is present but
/// unreadable, unparseable, or invalid is a hard error; the embedded default
/// is only used when no external file exists.
///
/// `explicit` is the operator's `QUANTICK_CONFIG`, read once by the launch
/// composition.
///
/// # Errors
///
/// Returns [`ConfigError`] when a present external file cannot be read, parsed,
/// or validated. The embedded default is validated in tests, so it never errors.
pub fn load(explicit: Option<&Path>) -> Result<(AppConfig, ConfigSource), ConfigError> {
    load_from(
        explicit.map(Path::to_path_buf),
        &crate::symbols_file::default_path(),
    )
}
