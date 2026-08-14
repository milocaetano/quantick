//! `ui-state.toml` — the workspace quantick reopens on.
//!
//! A trader arranges a cockpit once: the markets in the strip, the charts each
//! tab shows, the bar rule on each of them, the dock, the rail, the timezone,
//! the window. Before this file every one of those reset on every launch, and
//! rebuilding the workspace was the first ten minutes of every session (UX
//! audit §6, "the biggest single lever"). This is the file that remembers.
//!
//! **Two tiers, named apart.** `Workspace → Save workspace` writes this file
//! on demand and says so; `Save on exit` (on by default) writes it when the
//! window closes. The explicit action exists because a trader who deliberately
//! arranges a screen wants to *know* it is kept, and an autosave alone can
//! never answer that question. The automatic tier exists because the trader who
//! never opens the menu should still reopen where they left off.
//!
//! **What lives here, and what deliberately does not.** This file owns the
//! arrangement no other store owns. Chart layers stay in `chart-layers.toml`,
//! the indicator set in `indicators-state.toml`, drawing presets and paper
//! state in theirs. One field, one file: two stores describing one switch
//! would eventually disagree about a pixel, and the user would have no way to
//! tell which one won.
//!
//! **Restoring is filtered, never trusted.** The config is the catalogue of
//! what exists; this file is only a memory of what was open. A feed or symbol
//! that has since left the config is dropped with a log line rather than
//! resurrected — see [`Workspace::restore`]. A tab is only as good as its
//! market, so a workspace whose every tab is stale restores as nothing and the
//! app opens on its configured defaults, exactly as it did before this file.
//!
//! Same store discipline as its siblings: a versioned TOML next to the config
//! (override with `QUANTICK_UI_STATE`), read once at startup, written on an
//! explicit save or at exit, temp-file-and-rename so a crash mid-write cannot
//! leave half a workspace behind. Anything unreadable or from an unknown
//! version is ignored *entirely* — half-restoring a cockpit is worse than not
//! restoring one, because the trader cannot see which half is missing.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, DeclaredLayout};
use crate::state::BarSpec;

/// Environment override for the workspace file location.
pub const UI_STATE_ENV: &str = "QUANTICK_UI_STATE";
/// Default file, next to the working directory's config.
const UI_STATE_FILE: &str = "ui-state.toml";
/// Bumped on breaking format changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;

/// Which pane the chrome spoke for, in the file's vocabulary.
///
/// A twin of [`crate::pane::PaneSide`] rather than that enum itself, for the
/// same reason [`DeclaredLayout`] is a twin of `CanvasLayout`: the file is a
/// user-facing contract and must not drift when the canvas grows a side a file
/// should not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedFocus {
    /// quantick's own chart.
    Flow,
    /// The timeframe chart beside it.
    Time,
}

// Each twin converts both ways here, beside the vocabulary it belongs to,
// rather than in the app that happens to read it. It is the pattern
// `DeclaredLayout` → `CanvasLayout` already set (`tab.rs`), and it is what
// keeps adding a variant a one-file edit: the compiler then names every arm
// that has to grow, in the module that owns the names.
impl From<crate::pane::PaneSide> for SavedFocus {
    fn from(side: crate::pane::PaneSide) -> Self {
        match side {
            crate::pane::PaneSide::Flow => Self::Flow,
            crate::pane::PaneSide::Time => Self::Time,
        }
    }
}

impl From<SavedFocus> for crate::pane::PaneSide {
    fn from(focus: SavedFocus) -> Self {
        match focus {
            SavedFocus::Flow => Self::Flow,
            SavedFocus::Time => Self::Time,
        }
    }
}

/// Where the drawing rail was docked, in the file's vocabulary.
///
/// `Right` survives here as a *reading* vocabulary only: the rail no longer
/// offers the right edge (see [`crate::toolrail::ToolboxDock`]), but a
/// `ui-state.toml` written before that still says `right`, and refusing to
/// parse it would throw away the whole file — every other remembered panel
/// with it. It loads as `Left` and is written back as `left`, so the
/// migration happens once and is visible in the file afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedRailDock {
    Left,
    Right,
    Top,
    Bottom,
}

