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

/// Widest a saved split may be, matching the divider's own clamp. A file edited
/// by hand cannot ask for a pane the divider could not have produced.
const MIN_SAVED_FRACTION: f32 = 0.1;
/// Narrowest, same reason.
const MAX_SAVED_FRACTION: f32 = 0.9;

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

/// Where the drawing rail was docked, in the file's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedRailDock {
    Left,
    Right,
    Top,
    Bottom,
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
        }
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
        let before = self.tabs.len();
        self.tabs.retain(|tab| {
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
        if self.tabs.len() != before {
            // A dropped tab shifts the strip under the saved index, and the
            // clamp below is what keeps it pointing at a tab that exists.
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_RESTORED_PARTIAL",
                saved = before,
                restored = self.tabs.len(),
                action = "open_surviving_tabs",
                "part of the saved workspace could not be restored"
            );
        }
        for tab in &mut self.tabs {
            tab.split_fraction = tab
                .split_fraction
                .map(|f| f.clamp(MIN_SAVED_FRACTION, MAX_SAVED_FRACTION));
            // A time interval that no longer parses costs the pane its saved
            // interval, not the tab its market: the header's default is a
            // perfectly good chart, and dropping a whole market over the
            // second pane's parameter would be out of proportion.
            if let Some(spec) = &tab.time_bars
                && !matches!(BarSpec::parse(spec), Ok(BarSpec::Time(_)))
            {
                tab.time_bars = None;
            }
        }
        self.active_tab = self
            .active_tab
            .min(self.tabs.len().saturating_sub(1))
            .min(self.tabs.len());
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
        assert_eq!(restored.tabs[0].split_fraction, Some(MAX_SAVED_FRACTION));
        assert_eq!(restored.tabs[1].split_fraction, Some(MIN_SAVED_FRACTION));
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
