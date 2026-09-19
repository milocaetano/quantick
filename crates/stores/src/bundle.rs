//! One file that holds the whole cockpit — the thing a trader can copy, keep
//! and carry to another machine.
//!
//! The cockpit home ([`crate::home`]) fixed *where* the cockpit lives, so it stops
//! disappearing between launches. This module answers the other half: a
//! trader who has arranged a screen wants to keep that arrangement somewhere
//! they can see — a file in Documents, next to everything else they own —
//! and to come back to it later, or on another computer.
//!
//! **Why a bundle and not the workspace file.** `ui-state.toml` remembers the
//! tabs and the chrome, and the named arrangements inside it remember those
//! too. But which indicators are on and how they are tuned, the chart layers,
//! the drawing colours, the footprint and the hand-added symbols each live in
//! a store of their own. Reopening a named arrangement therefore brought back
//! *part* of a cockpit, and — worse — gave the trader no way to see which
//! part was missing. A bundle is every one of those stores under one roof:
//! what you saved is what comes back.
//!
//! **Sections are the stores' own files, verbatim.** Each section is the
//! parsed content of one store's file under that store's key. No section
//! knows it is in a bundle, and adding a ninth store adds no code here — it
//! adds one entry to the window's `COCKPIT_STORES`. That is what keeps this module from
//! becoming a second, drifting description of everything the app remembers.
//!
//! **Importing is all-or-nothing.** Every section is parsed with its real
//! type, and every replacement file is written, *before any store is
//! replaced* ([`apply`]). A bundle that fails anywhere is refused whole, with
//! the reason — half a cockpit is worse than none, because the trader cannot
//! see the half that is missing. It is the same rule
//! the arrangement store's `load` already applies to a single store, applied
//! across all of them. The honest limit: the final phase is a sequence of
//! renames, atomic one at a time but not as a group, so a failure *there* is
//! reported by name rather than hidden.
//!
//! **A bundle carries a cockpit, not a machine.** Keys that describe this
//! installation — the recent files, the named bookmarks, the replay folder —
//! are stripped on capture and kept on apply
//! ([`CockpitStore::local_keys`]), so opening a
//! colleague's workspace never costs the trader their own.
//!
//! The paper sidecar shares the durable home but is deliberately not a
//! section here: see the window's store table.

use std::path::{Path, PathBuf};

pub use quantick_workspace::bundle::Bundle;

use quantick_workspace::bundle::{
    CaptureFailure, CaptureStep, CaptureText, CaptureTransaction, CurrentText, FORMAT_VERSION,
    ImportFailure, ImportStep, ImportTransaction, InstalledStores,
};

use crate::home::CockpitStore;

/// The extension a workspace bundle carries. Its own rather than a bare
/// `.toml` so the file picker can offer "quantick workspace" and a trader can
/// tell one at a glance in a folder of other files.
pub const BUNDLE_EXTENSION: &str = "qws.toml";

/// The folder inside the documents shelf where exports land by default.
pub const BUNDLE_DIR: &str = "workspaces";

/// Where a store's file is, so capture and apply can be pointed at scratch
/// folders by a test instead of at the trader's real home.
pub type StorePath<'a> = &'a dyn Fn(&CockpitStore) -> PathBuf;

/// The live resolver: every store where it actually lives this run, through
/// the module's own `default_path` rather than a second copy of its rules —
/// so a bundle written under test lands in the same scratch file the app is
/// reading, and never in the trader's real cockpit.
pub fn live_paths(store: &CockpitStore) -> PathBuf {
    (store.path)()
}

/// Read every cockpit store into one bundle.
///
/// A store whose file is missing is skipped, not failed: a trader who never
/// opened the footprint has no footprint settings, and refusing to export
/// their cockpit over that would be absurd. A store whose file exists but
/// cannot be parsed *is* an error — exporting it would write a bundle that
/// could never be imported back.
pub fn capture(
    name: &str,
    stores: &[CockpitStore],
    path_of: StorePath<'_>,
) -> Result<Bundle, String> {
    let mut step = CaptureTransaction::begin(name, stores);
    let mut current_path = PathBuf::new();
    loop {
        step = match step {
            CaptureStep::Read(read) => {
                current_path = path_of(&stores[read.store_index()]);
                let text = match std::fs::read_to_string(&current_path) {
                    Ok(text) => CaptureText::Text(text),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        CaptureText::Missing
                    }
                    Err(error) => CaptureText::Failed(error.to_string()),
                };
                read.complete(text)
            }
            CaptureStep::Finished(result) => {
                return result.map_err(|error| match error {
                    CaptureFailure::Read { message, .. } => {
                        format!("could not read {}: {message}", current_path.display())
                    }
                    CaptureFailure::Parse { message, .. } => {
                        format!("{} is not readable: {message}", current_path.display())
                    }
                });
            }
        };
    }
}

/// Execute the core's validated staging/install protocol against real store paths.
/// Per-file installation remains atomic; a late rename can leave earlier stores installed.
pub fn apply<'stores>(
    bundle: &Bundle,
    stores: &'stores [CockpitStore],
    path_of: StorePath<'_>,
) -> Result<InstalledStores<'stores>, String> {
    // A session that writes no store (DS7) opens no workspace file either:
    // an import is every store written at once. Asked before the transaction
    // starts, so no store path is resolved first.
    quantick_workspace::write_refusal::guard_writes(&"workspace import")
        .map_err(|reason| format!("nothing was opened: {reason}"))?;
    apply_with_rename(bundle, stores, path_of, |temp, live| {
        std::fs::rename(temp, live)
    })
}