impl From<crate::toolrail::ToolboxDock> for SavedRailDock {
    fn from(dock: crate::toolrail::ToolboxDock) -> Self {
        match dock {
            crate::toolrail::ToolboxDock::Left => Self::Left,
            crate::toolrail::ToolboxDock::Top => Self::Top,
            crate::toolrail::ToolboxDock::Bottom => Self::Bottom,
        }
    }
}

impl From<SavedRailDock> for crate::toolrail::ToolboxDock {
    fn from(dock: SavedRailDock) -> Self {
        match dock {
            SavedRailDock::Left | SavedRailDock::Right => Self::Left,
            SavedRailDock::Top => Self::Top,
            SavedRailDock::Bottom => Self::Bottom,
        }
    }
}

/// Which dock tab was open, in the file's vocabulary. Absent means the dock
/// was collapsed to its strip, which is a state in its own right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedDockTab {
    L2,
    Bubbles,
    Session,
    Trading,
    Trades,
}

impl From<crate::dock::DockTab> for SavedDockTab {
    fn from(tab: crate::dock::DockTab) -> Self {
        match tab {
            crate::dock::DockTab::L2 => Self::L2,
            crate::dock::DockTab::Bubbles => Self::Bubbles,
            crate::dock::DockTab::Session => Self::Session,
            crate::dock::DockTab::Trading => Self::Trading,
            crate::dock::DockTab::Trades => Self::Trades,
        }
    }
}

impl From<SavedDockTab> for crate::dock::DockTab {
    fn from(tab: SavedDockTab) -> Self {
        match tab {
            SavedDockTab::L2 => Self::L2,
            SavedDockTab::Bubbles => Self::Bubbles,
            SavedDockTab::Session => Self::Session,
            SavedDockTab::Trading => Self::Trading,
            SavedDockTab::Trades => Self::Trades,
        }
    }
}

/// One remembered market and how its canvas was arranged.
///
/// Bar specs are stored as the `kind:parameter` text `default_bars` already
/// uses (`tick:50`, `time:1m`) rather than as a tagged struct: the file stays
/// hand-editable in the vocabulary the config documents, and
/// [`BarSpec::parse`] is the one gate both go through — so a hand-edited
/// workspace can never open a chart no control could have produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    /// Feed id, as the config names it.
    pub feed: String,
    /// Symbol, as the feed offers it.
    pub symbol: String,
    /// Which charts the canvas showed.
    pub layout: DeclaredLayout,
    /// The time pane's share of the canvas width.
    #[serde(default)]
    pub split_fraction: Option<f32>,
    /// The pane the chrome spoke for.
    #[serde(default)]
    pub focus: Option<SavedFocus>,
    /// The flow pane's bar rule.
    pub flow_bars: String,
    /// The time pane's interval, when the tab had one.
    #[serde(default)]
    pub time_bars: Option<String>,
}

/// The single-instance chrome around the tabs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedChrome {
    /// Display timezone, in whole minutes east of UTC.
    pub timezone_minutes: i32,
    /// Whether the dock (strip included) was on screen.
    pub dock_visible: bool,
    /// The open dock tab; absent means collapsed to the strip.
    #[serde(default)]
    pub dock_tab: Option<SavedDockTab>,
    /// Whether the drawing rail was on screen.
    pub rail_visible: bool,
    /// Which edge the rail was docked to.
    pub rail_dock: SavedRailDock,
    /// Whether the status bar showed fps/frame time.
    pub perf_readings: bool,
}

/// The workspace as a whole: what the app opens on.
///
/// [`Workspace::default`] is "nothing saved" — an empty tab list and no
/// chrome, which is precisely the state a fresh install is in, so every
/// consumer's "no file" path and its "empty file" path are the same code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    version: u32,
    /// Whether closing the window writes this file. On by default: the trader
    /// who never opens the Workspace menu should still reopen where they left
    /// off. Persisted here rather than in a settings file of its own, because
    /// it is a fact *about this file*.
    #[serde(default = "yes")]
    pub save_on_exit: bool,
    /// The window's inner size in points, as it was last saved.
    #[serde(default)]
    pub window: Option<[f32; 2]>,
    /// Which tab was on screen, as an index into `tabs`.
    #[serde(default)]
    pub active_tab: usize,
    /// The open markets, left to right as the strip showed them.
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    /// The chrome around them.
    #[serde(default)]
    pub chrome: Option<SavedChrome>,
    /// Arrangements the trader named and kept, newest last.
    ///
    /// Bookmarks, not startup settings: naming one never changes what the app
    /// opens on. The two are separate because the reason to name an
    /// arrangement is usually to have somewhere to come back *to*, and a
    /// "save this so I can return to it" that silently redefined the startup
    /// screen would be the opposite of a safety net.
    ///
    /// Absent in files written before named workspaces existed, which is why
    /// it defaults rather than bumping the format version — an older file is
    /// a workspace with no bookmarks, not an unreadable one.
    #[serde(default)]
    pub saved: Vec<NamedArrangement>,
}

