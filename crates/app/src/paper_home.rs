//! Where the paper-trading journal lives by default, and the consolidation
//! that rescues history from the cwd-relative era.
//!
//! Before this module every quantick path was cwd-relative — launching the
//! app from a different folder silently switched the journal, and even the
//! sidecar remembering the user's picked folder. Trades were never deleted,
//! but they were scattered; that is the "my old trades are gone" report.
//! The durable home is the user's documents folder
//! (`Documents/Quantick/paper-trades`), the one path that does not care
//! where the app was launched from.
//!
//! Resolution keeps its spirit — an explicit ask always wins:
//! `QUANTICK_TRADES_DIR` (one run) > the folder picked in-app
//! (`paper-state.toml`) > `[paper] trades_dir` in the config > the
//! documents home. Exactly once — recorded by a `.consolidated` marker in
//! the home itself, so the one-time-ness holds from every launch
//! directory — startup also *consolidates*: copy — never move, never
//! delete — what the reachable legacy locations still hold into the home,
//! retiring the pre-rescue pick. From then on a pick made in-app is a
//! standing choice again and survives every restart. The copy skips
//! byte-identical files, so the ongoing sweep of the cwd-relative legacy
//! folder stays safe to repeat.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use quantick_sim::history;

/// Overrides the history folder for one run — the autostart family's
/// explicit ask; nothing is consolidated or cleared under it.
pub(crate) const TRADES_DIR_ENV: &str = "QUANTICK_TRADES_DIR";
/// The cwd-relative folder of the legacy era: still the final fallback
/// when the platform reports no documents folder, and always a
/// consolidation source when it exists.
pub(crate) const LEGACY_TRADES_DIR: &str = "paper-trades";
/// The shelf inside the documents folder — one recognizable place for
/// everything quantick may keep there.
pub(crate) const DOCUMENTS_SHELF: &str = "Quantick";

/// That shelf as a path, when the platform reports a documents folder.
///
/// The one place the shelf is named. Everything quantick keeps for a trader —
/// the journal, the replay folder, the broker clock — hangs off this, so
/// renaming it is one edit rather than a hunt through three modules.
pub(crate) fn shelf_dir() -> Option<PathBuf> {
    documents_dir().map(|documents| documents.join(DOCUMENTS_SHELF))
}
/// The file that marks the home as already consolidated. It lives in the
/// home itself so the one-time rescue stays one-time from every launch
/// directory — a flag in the cwd-relative sidecar would not.
const CONSOLIDATED_MARKER: &str = ".consolidated";
/// How many `.imported-N` names a clash may try before giving up — far
/// beyond any real journal, a backstop against a pathological folder.
const MAX_IMPORT_RENAMES: usize = 99;

/// The user's documents folder, when the platform knows one.
pub(crate) fn documents_dir() -> Option<PathBuf> {
    dirs::document_dir()
}

/// The default journal home given a documents folder — and the legacy
/// cwd-relative folder when the platform reports none (headless CI, bare
/// setups): the old behaviour is honest there, inventing a home the user
/// cannot find is not.
pub(crate) fn default_trades_dir(documents: Option<PathBuf>) -> PathBuf {
    documents.map_or_else(
        || PathBuf::from(LEGACY_TRADES_DIR),
        |documents| documents.join(DOCUMENTS_SHELF).join(LEGACY_TRADES_DIR),
    )
}

/// The journal folder before the environment has its say: the user's own
/// in-app pick when one is stored, else the configured base, else the
/// documents home.
fn chosen(configured: Option<&str>, stored: Option<&str>, documents: Option<PathBuf>) -> PathBuf {
    stored
        .map(PathBuf::from)
        .or_else(|| configured.map(PathBuf::from))
        .unwrap_or_else(|| default_trades_dir(documents))
}

/// The journal folder for this run, without consolidation — the resolution
/// order from the module doc, for hosts that only need an answer.
pub(crate) fn resolve(configured: Option<&str>, stored: Option<&str>) -> PathBuf {
    std::env::var_os(TRADES_DIR_ENV).map_or_else(
        || chosen(configured, stored, documents_dir()),
        PathBuf::from,
    )
}

