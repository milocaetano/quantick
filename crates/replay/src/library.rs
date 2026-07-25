//! Scanning a folder of recorded sessions, and saying plainly what does not fit.
//!
//! The scan is deliberately shallow — the chosen folder and one level of
//! instrument sub-folders — so pointing quantick at a large directory can never
//! turn into a disk-wide crawl. Each candidate file is identified from its first
//! few kilobytes, so listing a folder of week-long recordings stays instant.
//!
//! Everything that is *not* a session is reported rather than skipped: a folder
//! that looks empty because every file was quietly ignored is the worst possible
//! answer to "why is my replay list blank?".

use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::format::{self, FormatError, FormatErrorKind, UtcOffset};
use crate::session::{self, SessionDate};

/// How much of a file is read to identify it. The metadata block and column
/// header live in the first few lines; a session's trades are never touched
/// while scanning.
const HEAD_BYTES: u64 = 16 * 1024;

/// How deep below the chosen folder the scan looks: the folder itself, plus one
/// level of instrument sub-folders.
const MAX_DEPTH: usize = 1;

/// A file in the folder that is not a playable session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemKind {
    /// The chosen path does not exist.
    FolderMissing,
    /// The chosen path exists but is a file, not a folder.
    NotAFolder,
    /// The folder could not be listed.
    FolderUnreadable,
    /// The folder holds no session files at all.
    NoSessions,
    /// A file that is not a `.csv`, so it was not considered.
    NotASessionFile,
    /// A raw MT5/order-flow recording that has to be converted first.
    NeedsImport,
    /// A `.csv` that could not be read from disk.
    Unreadable,
    /// A `.csv` whose header does not follow the replay format.
    BadFormat(FormatErrorKind),
}

impl ProblemKind {
    /// A short, stable token for logs and tests.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            ProblemKind::FolderMissing => "folder_missing",
            ProblemKind::NotAFolder => "not_a_folder",
            ProblemKind::FolderUnreadable => "folder_unreadable",
            ProblemKind::NoSessions => "no_sessions",
            ProblemKind::NotASessionFile => "not_a_session_file",
            ProblemKind::NeedsImport => "needs_import",
            ProblemKind::Unreadable => "unreadable",
            ProblemKind::BadFormat(kind) => kind.code(),
        }
    }

    /// What to change so the folder loads.
    #[must_use]
    pub fn advice(self) -> &'static str {
        match self {
            ProblemKind::FolderMissing => "Pick a folder that exists.",
            ProblemKind::NotAFolder => "Pick the folder that holds the session files, not a file.",
            ProblemKind::FolderUnreadable => {
                "Check quantick may list this folder, then open it again."
            }
            ProblemKind::NoSessions => {
                "Put one .csv per session day in the folder, or in a sub-folder named after the instrument."
            }
            ProblemKind::NotASessionFile => "Replay sessions are .csv files; this one was skipped.",
            ProblemKind::NeedsImport => {
                "Convert the recording first: cargo run -p quantick-replay --example import_mt5_ndjson -- --in <file> --out <replay folder>"
            }
            ProblemKind::Unreadable => "Check the file still exists and that quantick may read it.",
            ProblemKind::BadFormat(kind) => kind.advice(),
        }
    }

    /// Whether this stops the folder from being usable at all, as opposed to
    /// one file being left out.
    #[must_use]
    pub fn is_fatal(self) -> bool {
        matches!(
            self,
            ProblemKind::FolderMissing
                | ProblemKind::NotAFolder
                | ProblemKind::FolderUnreadable
                | ProblemKind::NoSessions
        )
    }
}

/// Something in the chosen folder that quantick could not play, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file this is about; `None` when it is about the folder itself.
    pub path: Option<PathBuf>,
    /// What kind of problem it is.
    pub kind: ProblemKind,
    /// What was found, in the folder's own words.
    pub detail: String,
}

impl Problem {
    /// The name to show: the file name, or the folder when there is no file.
    #[must_use]
    pub fn subject(&self) -> String {
        self.path.as_ref().map_or_else(
            || "folder".to_string(),
            |p| {
                p.file_name()
                    .map_or_else(|| p.display().to_string(), |n| n.to_string_lossy().into())
            },
        )
    }

    /// What to change so the folder loads.
    #[must_use]
    pub fn advice(&self) -> &'static str {
        self.kind.advice()
    }
}