/// One named arrangement: everything a workspace records about the window,
/// under a name the trader chose.
///
/// The same shape as the startup arrangement above, deliberately — one thing
/// is being described either way, and `capture`/`apply` in the app run the
/// same code for both. `save_on_exit` is not here: it governs the *file*, not
/// any one arrangement in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedArrangement {
    /// What the trader called it. Unique within the file — saving over an
    /// existing name replaces it, which is what "save as" means everywhere
    /// else and spares the menu a list of five things called "scalp".
    pub name: String,
    /// The window's inner size in points.
    #[serde(default)]
    pub window: Option<[f32; 2]>,
    /// Which tab was on screen.
    #[serde(default)]
    pub active_tab: usize,
    /// The open markets, left to right.
    #[serde(default)]
    pub tabs: Vec<SavedTab>,
    /// The chrome around them.
    #[serde(default)]
    pub chrome: Option<SavedChrome>,
}

/// Longest a workspace name may be.
///
/// The names sit in a menu, and a name wider than the menu is a name the
/// trader cannot read back — which defeats the point of naming it. Generous
/// enough for "scalp WIN manhã" and short enough to stay one line.
pub const MAX_WORKSPACE_NAME: usize = 40;

/// Clean up a name typed into the Save-as box: trimmed, collapsed whitespace,
/// truncated at [`MAX_WORKSPACE_NAME`]. `None` when nothing is left.
///
/// Whitespace is collapsed rather than rejected so " scalp  win " and
/// "scalp win" are the same bookmark; a trader who typed two spaces did not
/// mean to create a second one.
#[must_use]
pub fn clean_workspace_name(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(MAX_WORKSPACE_NAME).collect())
}

/// serde's default for [`Workspace::save_on_exit`] — a file written before the
/// field existed still means "yes", which is what the app has always done.
const fn yes() -> bool {
    true
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            save_on_exit: true,
            window: None,
            active_tab: 0,
            tabs: Vec::new(),
            chrome: None,
            saved: Vec::new(),
        }
    }
}

impl Workspace {
    /// A workspace describing a window as it stands.
    ///
    /// The format version is this build's, always — it is a property of the
    /// file, never something a caller chooses, which is why it is not a
    /// parameter and the field stays private.
    #[must_use]
    pub fn new(
        save_on_exit: bool,
        window: Option<[f32; 2]>,
        active_tab: usize,
        tabs: Vec<SavedTab>,
        chrome: Option<SavedChrome>,
    ) -> Self {
        Self {
            version: FORMAT_VERSION,
            save_on_exit,
            window,
            active_tab,
            tabs,
            chrome,
            saved: Vec::new(),
        }
    }

    /// The same, carrying `saved` bookmarks through.
    ///
    /// A separate constructor rather than a sixth parameter on the one above:
    /// every caller that captures the live window has no bookmarks to give
    /// (they come off disk, not off the screen), and threading an empty vec
    /// through all of them would only invite passing the wrong thing.
    #[must_use]
    pub fn with_saved(mut self, saved: Vec<NamedArrangement>) -> Self {
        self.saved = saved;
        self
    }

    /// The named arrangement called `name`, if the file has one.
    ///
    /// The app searches its own in-memory copy of the list, so this exists for
    /// the tests that assert what actually reached the disk — which is the one
    /// question a unit test on a persistence layer should be asking.
    #[cfg(test)]
    #[must_use]
    pub fn named(&self, name: &str) -> Option<&NamedArrangement> {
        self.saved.iter().find(|entry| entry.name == name)
    }