/// Resolve the journal home for this run and, exactly once, consolidate
/// the reachable legacy locations into it. The one-time-ness lives as a
/// marker file in the home itself (cwd-independent, unlike this sidecar);
/// after the rescue has run, a stored in-app pick is a live choice again
/// and wins, exactly as the picker's tooltip promises.
pub(crate) fn startup_home(
    configured: Option<&str>,
    stored: Option<&str>,
    state_path: &Path,
) -> (PathBuf, Option<ImportSummary>) {
    if cfg!(test) {
        // Tests must never scan, consolidate into, or journal under a
        // real documents folder — the same scratch discipline
        // `PaperTrading::new` applies. Per test thread rather than per
        // process, which is strictly more isolation than the shared folder
        // this replaced, and is what lets the directory be removed when the
        // thread ends instead of accumulating one per run for ever.
        return (crate::scratch::thread_dir("paper-home"), None);
    }
    if let Some(env) = std::env::var_os(TRADES_DIR_ENV) {
        // An explicit per-run ask — a QA or autostart run must never
        // touch, or even scan, the real home.
        return (PathBuf::from(env), None);
    }
    resolve_startup_home(
        configured,
        stored,
        state_path,
        documents_dir(),
        Path::new(LEGACY_TRADES_DIR),
    )
}

/// [`startup_home`] with its environment injected, so the whole decision
/// tree is testable against scratch folders.
fn resolve_startup_home(
    configured: Option<&str>,
    stored: Option<&str>,
    state_path: &Path,
    documents: Option<PathBuf>,
    legacy_dir: &Path,
) -> (PathBuf, Option<ImportSummary>) {
    if configured.is_some() {
        // An explicit `[paper] trades_dir` keeps the legacy chain whole:
        // whoever wrote the key chose a home on purpose.
        return (chosen(configured, stored, documents), None);
    }
    let dest = default_trades_dir(documents);
    let marker = dest.join(CONSOLIDATED_MARKER);
    if marker.exists() {
        // The rescue already ran. A pick made since is the user's own
        // standing choice — it must survive every restart.
        if let Some(stored) = stored {
            return (PathBuf::from(stored), None);
        }
        // Still sweep the cwd-relative legacy folder: it is the one
        // source a home-side marker cannot see, and the pass is
        // copies-only and idempotent.
        let summary = consolidate_into(&dest, std::slice::from_ref(&legacy_dir.to_path_buf()));
        return (dest, Some(summary));
    }
    let mut sources = Vec::new();
    if let Some(stored) = stored {
        sources.push(PathBuf::from(stored));
    }
    sources.push(legacy_dir.to_path_buf());
    let summary = consolidate_into(&dest, &sources);
    if summary.failed == 0 && summary.source_unreadable == 0 {
        // Only a clean pass may retire the pick and declare the rescue
        // done: clearing after a failed or unreadable copy would orphan
        // the very files the rescue exists to keep reachable.
        if stored.is_some() {
            crate::paper_state::clear_trades_dir(state_path);
        }
        write_consolidated_marker(&marker);
    }
    (dest, Some(summary))
}

/// Stamp the home as consolidated. A failed write only means the rescue
/// re-runs next launch — idempotent, so that is safe.
fn write_consolidated_marker(marker: &Path) {
    let written = marker
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            std::fs::write(
                marker,
                "quantick wrote this after consolidating legacy paper-trading history \
                 into this folder; deleting it re-runs that one-time import.\n",
            )
        });
    if let Err(error) = written {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "PAPER_HISTORY_MARKER_FAILED",
            path = %marker.display(),
            %error,
            action = "rescue_reruns_next_launch",
            "could not stamp the journal home as consolidated"
        );
    }
}

/// What one consolidation pass did. Copies only — the sources keep every
/// file, which is what makes running the pass again safe.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ImportSummary {
    /// Files newly copied into the home.
    pub copied: usize,
    /// Files already in the home, byte-identical — skipped.
    pub identical: usize,
    /// Files whose name was taken by different content — copied under an
    /// `.imported-N` suffix so neither version is lost.
    pub renamed: usize,
    /// Files that are not readable quantick-trades history — left in
    /// place and logged, never guessed at.
    pub ignored: usize,
    /// Copies that failed with an I/O error (logged, file left in place).
    pub failed: usize,
    /// Sources that exist but could not be listed — a pass that could not
    /// see everything must not be treated as complete.
    pub source_unreadable: usize,
}

