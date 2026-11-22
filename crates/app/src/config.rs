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
pub(crate) fn load(
    explicit: Option<&Path>,
    lookup: impl FnOnce(&str) -> Option<String>,
) -> Result<
    (
        AppConfig,
        ConfigSource,
        crate::surfaces::strategy_describe::provider::Worker,
    ),
    ConfigError,
> {
    use crate::surfaces::strategy_describe::provider::{Settings, Worker};
    let (text, source) = read_config_source(explicit.map(Path::to_path_buf))?;
    let mut table: toml::Table = toml::from_str(&text).map_err(|error| ConfigError::Parse {
        source: source.clone(),
        message: format!(
            "invalid TOML at byte {:?}",
            error.span().map(|span| span.start)
        ),
    })?;
    let invalid = |message: &str| ConfigError::Invalid {
        source: source.clone(),
        message: message.into(),
    };
    let settings = table
        .remove("strategy_describe")
        .map(toml::Value::try_into::<Settings>)
        .transpose()
        .map_err(|_| invalid("invalid strategy_describe settings"))?
        .unwrap_or_default();
    let worker = Worker::configured(settings, lookup).map_err(invalid)?;
    let public_config = toml::to_string(&table).map_err(|_| invalid("invalid chart settings"))?;
    let config = parse_config_source(
        &public_config,
        &source,
        &crate::symbols_file::default_path(),
    )?;
    Ok((config, source, worker))
}
