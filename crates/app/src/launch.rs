//! The composition root's inputs: every `QUANTICK_*` a default build reads.
//!
//! `main` calls [`LaunchConfig::capture`] once, before any thread, window or
//! owner exists, and nothing else in a default build reads a `QUANTICK_*` from
//! the process environment. What it captures is operator configuration a
//! trader is told about in `README.md` and `docs/`: the config file, the
//! opening market, the log format, the Binance book depth, and the folders for
//! replays, deal recordings, indicator scripts, the paper journal and the
//! bubble presets. Each is handed down typed — `main` passes the config path,
//! the selection and the log format; the folders are [`install`]ed once for the
//! stores that resolve them; the book depth goes to the feed crate.
//!
//! Capture, demo, automation and fault hooks are not configuration and are not
//! here. They compile only under the `harness` Cargo features or `cfg(test)`
//! (see `crate::hooks`), so a default binary names none of them; a harness
//! build's `main` captures them too, so no owner reads the environment. The window
//! hooks in `window` are the one pair captured beside the configuration, and
//! they are gated the same way.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::config::{AppConfig, StartupSelectionError};

// Gated inside the file (`#![cfg]`): the window hooks are harness.
pub(crate) mod window;

/// The commit this binary was built from, when the build said so. Build
/// metadata read at compile time; setting the variable at runtime does nothing.
pub(crate) const GIT_COMMIT: Option<&str> = option_env!("QUANTICK_GIT_COMMIT");

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogFormat {
    #[default]
    Text,
    Json,
}

/// Folders the operator can point a store at for one run. Resolved by the
/// stores themselves, which ask [`operator_paths`] instead of the environment.
#[derive(Debug, Default, Clone)]
pub(crate) struct OperatorPaths {
    /// `QUANTICK_REPLAY_DIR`: the folder the replay browser opens on.
    pub replay_dir: Option<String>,
    /// `QUANTICK_DEALS_DIR`: where deal recordings are written.
    pub deals_dir: Option<String>,
    /// `QUANTICK_INDICATORS_DIR`: the indicator script library.
    pub indicators_dir: Option<String>,
    /// `QUANTICK_TRADES_DIR`: the paper-trading journal folder.
    pub trades_dir: Option<OsString>,
    /// `QUANTICK_BUBBLES`: an explicit bubble presets file.
    pub bubbles: Option<OsString>,
}

/// Everything the executable reads before it builds anything.
#[derive(Default)]
pub(crate) struct LaunchConfig {
    pub log_format: LogFormat,
    /// `QUANTICK_CONFIG`: an explicit feeds file.
    pub config_path: Option<PathBuf>,
    /// `QUANTICK_DEFAULT_FEED` and `QUANTICK_DEFAULT_SYMBOL`, raw.
    feed: Option<OsString>,
    symbol: Option<OsString>,
    /// `QUANTICK_BOOK_DEPTH`, raw; the feed crate parses and clamps it.
    pub book_depth: Option<String>,
    pub paths: OperatorPaths,
    #[cfg(any(feature = "scenario-harness", test))]
    pub window: window::WindowHooks,
}

impl LaunchConfig {
    /// Read every configuration input, once, through `lookup`.
    pub fn capture(mut lookup: impl FnMut(&str) -> Option<OsString>) -> Self {
        let mut text = |name: &str| lookup(name).and_then(|value| value.into_string().ok());
        let log_format = if text("QUANTICK_LOG_FORMAT")
            .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        {
            LogFormat::Json
        } else {
            LogFormat::Text
        };
        let book_depth = text("QUANTICK_BOOK_DEPTH");
        let replay_dir = text("QUANTICK_REPLAY_DIR");
        let deals_dir = text("QUANTICK_DEALS_DIR");
        let indicators_dir = text("QUANTICK_INDICATORS_DIR");
        Self {
            log_format,
            config_path: lookup("QUANTICK_CONFIG").map(PathBuf::from),
            feed: lookup("QUANTICK_DEFAULT_FEED"),
            symbol: lookup("QUANTICK_DEFAULT_SYMBOL"),
            book_depth,
            paths: OperatorPaths {
                replay_dir,
                deals_dir,
                indicators_dir,
                trades_dir: lookup("QUANTICK_TRADES_DIR"),
                bubbles: lookup("QUANTICK_BUBBLES"),
            },
            #[cfg(any(feature = "scenario-harness", test))]
            window: window::WindowHooks::capture(&mut lookup),
        }
    }