impl ImportSummary {
    /// How many files landed in the home this pass.
    pub(crate) fn imported(&self) -> usize {
        self.copied + self.renamed
    }
}

/// The toast's account of a consolidation pass — counts only, and it says
/// "copies" out loud so the move is trustworthy.
pub(crate) fn import_toast(summary: &ImportSummary) -> String {
    if summary.imported() == 0 && summary.failed == 0 {
        return "SIM: nothing new to import - every file is already in the journal.".to_owned();
    }
    let mut text = format!(
        "SIM: imported {} trade file(s) into the journal (copies - originals untouched)",
        summary.imported(),
    );
    if summary.identical > 0 {
        text.push_str(&format!(", {} already present", summary.identical));
    }
    if summary.ignored > 0 {
        text.push_str(&format!(
            ", {} not trade history - skipped",
            summary.ignored
        ));
    }
    if summary.failed > 0 {
        text.push_str(&format!(", {} failed - see the log", summary.failed));
    }
    if summary.source_unreadable > 0 {
        text.push_str(&format!(
            ", {} folder(s) unreadable - see the log",
            summary.source_unreadable,
        ));
    }
    text
}

/// Copy every trade-history file under each of `sources` into `dest`,
/// preserving the `<SYMBOL>/<session>.csv` layout. A loose file at a
/// source's root travels by its `# symbol=` header; one with no readable
/// symbol stays put (a symbol is never guessed). A source that is, holds,
/// or lives inside `dest` is left alone.
pub(crate) fn consolidate_into(dest: &Path, sources: &[PathBuf]) -> ImportSummary {
    let mut summary = ImportSummary::default();
    for source in sources {
        if source.as_path() == dest || dest.starts_with(source) || source.starts_with(dest) {
            continue;
        }
        if !source.exists() {
            // A source that is simply not there holds nothing to rescue.
            continue;
        }
        let Ok(entries) = std::fs::read_dir(source) else {
            // Existing but unlistable is different from empty: the pass
            // did not see everything and must not claim it did.
            summary.source_unreadable += 1;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_HISTORY_SOURCE_UNREADABLE",
                path = %source.display(),
                action = "source_skipped",
                "could not list a consolidation source"
            );
            continue;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                if path == dest || dest.starts_with(&path) {
                    continue;
                }
                copy_symbol_folder(&path, dest, &mut summary);
            } else if is_history_file(&path) {
                copy_loose_file(&path, dest, &mut summary);
            }
        }
    }
    if summary != ImportSummary::default() {
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "PAPER_HISTORY_CONSOLIDATED",
            dest = %dest.display(),
            copied = summary.copied,
            identical = summary.identical,
            renamed = summary.renamed,
            ignored = summary.ignored,
            failed = summary.failed,
            source_unreadable = summary.source_unreadable,
            "paper-trading history consolidated into the journal home"
        );
    }
    summary
}

fn is_history_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == history::FILE_EXTENSION)
}

/// One `<SYMBOL>/` folder: every readable history file inside goes to the
/// same folder name under the home. Same parse gate as the loose-file
/// path — a foreign `.csv` (a price export, another tool's data) must not
/// be planted in the journal and then counted "unreadable" every session
/// after.
fn copy_symbol_folder(folder: &Path, dest: &Path, summary: &mut ImportSummary) {
    let Some(name) = folder.file_name() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_history_file(path))
        .collect();
    paths.sort();
    for path in paths {
        let is_history =
            std::fs::read_to_string(&path).is_ok_and(|text| history::parse(&text).is_ok());
        if !is_history {
            summary.ignored += 1;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_HISTORY_IMPORT_SKIPPED",
                path = %path.display(),
                action = "file_left_in_place",
                "csv is not quantick-trades history - not planting it in the journal"
            );
            continue;
        }
        copy_one(&path, &dest.join(name), summary);
    }
}

