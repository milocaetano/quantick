//! Workspace file document shared by desktop and headless arrangement consumers.
use crate::arrangement_document::{SavedChrome, SavedTab};
use serde::{Deserialize, Serialize};

/// Bumped on breaking format changes; readers refuse unknown versions.
pub const FORMAT_VERSION: u32 = 1;

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
    /// Whether opening a recording joins the session day before it, and a
    /// download fetches that day's tape as well.
    ///
    /// Top-level beside `replay_folder`, and written the moment the tick
    /// changes, for the same reason: it is a standing choice about how the
    /// trader rehearses, not a description of one arrangement of panes.
    ///
    /// Deliberately *not* in the app store's local-key list, unlike the folder above. The
    /// folder names a path on this machine and cannot travel; wanting
    /// yesterday on the chart is a way of working, and travels with a shared
    /// cockpit exactly as a starred tool does.
    ///
    /// `None` means "never chosen" — which is what every file written before
    /// this field existed says — and resolves to joining the day before, the
    /// answer a trader rehearsing an open gave when asked.
    #[serde(default)]
    pub replay_day_before: Option<bool>,
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
    /// the field existed says once the app loader has lifted anything the old chrome
    /// key held.
    ///
    /// Deliberately *not* in the app store's local-key list, unlike the folder above: a
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
    /// the app restoration filter applies to tabs: every name in a menu opens
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
            replay_day_before: None,
            favorite_tools: Vec::new(),
            recent_workspaces: Vec::new(),
        }
    }
}

impl Workspace {
    /// The encoded version, retained even when this reader cannot use it.
    #[must_use]
    pub const fn format_version(&self) -> u32 {
        self.version
    }

    /// Stamp the already-cloned write document with this format's version.
    /// Reading/refusing unknown versions remains the persistence adapter's job.
    #[must_use]
    pub fn into_current_format(mut self) -> Self {
        self.version = FORMAT_VERSION;
        self
    }

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
            replay_day_before: None,
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

    /// The same, carrying the *day before* choice through.
    ///
    /// Threaded like the folder above and for the same reason: it is a
    /// standing choice read off disk, not something the live window describes.
    /// `None` is "never chosen" and stays that way — a capture must not
    /// materialise today's default into the file, or tomorrow's default could
    /// never reach a trader who simply never touched the tick.
    #[must_use]
    pub fn with_replay_day_before(mut self, enabled: Option<bool>) -> Self {
        self.replay_day_before = enabled;
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

    /// Whether this workspace has a cockpit to restore at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_workspace_document_keeps_literal_wire_shape() {
        let mut workspace = Workspace::new(false, Some([1200.0, 800.0]), 2, Vec::new(), None)
            .with_replay_folder(Some("D:/tape".to_owned()))
            .with_replay_day_before(Some(false))
            .with_favorites(vec!["parallel-channel".to_owned()])
            .with_recent(vec!["D:/desk.qws.toml".to_owned()]);
        workspace.saved.push(NamedArrangement {
            name: "Desk".to_owned(),
            window: Some([900.0, 600.0]),
            active_tab: 1,
            tabs: Vec::new(),
            chrome: None,
        });
        let literal = concat!(
            "version = 1\n",
            "save_on_exit = false\n",
            "window = [\n    1200.0,\n    800.0,\n]\n",
            "active_tab = 2\n",
            "tabs = []\n",
            "replay_folder = \"D:/tape\"\n",
            "replay_day_before = false\n",
            "favorite_tools = [\"parallel-channel\"]\n",
            "recent_workspaces = [\"D:/desk.qws.toml\"]\n",
            "\n[[saved]]\n",
            "name = \"Desk\"\n",
            "window = [\n    900.0,\n    600.0,\n]\n",
            "active_tab = 1\n",
            "tabs = []\n",
        );
        assert_eq!(toml::to_string_pretty(&workspace).unwrap(), literal);
        assert_eq!(toml::from_str::<Workspace>(literal).unwrap(), workspace);
    }

    #[test]
    fn unknown_version_is_observable_until_the_write_adapter_stamps_its_clone() {
        let original: Workspace = toml::from_str("version = 99\n").unwrap();
        assert_eq!(original.format_version(), 99);
        assert_eq!(
            original.clone().into_current_format().format_version(),
            FORMAT_VERSION
        );
        assert_eq!(original.format_version(), 99);
        assert_eq!(
            toml::from_str::<Workspace>(&toml::to_string(&original).unwrap()).unwrap(),
            original
        );
        let old: Workspace = toml::from_str("version = 1\n").unwrap();
        assert_eq!(old, Workspace::default());
        assert!(old.save_on_exit);
        assert_eq!(old.replay_day_before, None);
        assert_eq!(old.first_market(), None);
    }
}
