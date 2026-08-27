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
/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub(crate) const UI_STATE_FILE: &str = "ui-state.toml";
/// Bumped on breaking format changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;

/// The keys in this file that describe *this installation* rather than the
/// arrangement, and so never travel in a workspace bundle.
///
/// Each of these already documents itself as a fact about the machine rather
/// than about a screen — the recent files, the named bookmarks, the replay
/// folder, whether this copy autosaves. A bundle carries a cockpit; opening a
/// colleague's must not replace the trader's own bookmarks with theirs, nor
/// re-point where their recordings live. See
/// [`crate::store_home::CockpitStore::local_keys`].
pub(crate) const LOCAL_KEYS: &[&str] = &[
    "recent_workspaces",
    "saved",
    "replay_folder",
    "save_on_exit",
];

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
    /// The context column's share of the canvas width.
    #[serde(default)]
    pub split_fraction: Option<f32>,
    /// Whether the context column was collapsed to its rail.
    ///
    /// Additive with a default, per this module's own migration policy: a
    /// workspace written before the rail existed is a workspace whose column
    /// was open, not an unreadable one. The width it springs back to is
    /// `split_fraction`, which such a file already carries.
    #[serde(default)]
    pub context_collapsed: bool,
    /// The pane the chrome spoke for.
    #[serde(default)]
    pub focus: Option<SavedFocus>,
    /// The flow pane's bar rule.
    pub flow_bars: String,
    /// The time pane's interval, when the tab had one.
    #[serde(default)]
    pub time_bars: Option<String>,
    /// Whether the flow pane's on-chart indicator legend was folded to its
    /// count puck.
    ///
    /// Per pane, like the bar rules above and for the same reason: the corner
    /// pressure that makes a trader fold one chart's legend is not on the
    /// other. Absent means expanded — a workspace written before the fold
    /// existed opens exactly as it closed.
    #[serde(default)]
    pub flow_legend_collapsed: bool,
    /// Whether the time pane's legend was folded. See
    /// [`Self::flow_legend_collapsed`]; a tab that never showed the split
    /// simply never had one to fold.
    #[serde(default)]
    pub time_legend_collapsed: bool,
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
    /// Where starred tools used to live, kept only to read files that still
    /// hold them there.
    ///
    /// Favorites were part of the arrangement once, which meant a bookmark
    /// saved before the trader starred anything wiped the rail on open and a
    /// dirty exit lost the stars outright. They are a standing choice, so they
    /// moved up to [`Workspace::favorite_tools`]; [`load`] lifts what an older
    /// file kept here and empties this, and an empty list writes no key — so a
    /// file migrates once and never carries two answers.
    #[serde(
        default,
        rename = "favorite_tools",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub legacy_favorite_tools: Vec<String>,
    /// Whether venue candle history was fetched in slices, newest first.
    ///
    /// Defaults to on rather than to `false`, which is what a missing field
    /// would otherwise mean: a workspace written before this switch existed
    /// describes a cockpit whose owner never chose the slower path, and
    /// reading their silence as "off" would hand them the old wait back with
    /// no way to know why.
    #[serde(default = "yes")]
    pub progressive_history: bool,
    /// Where the trader parked the drawing-properties popup, in screen points,
    /// or absent while it still places itself beside the object it configures.
    ///
    /// One position for every tool on the rail, because there is one window:
    /// the popup is rebuilt for whatever is selected, so a trader who drags it
    /// out of the way once has moved it for the next drawing too — which is
    /// the whole reason to move it. A position per tool would put the window
    /// somewhere new on every selection, which is the behaviour being fixed.
    ///
    /// Absent in files written before this field, and absent again after the
    /// double-click that restores automatic placement; either way the app
    /// places the popup itself, exactly as it did before this field existed.
    ///
    /// Screen points, and deliberately *not* repaired here: a position that no
    /// longer fits — a smaller window, the rail on another edge — is clamped
    /// into the chart when the popup draws, by the same code that repairs one
    /// dragged half off screen. The file records what the trader did; the
    /// screen decides what is still possible.
    #[serde(default)]
    pub inspector_position: Option<[f32; 2]>,
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
    /// The folder Market Replay reads recordings from, as the trader last
    /// pointed it.
    ///
    /// Top-level rather than inside [`SavedChrome`] for the same reason
    /// `save_on_exit` is: it is a fact about this installation, not about an
    /// arrangement of panes — opening a named bookmark must never silently
    /// re-point where the trader's recordings live.
    ///
    /// `None` means "never chosen", which is what a file written before this
    /// field existed says, and resolves to the default home rather than to
    /// nothing.
    #[serde(default)]
    pub replay_folder: Option<String>,
    /// Starred drawing tools pinned to the rail, by tool id, in the order the
    /// trader starred them.
    ///
    /// Top-level for the reason `replay_folder` is, and it is the same reason
    /// twice: this is a standing choice about how the trader works, not a
    /// description of one arrangement of panes. Kept inside [`SavedChrome`] it
    /// was written only when the whole cockpit was — so a crash or a session
    /// with autosave off lost it — and opening a bookmark saved before the
    /// star existed replaced the rail's pinned section with that bookmark's
    /// emptiness. Up here it is written the moment a star is clicked and
    /// nothing that restores an arrangement touches it.
    ///
    /// Empty means "nothing starred", which is also what a file written before
    /// the field existed says once [`load`] has lifted anything the old chrome
    /// key held.
    ///
    /// Deliberately *not* in [`LOCAL_KEYS`], unlike the folder above: a
    /// starred tool is part of the cockpit being shared, and it travelled in a
    /// bundle back when it lived in the chrome. Where the trader's recordings
    /// live is a fact about their machine; which tools they keep at hand is
    /// not, and a colleague opening the bundle wants the rail that goes with
    /// the screen.
    #[serde(default)]
    pub favorite_tools: Vec<String>,
    /// Workspace files exported or imported recently, newest first.
    ///
    /// Paths, not arrangements: the file on disk is the truth, and a copy
    /// kept here would go stale the moment the trader re-exported over it.
    /// An entry whose file has since gone is dropped when the menu is built
    /// rather than when it is clicked — the same rule
    /// [`Workspace::restore`] applies to tabs: every name in a menu opens
    /// something.
    ///
    /// Here rather than in [`SavedChrome`] because it is a fact about this
    /// installation, not about an arrangement of panes — opening a bookmark
    /// must not rewrite which files the trader visited.
    #[serde(default)]
    pub recent_workspaces: Vec<String>,
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
            replay_folder: None,
            favorite_tools: Vec::new(),
            recent_workspaces: Vec::new(),
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
            replay_folder: None,
            favorite_tools: Vec::new(),
            recent_workspaces: Vec::new(),
        }
    }

    /// The same, carrying the recently visited workspace files through.
    ///
    /// A separate constructor for the reason [`Workspace::with_saved`] is:
    /// the list comes off disk, not off the screen, so every capture site
    /// would otherwise have to remember to thread it.
    #[must_use]
    pub fn with_recent(mut self, recent: Vec<String>) -> Self {
        self.recent_workspaces = recent;
        self
    }

    /// The same, carrying the replay folder through.
    ///
    /// Separate from [`Workspace::new`] for the reason [`Workspace::with_saved`]
    /// is: the folder is a standing choice read off disk, not something the
    /// live window describes, and every capture site would otherwise have to
    /// remember to thread it.
    #[must_use]
    pub fn with_replay_folder(mut self, folder: Option<String>) -> Self {
        self.replay_folder = folder;
        self
    }

    /// The same, carrying the starred tools through.
    ///
    /// Threaded like the folder above and for the same reason: a capture of
    /// the live window describes panes, and the rail's pinned section is not
    /// one of them — it outlives every arrangement the trader opens.
    #[must_use]
    pub fn with_favorites(mut self, favorites: Vec<String>) -> Self {
        self.favorite_tools = favorites;
        self
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

    /// Lift starred tools out of the old chrome key and leave nothing behind.
    ///
    /// Read once, on load, so every reader of this file — the app at startup,
    /// the read-swap-write that records a single standing choice — sees one
    /// answer in one place. The startup chrome is the only source: it is the
    /// list the rail was actually wearing, since restoring a bookmark used to
    /// apply that bookmark's copy and then the app captured it back here. The
    /// bookmarks' copies are dropped rather than merged — nothing reads them
    /// any more, and a key that is written but never applied is a lie a later
    /// reader would believe.
    ///
    /// A file that already carries the top-level list keeps it: the trader's
    /// current stars win over whatever an arrangement remembers.
    fn lift_legacy_favorites(&mut self) {
        let lifted = if self.favorite_tools.is_empty() {
            let chrome = self
                .chrome
                .as_mut()
                .map(|chrome| std::mem::take(&mut chrome.legacy_favorite_tools));
            self.favorite_tools = chrome.unwrap_or_default();
            !self.favorite_tools.is_empty()
        } else {
            if let Some(chrome) = self.chrome.as_mut() {
                chrome.legacy_favorite_tools.clear();
            }
            false
        };
        for entry in &mut self.saved {
            if let Some(chrome) = entry.chrome.as_mut() {
                chrome.legacy_favorite_tools.clear();
            }
        }
        if lifted {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_FAVORITES_LIFTED",
                tools = self.favorite_tools.len(),
                action = "favorites_are_a_standing_choice",
                "starred tools moved out of the saved arrangement"
            );
        }
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
///
/// In the durable cockpit home rather than the launch directory — see
/// [`crate::store_home`] for why the arrangement used to vanish.
#[must_use]
pub fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(UI_STATE_FILE);
    }
    crate::store_home::resolve(UI_STATE_ENV, UI_STATE_FILE)
}

