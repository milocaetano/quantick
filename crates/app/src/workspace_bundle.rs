//! One file that holds the whole cockpit — the thing a trader can copy, keep
//! and carry to another machine.
//!
//! [`crate::store_home`] fixed *where* the cockpit lives, so it stops
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
//! adds one entry to [`COCKPIT_STORES`]. That is what keeps this module from
//! becoming a second, drifting description of everything the app remembers.
//!
//! **Importing is all-or-nothing.** Every section is parsed with its real
//! type *before any file is written* ([`CockpitStore::validate`]). A bundle
//! that fails anywhere is refused whole, with the reason — half a cockpit is
//! worse than none, because the trader cannot see the half that is missing.
//! It is the same rule [`crate::ui_state::load`] already applies to a single
//! store, applied across all of them.
//!
//! Paper trading is deliberately not here: see [`COCKPIT_STORES`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::store_home::CockpitStore;

/// Bumped on breaking format changes; unknown versions are refused rather
/// than half-read.
const FORMAT_VERSION: u32 = 1;

/// The extension a workspace bundle carries. Its own rather than a bare
/// `.toml` so the file picker can offer "quantick workspace" and a trader can
/// tell one at a glance in a folder of other files.
pub(crate) const BUNDLE_EXTENSION: &str = "qws.toml";

/// The folder inside the documents shelf where exports land by default.
pub(crate) const BUNDLE_DIR: &str = "workspaces";

/// How many exported bundles the Open-recent menu remembers.
///
/// A menu, not a history: past this the list is longer than the screen and
/// the entry a trader actually wants is harder to find than the file picker.
pub(crate) const MAX_RECENT: usize = 10;

/// The whole cockpit as one file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Bundle {
    version: u32,
    /// What the trader called this arrangement. Carried inside the file as
    /// well as in its name so a renamed file still says what it is.
    #[serde(default)]
    pub name: String,
    /// Each store's file content, by [`CockpitStore::key`]. A store whose
    /// file did not exist when the bundle was written simply has no section —
    /// that is a cockpit which never set that thing, not a broken bundle.
    #[serde(default)]
    sections: BTreeMap<String, toml::Value>,
}

impl Bundle {
    /// How many stores this bundle carries.
    pub fn len(&self) -> usize {
        self.sections.len()
    }
}

/// Where a store's file is, so capture and apply can be pointed at scratch
/// folders by a test instead of at the trader's real home.
pub(crate) type StorePath<'a> = &'a dyn Fn(&CockpitStore) -> PathBuf;

/// The live resolver: every store where it actually lives this run.
pub(crate) fn live_paths(store: &CockpitStore) -> PathBuf {
    crate::store_home::resolve(store.env, store.file)
}

/// Read every cockpit store into one bundle.
///
/// A store whose file is missing is skipped, not failed: a trader who never
/// opened the footprint has no footprint settings, and refusing to export
/// their cockpit over that would be absurd. A store whose file exists but
/// cannot be parsed *is* an error — exporting it would write a bundle that
/// could never be imported back.
pub(crate) fn capture(
    name: &str,
    stores: &[CockpitStore],
    path_of: StorePath<'_>,
) -> Result<Bundle, String> {
    let mut sections = BTreeMap::new();
    for store in stores {
        let path = path_of(store);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!("could not read {}: {error}", path.display()));
            }
        };
        let value: toml::Value = toml::from_str(&text)
            .map_err(|error| format!("{} is not readable: {error}", path.display()))?;
        sections.insert(store.key.to_owned(), value);
    }
    Ok(Bundle {
        version: FORMAT_VERSION,
        name: name.to_owned(),
        sections,
    })
}

