//! The script library: where `.pine` sources come from.
//!
//! Two origins, one list: **embedded** starter scripts compiled into the
//! binary (`include_str!`, the `EMBEDDED_DEFAULT` pattern from `config.rs`)
//! so the feature works with an empty folder, and **files** found by
//! scanning the indicators directory. The directory is created on first
//! scan so "drop a .pine file here" has a here.
//!
//! The window resolves the directory and hands it in; nothing here reads the
//! environment. The scan itself runs once at startup — the menu is fixed
//! for the session — but a loaded file-backed script follows its file: the
//! window polls mtimes on a debounce and reloads on a save.

use std::path::{Path, PathBuf};

mod catalog;
mod persistence;
pub use catalog::{EMBEDDED_SCRIPTS, POPULAR_INDICATORS, ScriptMetadata, script_title};
pub const MAX_SOURCE_BYTES: usize = 256 * 1024;

/// One loadable script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptEntry {
    /// Menu label (the file name).
    pub name: String,
    /// Where the text lives.
    pub origin: ScriptOrigin,
}

/// Where a script's text comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptOrigin {
    /// Compiled into the binary.
    Embedded(&'static str),
    /// A `.pine` file in the indicators directory.
    File(PathBuf),
}

/// A script source owned by the store. The file name is its stable identity.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScriptDocument {
    pub id: String,
    pub title: String,
    pub source: String,
    /// Embedded originals are read-only; editing starts from a named copy.
    pub read_only: bool,
}

/// Library operations shared by the editor and its control adapter.
/// Calls may perform file I/O and belong to explicit library actions, never paint.
pub trait ScriptLibraryPort {
    fn list(&self) -> &[ScriptEntry];
    fn read(&self, id: &str) -> Result<ScriptDocument, String>;
    fn copy(&mut self, id: &str, new_id: &str) -> Result<ScriptDocument, String>;
    fn save(&mut self, document: &ScriptDocument) -> Result<(), String>;
}

/// Value requests from either the editor surface or an admitted control call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScriptEditorAction {
    Inspect,
    Open { id: String },
    Copy { id: String, new_id: String },
    ReplaceSource { source: String },
    Save,
    Close,
}

/// The scanned library.
#[derive(Debug, Default)]
pub struct ScriptLibrary {
    entries: Vec<ScriptEntry>,
    directory: PathBuf,
}

impl ScriptLibrary {
    /// Embedded scripts plus a scan of the indicators directory `dir`
    /// (created on first run). File-system problems degrade to
    /// embedded-only — an unreadable folder must not take the feature down.
    pub fn open(dir: &Path) -> Self {
        if let Err(error) = std::fs::create_dir_all(dir) {
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
                directory: dir.to_owned(),
            };
        }
        Self::scan_dir(dir)
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
    /// Split from [`ScriptLibrary::open`] so the file half is testable
    /// without mutating process-global env — the same shape
    /// `quantick_replay::library::scan(root)` already uses.
    pub fn scan_dir(dir: &Path) -> Self {
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
                return Self {
                    entries,
                    directory: dir.to_owned(),
                };
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
            // A bundled identity stays immutable even if a same-named file is
            // dropped into the user directory. Editable copies need distinct ids.
            if entries
                .iter()
                .any(|entry| entry.name.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            entries.push(ScriptEntry {
                name,
                origin: ScriptOrigin::File(path),
            });
        }
        Self {
            entries,
            directory: dir.to_owned(),
        }
    }

    /// Every loadable script, embedded first, then files (sorted).
    pub fn entries(&self) -> &[ScriptEntry] {
        &self.entries
    }

    /// The file behind an entry (embedded scripts have none) plus its
    /// modification time — what the hot-reload poll compares.
    pub fn file_info(&self, index: usize) -> Option<(std::path::PathBuf, std::time::SystemTime)> {
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
    pub fn read(&self, index: usize) -> Option<Result<String, String>> {
        let entry = self.entries.get(index)?;
        Some(match &entry.origin {
            ScriptOrigin::Embedded(source) => Ok((*source).to_owned()),
            ScriptOrigin::File(path) => persistence::read_source(path),
        })
    }
}

impl ScriptLibraryPort for ScriptLibrary {
    fn list(&self) -> &[ScriptEntry] {
        self.entries()
    }

    fn read(&self, id: &str) -> Result<ScriptDocument, String> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.name == id)
            .ok_or_else(|| format!("unknown script: {id}"))?;
        let entry = &self.entries[index];
        Ok(ScriptDocument {
            id: entry.name.clone(),
            title: script_title(&entry.name).to_owned(),
            source: self.read(index).expect("the entry exists")?,
            read_only: matches!(entry.origin, ScriptOrigin::Embedded(_)),
        })
    }

    fn copy(&mut self, id: &str, new_id: &str) -> Result<ScriptDocument, String> {
        self.copy_document(id, new_id)
    }

    fn save(&mut self, document: &ScriptDocument) -> Result<(), String> {
        self.save_document(document)
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
            include_str!("../../pine/tests/corpus/ok/copilot.pine"),
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
            include_str!("../../pine/tests/corpus/ok/force_bar.pine"),
            "sync crates/pine/tests/corpus/ok/force_bar.pine with crates/app/scripts/force_bar.pine"
        );
    }

    /// Same pin, same reason: `exhaustion_reversal_semantics.rs` proves the
    /// force-bar / give-back pairs against the corpus copy, and a drift would
    /// leave a suite of passing tests describing a script nobody ships.
    #[test]
    fn the_embedded_exhaustion_reversal_matches_its_semantics_fixture() {
        assert_eq!(
            embedded("exhaustion_reversal.pine"),
            include_str!("../../pine/tests/corpus/ok/exhaustion_reversal.pine"),
            "sync crates/pine/tests/corpus/ok/exhaustion_reversal.pine with \
             crates/app/scripts/exhaustion_reversal.pine"
        );
    }

    /// A private scratch folder that removes itself when the test ends.
    fn scratch(tag: &str) -> crate::scratch::ScratchDir {
        crate::scratch::ScratchDir::new(tag)
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
    fn a_user_file_cannot_shadow_the_embedded_script_of_the_same_name() {
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
            matches!(matching[0].origin, ScriptOrigin::Embedded(_)),
            "built-in identities remain immutable"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_directory_degrades_to_embedded_only() {
        // A folder that is not there: inside this thread's scratch directory,
        // so nothing has to create it and nothing is left behind.
        let missing = crate::scratch::thread_dir("script-library").join("missing");
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
