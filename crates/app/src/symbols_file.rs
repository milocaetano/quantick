//! `quantick-symbols.toml` — instruments the user added from the UI.
//!
//! The shipped catalog names what quantick knows about; a venue's list moves
//! underneath it. B3's mini index rotates contract every two months — WINQ26
//! today, WINV26 next — and a broker serves the dated contract rather than the
//! `WIN$N` alias. Adding one from the source picker writes it here, so the
//! next launch still has it.
//!
//! A sidecar rather than the config itself: `quantick.toml` is hand-written,
//! comments and all, and a program that rewrote it would eat them. This file
//! is the app's to own — the header it writes says so, and deleting it loses
//! the additions and nothing else.
//!
//! Same discipline as `indicators-state.toml`: a versioned TOML in the working
//! directory (override with `QUANTICK_SYMBOLS`), read once at startup, written
//! when the set changes. An unknown version or an unreadable file starts empty
//! and says so.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Environment override for the file location.
const SYMBOLS_ENV: &str = "QUANTICK_SYMBOLS";
/// Default file, beside the working directory's config.
const SYMBOLS_FILE: &str = "quantick-symbols.toml";
/// Bumped on breaking layout changes; unknown versions start empty.
const FORMAT_VERSION: u32 = 1;

/// What the app writes at the top of the file, so whoever finds it knows what
/// it is without reading the source.
const HEADER: &str = "\
# quantick — symbols added from the app's source picker.
#
# Written by quantick, not by hand: your own quantick.toml is never modified.
# Each entry adds one instrument to a feed's catalog, on top of what the
# config already lists. Deleting this file loses these additions and nothing
# else — the shipped catalog is untouched by it.
#
# Typical use: a dated B3 contract (WINQ26, WDOZ25) that replaces the previous
# one every couple of months.
";

/// Symbols the user added, per feed id.
///
/// Ordered by feed id, and within a feed by when it was added — the picker
/// shows the config's own list first and these after it, so the order a user
/// sees is the order this file records.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedSymbols {
    #[serde(default)]
    version: u32,
    /// Feed id → the symbols added for it.
    #[serde(default)]
    feeds: BTreeMap<String, Vec<String>>,
}

impl AddedSymbols {
    /// The symbols added for `feed_id`, oldest first.
    #[must_use]
    pub fn for_feed(&self, feed_id: &str) -> &[String] {
        self.feeds.get(feed_id).map_or(&[], Vec::as_slice)
    }

    /// Whether `symbol` was added by the user for `feed_id` — which is what
    /// makes it removable; a shipped catalog entry is not the app's to drop.
    #[must_use]
    pub fn contains(&self, feed_id: &str, symbol: &str) -> bool {
        self.for_feed(feed_id).iter().any(|s| s == symbol)
    }

    /// Record `symbol` for `feed_id`. `false` when it was already recorded.
    pub fn add(&mut self, feed_id: &str, symbol: &str) -> bool {
        let list = self.feeds.entry(feed_id.to_owned()).or_default();
        if list.iter().any(|s| s == symbol) {
            return false;
        }
        list.push(symbol.to_owned());
        true
    }

    /// Forget `symbol` for `feed_id`. `false` when it was not recorded.
    pub fn remove(&mut self, feed_id: &str, symbol: &str) -> bool {
        let Some(list) = self.feeds.get_mut(feed_id) else {
            return false;
        };
        let before = list.len();
        list.retain(|s| s != symbol);
        let removed = list.len() != before;
        if list.is_empty() {
            self.feeds.remove(feed_id);
        }
        removed
    }

    /// Every (feed id, symbols) pair, for the merge into the catalog.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.feeds
            .iter()
            .map(|(feed, symbols)| (feed.as_str(), symbols.as_slice()))
    }
}

/// Where the file lives: `QUANTICK_SYMBOLS`, else the working directory.
#[must_use]
pub fn default_path() -> PathBuf {
    std::env::var_os(SYMBOLS_ENV).map_or_else(|| PathBuf::from(SYMBOLS_FILE), PathBuf::from)
}

