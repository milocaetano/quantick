//! The strategy library: the `.pine` files in one folder.
//!
//! The window resolves the folder (`QUANTICK_STRATEGIES_DIR`, read at
//! launch, else [`DEFAULT_DIR`] beside the working directory) and hands it
//! in. A script is named by its bare file name, never a path, so a caller
//! cannot read outside the folder.

use std::fmt;
use std::path::{Path, PathBuf};

/// The folder the library opens when the launch named none, relative to the
/// working directory.
pub const DEFAULT_DIR: &str = "strategies";

/// The largest script the library reads. A strategy script is tens of lines;
/// the bound keeps a stray file from being compiled whole.
pub const SCRIPT_MAX_BYTES: u64 = 64 * 1024;

const EXTENSION: &str = "pine";

/// Why the library could not answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibraryError {
    /// Not a bare `<name>.pine` file name.
    BadName(String),
    /// No such script in the folder.
    Missing(String),
    /// Larger than [`SCRIPT_MAX_BYTES`].
    TooLarge(String),
    /// The folder or the file could not be read; the cause as the OS said it.
    Unreadable { path: String, cause: String },
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName(name) => write!(
                f,
                "`{name}` is not a script name: give a bare file name ending in .pine"
            ),
            Self::Missing(name) => write!(f, "the strategy library has no `{name}`"),
            Self::TooLarge(name) => {
                write!(f, "`{name}` is larger than {SCRIPT_MAX_BYTES} bytes")
            }
            Self::Unreadable { path, cause } => write!(f, "cannot read {path}: {cause}"),
        }
    }
}

impl std::error::Error for LibraryError {}

/// The `.pine` files in one folder.
#[derive(Debug, Clone)]
pub struct Library {
    dir: PathBuf,
}

impl Library {
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder `configured` names, or [`DEFAULT_DIR`].
    #[must_use]
    pub fn resolve(configured: Option<&str>) -> Self {
        Self::new(configured.unwrap_or(DEFAULT_DIR))
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every script file name in the folder, sorted.
    ///
    /// # Errors
    ///
    /// The folder cannot be read.
    pub fn names(&self) -> Result<Vec<String>, LibraryError> {
        let unreadable = |cause: std::io::Error| LibraryError::Unreadable {
            path: self.dir.display().to_string(),
            cause: cause.to_string(),
        };
        let mut names: Vec<String> = std::fs::read_dir(&self.dir)
            .map_err(unreadable)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| bare_script_name(name))
            .collect();
        names.sort();
        Ok(names)
    }

    /// The source of script `name`.
    ///
    /// # Errors
    ///
    /// A name that is not a bare `.pine` file name, a missing or oversized
    /// file, or one that cannot be read as text.
    pub fn read(&self, name: &str) -> Result<String, LibraryError> {
        if !bare_script_name(name) {
            return Err(LibraryError::BadName(name.to_owned()));
        }
        let path = self.dir.join(name);
        let unreadable = |cause: std::io::Error| LibraryError::Unreadable {
            path: path.display().to_string(),
            cause: cause.to_string(),
        };
        let meta = match std::fs::metadata(&path) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => return Err(LibraryError::Missing(name.to_owned())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(LibraryError::Missing(name.to_owned()));
            }
            Err(error) => return Err(unreadable(error)),
        };
        if meta.len() > SCRIPT_MAX_BYTES {
            return Err(LibraryError::TooLarge(name.to_owned()));
        }
        std::fs::read_to_string(&path).map_err(unreadable)
    }
}

/// `<stem>.pine` with a non-empty stem, no path, drive or hidden-file dot.
fn bare_script_name(name: &str) -> bool {
    !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && name
            .strip_suffix(EXTENSION)
            .is_some_and(|stem| stem.len() > 1 && stem.ends_with('.'))
}