/// Check every section against its store's real type — the gate that makes
/// importing all-or-nothing. Returns the stores that would be written.
///
/// A section this build does not know is reported and skipped rather than
/// refused: the bundle's own version is the compatibility gate, and a store
/// that has since been removed must not make an otherwise good cockpit
/// unopenable.
fn check<'a>(bundle: &Bundle, stores: &'a [CockpitStore]) -> Result<Vec<&'a CockpitStore>, String> {
    if bundle.version != FORMAT_VERSION {
        return Err(format!(
            "workspace file version {} (this build reads {FORMAT_VERSION})",
            bundle.version
        ));
    }
    let known: Vec<&CockpitStore> = stores
        .iter()
        .filter(|store| bundle.sections.contains_key(store.key))
        .collect();
    for key in bundle.sections.keys() {
        if !stores.iter().any(|store| store.key == *key) {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "WORKSPACE_SECTION_UNKNOWN",
                section = %key,
                action = "skip_section",
                "a workspace file names a store this build does not have"
            );
        }
    }
    for store in &known {
        let value = &bundle.sections[store.key];
        let text = toml::to_string_pretty(value)
            .map_err(|error| format!("section \"{}\" cannot be written: {error}", store.key))?;
        (store.validate)(&text)
            .map_err(|error| format!("section \"{}\" is not valid: {error}", store.key))?;
    }
    Ok(known)
}

/// Write every section of `bundle` over the live stores.
///
/// Checked whole first: if any section is bad, nothing is written at all and
/// the cockpit on screen is exactly as it was. Returns the stores written.
pub(crate) fn apply<'a>(
    bundle: &Bundle,
    stores: &'a [CockpitStore],
    path_of: StorePath<'_>,
) -> Result<Vec<&'a str>, String> {
    let stores = check(bundle, stores)?;
    // Serialize everything before touching the disk, for the same reason:
    // a section that fails to render must not leave the earlier ones applied.
    let mut pending: Vec<(PathBuf, String)> = Vec::with_capacity(stores.len());
    for store in &stores {
        let text = toml::to_string_pretty(&bundle.sections[store.key])
            .map_err(|error| format!("section \"{}\" cannot be written: {error}", store.key))?;
        pending.push((path_of(store), text));
    }
    let mut written = Vec::with_capacity(pending.len());
    for ((path, text), store) in pending.into_iter().zip(&stores) {
        write_atomically(&path, &text).map_err(|error| {
            format!(
                "wrote {} of {} stores, then {} failed: {error}. The rest of the workspace is on \
                 disk; re-import to finish.",
                written.len(),
                stores.len(),
                path.display()
            )
        })?;
        written.push(store.key);
    }
    Ok(written)
}

/// Read a bundle from disk, refusing anything that is not one.
pub(crate) fn read(path: &Path, stores: &[CockpitStore]) -> Result<Bundle, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let bundle: Bundle = toml::from_str(&text)
        .map_err(|error| format!("{} is not a quantick workspace: {error}", path.display()))?;
    // Check on the way in, so a broken file is refused at the moment the
    // trader chose it rather than after the app has started applying it.
    check(&bundle, stores)?;
    Ok(bundle)
}