/// A loose root-level file: its symbol comes from the `# symbol=` header,
/// or it stays behind.
fn copy_loose_file(path: &Path, dest: &Path, summary: &mut ImportSummary) {
    let symbol = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| history::parse(&text).ok())
        .and_then(|parsed| parsed.symbol);
    match symbol {
        Some(symbol) => {
            let folder = dest.join(crate::paper_chrome::sanitize_symbol(&symbol));
            copy_one(path, &folder, summary);
        }
        None => {
            summary.ignored += 1;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_HISTORY_IMPORT_SKIPPED",
                path = %path.display(),
                action = "file_left_in_place",
                "history file carries no readable symbol - not guessing a folder for it"
            );
        }
    }
}

fn copy_one(file: &Path, folder: &Path, summary: &mut ImportSummary) {
    match try_copy_one(file, folder) {
        Ok(Outcome::Copied) => summary.copied += 1,
        Ok(Outcome::Identical) => summary.identical += 1,
        Ok(Outcome::Renamed) => summary.renamed += 1,
        Err(error) => {
            summary.failed += 1;
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_HISTORY_IMPORT_FAILED",
                path = %file.display(),
                %error,
                action = "file_left_in_place",
                "could not copy a history file into the journal home"
            );
        }
    }
}

enum Outcome {
    Copied,
    Identical,
    Renamed,
}

/// Copy `file` into `folder`: keep the name when free, skip when the same
/// bytes are already there (under any candidate name — that is what makes
/// a re-run change nothing), suffix `.imported-N` when the name is taken
/// by different content.
fn try_copy_one(file: &Path, folder: &Path) -> std::io::Result<Outcome> {
    let Some(name) = file.file_name() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "history file has no name",
        ));
    };
    let source_bytes = std::fs::read(file)?;
    let mut target = folder.join(name);
    let mut suffix = 0usize;
    loop {
        if !target.exists() {
            std::fs::create_dir_all(folder)?;
            // Temp-and-rename, the paper-state discipline: a crash
            // mid-copy must not leave a torn journal that parses as a
            // shorter session and double-counts after the next pass.
            let temp = target.with_extension("csv.tmp");
            std::fs::write(&temp, &source_bytes)
                .and_then(|()| std::fs::rename(&temp, &target))
                .inspect_err(|_| {
                    let _ = std::fs::remove_file(&temp);
                })?;
            return Ok(if suffix == 0 {
                Outcome::Copied
            } else {
                Outcome::Renamed
            });
        }
        if std::fs::read(&target)? == source_bytes {
            return Ok(Outcome::Identical);
        }
        suffix += 1;
        if suffix > MAX_IMPORT_RENAMES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "too many name clashes for one history file",
            ));
        }
        target = folder.join(import_name(name, suffix));
    }
}

/// `20260101-000000.csv` → `20260101-000000.imported-1.csv`: the session
/// stamp stays the sort key, the suffix says where the file came from, and
/// the kept extension keeps the ledger reading it.
fn import_name(name: &OsStr, suffix: usize) -> PathBuf {
    let name = Path::new(name);
    let stem = name
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = name
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_else(|| history::FILE_EXTENSION.to_owned());
    PathBuf::from(format!("{stem}.imported-{suffix}.{extension}"))
}

crate::hooks::declare_hooks!["QUANTICK_TRADES_DIR"];

#[cfg(test)]
mod tests {
    use super::*;

