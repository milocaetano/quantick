//! Naming, tagging and deleting a session file — the ledger's three writes.
//!
//! A name and tags are append-only comment lines in the session's own file
//! (`quantick_sim::history` owns their spelling), so an edit never rewrites a
//! trade row and a crash mid-edit costs at most that one line. A delete moves
//! the file into `<trades folder>/.deleted/<symbol>/`, which no reader of the
//! history lists, and refuses any file this app is still writing.
//!
//! Every edit first finds the file the filesystem itself would open for the
//! key and acts on — and compares — that file's own path. A case-insensitive
//! volume opens `btcusdt/…` as `BTCUSDT/…`, and an 8.3 short name as its long
//! one, so comparing the caller's spelling would let a second spelling of the
//! live session past the one refusal that protects it.

use std::path::{Path, PathBuf};

use quantick_sim::history::{self, TradeRef};

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
    /// The session holds no such trade.
    TradeNotFound(SessionKey, TradeRef),
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
            Self::TradeNotFound(key, trade) => write!(
                f,
                "session `{key}` holds no trade opened at {} and closed at {}{}",
                trade.opened_ms,
                trade.closed_ms,
                match trade.nth {
                    0 => String::new(),
                    nth => format!(" after {nth} with the same times"),
                }
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
        trade: TradeRef,
        tags: Vec<String>,
    },
    /// Move a session out of the history.
    Delete { session: SessionKey },
}

/// What an edit did. Its session is the file's own spelling of the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JournalEditOutcome {
    Renamed {
        session: SessionKey,
        name: Option<String>,
    },
    Tagged {
        session: SessionKey,
        trade: TradeRef,
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

/// The toast an edit leaves, done or refused.
#[must_use]
pub fn edit_toast(result: &Result<JournalEditOutcome, SessionEditError>) -> String {
    match result {
        Ok(done) => done.sentence(),
        Err(refused) => format!("Trades: {refused}."),
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
        JournalEdit::Rename { session, name } => {
            let (session, path) = resolve(&scope.dir, session)?;
            Ok(JournalEditOutcome::Renamed {
                name: rename_file(&path, &session, name)?,
                session,
            })
        }
        JournalEdit::Tag {
            session,
            trade,
            tags,
        } => {
            let (session, path) = resolve(&scope.dir, session)?;
            let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
            Ok(JournalEditOutcome::Tagged {
                tags: tag_file(&path, &session, *trade, &tags)?,
                trade: *trade,
                session,
            })
        }
        JournalEdit::Delete { session } => {
            let (session, path) = resolve(&scope.dir, session)?;
            Ok(JournalEditOutcome::Deleted {
                moved_to: delete_file(&scope.dir, &path, &session, &scope.live)?,
                session,
            })
        }
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
    let (key, path) = resolve(dir, key)?;
    rename_file(&path, &key, name)
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
    trade: TradeRef,
    tags: &[&str],
) -> Result<Vec<String>, SessionEditError> {
    let (key, path) = resolve(dir, key)?;
    tag_file(&path, &key, trade, tags)
}

/// Move a session file out of the history, into [`DELETED_DIR`]. `live` is
/// every file this app is still journaling to; each is refused, however
/// the key spells it. Answers where the file went.
///
/// # Errors
///
/// The live session, a missing file, a refused store, or a failed move.
pub fn delete_session(
    dir: &Path,
    key: &SessionKey,
    live: &[PathBuf],
) -> Result<PathBuf, SessionEditError> {
    let (key, path) = resolve(dir, key)?;
    delete_file(dir, &path, &key, live)
}

/// The file `key` opens under `dir`, as the filesystem names it: its key
/// spelled the way the folder lists it, and its canonical path.
fn resolve(dir: &Path, key: &SessionKey) -> Result<(SessionKey, PathBuf), SessionEditError> {
    let path = key.path(dir);
    if !path.is_file() {
        return Err(SessionEditError::NotFound(key.clone()));
    }
    let io = |error: std::io::Error| SessionEditError::Io(error.to_string());
    let path = std::fs::canonicalize(path).map_err(io)?;
    let root = std::fs::canonicalize(dir).map_err(io)?;
    // A link out of the trades folder resolves to no key of it, and is no
    // session of it either.
    let own = SessionKey::of(&root, &path)
        .ok_or_else(|| SessionEditError::InvalidKey(key.to_string()))?;
    Ok((own, path))
}

fn rename_file(
    path: &Path,
    key: &SessionKey,
    name: &str,
) -> Result<Option<String>, SessionEditError> {
    read_session(path, key)?;
    append(path, &history::write_name(name))?;
    let kept = history::clean_session_name(name);
    Ok((!kept.is_empty()).then_some(kept))
}

fn tag_file(
    path: &Path,
    key: &SessionKey,
    trade: TradeRef,
    tags: &[&str],
) -> Result<Vec<String>, SessionEditError> {
    let tags =
        history::normalize_tags(tags.iter().copied()).map_err(SessionEditError::InvalidTag)?;
    if !read_session(path, key)?.refs().contains(&trade) {
        return Err(SessionEditError::TradeNotFound(key.clone(), trade));
    }
    append(path, &history::write_tags(trade, &tags))?;
    Ok(tags)
}

fn delete_file(
    dir: &Path,
    path: &Path,
    key: &SessionKey,
    live: &[PathBuf],
) -> Result<PathBuf, SessionEditError> {
    // Canonical against canonical: the spelling a tab journals under and
    // the spelling a caller sent may differ and still be one file.
    if live
        .iter()
        .any(|live| std::fs::canonicalize(live).is_ok_and(|live| same_file(&live, path)))
    {
        return Err(SessionEditError::LiveSession(key.clone()));
    }
    guard(path)?;
    let (folder, file) = key.parts();
    let bin = dir.join(DELETED_DIR).join(folder);
    std::fs::create_dir_all(&bin).map_err(|error| SessionEditError::Io(error.to_string()))?;
    let target = free_name(&bin, file);
    std::fs::rename(path, &target).map_err(|error| SessionEditError::Io(error.to_string()))?;
    Ok(target)
}

/// The file parsed, or why it cannot be edited.
fn read_session(path: &Path, key: &SessionKey) -> Result<history::TradeHistory, SessionEditError> {
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

/// Whether two canonical paths are one file: equal, or on Windows equal but
/// for ASCII case, which is how its volumes compare names.
fn same_file(a: &Path, b: &Path) -> bool {
    a == b || (cfg!(windows) && a.as_os_str().eq_ignore_ascii_case(b.as_os_str()))
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