    /// Whether this workspace has a cockpit to restore at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The saved workspace with every tab the live `config` can no longer open
    /// removed, and every out-of-range value brought back inside the domain
    /// its control enforces.
    ///
    /// The config is the catalogue of what exists; this file is a memory of
    /// what was open. A feed removed from the config, a symbol a broker
    /// stopped offering, a bar spec left over from an older vocabulary — each
    /// is dropped and logged rather than opened, because a tab whose market
    /// cannot resolve is a tab that would sit dead on screen with no
    /// explanation.
    ///
    /// Symbols the user added from the source picker live in
    /// `quantick-symbols.toml` and are folded into the catalogue before the
    /// app restores, so a dated B3 contract added by hand survives a restart
    /// like any shipped one.
    #[must_use]
    pub fn restore(mut self, config: &AppConfig) -> Self {
        self.active_tab = filter_tabs(&mut self.tabs, self.active_tab, config);
        // Bookmarks go through exactly the same gate. A named arrangement is
        // reopened months after it was saved, so it is *more* likely than the
        // startup one to name a market that has since left the config — and a
        // bookmark that resurrects a dead feed on click would be worse than
        // one that quietly comes back one tab lighter and says so in the log.
        for entry in &mut self.saved {
            entry.active_tab = filter_tabs(&mut entry.tabs, entry.active_tab, config);
        }
        // A bookmark with nothing left to open is not a bookmark. Dropping it
        // keeps the menu honest: every name in it opens something.
        self.saved.retain(|entry| {
            if entry.tabs.is_empty() {
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "UI_STATE_NAMED_DROPPED",
                    name = %entry.name,
                    action = "forget_bookmark",
                    "a named workspace has no market the configuration still offers"
                );
                return false;
            }
            true
        });
        self
    }

    /// The market a restored workspace opens its first tab on, if it has one.
    ///
    /// The startup market is decided before the app exists — the window's
    /// first feed is spawned by `main` — so this is read there and the rest of
    /// the workspace is applied by the app. One loader, two readers, both
    /// read-only and both at startup.
    #[must_use]
    pub fn first_market(&self) -> Option<(&str, &str)> {
        self.tabs
            .first()
            .map(|tab| (tab.feed.as_str(), tab.symbol.as_str()))
    }
}

/// Drop every tab `config` can no longer open, bring the survivors' values
/// back inside the domains their controls enforce, and return an active-tab
/// index that still points at one of them.
///
/// Shared by the startup arrangement and every named one, so a bookmark can
/// never be restored under looser rules than the screen the app opens on.
fn filter_tabs(tabs: &mut Vec<SavedTab>, active_tab: usize, config: &AppConfig) -> usize {
    let before = tabs.len();
    tabs.retain(|tab| {
        let known = config
            .feed(&tab.feed)
            .is_some_and(|feed| feed.symbols.contains(&tab.symbol));
        if !known {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_TAB_DROPPED",
                feed = %tab.feed,
                symbol = %tab.symbol,
                action = "skip_tab",
                "saved workspace names a market the configuration no longer offers"
            );
            return false;
        }
        if let Err(error) = BarSpec::parse(&tab.flow_bars) {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_TAB_DROPPED",
                feed = %tab.feed,
                symbol = %tab.symbol,
                spec = %tab.flow_bars,
                %error,
                action = "skip_tab",
                "saved workspace names a bar spec no control could produce"
            );
            return false;
        }
        true
    });
    if tabs.len() != before {
        // A dropped tab shifts the strip under the saved index, and the clamp
        // below is what keeps it pointing at a tab that exists.
        tracing::info!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "UI_STATE_RESTORED_PARTIAL",
            saved = before,
            restored = tabs.len(),
            action = "open_surviving_tabs",
            "part of a saved arrangement could not be restored"
        );
    }
    for tab in tabs.iter_mut() {
        // The divider's own clamp, not a second copy of its range: a
        // hand-edited fraction must land exactly where a drag could have left
        // it, and two constants for one domain is how they drift.
        tab.split_fraction = tab.split_fraction.map(crate::pane::clamp_pane_fraction);
        // A time interval that no longer parses costs the pane its saved
        // interval, not the tab its market: the header's default is a
        // perfectly good chart, and dropping a whole market over the second
        // pane's parameter would be out of proportion.
        if let Some(spec) = &tab.time_bars
            && !matches!(BarSpec::parse(spec), Ok(BarSpec::Time(_)))
        {
            tab.time_bars = None;
        }
    }
    active_tab.min(tabs.len().saturating_sub(1))
}