    /// A home path that does not exist yet: several of these tests assert on
    /// what happens when the folder is missing, so it must not be created.
    /// It sits inside this thread's own directory, which is removed when the
    /// thread ends whether or not anything ever wrote there.
    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        crate::scratch::thread_dir("paper-home-test")
            .join(NEXT.fetch_add(1, Ordering::Relaxed).to_string())
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("test paths have parents")).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// A minimal valid quantick-trades file — the parse gate lets only
    /// real history into the home, so fixtures must be real history.
    fn history_text(symbol: &str) -> String {
        history::write_header(symbol, history::SessionSource::Live)
    }

    #[test]
    fn the_documents_home_hangs_off_the_documents_folder() {
        assert_eq!(
            default_trades_dir(Some(PathBuf::from("/docs"))),
            Path::new("/docs").join("Quantick").join("paper-trades"),
        );
        assert_eq!(
            default_trades_dir(None),
            PathBuf::from("paper-trades"),
            "no documents folder keeps the legacy cwd-relative behaviour"
        );
    }

    #[test]
    fn explicit_choices_outrank_the_documents_home() {
        let documents = Some(PathBuf::from("/docs"));
        assert_eq!(
            chosen(Some("cfg"), Some("picked"), documents.clone()),
            PathBuf::from("picked"),
            "the in-app pick wins over the config"
        );
        assert_eq!(
            chosen(Some("cfg"), None, documents.clone()),
            PathBuf::from("cfg"),
            "the config wins over the default"
        );
        assert_eq!(
            chosen(None, None, documents),
            Path::new("/docs").join("Quantick").join("paper-trades"),
        );
    }

    #[test]
    fn consolidation_copies_the_symbol_layout_and_reruns_change_nothing() {
        let root = scratch();
        let source = root.join("source");
        let dest = root.join("dest");
        write(
            &source.join("BTCUSDT").join("s1.csv"),
            &history_text("BTCUSDT"),
        );
        write(&source.join("BTCUSDT").join("notes.txt"), "not history\n");

        let first = consolidate_into(&dest, std::slice::from_ref(&source));
        assert_eq!(first.copied, 1);
        assert_eq!(first.imported(), 1);
        assert_eq!(
            std::fs::read_to_string(dest.join("BTCUSDT").join("s1.csv")).unwrap(),
            history_text("BTCUSDT")
        );
        assert!(
            source.join("BTCUSDT").join("s1.csv").exists(),
            "the source keeps its file - consolidation copies"
        );
        assert!(
            !dest.join("BTCUSDT").join("notes.txt").exists(),
            "only history files travel"
        );

        let second = consolidate_into(&dest, &[source]);
        assert_eq!(second.identical, 1, "a re-run finds the copy and skips");
        assert_eq!(second.imported(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_clash_with_different_content_keeps_both_and_stays_idempotent() {
        let root = scratch();
        let source = root.join("source");
        let dest = root.join("dest");
        let laptop = history_text("WINQ26");
        let resident = history_text("WINQ26F");
        write(&source.join("WINQ26").join("s1.csv"), &laptop);
        write(&dest.join("WINQ26").join("s1.csv"), &resident);

        let first = consolidate_into(&dest, std::slice::from_ref(&source));
        assert_eq!(first.renamed, 1, "different content under one name");
        assert_eq!(
            std::fs::read_to_string(dest.join("WINQ26").join("s1.imported-1.csv")).unwrap(),
            laptop
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("WINQ26").join("s1.csv")).unwrap(),
            resident,
            "the resident file is never overwritten"
        );

        let second = consolidate_into(&dest, &[source]);
        assert_eq!(
            second.identical, 1,
            "the renamed copy satisfies the next run"
        );
        assert_eq!(second.renamed, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn loose_files_travel_by_their_header_or_stay_behind() {
        let root = scratch();
        let source = root.join("source");
        let dest = root.join("dest");
        write(
            &source.join("loose.csv"),
            &history::write_header("WDO$", history::SessionSource::Live),
        );
        write(&source.join("junk.csv"), "not a history file at all\n");

        let summary = consolidate_into(&dest, std::slice::from_ref(&source));
        assert_eq!(summary.copied, 1, "the header names the folder");
        assert!(dest.join("WDO$").join("loose.csv").exists());
        assert_eq!(summary.ignored, 1, "no symbol, no guess");
        assert!(
            source.join("junk.csv").exists() && !dest.join("junk.csv").exists(),
            "the unreadable file stays exactly where it was"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn foreign_csvs_inside_symbol_folders_stay_behind() {
        let root = scratch();
        let source = root.join("source");
        let dest = root.join("dest");
        write(
            &source.join("EXPORTS").join("prices.csv"),
            "date,open,close\n",
        );
        write(
            &source.join("EXPORTS").join("real.csv"),
            &history_text("EXPORTS"),
        );

        let summary = consolidate_into(&dest, std::slice::from_ref(&source));
        assert_eq!(summary.copied, 1, "only the real history travels");
        assert_eq!(
            summary.ignored, 1,
            "the foreign csv is reported, not planted"
        );
        assert!(dest.join("EXPORTS").join("real.csv").exists());
        assert!(
            !dest.join("EXPORTS").join("prices.csv").exists(),
            "a price export must not live in the journal"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_rescue_runs_once_and_then_the_pick_wins_again() {
        let root = scratch();
        let documents = root.join("docs");
        let picked = root.join("picked");
        let legacy = root.join("legacy-cwd");
        let state_path = root.join("paper-state.toml");
        write(
            &picked.join("BTCUSDT").join("s1.csv"),
            &history_text("BTCUSDT"),
        );
        crate::paper_state::save(
            &state_path,
            &crate::paper_state::PaperState {
                trades_dir: Some(picked.display().to_string()),
                ..Default::default()
            },
        );

        // First launch: the rescue copies the pick into the home, retires
        // the pick, and stamps the home.
        let (home, summary) = resolve_startup_home(
            None,
            Some(&picked.display().to_string()),
            &state_path,
            Some(documents.clone()),
            &legacy,
        );
        let dest = documents.join("Quantick").join("paper-trades");
        assert_eq!(home, dest);
        assert_eq!(summary.expect("first launch consolidates").copied, 1);
        assert!(dest.join(".consolidated").exists(), "the home is stamped");
        assert_eq!(
            crate::paper_state::load(&state_path).trades_dir,
            None,
            "the migrated pick is retired"
        );

        // The user picks a folder again: from now on it must win every
        // launch — the rescue never re-runs against it.
        let repicked = root.join("repicked");
        std::fs::create_dir_all(&repicked).unwrap();
        let (home, summary) = resolve_startup_home(
            None,
            Some(&repicked.display().to_string()),
            &state_path,
            Some(documents.clone()),
            &legacy,
        );
        assert_eq!(home, repicked, "a post-rescue pick is a standing choice");
        assert!(summary.is_none(), "and nothing is consolidated behind it");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreadable_or_failing_pass_keeps_the_pick_and_the_rescue_pending() {
        let root = scratch();
        let documents = root.join("docs");
        // The "picked folder" is a FILE: exists, cannot be listed.
        let picked = root.join("picked-but-broken");
        write(&picked, "not a directory");
        let legacy = root.join("legacy-cwd");
        let state_path = root.join("paper-state.toml");
        crate::paper_state::save(
            &state_path,
            &crate::paper_state::PaperState {
                trades_dir: Some(picked.display().to_string()),
                ..Default::default()
            },
        );

        let (home, summary) = resolve_startup_home(
            None,
            Some(&picked.display().to_string()),
            &state_path,
            Some(documents.clone()),
            &legacy,
        );
        let dest = documents.join("Quantick").join("paper-trades");
        assert_eq!(home, dest, "the run still gets a working home");
        assert_eq!(
            summary.expect("a pass ran").source_unreadable,
            1,
            "the unlistable source is counted, not ignored"
        );
        assert!(
            !dest.join(".consolidated").exists(),
            "an incomplete pass must not claim the rescue is done"
        );
        assert_eq!(
            crate::paper_state::load(&state_path).trades_dir,
            Some(picked.display().to_string()),
            "and the pick pointing at the user's trades is kept"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_source_that_is_or_holds_the_home_is_left_alone() {
        let root = scratch();
        let dest = root.join("dest");
        write(&dest.join("BTCUSDT").join("s1.csv"), "resident\n");

        let itself = consolidate_into(&dest, std::slice::from_ref(&dest));
        assert_eq!(itself, ImportSummary::default(), "never import the home");

        // The home nested inside a source: the parent scan must not pull
        // the home's own files through the loose-file path.
        let parent = consolidate_into(&dest, std::slice::from_ref(&root));
        assert_eq!(
            parent,
            ImportSummary::default(),
            "a folder holding the home contributes nothing of the home"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