    /// Whether this run named its opening market. An explicit request for one
    /// run wins over the market the saved workspace was last on.
    pub fn names_market(&self) -> bool {
        self.feed.is_some() || self.symbol.is_some()
    }

    /// The requested opening feed and symbol, as text.
    ///
    /// # Errors
    ///
    /// A value that is not Unicode names its variable.
    pub fn selection(&self) -> Result<(Option<String>, Option<String>), StartupSelectionError> {
        let unicode = |value: &Option<OsString>, variable: &'static str| match value {
            None => Ok(None),
            Some(raw) => raw
                .to_str()
                .map(|text| Some(text.to_owned()))
                .ok_or(StartupSelectionError::NonUnicode { variable }),
        };
        Ok((
            unicode(&self.feed, "QUANTICK_DEFAULT_FEED")?,
            unicode(&self.symbol, "QUANTICK_DEFAULT_SYMBOL")?,
        ))
    }

    /// Apply the requested opening market to a loaded config; see
    /// [`crate::config::apply_startup_selection`].
    ///
    /// # Errors
    ///
    /// A value that is not Unicode, or a pair the catalog does not offer.
    pub fn apply_selection(&self, config: &mut AppConfig) -> Result<(), StartupSelectionError> {
        let (feed, symbol) = self.selection()?;
        crate::config::apply_startup_selection(config, feed.as_deref(), symbol.as_deref())
    }

    /// The native window this run opens: the saved size (see
    /// [`Self::window_size`]), the title and the bundled icon.
    pub fn native_options(&self, saved: Option<[f32; 2]>) -> eframe::NativeOptions {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .expect("bundled assets/icon.png is a valid PNG");
        eframe::NativeOptions {
            // No `with_min_inner_size`: the window has no floor. Below roughly
            // 900x560 the chrome stops collapsing and starts clipping — the
            // drawing rail falls past its Minimal stage
            // (docs/drawing-toolbar-ux.md §2.8) — but that is a layout that
            // reads badly, not one that breaks, and a trader parking the chart
            // in a sliver beside another window is a real thing to want. The
            // one place a floor is still kept is what the app *reopens* at;
            // see [`REOPEN_FLOOR_PX`].
            viewport: eframe::egui::ViewportBuilder::default()
                .with_inner_size(self.window_size(saved))
                .with_title("quantick")
                .with_icon(icon),
            ..Default::default()
        }
    }

    /// The size the window opens at: the saved one, floored; or, in a build
    /// with the scenario harness, the size a validation run asked for.
    pub fn window_size(&self, saved: Option<[f32; 2]>) -> [f32; 2] {
        #[cfg(any(feature = "scenario-harness", test))]
        if let Some(size) = self.window.requested_size() {
            return size;
        }
        restore_size(saved)
    }
}