/// The workspace file the app opens with and writes back to.
#[must_use]
pub fn default_path() -> PathBuf {
    std::env::var_os(UI_STATE_ENV).map_or_else(|| PathBuf::from(UI_STATE_FILE), PathBuf::from)
}

/// Load the saved workspace; the default (nothing saved) when the file is
/// missing, unreadable or from an unknown version — reported, never half-read.
#[must_use]
pub fn load(path: &Path) -> Workspace {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Workspace::default();
    };
    match toml::from_str::<Workspace>(&text) {
        Ok(workspace) if workspace.version == FORMAT_VERSION => workspace,
        Ok(workspace) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_VERSION",
                path = %path.display(),
                version = workspace.version,
                action = "opening_on_defaults",
                "workspace file is from an unknown version"
            );
            Workspace::default()
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_UNREADABLE",
                path = %path.display(),
                %error,
                action = "opening_on_defaults",
                "workspace file is unreadable"
            );
            Workspace::default()
        }
    }
}

/// Write the workspace. `true` when it reached the disk — the Workspace menu
/// says so on the status line either way, and a trader who is told "saved"
/// when nothing was written would find out at the worst possible moment.
pub fn save(path: &Path, workspace: &Workspace) -> bool {
    let mut workspace = workspace.clone();
    workspace.version = FORMAT_VERSION;
    let text = match toml::to_string_pretty(&workspace) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_WRITE_FAILED",
                %error,
                action = "workspace_not_saved",
                "could not serialize the workspace"
            );
            return false;
        }
    };
    // Temp sibling + rename, as the sibling stores do: `fs::write` truncates
    // first, so a crash mid-write would leave a half file that `load` then
    // reports unreadable — the whole cockpit gone rather than one stale field.
    let temp = path.with_extension("toml.tmp");
    match std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)) {
        Ok(()) => true,
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_WRITE_FAILED",
                path = %path.display(),
                %error,
                action = "workspace_not_saved",
                "could not save the workspace"
            );
            false
        }
    }
}

