//! The script library: where `.pine` sources come from.
//!
//! Two origins, one list: **embedded** starter scripts compiled into the
//! binary (`include_str!`, the `EMBEDDED_DEFAULT` pattern from `config.rs`)
//! so the feature works with an empty folder, and **files** found by
//! scanning the indicators directory. The directory is created on first
//! scan so "drop a .pine file here" has a here.
//!
//! The directory resolves from `QUANTICK_INDICATORS_DIR`, falling back to
//! `./indicators` beside the working directory — the same precedence spirit
//! as `QUANTICK_CONFIG`. Scanning is deliberately at-startup-only for now;
//! the hot-reload mtime poll is the M4 milestone.

use std::path::PathBuf;

/// Environment override for the scripts folder.
pub(crate) const INDICATORS_DIR_ENV: &str = "QUANTICK_INDICATORS_DIR";
/// Default scripts folder, relative to the working directory.
const DEFAULT_DIR: &str = "indicators";

/// Embedded starter scripts: (menu name, source). Each doubles as a
/// conformance fixture in the pine crate's corpus.
pub(crate) const EMBEDDED_SCRIPTS: &[(&str, &str)] = &[
    ("ema.pine", include_str!("../../scripts/ema.pine")),
    ("cvd.pine", include_str!("../../scripts/cvd.pine")),
    (
        "delta_histogram.pine",
        include_str!("../../scripts/delta_histogram.pine"),
    ),
    (
        "vwap_cumulative.pine",
        include_str!("../../scripts/vwap_cumulative.pine"),
    ),
    ("zigzag.pine", include_str!("../../scripts/zigzag.pine")),
    (
        "range_box.pine",
        include_str!("../../scripts/range_box.pine"),
    ),
];

/// One loadable script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptEntry {
    /// Menu label (the file name).
    pub name: String,
    /// Where the text lives.
    pub origin: ScriptOrigin,
}

/// Where a script's text comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScriptOrigin {
    /// Compiled into the binary.
    Embedded(&'static str),
    /// A `.pine` file in the indicators directory.
    File(PathBuf),
}

/// The scanned library.
#[derive(Debug, Default)]
pub(crate) struct ScriptLibrary {
    entries: Vec<ScriptEntry>,
}

impl ScriptLibrary {
    /// Embedded scripts plus a scan of the indicators directory (created on
    /// first run). File-system problems degrade to embedded-only — an
    /// unreadable folder must not take the feature down.
    pub(crate) fn scan() -> Self {
        let mut entries: Vec<ScriptEntry> = EMBEDDED_SCRIPTS
            .iter()
            .map(|(name, source)| ScriptEntry {
                name: (*name).to_owned(),
                origin: ScriptOrigin::Embedded(source),
            })
            .collect();

        let dir = std::env::var(INDICATORS_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_DIR));
        if let Err(error) = std::fs::create_dir_all(&dir) {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_DIR_UNAVAILABLE",
                dir = %dir.display(),
                error = %error,
                action = "embedded_scripts_only",
                "cannot create the indicators directory; using embedded scripts only"
            );
            return Self { entries };
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|read| {
                read.filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|ext| ext == "pine"))
                    .collect()
            })
            .unwrap_or_default();
        // Deterministic menu order whatever the OS returns.
        files.sort();
        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            // A file shadows an embedded script of the same name — the user's
            // copy wins, and the menu never shows two identical labels.
            entries.retain(|e| e.name != name);
            entries.push(ScriptEntry {
                name,
                origin: ScriptOrigin::File(path),
            });
        }
        Self { entries }
    }

    /// Every loadable script, embedded first, then files (sorted).
    pub(crate) fn entries(&self) -> &[ScriptEntry] {
        &self.entries
    }

    /// The file behind an entry (embedded scripts have none) plus its
    /// modification time — what the hot-reload poll compares.
    pub(crate) fn file_info(
        &self,
        index: usize,
    ) -> Option<(std::path::PathBuf, std::time::SystemTime)> {
        match &self.entries.get(index)?.origin {
            ScriptOrigin::Embedded(_) => None,
            ScriptOrigin::File(path) => {
                let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
                Some((path.clone(), mtime))
            }
        }
    }

    /// Read a script's text. Embedded text is free; a file read can fail and
    /// the caller surfaces that as the slot's error.
    pub(crate) fn read(&self, index: usize) -> Option<Result<String, String>> {
        let entry = self.entries.get(index)?;
        Some(match &entry.origin {
            ScriptOrigin::Embedded(source) => Ok((*source).to_owned()),
            ScriptOrigin::File(path) => std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_scripts_compile_against_the_dialect() {
        for (name, source) in EMBEDDED_SCRIPTS {
            if let Err(errors) = quantick_pine::compile(source, name) {
                let rendered: Vec<String> = errors.iter().map(|e| e.render(name, source)).collect();
                panic!("embedded {name} must compile:\n{}", rendered.join("\n"));
            }
        }
    }

    #[test]
    fn embedded_names_are_unique() {
        let mut names: Vec<&str> = EMBEDDED_SCRIPTS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EMBEDDED_SCRIPTS.len());
    }
}
