//! Naming, tagging and deleting a session file — the ledger's three writes.
//!
//! A name and tags are append-only comment lines in the session's own file
//! (`quantick_sim::history` owns their spelling), so an edit never rewrites a
//! trade row and a crash mid-edit costs at most that one line. A delete moves
//! the file into `<trades folder>/.deleted/<symbol>/`, which no reader of the
//! history lists, and refuses any file this app is still writing.

use std::path::{Path, PathBuf};

use quantick_sim::history::{self, TradeTimes};

use super::sessions::SessionKey;

/// The folder deleted sessions move into, beside the symbol folders. Its dot
/// keeps it out of every listing of the history.
pub const DELETED_DIR: &str = ".deleted";

/// Why an edit was refused, with a stable [`Self::code`] a caller branches on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEditError {
    /// The key is not `<symbol folder>/<file>.csv`.
    InvalidKey(String),
    /// No such session file.
    NotFound(SessionKey),
    /// The file this app is still journaling to; it cannot be deleted.
    LiveSession(SessionKey),
    /// The session holds no trade with these times.
    TradeNotFound(SessionKey, TradeTimes),
    /// A tag that is not one tag, or too many of them.
    InvalidTag(String),
    /// The file is not a readable quantick-trades file.
    Unreadable(SessionKey, String),
    /// This session writes no store (another instance owns them).
    WritesRefused(String),
    /// The filesystem refused the write.
    Io(String),
}

impl SessionEditError {
    /// A stable token for the refusal.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidKey(_) => "invalid_session",
            Self::NotFound(_) => "session_not_found",
            Self::LiveSession(_) => "live_session",
            Self::TradeNotFound(..) => "trade_not_found",
            Self::InvalidTag(_) => "invalid_tag",
            Self::Unreadable(..) => "session_unreadable",
            Self::WritesRefused(_) => "writes_refused",
            Self::Io(_) => "write_failed",
        }
    }

    /// Whether the same call may succeed later without changing its input.
    #[must_use]
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Io(_))
    }
}

impl std::fmt::Display for SessionEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidKey(key) => write!(
                f,
                "`{key}` is not a session - send `<symbol folder>/<file>.csv` as listed"
            ),
            Self::NotFound(key) => write!(f, "no session file `{key}`"),
            Self::LiveSession(key) => write!(
                f,
                "`{key}` is the session being traded now - it cannot be deleted while it is written"
            ),
            Self::TradeNotFound(key, (opened, closed)) => write!(
                f,
                "session `{key}` holds no trade opened at {opened} and closed at {closed}"
            ),
            Self::InvalidTag(message) => f.write_str(message),
            Self::Unreadable(key, message) => {
                write!(
                    f,
                    "session `{key}` is not a readable trades file: {message}"
                )
            }
            Self::WritesRefused(reason) => write!(f, "this session writes no store: {reason}"),
            Self::Io(message) => write!(f, "the session file could not be written: {message}"),
        }
    }
}

/// Where the journal lives and what this app is still writing into it — what
/// every edit and listing needs from its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalScope {
    /// The trades folder.
    pub dir: PathBuf,
    /// Every session file a tab is still journaling to.
    pub live: Vec<PathBuf>,
}

/// One edit, as the ledger's gestures and the control plane both ask for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalEdit {
    /// Name a session; an empty name clears it.
    Rename { session: SessionKey, name: String },
    /// Set one trade's tags; none clears them.
    Tag {
        session: SessionKey,
        times: TradeTimes,
        tags: Vec<String>,
    },
    /// Move a session out of the history.
    Delete { session: SessionKey },
}

/// What an edit did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalEditOutcome {
    Renamed {
        session: SessionKey,
        name: Option<String>,
    },
    Tagged {
        session: SessionKey,
        times: TradeTimes,
        tags: Vec<String>,
    },
    Deleted {
        session: SessionKey,
        /// Where the file went.
        moved_to: PathBuf,
    },
}

impl JournalEditOutcome {
    /// The session the edit acted on.
    #[must_use]
    pub fn session(&self) -> &SessionKey {
        match self {
            Self::Renamed { session, .. }
            | Self::Tagged { session, .. }
            | Self::Deleted { session, .. } => session,
        }
    }

    /// The acknowledgement a trader reads.
    #[must_use]
    pub fn sentence(&self) -> String {
        match self {
            Self::Renamed {
                name: Some(name), ..
            } => format!("Session renamed \"{name}\"."),
            Self::Renamed { name: None, .. } => "Session name cleared.".to_owned(),
            Self::Tagged { tags, .. } if tags.is_empty() => "Trade tags cleared.".to_owned(),
            Self::Tagged { tags, .. } => format!("Trade tagged #{}.", tags.join(" #")),
            Self::Deleted { session, .. } => format!(
                "Session {} deleted - moved to {DELETED_DIR}, recoverable by hand.",
                session.parts().1
            ),
        }
    }
}