/// Forget the saved workspace: the next launch opens on the configured
/// defaults. `true` when nothing is left on disk — a file that was never
/// written is already forgotten, so a missing file is a success.
pub fn forget(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_FORGET_FAILED",
                path = %path.display(),
                %error,
                action = "workspace_kept",
                "could not delete the workspace file"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "quantick-ui-state-{name}-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    fn sample() -> Workspace {
        Workspace {
            version: FORMAT_VERSION,
            save_on_exit: true,
            window: Some([1600.0, 900.0]),
            active_tab: 1,
            tabs: vec![
                SavedTab {
                    feed: "binance".to_owned(),
                    symbol: "BTCUSDT".to_owned(),
                    layout: DeclaredLayout::TimeAndFlow,
                    split_fraction: Some(0.5),
                    focus: Some(SavedFocus::Flow),
                    flow_bars: "tick:50".to_owned(),
                    time_bars: Some("time:1m".to_owned()),
                },
                SavedTab {
                    feed: "binance".to_owned(),
                    symbol: "ETHUSDT".to_owned(),
                    layout: DeclaredLayout::Flow,
                    split_fraction: None,
                    focus: None,
                    flow_bars: "dollar:500000".to_owned(),
                    time_bars: None,
                },
            ],
            chrome: Some(SavedChrome {
                timezone_minutes: -180,
                dock_visible: true,
                dock_tab: Some(SavedDockTab::Trading),
                rail_visible: true,
                rail_dock: SavedRailDock::Left,
                perf_readings: true,
            }),
            saved: Vec::new(),
        }
    }

    #[test]
    fn a_saved_workspace_comes_back_exactly() {
        let path = temp_path("round-trip");
        assert!(save(&path, &sample()));
        assert_eq!(load(&path), sample());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_file_opens_on_the_defaults() {
        let workspace = load(&temp_path("never-written"));
        assert_eq!(workspace, Workspace::default());
        assert!(workspace.is_empty());
        assert!(workspace.save_on_exit, "autosave is the opening default");
    }

    #[test]
    fn an_unreadable_file_restores_nothing_rather_than_half_a_cockpit() {
        let path = temp_path("garbage");
        std::fs::write(&path, "this is not toml {{{").unwrap();
        assert_eq!(load(&path), Workspace::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_file_from_another_version_restores_nothing() {
        let path = temp_path("future");
        std::fs::write(&path, "version = 99\ntabs = []\n").unwrap();
        assert_eq!(load(&path), Workspace::default());
        let _ = std::fs::remove_file(&path);
    }

    /// A catalogue with one feed and two symbols — enough to be selective
    /// about what a saved workspace is allowed to reopen.
    fn catalogue() -> AppConfig {
        AppConfig {
            default_feed: "binance".to_owned(),
            default_symbol: "BTCUSDT".to_owned(),
            feeds: vec![crate::config::FeedConfig {
                id: "binance".to_owned(),
                name: "Binance".to_owned(),
                provider: crate::config::ProviderKind::Binance,
                symbols: vec!["BTCUSDT".to_owned(), "ETHUSDT".to_owned()],
                bubble_preset: None,
                symbol_bubble_presets: Default::default(),
                default_layout: None,
                default_bars: None,
            }],
            metatrader: crate::config::MetaTraderSettings::default(),
            paper: crate::config::PaperSettings::default(),
        }
    }

    #[test]
    fn a_market_the_configuration_no_longer_offers_is_not_reopened() {
        let mut workspace = sample();
        workspace.tabs.push(SavedTab {
            feed: "a-venue-that-was-removed".to_owned(),
            symbol: "BTCUSDT".to_owned(),
            layout: DeclaredLayout::Flow,
            split_fraction: None,
            focus: None,
            flow_bars: "tick:50".to_owned(),
            time_bars: None,
        });
        workspace.tabs.push(SavedTab {
            feed: "binance".to_owned(),
            symbol: "A-SYMBOL-THE-VENUE-DELISTED".to_owned(),
            layout: DeclaredLayout::Flow,
            split_fraction: None,
            focus: None,
            flow_bars: "tick:50".to_owned(),
            time_bars: None,
        });
        let restored = workspace.restore(&catalogue());
        assert_eq!(
            restored.tabs.len(),
            2,
            "only the markets the catalogue still offers come back"
        );
        assert!(
            restored
                .tabs
                .iter()
                .all(|tab| tab.feed == "binance" && tab.symbol.ends_with("USDT")),
            "a stale entry must not survive as a dead tab"
        );
    }

    #[test]
    fn a_bar_rule_no_control_could_produce_costs_that_tab_its_place() {
        let mut workspace = sample();
        workspace.tabs[1].flow_bars = "tick:0".to_owned();
        let restored = workspace.restore(&catalogue());
        assert_eq!(restored.tabs.len(), 1);
        assert_eq!(restored.tabs[0].symbol, "BTCUSDT");
    }

    #[test]
    fn a_time_pane_interval_that_no_longer_parses_costs_the_pane_not_the_market() {
        let mut workspace = sample();
        workspace.tabs[0].time_bars = Some("time:99h".to_owned());
        let restored = workspace.restore(&catalogue());
        assert_eq!(
            restored.tabs.len(),
            2,
            "the market survives its second pane's parameter"
        );
        assert_eq!(
            restored.tabs[0].time_bars, None,
            "and opens on the header default instead"
        );
    }

    #[test]
    fn a_hand_edited_divider_is_brought_back_inside_the_range_the_drag_enforces() {
        let mut workspace = sample();
        workspace.tabs[0].split_fraction = Some(0.99);
        workspace.tabs[1].split_fraction = Some(-3.0);
        let restored = workspace.restore(&catalogue());
        // Exactly where the divider's own clamp would have left it — the
        // file may not reach a split a drag could not produce.
        assert_eq!(
            restored.tabs[0].split_fraction,
            Some(crate::pane::clamp_pane_fraction(0.99))
        );
        assert_eq!(
            restored.tabs[1].split_fraction,
            Some(crate::pane::clamp_pane_fraction(-3.0))
        );
    }

    #[test]
    fn the_active_tab_never_points_past_what_survived() {
        let mut workspace = sample();
        workspace.active_tab = 7;
        workspace.tabs.truncate(1);
        assert_eq!(workspace.restore(&catalogue()).active_tab, 0);
    }

    #[test]
    fn a_workspace_whose_every_market_is_stale_restores_as_nothing() {
        let mut workspace = sample();
        for tab in &mut workspace.tabs {
            tab.feed = "gone".to_owned();
        }
        let restored = workspace.restore(&catalogue());
        assert!(restored.is_empty(), "the app then opens on its config");
        assert!(
            restored.first_market().is_none(),
            "and `main` has no market to spawn from the file"
        );
    }

    #[test]
    fn autosave_switched_off_stays_off_across_a_restart() {
        let path = temp_path("autosave-off");
        let mut workspace = sample();
        workspace.save_on_exit = false;
        assert!(save(&path, &workspace));
        assert!(
            !load(&path).save_on_exit,
            "the setting lives in the file it governs and must survive it"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_name_is_trimmed_collapsed_and_bounded() {
        assert_eq!(
            clean_workspace_name("  scalp  win "),
            Some("scalp win".into())
        );
        assert_eq!(
            clean_workspace_name("scalp\t\twin"),
            Some("scalp win".into())
        );
        assert_eq!(clean_workspace_name("   "), None);
        assert_eq!(clean_workspace_name(""), None);
        let long = "x".repeat(MAX_WORKSPACE_NAME + 20);
        assert_eq!(
            clean_workspace_name(&long).map(|name| name.chars().count()),
            Some(MAX_WORKSPACE_NAME),
            "a name wider than the menu is a name the trader cannot read back"
        );
    }

    #[test]
    fn a_bookmark_goes_through_the_same_gate_as_the_startup_screen() {
        let mut workspace = sample();
        workspace.saved.push(NamedArrangement {
            name: "mixed".to_owned(),
            window: None,
            active_tab: 1,
            tabs: vec![
                SavedTab {
                    feed: "gone".to_owned(),
                    symbol: "BTCUSDT".to_owned(),
                    layout: DeclaredLayout::Flow,
                    split_fraction: None,
                    focus: None,
                    flow_bars: "tick:50".to_owned(),
                    time_bars: None,
                },
                SavedTab {
                    feed: "binance".to_owned(),
                    symbol: "ETHUSDT".to_owned(),
                    layout: DeclaredLayout::Flow,
                    split_fraction: None,
                    focus: None,
                    flow_bars: "tick:50".to_owned(),
                    time_bars: None,
                },
            ],
            chrome: None,
        });
        let restored = workspace.restore(&catalogue());
        let entry = restored.named("mixed").expect("the bookmark survives");
        assert_eq!(entry.tabs.len(), 1, "its dead market is dropped too");
        assert_eq!(
            entry.active_tab, 0,
            "and its active index follows what survived"
        );
    }

    #[test]
    fn a_bookmark_with_nothing_left_to_open_is_forgotten() {
        let mut workspace = sample();
        workspace.saved.push(NamedArrangement {
            name: "all-gone".to_owned(),
            window: None,
            active_tab: 0,
            tabs: vec![SavedTab {
                feed: "gone".to_owned(),
                symbol: "BTCUSDT".to_owned(),
                layout: DeclaredLayout::Flow,
                split_fraction: None,
                focus: None,
                flow_bars: "tick:50".to_owned(),
                time_bars: None,
            }],
            chrome: None,
        });
        let restored = workspace.restore(&catalogue());
        assert!(
            restored.named("all-gone").is_none(),
            "every name in the menu has to open something"
        );
    }

    /// Named workspaces arrived after the format shipped, so a file written
    /// without them still has to load — the field defaults rather than the
    /// version bumping.
    #[test]
    fn a_file_written_before_bookmarks_existed_still_loads() {
        let path = temp_path("no-bookmarks");
        std::fs::write(
            &path,
            "version = 1\nsave_on_exit = true\nactive_tab = 0\n\n\
             [[tabs]]\nfeed = \"binance\"\nsymbol = \"BTCUSDT\"\n\
             layout = \"flow\"\nflow_bars = \"tick:50\"\n",
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.tabs.len(), 1, "the arrangement still reads");
        assert!(loaded.saved.is_empty(), "and it simply has no bookmarks");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn forgetting_a_workspace_that_was_never_saved_succeeds() {
        assert!(forget(&temp_path("absent")));
    }

    #[test]
    fn forgetting_removes_the_file() {
        let path = temp_path("forget");
        assert!(save(&path, &sample()));
        assert!(forget(&path));
        assert!(load(&path).is_empty());
    }
}