/// Parse a workspace file, reporting why it is not one.
///
/// The gate a bundle section goes through before anything is written — see
/// [`crate::workspace_bundle`]. Deliberately stricter than [`load`], which
/// answers "open on the defaults" for a broken file because a trader
/// launching the app wants a window either way; an *import* has a trader
/// watching, and must say what was wrong instead.
pub(crate) fn validate(text: &str) -> Result<(), String> {
    let workspace: Workspace = toml::from_str(text).map_err(|error| error.to_string())?;
    if workspace.version == FORMAT_VERSION {
        Ok(())
    } else {
        Err(format!(
            "workspace format version {} (this build reads {FORMAT_VERSION})",
            workspace.version
        ))
    }
}

/// What reading the file produced, before a caller decides what to do about
/// one it cannot use.
///
/// The two readers below want opposite things from a file this build cannot
/// parse — [`load`] wants a window on screen, [`load_for_edit`] wants the file
/// left alone — and the parse itself is the same either way. Naming the
/// outcomes keeps that one parse in one place.
enum Read {
    /// Parsed, this build's version, legacy fields already lifted.
    Workspace(Box<Workspace>),
    /// No file here yet.
    Missing,
    /// A file that is here and did not parse.
    Unreadable(String),
    /// A file from a version this build does not know.
    UnknownVersion(u32),
}