/// Load the added symbols; empty when missing, unreadable or from an unknown
/// version (reported, never half-read).
#[must_use]
pub fn load(path: &std::path::Path) -> AddedSymbols {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        // No file is the normal case — nothing has been added yet. Anything
        // else is a file that exists and cannot be read, which costs the user
        // their additions and should not do so quietly.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return AddedSymbols::default();
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_UNREADABLE",
                path = %path.display(),
                error = %error,
                action = "starting_empty",
                "cannot open the added-symbols file"
            );
            return AddedSymbols::default();
        }
    };
    match toml::from_str::<AddedSymbols>(&text) {
        Ok(added) if added.version == FORMAT_VERSION => added,
        Ok(added) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_VERSION",
                path = %path.display(),
                version = added.version,
                action = "starting_empty",
                "added-symbols file is from an unknown version"
            );
            AddedSymbols::default()
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "SYMBOL_CATALOG_UNREADABLE",
                path = %path.display(),
                error = %error,
                action = "starting_empty",
                "cannot read the added-symbols file"
            );
            AddedSymbols::default()
        }
    }
}

/// Write the added symbols, header and all.
///
/// # Errors
///
/// Returns what went wrong, for the caller to surface with the path: a
/// read-only working directory is a real thing, and an addition that looked
/// like it stuck but did not is the kind of quiet loss the user only finds
/// out about at the next launch.
pub fn save(path: &std::path::Path, added: &AddedSymbols) -> Result<(), String> {
    let file = AddedSymbols {
        version: FORMAT_VERSION,
        feeds: added.feeds.clone(),
    };
    let body = toml::to_string_pretty(&file).map_err(|error| error.to_string())?;
    // Temp sibling + rename, like the indicator state: `fs::write` truncates
    // first, so a crash mid-write would leave a half file that the next load
    // reports unreadable and starts empty.
    let temp = path.with_extension("toml.tmp");
    let text = format!("{HEADER}\n{body}");
    std::fs::write(&temp, text)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temp);
            error.to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "quantick-symbols-{}-{}.toml",
            name,
            std::process::id()
        ))
    }

    #[test]
    fn adding_is_idempotent_and_ordered_by_when_it_happened() {
        let mut added = AddedSymbols::default();
        assert!(added.add("mt", "WINQ26"));
        assert!(added.add("mt", "WDOZ25"));
        assert!(
            !added.add("mt", "WINQ26"),
            "the same symbol twice records once"
        );
        assert_eq!(added.for_feed("mt"), ["WINQ26", "WDOZ25"]);
        assert!(added.contains("mt", "WDOZ25"));
        assert!(!added.contains("mt", "NOPE"));
        assert!(added.for_feed("other").is_empty());
    }

    #[test]
    fn removing_forgets_the_symbol_and_the_feed_when_it_empties() {
        let mut added = AddedSymbols::default();
        added.add("mt", "WINQ26");
        added.add("mt", "WDOZ25");

        assert!(added.remove("mt", "WINQ26"));
        assert_eq!(added.for_feed("mt"), ["WDOZ25"]);
        assert!(!added.remove("mt", "WINQ26"), "gone stays gone");

        assert!(added.remove("mt", "WDOZ25"));
        assert!(
            added.entries().next().is_none(),
            "a feed with nothing added leaves no entry behind"
        );
    }

    #[test]
    fn a_round_trip_keeps_every_addition_and_the_file_explains_itself() {
        let path = scratch("round-trip");
        let _ = std::fs::remove_file(&path);
        let mut added = AddedSymbols::default();
        added.add("metatrader-b3", "WINQ26");
        added.add("binance", "SOLUSDT");

        save(&path, &added).expect("the scratch file is writable");

        let text = std::fs::read_to_string(&path).expect("the file was written");
        assert!(
            text.starts_with("# quantick"),
            "the file says what it is: {text}"
        );
        assert!(
            text.contains("Deleting this file"),
            "and that it is safe to delete: {text}"
        );
        assert_eq!(load(&path), {
            let mut expected = added.clone();
            expected.version = FORMAT_VERSION;
            expected
        });
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_or_unreadable_file_starts_empty_rather_than_failing() {
        let missing = scratch("missing");
        let _ = std::fs::remove_file(&missing);
        assert_eq!(load(&missing), AddedSymbols::default());

        let broken = scratch("broken");
        std::fs::write(&broken, "this is not toml {{{").expect("write");
        assert_eq!(
            load(&broken),
            AddedSymbols::default(),
            "a corrupt sidecar costs the additions, never the launch"
        );
        let _ = std::fs::remove_file(&broken);
    }

    #[test]
    fn a_file_from_another_version_starts_empty() {
        let path = scratch("version");
        std::fs::write(&path, "version = 99\n[feeds]\nmt = [\"WINQ26\"]\n").expect("write");
        assert_eq!(load(&path), AddedSymbols::default());
        let _ = std::fs::remove_file(&path);
    }
}