/// A session file found in the folder, identified without parsing its trades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEntry {
    /// Full path to the file.
    pub path: PathBuf,
    /// Instrument, from the `# symbol=` line or the folder layout.
    pub symbol: String,
    /// Session day, when the file name follows the convention.
    pub date: Option<SessionDate>,
    /// File size, for the "this one is big" signal in the picker.
    pub size_bytes: u64,
    /// Offset the file's clock readings are expressed in.
    pub timezone: UtcOffset,
    /// Whether the file declared that offset, or defaulted to UTC.
    pub timezone_declared: bool,
    /// How the recorder decided the aggressor side, when it said.
    pub side_source: Option<String>,
    /// Whether the file carries bid/ask alongside each print.
    pub has_quotes: bool,
    /// Non-fatal remarks: the file loads, but something is worth knowing.
    pub notes: Vec<String>,
}

impl SessionEntry {
    /// Instrument and day, e.g. `WINJ26 · 2026-03-16`.
    #[must_use]
    pub fn label(&self) -> String {
        match self.date {
            Some(date) => format!("{} · {}", self.symbol, date.label()),
            None => format!("{} · {}", self.symbol, self.file_name()),
        }
    }

    /// Just the file name.
    #[must_use]
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into())
    }

    /// Size rendered for a person, e.g. `14.6 MB`.
    #[must_use]
    pub fn size_label(&self) -> String {
        let bytes = self.size_bytes as f64;
        if bytes >= 1024.0 * 1024.0 {
            format!("{:.1} MB", bytes / (1024.0 * 1024.0))
        } else if bytes >= 1024.0 {
            format!("{:.0} kB", bytes / 1024.0)
        } else {
            format!("{} B", self.size_bytes)
        }
    }
}

/// Everything the chosen folder holds: what can be played, and what cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    /// The folder that was scanned.
    pub root: PathBuf,
    /// Playable sessions, sorted by instrument then day.
    pub sessions: Vec<SessionEntry>,
    /// Everything that did not fit, with the reason.
    pub problems: Vec<Problem>,
}

impl Library {
    /// Whether the folder yielded nothing to play.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// The instruments found, in display order, without repeats.
    #[must_use]
    pub fn symbols(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for session in &self.sessions {
            if !out.contains(&session.symbol) {
                out.push(session.symbol.clone());
            }
        }
        out
    }
}

/// Scan `root` for playable sessions.
///
/// Never fails: a folder that cannot be listed comes back as a [`Library`] with
/// no sessions and a [`Problem`] explaining why, which is what the UI shows.
#[must_use]
pub fn scan(root: &Path) -> Library {
    let mut library = Library {
        root: root.to_path_buf(),
        sessions: Vec::new(),
        problems: Vec::new(),
    };

    match std::fs::metadata(root) {
        Err(e) => {
            let kind = if e.kind() == std::io::ErrorKind::NotFound {
                ProblemKind::FolderMissing
            } else {
                ProblemKind::FolderUnreadable
            };
            library.problems.push(Problem {
                path: None,
                kind,
                detail: format!("{}: {e}", root.display()),
            });
            return library;
        }
        Ok(meta) if !meta.is_dir() => {
            library.problems.push(Problem {
                path: None,
                kind: ProblemKind::NotAFolder,
                detail: format!("{} is a file", root.display()),
            });
            return library;
        }
        Ok(_) => {}
    }

    visit(root, 0, &mut library);

    // Deterministic order: instrument, then day, then file name. The picker
    // must list the same sessions in the same order on every open.
    library.sessions.sort_by(|a, b| {
        a.symbol
            .cmp(&b.symbol)
            .then(a.date.cmp(&b.date))
            .then(a.path.cmp(&b.path))
    });
    library.problems.sort_by(|a, b| a.path.cmp(&b.path));

    // "Nothing here" only helps when nothing else was said. A folder whose files
    // were all rejected already carries the reason, and burying that under a
    // generic line is how a person ends up not reading either.
    if library.sessions.is_empty() && library.problems.is_empty() {
        library.problems.insert(
            0,
            Problem {
                path: None,
                kind: ProblemKind::NoSessions,
                detail: format!("no replay sessions in {}", root.display()),
            },
        );
    }
    library
}

fn visit(dir: &Path, depth: usize, library: &mut Library) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            library.problems.push(Problem {
                path: (depth > 0).then(|| dir.to_path_buf()),
                kind: ProblemKind::FolderUnreadable,
                detail: format!("{}: {e}", dir.display()),
            });
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if depth < MAX_DEPTH {
                visit(&path, depth + 1, library);
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or_default();
        classify(&path, size, library);
    }
}

