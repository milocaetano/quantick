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

    /// The embedded scripts alone, over `dir`: for a caller that found the
    /// folder unreadable and reports that itself.
    pub fn bundled(dir: &Path) -> Self {
        Self {
            entries: Self::embedded(),
            directory: dir.to_owned(),
        }
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

    /// The index of the entry named `name`, the key every by-name caller uses.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|entry| entry.name == name)
    }

    /// Keep `source` as the new library file `name`: the strategy desk's
    /// "save as" of a running script.
    ///
    /// # Errors
    ///
    /// A name that is not `letters-digits_.pine`, one already taken, or a
    /// write that failed.
    pub fn save_as(&mut self, name: &str, source: &str) -> Result<ScriptDocument, String> {
        self.create_document(name, source)
    }

    /// What a saved entry naming `name` loads.
    pub fn load(&self, name: &str) -> LoadedScript {
        let loaded = self.index_of(name).and_then(|index| self.load_at(index));
        loaded.unwrap_or_else(|| LoadedScript::Missing {
            message: format!("{name} is not in the script library"),
        })
    }

    /// What the entry at `index` loads, as a menu add picks it; `None` when
    /// the library has no entry there.
    pub fn load_at(&self, index: usize) -> Option<LoadedScript> {
        Some(match self.read(index)? {
            Ok(text) => LoadedScript::Read { index, text },
            Err(message) => LoadedScript::Unreadable { index, message },
        })
    }
}

/// [`ScriptLibrary::load`]: each case carries the message its error slot
/// shows, and the library index the hot-reload watch keys on when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadedScript {
    Read {
        index: usize,
        text: String,
    },
    /// Listed, but its file could not be read.
    Unreadable {
        index: usize,
        message: String,
    },
    /// The library no longer has it.
    Missing {
        message: String,
    },
}

impl LoadedScript {
    /// The library index, for every case that has one.
    pub fn index(&self) -> Option<usize> {
        match self {
            Self::Read { index, .. } | Self::Unreadable { index, .. } => Some(*index),
            Self::Missing { .. } => None,
        }
    }
}

impl ScriptLibraryPort for ScriptLibrary {
    fn list(&self) -> &[ScriptEntry] {
        self.entries()
    }

    fn read(&self, id: &str) -> Result<ScriptDocument, String> {
        let index = self
            .index_of(id)
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
#[path = "script_library_tests.rs"]
mod tests;