pub fn apply_with_rename<'stores>(
    bundle: &Bundle,
    stores: &'stores [CockpitStore],
    path_of: StorePath<'_>,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<InstalledStores<'stores>, String> {
    let mut step = ImportTransaction::begin(bundle, stores);
    // Resolved once per Prepare, never eagerly before validation or earlier stages.
    let mut paths: Vec<Option<PathBuf>> = vec![None; stores.len()];
    loop {
        step = match step {
            ImportStep::Notice(notice) => {
                tracing::info!(target: "quantick::app", schema_version = 1_u8,
                    event_code = "WORKSPACE_SECTION_UNKNOWN", section = %notice.key(),
                    action = "skip_section", "a workspace file names a store this build does not have");
                notice.acknowledge()
            }
            ImportStep::Prepare(prepare) => {
                let index = prepare.store_index();
                let path = path_of(&stores[index]);
                let current = if prepare.needs_current_text() {
                    std::fs::read_to_string(&path)
                        .map_or(CurrentText::Unavailable, CurrentText::Available)
                } else {
                    CurrentText::Unavailable
                };
                paths[index] = Some(path);
                prepare.complete(current)
            }
            ImportStep::Stage(stage) => {
                let path = paths[stage.store_index()]
                    .as_ref()
                    .expect("Prepare resolved path");
                let result = std::fs::write(path.with_extension("importing"), stage.text());
                stage.complete(result)
            }
            ImportStep::Install(install) => {
                let path = paths[install.store_index()]
                    .as_ref()
                    .expect("stage retained path");
                let result = rename(&path.with_extension("importing"), path);
                install.complete(result)
            }
            ImportStep::Cleanup(cleanup) => {
                let path = paths[cleanup.store_index()]
                    .as_ref()
                    .expect("attempt retained path");
                let _ = std::fs::remove_file(path.with_extension("importing"));
                cleanup.complete()
            }
            ImportStep::Finished(result) => {
                return result.map_err(|error| import_error(error, stores, &paths));
            }
        };
    }
}

/// The sentence for a bundle this build cannot read, on open and on apply.
fn version_refused(found: u32) -> String {
    format!("workspace file version {found} (this build reads {FORMAT_VERSION})")
}

pub fn import_error(
    error: ImportFailure,
    stores: &[CockpitStore],
    paths: &[Option<PathBuf>],
) -> String {
    match error {
        ImportFailure::Version(found) => version_refused(found),
        ImportFailure::InvalidSection { store_index, error } => format!(
            "section \"{}\" is not valid: {error}",
            stores[store_index].key
        ),
        ImportFailure::RenderSection { store_index, error } => format!(
            "section \"{}\" cannot be written: {error}",
            stores[store_index].key
        ),
        ImportFailure::Stage { store_index, error } => format!(
            "could not stage {}: {error}. Nothing was changed.",
            paths[store_index].as_ref().expect("staging path").display()
        ),
        ImportFailure::Install {
            store_index,
            installed,
            total,
            error,
        } => format!(
            "replaced {installed} of {total} settings groups, then {} failed: {error}. Open the file again to finish.",
            paths[store_index]
                .as_ref()
                .expect("installation path")
                .display()
        ),
    }
}

/// Read a bundle from disk, refusing anything that is not one.
pub fn read(path: &Path) -> Result<Bundle, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let bundle: Bundle = toml::from_str(&text)
        .map_err(|error| format!("{} is not a quantick workspace: {error}", path.display()))?;
    // The version, on the way in, so a file this build cannot read is refused
    // at the moment the trader chose it. The per-section check belongs to
    // `apply`, which is the only thing that writes — running it here too
    // would validate and render every section twice for one import, and log
    // each unknown section twice as if two imports had happened.
    if bundle.version != FORMAT_VERSION {
        return Err(version_refused(bundle.version));
    }
    Ok(bundle)
}

/// Write a bundle to disk, creating its folder if need be.
pub fn write(path: &Path, bundle: &Bundle) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let text = toml::to_string_pretty(bundle)
        .map_err(|error| format!("could not write the workspace: {error}"))?;
    write_atomically(path, &text).map_err(|error| format!("could not save the workspace: {error}"))
}

/// Temp sibling + rename, as every store in this app does: `fs::write`
/// truncates first, so a crash mid-write would leave a half file that the
/// next read reports unreadable — the whole cockpit gone rather than a
/// stale one.
pub fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    quantick_workspace::write_refusal::guard_write(path).map_err(std::io::Error::other)?;
    let temp = path.with_extension("tmp");
    match std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// The recent entries whose file is still there, newest first.
///
/// Filtered when the menu is built rather than when an entry is clicked, by
/// the rule the arrangement's restore already sets: every name
/// in a menu opens something. The stored list is left alone — a recording on
/// a drive that is merely unplugged today comes back when it is plugged in.
pub fn existing_recent(recent: &[String]) -> Vec<PathBuf> {
    recent
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect()
}

/// What the Open-recent menu calls an entry: the file's own name, without the
/// bundle extension, because that is what the trader typed when they saved it.
pub fn recent_label(path: &Path) -> String {
    path.file_name()
        .map(|name| {
            let text = name.to_string_lossy();
            text.strip_suffix(&format!(".{BUNDLE_EXTENSION}"))
                .unwrap_or(&text)
                .to_owned()
        })
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// A file name for `name` that a file system will accept, keeping the
/// trader's own words wherever it can.
///
/// Only the characters Windows forbids are replaced — a trader who called an
/// arrangement "scalp WIN manhã" gets a file called `scalp WIN manhã`, not a
/// transliterated one.
pub fn file_name_for(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            other if other.is_control() => '-',
            other => other,
        })
        .collect();
    let trimmed = safe.trim().trim_end_matches('.');
    let stem = if trimmed.is_empty() {
        "workspace"
    } else {
        trimmed
    };
    format!("{stem}.{BUNDLE_EXTENSION}")
}