/// Apply one edit inside `scope` — the one path the ledger and the control
/// plane share, so they refuse the same things in the same words.
///
/// # Errors
///
/// Whatever the edit itself refuses.
pub fn apply_journal_edit(
    scope: &JournalScope,
    edit: &JournalEdit,
) -> Result<JournalEditOutcome, SessionEditError> {
    match edit {
        JournalEdit::Rename { session, name } => Ok(JournalEditOutcome::Renamed {
            session: session.clone(),
            name: rename_session(&scope.dir, session, name)?,
        }),
        JournalEdit::Tag {
            session,
            times,
            tags,
        } => {
            let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
            Ok(JournalEditOutcome::Tagged {
                session: session.clone(),
                times: *times,
                tags: tag_trade(&scope.dir, session, *times, &tags)?,
            })
        }
        JournalEdit::Delete { session } => Ok(JournalEditOutcome::Deleted {
            session: session.clone(),
            moved_to: delete_session(&scope.dir, session, &scope.live)?,
        }),
    }
}

/// Name the session (an empty name clears it). Answers the name as kept.
///
/// # Errors
///
/// A missing or unreadable file, a refused store, or a failed append.
pub fn rename_session(
    dir: &Path,
    key: &SessionKey,
    name: &str,
) -> Result<Option<String>, SessionEditError> {
    let path = key.path(dir);
    read_session(&path, key)?;
    append(&path, &history::write_name(name))?;
    let kept = history::clean_session_name(name);
    Ok((!kept.is_empty()).then_some(kept))
}

/// Set one trade's tags (none clears them). Answers the tags as kept.
///
/// # Errors
///
/// A tag that is not one, a trade the file does not hold, a missing or
/// unreadable file, a refused store, or a failed append.
pub fn tag_trade(
    dir: &Path,
    key: &SessionKey,
    times: TradeTimes,
    tags: &[&str],
) -> Result<Vec<String>, SessionEditError> {
    let tags =
        history::normalize_tags(tags.iter().copied()).map_err(SessionEditError::InvalidTag)?;
    let path = key.path(dir);
    let parsed = read_session(&path, key)?;
    if !parsed
        .trades
        .iter()
        .any(|trade| (trade.opened_ms, trade.closed_ms) == times)
    {
        return Err(SessionEditError::TradeNotFound(key.clone(), times));
    }
    append(&path, &history::write_tags(times, &tags))?;
    Ok(tags)
}

/// Move a session file out of the history, into [`DELETED_DIR`]. `live` is
/// every file this app is still journaling to; each is refused. Answers
/// where the file went.
///
/// # Errors
///
/// The live session, a missing file, a refused store, or a failed move.
pub fn delete_session(
    dir: &Path,
    key: &SessionKey,
    live: &[PathBuf],
) -> Result<PathBuf, SessionEditError> {
    if live
        .iter()
        .any(|path| SessionKey::of(dir, path).as_ref() == Some(key))
    {
        return Err(SessionEditError::LiveSession(key.clone()));
    }
    let path = key.path(dir);
    if !path.is_file() {
        return Err(SessionEditError::NotFound(key.clone()));
    }
    guard(&path)?;
    let (folder, file) = key.parts();
    let bin = dir.join(DELETED_DIR).join(folder);
    std::fs::create_dir_all(&bin).map_err(|error| SessionEditError::Io(error.to_string()))?;
    let target = free_name(&bin, file);
    std::fs::rename(&path, &target).map_err(|error| SessionEditError::Io(error.to_string()))?;
    Ok(target)
}

/// The file parsed, or why it cannot be edited.
fn read_session(path: &Path, key: &SessionKey) -> Result<history::TradeHistory, SessionEditError> {
    if !path.is_file() {
        return Err(SessionEditError::NotFound(key.clone()));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| SessionEditError::Unreadable(key.clone(), error.to_string()))?;
    history::parse(&text)
        .map_err(|error| SessionEditError::Unreadable(key.clone(), error.to_string()))
}

/// Append one comment line, starting it on a line of its own: a torn final
/// row (a crash mid-append) must not swallow the line written after it.
fn append(path: &Path, line: &str) -> Result<(), SessionEditError> {
    use std::io::{Read, Seek, SeekFrom, Write};
    guard(path)?;
    let io = |error: std::io::Error| SessionEditError::Io(error.to_string());
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .append(true)
        .open(path)
        .map_err(io)?;
    let length = file.metadata().map_err(io)?.len();
    let mut text = String::new();
    if length > 0 {
        let mut last = [0_u8; 1];
        file.seek(SeekFrom::End(-1)).map_err(io)?;
        file.read_exact(&mut last).map_err(io)?;
        if last[0] != b'\n' {
            text.push('\n');
        }
    }
    text.push_str(line);
    file.write_all(text.as_bytes()).map_err(io)
}

fn guard(path: &Path) -> Result<(), SessionEditError> {
    quantick_workspace::write_refusal::guard_write(path)
        .map_err(|refused| SessionEditError::WritesRefused(refused.to_string()))
}

/// `file` under `bin`, or `<stem>.copy-N.csv` beside it when a session of
/// that name was deleted before.
fn free_name(bin: &Path, file: &str) -> PathBuf {
    let plain = bin.join(file);
    if !plain.exists() {
        return plain;
    }
    let stem = file.strip_suffix(".csv").unwrap_or(file);
    (1..)
        .map(|copy| bin.join(format!("{stem}.copy-{copy}.csv")))
        .find(|candidate| !candidate.exists())
        .expect("an unbounded range finds a free name")
}
