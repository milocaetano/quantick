//! Aggression-bubble presets: the document is `quantick_stores::bubble_presets`;
//! this is where the environment reaches it.

use std::path::{Path, PathBuf};

pub use quantick_stores::bubble_presets::*;

/// Path the panel reads from and writes to.
#[must_use]
pub fn presets_path() -> PathBuf {
    std::env::var_os(PRESETS_ENV).map_or_else(|| PathBuf::from(PRESETS_PATH), PathBuf::from)
}

/// Load presets, falling back to the embedded file.
///
/// Returns the presets, where they came from, and — when an external file
/// exists but could not be read or parsed — the error to surface. In that case
/// the returned presets are the embedded ones, and the source says so.
#[must_use]
pub fn load() -> (BubblePresetFile, PresetSource, Option<String>) {
    let (path, source): (PathBuf, fn(PathBuf) -> PresetSource) = match std::env::var_os(PRESETS_ENV)
    {
        Some(raw) => (PathBuf::from(raw), PresetSource::EnvPath),
        None => (PathBuf::from(PRESETS_PATH), PresetSource::WorkingDir),
    };
    if !Path::new(&path).is_file() {
        return (embedded(), PresetSource::Embedded, None);
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok(file) => {
                report_retired_keys(&text, &path);
                (file, source(path), None)
            }
            Err(message) => (
                embedded(),
                PresetSource::Embedded,
                Some(format!("{}: {message}", path.display())),
            ),
        },
        Err(error) => (
            embedded(),
            PresetSource::Embedded,
            Some(format!("cannot read {}: {error}", path.display())),
        ),
    }
}

/// Write `file` to [`presets_path`], returning where it landed.
///
/// # Errors
///
/// Returns a human-readable message when the file cannot be serialized or
/// written.
pub fn save(file: &BubblePresetFile) -> Result<PathBuf, String> {
    save_to(presets_path(), file)
}

crate::hooks::declare_hooks!["QUANTICK_BUBBLES"];