/// Why this session must write no store: `Some` when the environment's
/// variable names (`environment`) include a `QUANTICK_*` not `registered`, all
/// of them logged in one line and carried by name. `main` asks before any
/// store is read or written and, on `Some`, refuses every store write for the
/// session.
///
/// A capture run points each store at scratch through a harness hook; against
/// a build without that hook, the run would otherwise save over the trader's
/// real cockpit (decision DS7). Refusing the session's writes, rather than
/// guessing which variable meant what, is the one answer that cannot write
/// half of it. Only the prefix is spelled here: no hook name is.
pub(crate) fn persistence_refusal<'a>(
    environment: impl Iterator<Item = &'a str>,
    registered: &std::collections::BTreeSet<&'static str>,
) -> Option<crate::store_home::WritesRefused> {
    let unknown = crate::hooks::unknown_hooks(environment, registered);
    if unknown.is_empty() {
        return None;
    }
    let names = unknown.join(", ");
    tracing::warn!(
        target: "quantick::app",
        event_code = "UNKNOWN_HOOK",
        hooks = %names,
        action = "store_writes_refused",
        "this build reads none of these; nothing is saved this session. \
         Check the spelling against .claude/skills/ui-harness/references/hook-registry.md; \
         harness hooks need a `--features harness` build"
    );
    Some(crate::store_home::WritesRefused { hooks: unknown })
}

static OPERATOR_PATHS: std::sync::OnceLock<OperatorPaths> = std::sync::OnceLock::new();

/// Hand the stores their folders. Called once by `main`, before any store is
/// read; a second call is ignored.
pub(crate) fn install(paths: OperatorPaths) {
    let _ = OPERATOR_PATHS.set(paths);
}

/// The operator's folders for this run. Nothing installed — every test —
/// means no override, so a test never reads the trader's own environment.
pub(crate) fn operator_paths() -> &'static OperatorPaths {
    OPERATOR_PATHS.get_or_init(OperatorPaths::default)
}

crate::hooks::declare_hooks![
    "QUANTICK_BOOK_DEPTH",
    "QUANTICK_BUBBLES",
    "QUANTICK_CONFIG",
    "QUANTICK_DEALS_DIR",
    "QUANTICK_DEFAULT_FEED",
    "QUANTICK_DEFAULT_SYMBOL",
    "QUANTICK_INDICATORS_DIR",
    "QUANTICK_LOG_FORMAT",
    "QUANTICK_REPLAY_DIR",
    "QUANTICK_TRADES_DIR"
];

/// Size the window opens at when nothing asks for another.
const DEFAULT_WINDOW_PX: [f32; 2] = [1100.0, 650.0];
/// Smallest window the app will *reopen* at, whatever the last session left
/// behind.
///
/// The window itself has no minimum — it drags down to nothing, which is the
/// point. But a size is remembered across launches, and a chart squeezed to a
/// sliver and then closed would come back as a sliver: a window with no title
/// bar to grab and no edge to find. That is a trap the trader cannot get out
/// of from inside the app, so the *restore* path floors what it reads.
///
/// Small enough to be a deliberately tiny window, large enough to have an edge
/// and a title bar to drag. It is a recovery floor, not a layout one: nothing
/// about the chrome is promised at this size.
const REOPEN_FLOOR_PX: [f32; 2] = [320.0, 240.0];

/// The size a saved workspace reopens at, floored at [`REOPEN_FLOOR_PX`].
///
/// Not to protect the layout, which is free to be cramped, but so a session
/// closed on a sliver of a window reopens on something the trader can grab.
/// It is the whole of the restore policy and reads no environment.
fn restore_size(saved: Option<[f32; 2]>) -> [f32; 2] {
    saved.map_or(DEFAULT_WINDOW_PX, |[width, height]| {
        [
            width.max(REOPEN_FLOOR_PX[0]),
            height.max(REOPEN_FLOOR_PX[1]),
        ]
    })
}

#[cfg(test)]
mod launch_config_tests {
    use super::*;