/// Write a bundle to disk, creating its folder if need be.
pub(crate) fn write(path: &Path, bundle: &Bundle) -> Result<(), String> {
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
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = path.with_extension("tmp");
    match std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// The folder exports land in by default: `Documents/Quantick/workspaces`,
/// beside the journal. `None` when the platform reports no documents folder,
/// in which case the picker simply opens wherever it likes.
pub(crate) fn default_dir() -> Option<PathBuf> {
    crate::store_home::home().map(|home| home.join(BUNDLE_DIR))
}

/// Put `path` at the head of the recent list, keeping it a menu.
///
/// Visiting a file already in the list moves it to the top rather than
/// duplicating it, which is what every recent-files menu does and what stops
/// the one file a trader uses daily from pushing everything else out.
pub(crate) fn remember_recent(recent: &mut Vec<String>, path: &Path) {
    let entry = path.to_string_lossy().into_owned();
    recent.retain(|existing| *existing != entry);
    recent.insert(0, entry);
    recent.truncate(MAX_RECENT);
}

/// The recent entries whose file is still there, newest first.
///
/// Filtered when the menu is built rather than when an entry is clicked, by
/// the rule [`crate::ui_state::Workspace::restore`] already sets: every name
/// in a menu opens something. The stored list is left alone — a recording on
/// a drive that is merely unplugged today comes back when it is plugged in.
pub(crate) fn existing_recent(recent: &[String]) -> Vec<PathBuf> {
    recent
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect()
}

/// What the Open-recent menu calls an entry: the file's own name, without the
/// bundle extension, because that is what the trader typed when they saved it.
pub(crate) fn recent_label(path: &Path) -> String {
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
pub(crate) fn file_name_for(name: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store_home::COCKPIT_STORES;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quantick-bundle-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// A cockpit on disk: one valid file per store, in a scratch folder.
    fn cockpit(dir: &Path) {
        let files = [
            ("ui-state.toml", "version = 1\nactive_tab = 0\ntabs = []\n"),
            ("indicators-state.toml", "version = 1\nindicators = []\n"),
            ("indicator-presets.toml", "version = 1\npresets = []\n"),
            (
                "chart-layers.toml",
                "version = 1\n\n[layers]\ngrid = true\nheatmap = false\n",
            ),
            ("quantick-drawing-presets.toml", "version = 1\n"),
            ("footprint-settings.toml", "version = 2\n\n[config]\n"),
            ("footprint-presets.toml", "version = 1\n"),
            (
                "quantick-symbols.toml",
                "version = 1\n\n[feeds]\nbinance = [\"BTCUSDT\"]\n",
            ),
        ];
        for (name, body) in files {
            std::fs::write(dir.join(name), body).expect("write store");
        }
    }

    fn paths_in(dir: &Path) -> impl Fn(&CockpitStore) -> PathBuf + use<'_> {
        move |store: &CockpitStore| dir.join(store.file)
    }

    /// The promise the feature makes: save it, change everything, load it,
    /// and the cockpit is back — every store, not some of them.
    #[test]
    fn a_saved_cockpit_comes_back_whole() {
        let source = scratch("round-trip-source");
        let live = scratch("round-trip-live");
        cockpit(&source);
        let bundle =
            capture("scalp WIN manhã", COCKPIT_STORES, &paths_in(&source)).expect("capture");
        assert_eq!(bundle.len(), COCKPIT_STORES.len(), "every store travels");

        let file = scratch("round-trip-file").join(file_name_for(&bundle.name));
        write(&file, &bundle).expect("write");
        let reread = read(&file, COCKPIT_STORES).expect("read");
        assert_eq!(reread, bundle, "the file is a faithful copy");

        // A live cockpit that says something else entirely.
        cockpit(&live);
        std::fs::write(
            live.join("chart-layers.toml"),
            "version = 1\n\n[layers]\ngrid = false\n",
        )
        .unwrap();
        let written = apply(&reread, COCKPIT_STORES, &paths_in(&live)).expect("apply");
        assert_eq!(written.len(), COCKPIT_STORES.len());
        assert!(
            std::fs::read_to_string(live.join("chart-layers.toml"))
                .unwrap()
                .contains("grid = true"),
            "the imported cockpit replaced the live one"
        );
    }

    /// The rule the module exists to keep: a bundle that is wrong anywhere
    /// changes nothing, so the trader is never left with a cockpit that is
    /// half theirs and half someone else's.
    #[test]
    fn a_bundle_with_one_bad_section_writes_nothing_at_all() {
        let source = scratch("refuse-source");
        let live = scratch("refuse-live");
        cockpit(&source);
        let mut bundle = capture("broken", COCKPIT_STORES, &paths_in(&source)).expect("capture");
        // A chart-layers section from a version this build does not read.
        bundle.sections.insert(
            "chart_layers".to_owned(),
            toml::from_str("version = 99\n").expect("a table"),
        );

        cockpit(&live);
        let before: Vec<String> = COCKPIT_STORES
            .iter()
            .map(|store| std::fs::read_to_string(live.join(store.file)).unwrap())
            .collect();

        let error =
            apply(&bundle, COCKPIT_STORES, &paths_in(&live)).expect_err("the import is refused");
        assert!(
            error.contains("chart_layers") && error.contains("99"),
            "the trader is told which section and why: {error}"
        );
        let after: Vec<String> = COCKPIT_STORES
            .iter()
            .map(|store| std::fs::read_to_string(live.join(store.file)).unwrap())
            .collect();
        assert_eq!(before, after, "not one store was touched");
    }

    /// The same refusal on the way in, so a bad file is caught when the
    /// trader picks it rather than once the app is already writing.
    #[test]
    fn a_file_that_is_not_a_workspace_is_refused_when_it_is_opened() {
        let dir = scratch("refuse-read");
        let path = dir.join("not-a-workspace.qws.toml");
        std::fs::write(&path, "this is not toml {{{").unwrap();
        assert!(read(&path, COCKPIT_STORES).is_err());

        let future = dir.join("future.qws.toml");
        std::fs::write(&future, "version = 99\nname = \"from tomorrow\"\n").unwrap();
        let error = read(&future, COCKPIT_STORES).expect_err("refused");
        assert!(
            error.contains("99"),
            "the reason names the version: {error}"
        );
    }

    /// A trader who never opened the footprint has no footprint file. That is
    /// a cockpit which never set that thing, not a broken export.
    #[test]
    fn a_store_that_was_never_written_is_simply_absent() {
        let source = scratch("partial-source");
        let live = scratch("partial-live");
        cockpit(&source);
        std::fs::remove_file(source.join("footprint-presets.toml")).unwrap();
        let bundle = capture("no footprint", COCKPIT_STORES, &paths_in(&source)).expect("capture");
        assert_eq!(bundle.len(), COCKPIT_STORES.len() - 1);
        cockpit(&live);
        let written = apply(&bundle, COCKPIT_STORES, &paths_in(&live)).expect("apply");
        assert_eq!(written.len(), COCKPIT_STORES.len() - 1);
        assert!(
            !written.contains(&"footprint_presets"),
            "an absent section writes nothing, rather than an empty file"
        );
    }

    /// A store this build does not have must not make an otherwise good
    /// cockpit unopenable — the bundle version is the compatibility gate.
    #[test]
    fn a_section_from_a_store_this_build_lacks_is_skipped_not_fatal() {
        let source = scratch("unknown-source");
        let live = scratch("unknown-live");
        cockpit(&source);
        let mut bundle =
            capture("from a later build", COCKPIT_STORES, &paths_in(&source)).expect("capture");
        bundle.sections.insert(
            "a_store_added_after_this_build".to_owned(),
            toml::from_str("version = 1\n").expect("a table"),
        );
        cockpit(&live);
        let written =
            apply(&bundle, COCKPIT_STORES, &paths_in(&live)).expect("the known stores still apply");
        assert_eq!(written.len(), COCKPIT_STORES.len());
    }

    /// A cockpit that cannot be parsed must not become a bundle that can
    /// never be imported back.
    #[test]
    fn exporting_an_unreadable_store_fails_rather_than_writing_a_dead_bundle() {
        let source = scratch("bad-source");
        cockpit(&source);
        std::fs::write(source.join("ui-state.toml"), "not toml {{{").unwrap();
        assert!(capture("doomed", COCKPIT_STORES, &paths_in(&source)).is_err());
    }

    #[test]
    fn a_name_becomes_a_file_the_system_accepts_without_losing_the_words() {
        assert_eq!(
            file_name_for("scalp WIN manhã"),
            format!("scalp WIN manhã.{BUNDLE_EXTENSION}"),
            "accented words survive; only forbidden characters go"
        );
        assert_eq!(
            file_name_for("scalp/WIN:2?"),
            format!("scalp-WIN-2-.{BUNDLE_EXTENSION}")
        );
        assert_eq!(
            file_name_for("   "),
            format!("workspace.{BUNDLE_EXTENSION}"),
            "a nameless export still lands somewhere openable"
        );
    }

    #[test]
    fn revisiting_a_file_moves_it_up_instead_of_duplicating_it() {
        let mut recent = Vec::new();
        remember_recent(&mut recent, Path::new("D:/a.qws.toml"));
        remember_recent(&mut recent, Path::new("D:/b.qws.toml"));
        remember_recent(&mut recent, Path::new("D:/a.qws.toml"));
        assert_eq!(recent, vec!["D:/a.qws.toml", "D:/b.qws.toml"]);
    }

    /// A menu, not a history.
    #[test]
    fn the_recent_list_stays_a_menu() {
        let mut recent = Vec::new();
        for index in 0..MAX_RECENT + 5 {
            remember_recent(&mut recent, Path::new(&format!("D:/w{index}.qws.toml")));
        }
        assert_eq!(recent.len(), MAX_RECENT);
        assert_eq!(recent[0], format!("D:/w{}.qws.toml", MAX_RECENT + 4));
    }

    /// Every name in the menu opens something — but a file that is merely
    /// on an unplugged drive today is not forgotten forever.
    #[test]
    fn an_entry_whose_file_is_gone_leaves_the_menu_but_not_the_list() {
        let dir = scratch("recent-prune");
        let here = dir.join("here.qws.toml");
        std::fs::write(&here, "version = 1\n").unwrap();
        let stored = vec![
            here.to_string_lossy().into_owned(),
            dir.join("gone.qws.toml").to_string_lossy().into_owned(),
        ];
        let shown = existing_recent(&stored);
        assert_eq!(shown, vec![here], "only what opens is offered");
        assert_eq!(stored.len(), 2, "the stored list is left alone");
    }

    #[test]
    fn a_recent_entry_is_labelled_by_the_name_the_trader_typed() {
        assert_eq!(
            recent_label(Path::new(&format!("D:/desk/scalp WIN.{BUNDLE_EXTENSION}"))),
            "scalp WIN"
        );
    }

    /// A store no line of this module mentions, standing in for the ninth
    /// one somebody adds later.
    fn validate_fake(text: &str) -> Result<(), String> {
        if text.contains("shape = ") {
            Ok(())
        } else {
            Err("not a fake store".to_owned())
        }
    }

    /// A registry of exactly one made-up store.
    const FAKE_REGISTRY: &[CockpitStore] = &[CockpitStore {
        key: "fake_store",
        env: "QUANTICK_FAKE_STORE",
        file: "fake-store.toml",
        validate: validate_fake,
    }];

    /// The port's own test: a store this module has never heard of travels
    /// through capture, write, read and apply on registration alone. If this
    /// passes, adding a ninth real store is one entry in `COCKPIT_STORES`.
    #[test]
    fn a_store_this_module_never_mentions_travels_on_registration_alone() {
        let source = scratch("fake-source");
        let live = scratch("fake-live");
        std::fs::write(source.join("fake-store.toml"), "shape = \"round\"\n").unwrap();
        std::fs::write(live.join("fake-store.toml"), "shape = \"square\"\n").unwrap();

        let bundle = capture("fake", FAKE_REGISTRY, &paths_in(&source)).expect("capture");
        assert_eq!(bundle.len(), 1);
        let file = scratch("fake-file").join(file_name_for("fake"));
        write(&file, &bundle).expect("write");
        let reread = read(&file, FAKE_REGISTRY).expect("read");
        let written = apply(&reread, FAKE_REGISTRY, &paths_in(&live)).expect("apply");

        assert_eq!(written, vec!["fake_store"]);
        assert!(
            std::fs::read_to_string(live.join("fake-store.toml"))
                .unwrap()
                .contains("round"),
            "the made-up store round-tripped with no code of its own here"
        );
    }

    /// And the fake store's refusal reaches the same all-or-nothing gate,
    /// so a ninth store inherits the guarantee rather than re-implementing it.
    #[test]
    fn a_registered_store_that_refuses_its_section_stops_the_whole_import() {
        let live = scratch("fake-refuse-live");
        std::fs::write(live.join("fake-store.toml"), "shape = \"square\"\n").unwrap();
        let bundle = Bundle {
            version: FORMAT_VERSION,
            name: "bad".to_owned(),
            sections: [(
                "fake_store".to_owned(),
                toml::from_str::<toml::Value>("colour = \"red\"\n").unwrap(),
            )]
            .into_iter()
            .collect(),
        };
        assert!(apply(&bundle, FAKE_REGISTRY, &paths_in(&live)).is_err());
        assert!(
            std::fs::read_to_string(live.join("fake-store.toml"))
                .unwrap()
                .contains("square"),
            "the live store is untouched"
        );
    }
}