fn read(path: &Path) -> Read {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(error) => return Read::Unreadable(error.to_string()),
    };
    match toml::from_str::<Workspace>(&text) {
        Ok(mut workspace) if workspace.version == FORMAT_VERSION => {
            workspace.lift_legacy_favorites();
            Read::Workspace(Box::new(workspace))
        }
        Ok(workspace) => Read::UnknownVersion(workspace.version),
        Err(error) => Read::Unreadable(error.to_string()),
    }
}

/// Load the saved workspace; the default (nothing saved) when the file is
/// missing, unreadable or from an unknown version — reported, never half-read.
#[must_use]
pub fn load(path: &Path) -> Workspace {
    match read(path) {
        Read::Workspace(workspace) => *workspace,
        Read::Missing => Workspace::default(),
        Read::UnknownVersion(version) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_VERSION",
                path = %path.display(),
                version,
                action = "opening_on_defaults",
                "workspace file is from an unknown version"
            );
            Workspace::default()
        }
        Read::Unreadable(error) => {
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

/// The file as it stands, for an edit that changes one field and writes the
/// rest back untouched. `None` means "do not write here".
///
/// [`load`] answers "open on the defaults" for a file it cannot use, which is
/// right at startup: a trader launching the app wants a window either way, and
/// nothing is written until they ask for it. It is the wrong answer for a
/// read-swap-write. Swapping one field into `Workspace::default()` and saving
/// *that* would replace a workspace this build merely failed to understand —
/// one written by a newer build, one a bad shutdown truncated — with an empty
/// one, and it would happen on a single click, with the trader's tabs,
/// bookmarks and replay folder inside the file being discarded.
///
/// A missing file is `Some(default)`: writing the first one loses nothing.
#[must_use]
pub fn load_for_edit(path: &Path) -> Option<Workspace> {
    match read(path) {
        Read::Workspace(workspace) => Some(*workspace),
        Read::Missing => Some(Workspace::default()),
        Read::UnknownVersion(version) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_NOT_OURS_TO_EDIT",
                path = %path.display(),
                version,
                action = "leave_the_file_alone",
                "a workspace from an unknown version is not rewritten"
            );
            None
        }
        Read::Unreadable(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "UI_STATE_NOT_OURS_TO_EDIT",
                path = %path.display(),
                %error,
                action = "leave_the_file_alone",
                "an unreadable workspace is not rewritten"
            );
            None
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
                    context_collapsed: false,
                    focus: Some(SavedFocus::Flow),
                    flow_bars: "tick:50".to_owned(),
                    time_bars: Some("time:1m".to_owned()),
                    flow_legend_collapsed: false,
                    time_legend_collapsed: false,
                },
                SavedTab {
                    feed: "binance".to_owned(),
                    symbol: "ETHUSDT".to_owned(),
                    layout: DeclaredLayout::Flow,
                    split_fraction: None,
                    context_collapsed: false,
                    focus: None,
                    flow_bars: "dollar:500000".to_owned(),
                    time_bars: None,
                    flow_legend_collapsed: false,
                    time_legend_collapsed: false,
                },
            ],
            chrome: Some(SavedChrome {
                timezone_minutes: -180,
                dock_visible: true,
                dock_tab: Some(SavedDockTab::Trading),
                rail_visible: true,
                rail_dock: SavedRailDock::Left,
                perf_readings: true,
                legacy_favorite_tools: Vec::new(),
                progressive_history: false,
                inspector_position: Some([412.5, 640.0]),
            }),
            saved: Vec::new(),
            replay_folder: Some("D:/tape".to_owned()),
            favorite_tools: vec!["parallel-channel".to_owned()],
            recent_workspaces: vec!["D:/desk/scalp.qws.toml".to_owned()],
        }
    }

    #[test]
    fn a_saved_workspace_comes_back_exactly() {
        let path = temp_path("round-trip");
        assert!(save(&path, &sample()));
        assert_eq!(load(&path), sample());
        let _ = std::fs::remove_file(&path);
    }

    /// The whole point of the field: it survives the trip to disk, so the next
    /// launch opens the browser on the folder the trader chose rather than on
    /// nowhere.
    #[test]
    fn the_replay_folder_survives_a_round_trip() {
        let path = temp_path("replay-folder");
        assert!(save(&path, &sample()));
        assert_eq!(load(&path).replay_folder.as_deref(), Some("D:/tape"));
        let _ = std::fs::remove_file(&path);
    }

    /// A workspace written before the field existed is a workspace with no
    /// pick — not an unreadable one, and not a pick of "".
    #[test]
    fn a_file_from_before_the_field_has_no_pick() {
        let path = temp_path("older-file");
        let mut older = sample();
        older.replay_folder = None;
        assert!(save(&path, &older));
        let body = std::fs::read_to_string(&path).expect("written");
        assert!(
            !body.contains("replay_folder"),
            "an absent pick writes no key: {body}"
        );
        assert_eq!(load(&path).replay_folder, None);
        let _ = std::fs::remove_file(&path);
    }

    /// The whole point of moving the field up: a star clicked once is still
    /// there at the next launch, whatever the arrangement did in between.
    #[test]
    fn the_starred_tools_survive_a_round_trip() {
        let path = temp_path("favorites");
        assert!(save(&path, &sample()));
        assert_eq!(
            load(&path).favorite_tools,
            vec!["parallel-channel".to_owned()]
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A file written while favorites lived in the chrome opens with the stars
    /// it had. The trader starred those tools; a format change is not a reason
    /// to make them do it again.
    #[test]
    fn a_file_that_kept_its_stars_in_the_chrome_still_opens_on_them() {
        let path = temp_path("legacy-favorites");
        let mut older = sample();
        older.favorite_tools = Vec::new();
        older.chrome.as_mut().expect("chrome").legacy_favorite_tools =
            vec!["measure".to_owned(), "fib-retracement".to_owned()];
        assert!(save(&path, &older));
        let loaded = load(&path);
        assert_eq!(
            loaded.favorite_tools,
            vec!["measure".to_owned(), "fib-retracement".to_owned()],
            "the stars move up rather than vanish"
        );
        assert!(
            loaded
                .chrome
                .expect("chrome")
                .legacy_favorite_tools
                .is_empty(),
            "and the old key is left empty, so the file carries one answer"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The same migration against a file written by the shipped build rather
    /// than by this module's own serializer.
    ///
    /// The test above proves the round trip; this one proves the *format*. A
    /// real `ui-state.toml` puts the stars inside `[chrome]` and has no
    /// top-level key at all, and it is the file every trader upgrading to this
    /// build is holding — reading it wrong empties their rail on first launch.
    #[test]
    fn a_workspace_written_by_the_shipped_build_keeps_its_stars() {
        let path = temp_path("shipped-file");
        std::fs::write(
            &path,
            "version = 1\n\
             save_on_exit = true\n\
             active_tab = 0\n\
             saved = []\n\
             \n\
             [[tabs]]\n\
             feed = \"binance\"\n\
             symbol = \"WINQ26\"\n\
             layout = \"time+flow\"\n\
             flow_bars = \"imbalance:5000\"\n\
             \n\
             [chrome]\n\
             timezone_minutes = -180\n\
             dock_visible = false\n\
             rail_visible = true\n\
             rail_dock = \"left\"\n\
             perf_readings = true\n\
             favorite_tools = [\n\
             \"fixed-range-profile\",\n\
             \"measure\",\n\
             \"horizontal-line\",\n\
             ]\n\
             progressive_history = true\n",
        )
        .unwrap();

        assert_eq!(
            load(&path).favorite_tools,
            vec![
                "fixed-range-profile".to_owned(),
                "measure".to_owned(),
                "horizontal-line".to_owned(),
            ],
            "the rail an upgrading trader already had is the rail they get"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Migration is one-way and one-time: once lifted, the old key is gone
    /// from the file, so nothing can resurrect a stale list later.
    #[test]
    fn a_migrated_file_stops_writing_the_old_key() {
        let path = temp_path("legacy-rewritten");
        let mut older = sample();
        older.favorite_tools = Vec::new();
        older.chrome.as_mut().expect("chrome").legacy_favorite_tools = vec!["measure".to_owned()];
        assert!(save(&path, &older));
        let lifted = load(&path);
        assert!(save(&path, &lifted));
        let body = std::fs::read_to_string(&path).expect("written");
        assert_eq!(
            body.matches("favorite_tools").count(),
            1,
            "exactly one favorites key, at the top level: {body}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The trader's live stars beat whatever an old arrangement remembers —
    /// otherwise unstarring a tool would come back on the next launch.
    #[test]
    fn the_top_level_stars_win_over_the_old_chrome_key() {
        let path = temp_path("both-favorites");
        let mut mixed = sample();
        mixed.favorite_tools = vec!["measure".to_owned()];
        mixed.chrome.as_mut().expect("chrome").legacy_favorite_tools =
            vec!["parallel-channel".to_owned()];
        assert!(save(&path, &mixed));
        assert_eq!(load(&path).favorite_tools, vec!["measure".to_owned()]);
        let _ = std::fs::remove_file(&path);
    }

    /// A bookmark's copy is dropped, not merged: nothing applies it any more,
    /// and a key that is written but never read is a lie in waiting.
    #[test]
    fn a_bookmarks_copy_of_the_stars_is_dropped() {
        let path = temp_path("bookmark-favorites");
        let mut with_bookmark = sample();
        with_bookmark.saved = vec![NamedArrangement {
            name: "scalp".to_owned(),
            window: None,
            active_tab: 0,
            tabs: sample().tabs,
            chrome: Some(SavedChrome {
                legacy_favorite_tools: vec!["measure".to_owned()],
                ..sample().chrome.expect("chrome")
            }),
        }];
        assert!(save(&path, &with_bookmark));
        let loaded = load(&path);
        assert!(
            loaded.saved[0]
                .chrome
                .as_ref()
                .expect("chrome")
                .legacy_favorite_tools
                .is_empty(),
            "a bookmark no longer carries stars"
        );
        assert_eq!(
            loaded.favorite_tools,
            vec!["parallel-channel".to_owned()],
            "and it does not overwrite the trader's own"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The Open-recent menu is only useful if it is still there tomorrow.
    #[test]
    fn the_recent_workspace_list_survives_a_restart() {
        let path = temp_path("recent");
        assert!(save(&path, &sample()));
        assert_eq!(
            load(&path).recent_workspaces,
            vec!["D:/desk/scalp.qws.toml".to_owned()]
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A workspace written before the list existed is a workspace with no
    /// recents, not an unreadable one.
    #[test]
    fn a_file_from_before_the_recent_list_still_loads() {
        let path = temp_path("pre-recent");
        std::fs::write(&path, "version = 1\ntabs = []\n").unwrap();
        assert!(load(&path).recent_workspaces.is_empty());
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

    /// A workspace written before the progressive switch existed says nothing
    /// about it. Silence is not a choice, and reading it as "off" would hand
    /// the trader back the wait this feature removes.
    #[test]
    fn a_workspace_written_before_the_switch_reopens_progressive() {
        let path = temp_path("pre-progressive-chrome");
        std::fs::write(
            &path,
            concat!(
                "version = 1\n",
                "tabs = []\n",
                "[chrome]\n",
                "timezone_minutes = -180\n",
                "dock_visible = true\n",
                "rail_visible = true\n",
                "rail_dock = \"left\"\n",
                "perf_readings = true\n",
            ),
        )
        .unwrap();
        let chrome = load(&path).chrome.expect("the chrome section is readable");
        assert!(
            chrome.progressive_history,
            "a missing field means the trader never chose the slower path"
        );
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
            context_collapsed: false,
            focus: None,
            flow_bars: "tick:50".to_owned(),
            time_bars: None,
            flow_legend_collapsed: false,
            time_legend_collapsed: false,
        });
        workspace.tabs.push(SavedTab {
            feed: "binance".to_owned(),
            symbol: "A-SYMBOL-THE-VENUE-DELISTED".to_owned(),
            layout: DeclaredLayout::Flow,
            split_fraction: None,
            context_collapsed: false,
            focus: None,
            flow_bars: "tick:50".to_owned(),
            time_bars: None,
            flow_legend_collapsed: false,
            time_legend_collapsed: false,
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
                    context_collapsed: false,
                    focus: None,
                    flow_bars: "tick:50".to_owned(),
                    time_bars: None,
                    flow_legend_collapsed: false,
                    time_legend_collapsed: false,
                },
                SavedTab {
                    feed: "binance".to_owned(),
                    symbol: "ETHUSDT".to_owned(),
                    layout: DeclaredLayout::Flow,
                    split_fraction: None,
                    context_collapsed: false,
                    focus: None,
                    flow_bars: "tick:50".to_owned(),
                    time_bars: None,
                    flow_legend_collapsed: false,
                    time_legend_collapsed: false,
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
                context_collapsed: false,
                focus: None,
                flow_bars: "tick:50".to_owned(),
                time_bars: None,
                flow_legend_collapsed: false,
                time_legend_collapsed: false,
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

    /// Same rule for the popup position: a chrome section written before the
    /// field existed describes a cockpit whose owner never dragged the popup,
    /// and reading their silence as anything but "place it yourself" would
    /// park the window at a pixel they never chose.
    #[test]
    fn a_chrome_written_before_the_popup_position_still_loads() {
        let path = temp_path("no-popup-position");
        std::fs::write(
            &path,
            "version = 1\nsave_on_exit = true\nactive_tab = 0\n\n\
             [[tabs]]\nfeed = \"binance\"\nsymbol = \"BTCUSDT\"\n\
             layout = \"flow\"\nflow_bars = \"tick:50\"\n\n\
             [chrome]\ntimezone_minutes = 0\ndock_visible = true\n\
             rail_visible = true\nrail_dock = \"left\"\nperf_readings = false\n",
        )
        .unwrap();
        let chrome = load(&path).chrome.expect("the chrome still reads");
        assert_eq!(
            chrome.inspector_position, None,
            "no remembered position means automatic placement, as before"
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

#[cfg(test)]
mod migration_tests {
    use super::*;

    /// A workspace written before the canvas could stack context charts.
    ///
    /// Checked in as text rather than built from today's structs, which is the
    /// whole point: a fixture built from `SavedTab` would gain every new field
    /// the moment one is added, and would go on passing while a real file from
    /// last release stopped opening. This is what a trader's own
    /// `ui-state.toml` looked like, keys and all.
    const V1_WORKSPACE: &str = r#"
version = 1
active_tab = 0
timezone_offset_minutes = -180
save_on_exit = true

[[tabs]]
feed = "binance"
symbol = "BTCUSDT"
layout = "time+flow"
split_fraction = 0.35
focus = "time"
flow_bars = "tick:50"
time_bars = "time:60000"
"#;

    /// The migration guarantee: an old file opens, and every field the canvas
    /// has learned since defaults to what that file meant.
    #[test]
    fn a_workspace_with_no_context_keys_still_opens() {
        let workspace: Workspace =
            toml::from_str(V1_WORKSPACE).expect("a v1 workspace must still parse");
        assert_eq!(workspace.version, FORMAT_VERSION, "the format is still 1");
        assert_eq!(workspace.tabs.len(), 1);

        let tab = &workspace.tabs[0];
        assert_eq!(tab.feed, "binance");
        assert_eq!(tab.symbol, "BTCUSDT");
        assert_eq!(
            tab.layout,
            DeclaredLayout::TimeAndFlow,
            "the layout it was saved with"
        );
        assert_eq!(tab.split_fraction, Some(0.35));
        assert_eq!(
            tab.focus,
            Some(SavedFocus::Time),
            "the pane the chrome spoke for"
        );
        assert_eq!(tab.flow_bars, "tick:50");
        assert_eq!(tab.time_bars.as_deref(), Some("time:60000"));

        // The field the rail added. A file written before it existed is a
        // workspace whose column was open — not an unreadable one, and not one
        // that opens with its charts put away.
        assert!(
            !tab.context_collapsed,
            "an old workspace must not open with its context column collapsed"
        );
    }

    /// The vocabulary grew; the old names still mean what they meant. A file
    /// naming a layout this build has since added a sibling to must not start
    /// resolving to the sibling.
    #[test]
    fn the_layout_names_a_v1_file_uses_still_resolve_to_the_same_layouts() {
        for (name, expected) in [
            ("flow", DeclaredLayout::Flow),
            ("time", DeclaredLayout::Time),
            ("time+flow", DeclaredLayout::TimeAndFlow),
        ] {
            assert_eq!(
                DeclaredLayout::parse(name),
                Some(expected),
                "the v1 name {name} changed meaning"
            );
        }
    }

    /// A workspace this build writes must read back as itself, collapse
    /// included — the other half of the same promise.
    #[test]
    fn a_collapsed_column_survives_a_round_trip() {
        let mut workspace = sample_workspace();
        workspace.tabs[0].context_collapsed = true;
        workspace.tabs[0].split_fraction = Some(0.42);

        let text = toml::to_string(&workspace).expect("a workspace serialises");
        let restored: Workspace = toml::from_str(&text).expect("and reads back");
        assert!(restored.tabs[0].context_collapsed);
        assert_eq!(
            restored.tabs[0].split_fraction,
            Some(0.42),
            "the width the column springs back to went missing"
        );
    }

    fn sample_workspace() -> Workspace {
        Workspace {
            tabs: vec![SavedTab {
                feed: "binance".to_owned(),
                symbol: "BTCUSDT".to_owned(),
                layout: DeclaredLayout::TimeAndFlow,
                split_fraction: Some(0.35),
                context_collapsed: false,
                focus: None,
                flow_bars: "tick:50".to_owned(),
                time_bars: None,
                flow_legend_collapsed: false,
                time_legend_collapsed: false,
            }],
            ..Workspace::default()
        }
    }
}