    #[test]
    fn captured_inputs_survive_changes_to_the_lookup_source() {
        let mut values = std::collections::BTreeMap::from([
            ("QUANTICK_LOG_FORMAT", OsString::from("JSON")),
            ("QUANTICK_CONFIG", OsString::from("feeds.toml")),
            ("QUANTICK_DEFAULT_SYMBOL", OsString::from("BTC")),
            ("QUANTICK_BOOK_DEPTH", OsString::from("250")),
            ("QUANTICK_TRADES_DIR", OsString::from("journal")),
            ("QUANTICK_BUBBLES", OsString::from("bubbles.toml")),
        ]);
        let launch = LaunchConfig::capture(|name| values.get(name).cloned());
        values.clear();
        assert_eq!(launch.log_format, LogFormat::Json);
        assert_eq!(launch.config_path, Some(PathBuf::from("feeds.toml")));
        assert!(launch.names_market());
        assert_eq!(
            launch.selection().expect("unicode"),
            (None, Some("BTC".to_owned()))
        );
        assert_eq!(launch.book_depth.as_deref(), Some("250"));
        assert_eq!(launch.paths.trades_dir, Some(OsString::from("journal")));
        assert_eq!(launch.paths.bubbles, Some(OsString::from("bubbles.toml")));
        assert_eq!(launch.paths.replay_dir, None);
    }

    /// Every name the root reads is declared: here as configuration, or in
    /// `window` as a gated hook. Nothing is read that the registry cannot name.
    #[test]
    fn every_launch_read_is_declared() {
        let mut reads = Vec::new();
        let _ = LaunchConfig::capture(|name| {
            reads.push(name.to_owned());
            None
        });
        let declared: Vec<&str> = HOOKS
            .iter()
            .chain(window::HOOKS)
            .map(|spec| spec.name)
            .collect();
        for name in &reads {
            assert!(
                declared.contains(&name.as_str()),
                "{name} is read at launch but declared nowhere"
            );
        }
        for spec in HOOKS {
            assert!(reads.iter().any(|name| name == spec.name), "{}", spec.name);
        }
    }

    #[test]
    fn absent_and_invalid_values_preserve_defaults() {
        for raw in [None, Some(""), Some("true"), Some(" 1 "), Some("JSON ")] {
            let launch = LaunchConfig::capture(|name| {
                (name == "QUANTICK_LOG_FORMAT")
                    .then_some(raw)
                    .flatten()
                    .map(OsString::from)
            });
            assert_eq!(launch.log_format, LogFormat::Text);
            assert!(!launch.names_market());
        }
        for raw in ["JSON", "json", "JsOn"] {
            let launch =
                LaunchConfig::capture(|name| (name == "QUANTICK_LOG_FORMAT").then(|| raw.into()));
            assert_eq!(launch.log_format, LogFormat::Json);
        }
        let launch = LaunchConfig::default();
        assert_eq!(launch.window_size(None), DEFAULT_WINDOW_PX);
        assert_eq!(launch.window_size(Some([900.0, 700.0])), [900.0, 700.0]);
    }

    #[cfg(windows)]
    #[test]
    fn invalid_unicode_is_ignored_or_refused_by_name() {
        use std::os::windows::ffi::OsStringExt;
        let invalid = OsString::from_wide(&[0xD800]);
        let launch = LaunchConfig::capture(|_| Some(invalid.clone()));
        assert_eq!(launch.log_format, LogFormat::Text);
        assert_eq!(launch.book_depth, None);
        assert_eq!(launch.window_size(None), DEFAULT_WINDOW_PX);
        assert!(matches!(
            launch.selection(),
            Err(StartupSelectionError::NonUnicode {
                variable: "QUANTICK_DEFAULT_FEED"
            })
        ));
    }

    /// The window drags to nothing, but a session closed on nothing must not
    /// reopen on nothing: the restore path is the one place a floor survives,
    /// and it is a recovery floor, not a layout one.
    #[test]
    fn a_saved_sliver_reopens_on_something_the_trader_can_grab() {
        assert_eq!(restore_size(Some([0.0, 0.0])), REOPEN_FLOOR_PX);
        assert_eq!(
            restore_size(Some([4.0, 900.0])),
            [REOPEN_FLOOR_PX[0], 900.0]
        );
        assert_eq!(restore_size(Some([1280.0, 720.0])), [1280.0, 720.0]);
        assert_eq!(restore_size(None), DEFAULT_WINDOW_PX);
    }
}
