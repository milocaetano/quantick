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
//! as `QUANTICK_CONFIG`. The scan itself runs once at startup — the menu is
//! fixed for the session — but a loaded file-backed script follows its file:
//! the app polls mtimes on a debounce and reloads on a save.

use std::path::{Path, PathBuf};

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
    ("copilot.pine", include_str!("../../scripts/copilot.pine")),
    (
        "force_bar.pine",
        include_str!("../../scripts/force_bar.pine"),
    ),
    (
        "exhaustion_reversal.pine",
        include_str!("../../scripts/exhaustion_reversal.pine"),
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
            return Self {
                entries: Self::embedded(),
            };
        }
        Self::scan_dir(&dir)
    }

    /// The embedded scripts, which every library starts from.
    fn embedded() -> Vec<ScriptEntry> {
        EMBEDDED_SCRIPTS
            .iter()
            .map(|(name, source)| ScriptEntry {
                name: (*name).to_owned(),
                origin: ScriptOrigin::Embedded(source),
            })
            .collect()
    }

    /// Embedded scripts plus the `.pine` files in `dir`.
    ///
    /// Split from [`ScriptLibrary::scan`] so the file half is testable
    /// without mutating process-global env — the same shape
    /// `quantick_replay::library::scan(root)` already uses.
    pub(crate) fn scan_dir(dir: &Path) -> Self {
        let mut entries = Self::embedded();
        let read = match std::fs::read_dir(dir) {
            Ok(read) => read,
            Err(error) => {
                // Its sibling `create_dir_all` failure logs; this one used to
                // swallow the error into an empty list.
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_DIR_UNREADABLE",
                    dir = %dir.display(),
                    error = %error,
                    action = "embedded_scripts_only",
                    "cannot read the indicators directory; using embedded scripts only"
                );
                return Self { entries };
            }
        };
        let mut files: Vec<PathBuf> = read
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
            .map(|entry| entry.path())
            .filter(|path| {
                // Lowercased, like the replay scan: on the primary dev
                // platform `EMA.PINE` is the same file to the OS and would
                // otherwise never appear in the menu. And directories named
                // `x.pine` are skipped above — a menu entry that can only
                // fail to load is worse than no entry.
                path.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pine"))
            })
            .collect();
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

    fn embedded(name: &str) -> &'static str {
        EMBEDDED_SCRIPTS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is embedded"))
            .1
    }

    /// The copilot ships twice: embedded here and as the pine crate's
    /// semantics fixture (`copilot_semantics.rs` proves the marker logic
    /// against the corpus copy). If the copies drift, the semantics test
    /// silently proves a script the app no longer embeds — so pin them.
    #[test]
    fn the_embedded_copilot_matches_its_semantics_fixture() {
        assert_eq!(
            embedded("copilot.pine"),
            include_str!("../../../pine/tests/corpus/ok/copilot.pine"),
            "sync crates/pine/tests/corpus/ok/copilot.pine with crates/app/scripts/copilot.pine"
        );
    }

    /// Same pin, same reason: `force_bar_semantics.rs` proves the three
    /// classifications against the corpus copy, and a drift would leave it
    /// proving a script nobody ships.
    #[test]
    fn the_embedded_force_bar_matches_its_semantics_fixture() {
        assert_eq!(
            embedded("force_bar.pine"),
            include_str!("../../../pine/tests/corpus/ok/force_bar.pine"),
            "sync crates/pine/tests/corpus/ok/force_bar.pine with crates/app/scripts/force_bar.pine"
        );
    }

    /// Same pin, same reason: `exhaustion_reversal_semantics.rs` proves the
    /// force-bar / give-back pairs against the corpus copy, and a drift would
    /// leave twelve passing tests describing a script nobody ships.
    #[test]
    fn the_embedded_exhaustion_reversal_matches_its_semantics_fixture() {
        assert_eq!(
            embedded("exhaustion_reversal.pine"),
            include_str!("../../../pine/tests/corpus/ok/exhaustion_reversal.pine"),
            "sync crates/pine/tests/corpus/ok/exhaustion_reversal.pine with \
             crates/app/scripts/exhaustion_reversal.pine"
        );
    }

    #[test]
    fn embedded_scripts_compile_against_the_dialect() {
        for (name, source) in EMBEDDED_SCRIPTS {
            if let Err(errors) = quantick_pine::compile(source, name) {
                let rendered: Vec<String> = errors.iter().map(|e| e.render(name, source)).collect();
                panic!("embedded {name} must compile:\n{}", rendered.join("\n"));
            }
        }
    }

    /// A private scratch folder, removed and recreated so two runs on one
    /// machine cannot collide (the shape `drawings::presets` already uses).
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quantick-script-library-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir is creatable");
        dir
    }

    #[test]
    fn a_pine_file_joins_the_menu_in_a_deterministic_order() {
        let dir = scratch("order");
        std::fs::write(
            dir.join("zeta.pine"),
            "//@version=5
plot(close)
",
        )
        .unwrap();
        std::fs::write(
            dir.join("alpha.pine"),
            "//@version=5
plot(open)
",
        )
        .unwrap();
        // Not a script: neither belongs in the menu.
        std::fs::write(dir.join("notes.txt"), "hello").unwrap();
        std::fs::create_dir_all(dir.join("folder.pine")).unwrap();

        let library = ScriptLibrary::scan_dir(&dir);
        let names: Vec<&str> = library.entries().iter().map(|e| e.name.as_str()).collect();
        let files: Vec<&&str> = names
            .iter()
            .filter(|n| **n == "alpha.pine" || **n == "zeta.pine")
            .collect();
        assert_eq!(
            files,
            vec![&"alpha.pine", &"zeta.pine"],
            "sorted, {names:?}"
        );
        assert!(!names.contains(&"notes.txt"), "only .pine files: {names:?}");
        assert!(
            !names.contains(&"folder.pine"),
            "a directory named like a script is not a script: {names:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_user_file_shadows_the_embedded_script_of_the_same_name() {
        let dir = scratch("shadow");
        let (embedded_name, _) = EMBEDDED_SCRIPTS[0];
        std::fs::write(
            dir.join(embedded_name),
            "//@version=5
plot(high)
",
        )
        .unwrap();

        let library = ScriptLibrary::scan_dir(&dir);
        let matching: Vec<&ScriptEntry> = library
            .entries()
            .iter()
            .filter(|e| e.name == embedded_name)
            .collect();
        assert_eq!(matching.len(), 1, "never two rows with the same label");
        assert!(
            matches!(matching[0].origin, ScriptOrigin::File(_)),
            "the user's copy wins"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_directory_degrades_to_embedded_only() {
        let missing = std::env::temp_dir().join(format!(
            "quantick-script-library-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&missing);
        let library = ScriptLibrary::scan_dir(&missing);
        assert_eq!(
            library.entries().len(),
            EMBEDDED_SCRIPTS.len(),
            "the feature survives a folder that is not there"
        );
    }

    #[test]
    fn a_file_that_disappears_reports_an_error_rather_than_nothing() {
        let dir = scratch("vanish");
        let path = dir.join("gone.pine");
        std::fs::write(
            &path,
            "//@version=5
plot(close)
",
        )
        .unwrap();
        let library = ScriptLibrary::scan_dir(&dir);
        let index = library
            .entries()
            .iter()
            .position(|e| e.name == "gone.pine")
            .expect("the file was scanned");
        std::fs::remove_file(&path).unwrap();
        assert!(
            matches!(library.read(index), Some(Err(_))),
            "a file that no longer reads is an error, not a silent skip"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_names_are_unique() {
        let mut names: Vec<&str> = EMBEDDED_SCRIPTS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EMBEDDED_SCRIPTS.len());
    }
}