/// Decide what one file is, reading only its head.
fn classify(path: &Path, size_bytes: u64, library: &mut Library) {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if extension != format::FILE_EXTENSION {
        // Only speak up about files a person might have expected to work.
        // Screenshots and stray notes in the folder are not the user's mistake.
        let kind = match extension.as_str() {
            "ndjson" | "jsonl" => Some(ProblemKind::NeedsImport),
            "txt" | "json" | "scid" | "nrd" | "gz" | "zip" | "tsv" => {
                Some(ProblemKind::NotASessionFile)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            library.problems.push(Problem {
                path: Some(path.to_path_buf()),
                kind,
                detail: match kind {
                    ProblemKind::NeedsImport => "raw recording, not a replay session".to_string(),
                    _ => format!("`.{extension}` is not a replay session file"),
                },
            });
        }
        return;
    }

    let head = match read_head(path) {
        Ok(head) => head,
        Err(e) => {
            library.problems.push(Problem {
                path: Some(path.to_path_buf()),
                kind: ProblemKind::Unreadable,
                detail: e.to_string(),
            });
            return;
        }
    };

    match format::parse_header(&head) {
        Ok(header) => {
            let mut notes = Vec::new();
            if !header.timezone_declared {
                notes.push("no `# timezone=` line — times are read as UTC".to_string());
            }
            if !header.columns.ignored().is_empty() {
                notes.push(format!(
                    "ignored column(s): {}",
                    header.columns.ignored().join(", ")
                ));
            }
            let date = session::date_from_path(path);
            if date.is_none() {
                notes.push("file name is not YYYYMMDD, so the session day is unknown".to_string());
            }
            let symbol = header
                .symbol
                .clone()
                .or_else(|| session::symbol_from_path(path))
                .unwrap_or_else(|| "unknown".to_string());
            library.sessions.push(SessionEntry {
                path: path.to_path_buf(),
                symbol,
                date,
                size_bytes,
                timezone: header.timezone,
                timezone_declared: header.timezone_declared,
                side_source: header.side_source,
                has_quotes: header.columns.has_quotes(),
                notes,
            });
        }
        Err(FormatError { kind, detail, line }) => {
            library.problems.push(Problem {
                path: Some(path.to_path_buf()),
                kind: ProblemKind::BadFormat(kind),
                detail: if line == 0 {
                    detail
                } else {
                    format!("line {line}: {detail}")
                },
            });
        }
    }
}

/// Read the first [`HEAD_BYTES`] of a file as text, cut back to the last
/// complete line so a truncated row is never parsed as a real one.
fn read_head(path: &Path) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    let mut buffer = Vec::with_capacity(HEAD_BYTES as usize);
    file.take(HEAD_BYTES).read_to_end(&mut buffer)?;
    let text = String::from_utf8_lossy(&buffer).into_owned();
    match text.rfind('\n') {
        Some(at) if buffer.len() as u64 == HEAD_BYTES => Ok(text[..=at].to_string()),
        _ => Ok(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU32, Ordering};

    /// A unique scratch folder per test, cleaned up on the way out.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "quantick-replay-{name}-{}-{id}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, relative: &str, contents: &str) -> PathBuf {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, contents).unwrap();
            path
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const GOOD: &str = "\
# quantick-replay 1
# symbol=WINJ26
# timezone=-03:00
Date,Time,Price,Bid,Ask,Volume,Side
2026-03-16,10:00:00.000,182035,182030,182035,12,B
";

    #[test]
    fn finds_sessions_in_instrument_sub_folders() {
        let scratch = Scratch::new("subfolders");
        scratch.write("WINJ26/20260316.csv", GOOD);
        scratch.write("WINJ26/20260317.csv", GOOD);

        let library = scan(scratch.path());
        assert_eq!(library.sessions.len(), 2);
        assert_eq!(library.symbols(), ["WINJ26"]);
        assert_eq!(library.sessions[0].label(), "WINJ26 · 2026-03-16");
        assert_eq!(library.sessions[1].label(), "WINJ26 · 2026-03-17");
        assert!(library.sessions[0].has_quotes);
        assert!(library.problems.is_empty(), "{:?}", library.problems);
    }

    #[test]
    fn finds_flat_files_named_symbol_date() {
        let scratch = Scratch::new("flat");
        scratch.write("WINJ26-20260316.csv", GOOD);

        let library = scan(scratch.path());
        assert_eq!(library.sessions.len(), 1);
        assert_eq!(library.sessions[0].symbol, "WINJ26");
    }

    #[test]
    fn sessions_are_listed_in_a_stable_order() {
        let scratch = Scratch::new("order");
        scratch.write("WDO/20260317.csv", GOOD);
        scratch.write("WDO/20260316.csv", GOOD);
        scratch.write("WINJ26/20260316.csv", GOOD);

        let first = scan(scratch.path());
        let second = scan(scratch.path());
        assert_eq!(first, second);
        let dates: Vec<String> = first
            .sessions
            .iter()
            .map(|s| format!("{}/{}", s.symbol, s.date.unwrap().label()))
            .collect();
        // `# symbol=WINJ26` in the file wins over the folder name, so both WDO
        // files report WINJ26 here; the point is the order is deterministic.
        assert_eq!(dates.len(), 3);
        assert!(dates.windows(2).all(|w| w[0] <= w[1]), "{dates:?}");
    }

    #[test]
    fn a_missing_folder_is_reported_not_silently_empty() {
        let library = scan(Path::new("does/not/exist/anywhere"));
        assert!(library.is_empty());
        assert_eq!(library.problems[0].kind, ProblemKind::FolderMissing);
        assert!(library.problems[0].kind.is_fatal());
    }

    #[test]
    fn an_empty_folder_says_what_it_expected() {
        let scratch = Scratch::new("empty");
        let library = scan(scratch.path());
        assert_eq!(library.problems[0].kind, ProblemKind::NoSessions);
        assert!(library.problems[0].advice().contains(".csv"));
    }

    #[test]
    fn raw_recordings_point_at_the_importer() {
        let scratch = Scratch::new("ndjson");
        scratch.write(
            "mt5_orderflow_20260316.ndjson",
            "{\"event_type\":\"tick\"}\n",
        );

        let library = scan(scratch.path());
        assert!(library.is_empty());
        let problem = library
            .problems
            .iter()
            .find(|p| p.kind == ProblemKind::NeedsImport)
            .expect("needs-import problem");
        assert!(problem.advice().contains("import_mt5_ndjson"));
        assert_eq!(problem.subject(), "mt5_orderflow_20260316.ndjson");
    }

    #[test]
    fn a_malformed_csv_is_reported_with_its_reason() {
        let scratch = Scratch::new("malformed");
        scratch.write("WINJ26/20260316.csv", "Date,Time,Price,Volume\n1,2,3,4\n");

        let library = scan(scratch.path());
        assert!(library.is_empty());
        // The file's own reason is the whole message — no generic "nothing here"
        // line on top of it.
        assert_eq!(library.problems.len(), 1);
        let problem = &library.problems[0];
        assert_eq!(
            problem.kind,
            ProblemKind::BadFormat(FormatErrorKind::MissingColumn)
        );
        assert!(problem.detail.contains("Side"), "{}", problem.detail);
        assert!(problem.advice().contains("Side"));
    }

    #[test]
    fn undeclared_timezone_and_unknown_day_come_back_as_notes() {
        let scratch = Scratch::new("notes");
        scratch.write(
            "WINJ26/session-one.csv",
            "Date,Time,Price,Volume,Side,Note\n2026-03-16,10:00:00,1,1,B,x\n",
        );

        let library = scan(scratch.path());
        let session = &library.sessions[0];
        assert!(session.date.is_none());
        assert_eq!(session.notes.len(), 3, "{:?}", session.notes);
        assert!(session.notes.iter().any(|n| n.contains("UTC")));
        assert!(session.notes.iter().any(|n| n.contains("Note")));
        assert!(session.notes.iter().any(|n| n.contains("YYYYMMDD")));
    }

    #[test]
    fn the_scan_does_not_descend_past_instrument_folders() {
        let scratch = Scratch::new("depth");
        scratch.write("WINJ26/nested/deeper/20260316.csv", GOOD);

        let library = scan(scratch.path());
        assert!(library.is_empty(), "{:?}", library.sessions);
    }

    #[test]
    fn size_labels_scale_with_the_file() {
        let entry = |bytes: u64| SessionEntry {
            path: PathBuf::from("x.csv"),
            symbol: "X".into(),
            date: None,
            size_bytes: bytes,
            timezone: UtcOffset::UTC,
            timezone_declared: false,
            side_source: None,
            has_quotes: false,
            notes: Vec::new(),
        };
        assert_eq!(entry(512).size_label(), "512 B");
        assert_eq!(entry(2048).size_label(), "2 kB");
        assert_eq!(entry(15 * 1024 * 1024).size_label(), "15.0 MB");
    }
}
