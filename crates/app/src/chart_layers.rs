//! Which chart layers are painted, and where each choice is persisted.
//!
//! The canvas stacks a dozen independent things — resting liquidity, candles,
//! bubbles, indicator plots, drawings, the axes' grid, the last-price line, the
//! backfill divider, the crosshair. The right-click layer menu is one place to
//! switch any of them off.
//!
//! It is deliberately *not* a second copy of their state. Each entry resolves
//! to the one field that already owns that layer (see
//! [`crate::pane::ChartPane::layer_visible`]), so the menu and the toolbar/dock
//! can never disagree about a pixel. This module only names the layers and
//! stores the switches nothing else stores.
//!
//! The layers belong to the pane that draws them (§11: the flow pane has the
//! tape and everything read off it, the time pane has neither), so the menu is
//! the pane's and each pane answers for its own canvas. Two of them are not the
//! pane's to switch — the grid lives in the window's shared chart style, and
//! hiding an indicator has to reach the state file the app owns — so the menu
//! records those as [`quantick_layers::LayerActions`] and the app settles them.
//!
//! One layer is switchable here but persisted elsewhere: the live lane's marks
//! belong to the order-flow preset, which a feed may declare and the dock
//! saves as a unit. Writing them here too would give one field two files to
//! disagree over, so the preset stays their home.
//!
//! The tape's own three — whether it is on the canvas at all, and which of its
//! two flow layers it draws — *are* stored here, and for the opposite reason:
//! the preset explicitly refuses to carry them (a look may not switch a layer),
//! which left them with no home at all and the tape opening on its defaults
//! every launch. They resolve to the fields on
//! [`quantick_orderflow::LiveLaneStyle`], same as every other entry resolves to
//! the one field that already owns its layer.
//!
//! The file records each layer's *state*, not a list of hidden ones, because
//! the layers do not share one default. An absent entry means "whatever the
//! app decided", which is what keeps a feed's preset and the autostart env
//! vars behaving exactly as they did before this file existed.
//!
//! What a *fresh* install opens with is `config/chart-layers.toml`, compiled
//! into the binary ([`EMBEDDED_DEFAULT`]) and read whenever the trader has no
//! file of their own. Opening state is a product decision someone may want
//! different, so it is shipped config like `feeds.toml` and `bubbles.toml`,
//! never a `Default` impl or a `set_*(false)` at startup — those put it where
//! it cannot be changed without a build. Today that file opens every flow
//! layer and holds back only the backfill divider.
//!
//! Same store discipline as the indicator state: a versioned TOML next to the
//! config (override with `QUANTICK_CHART_LAYERS`), read once at startup,
//! written when a switch flips, temp-file-and-rename so a crash mid-write
//! cannot leave half a file behind. Anything unreadable falls back to the
//! shipped default rather than to a half-read file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quantick_layers::{DocumentError, LayerDocument};

/// What a launch opens with when the trader has no file of their own, shipped
/// as config rather than decided by a field initialiser: which layers a fresh
/// chart draws is a product decision someone may want different, and burying
/// it in a `Default` impl puts it where no one can change it without a build.
/// Same discipline as `feeds.toml` and `bubbles.toml`.
const EMBEDDED_DEFAULT: &str = include_str!("../config/chart-layers.toml");

/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub(crate) const LAYERS_FILE: &str = "chart-layers.toml";

pub(crate) use quantick_layers::{ChartLayer, LayerBlock};
mod session;
pub(crate) use session::{maintain, restore};

/// The layer-visibility file the app opens with and writes back to.
///
/// Under test it is a scratch file of its own per app instead. The app's own
/// tests build many apps and draw many frames in one process, and a store
/// rooted in the working directory would have them restoring one another's
/// canvas — and rewriting the repo's copy while they did it.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(LAYERS_FILE);
    }
    crate::store_home::resolve(LAYERS_FILE)
}

/// Parse a layer-visibility file, reporting why it is not one. The gate a
/// bundle section goes through — see [`crate::workspace_bundle`].
pub(crate) fn validate(text: &str) -> Result<(), String> {
    LayerDocument::parse(text)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Load the stored visibility **over** [`shipped_default`], falling back to the
/// shipped answer alone when the file is missing, unreadable or from an unknown
/// version — all three mean "we do not know what this trader chose", which is
/// the question a first launch asks. Unknown ids are dropped one by one, so a
/// file from a newer build still restores the layers this one has.
///
/// The trader's file is laid *on top of* the shipped one rather than replacing
/// it, and that is the whole difference between this reader and the one that
/// shipped the defaults. A layer the file does not mention has no answer in it,
/// and the two candidate readings of that silence are not equal:
///
/// - "whatever the code decides" — which for all four flow layers is *off*
///   ([`crate::pane::ChartPane`]'s field initialisers, and the
///   `set_depth_visible(false)` at startup), so an older file pins them off
///   forever;
/// - "whatever this build ships" — the answer in `config/chart-layers.toml`.
///
/// The first reading is why shipping the defaults changed nothing for anyone
/// who already had a cockpit: their file predated the flow-layer keys, so it
/// answered "off" to a question it had never been asked. Worse, it then froze:
/// [`crate::app::QuantickApp::maintain_chart_layers`] rewrites the *whole* map
/// on the first switch of the session, so one unrelated click turns a silent
/// file into an explicit `heatmap = false`. A default nobody can receive is not
/// a default, so silence now means the shipped answer.
///
/// What that costs is precise and small: a layer absent from **both** files is
/// still the app's to decide — `lane_marks` is the one, deliberately, because
/// the order-flow preset is its home. And an explicit `false` in the trader's
/// file still outranks everything, which is the promise that makes a shipped
/// default acceptable at all (`the_traders_own_choice_outranks_the_shipped_default`).
#[must_use]
pub(crate) fn load(path: &Path) -> BTreeMap<ChartLayer, bool> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return shipped_default();
    };
    let stored = match LayerDocument::parse(&text) {
        Ok(file) => file,
        Err(DocumentError::Version(version)) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYERS_VERSION",
                path = %path.display(),
                version,
                action = "keeping_shipped_visibility",
                "chart layer file is from an unknown version"
            );
            return shipped_default();
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYERS_UNREADABLE",
                path = %path.display(),
                %error,
                action = "keeping_shipped_visibility",
                "chart layer file is unreadable"
            );
            return shipped_default();
        }
    };
    // The trader's answers over the shipped ones, never instead of them.
    let mut states = shipped_default();
    states.extend(stored.resolve(&crate::pane::registered_layers()));
    states
}

/// The built-in opening state, from the config compiled into the binary.
///
/// A launch with no file of the trader's own lands here, and so does one whose
/// file this build cannot read: an unreadable file means "we do not know what
/// they chose", which is the same question a first launch asks, and answering
/// it from the shipped file keeps one answer instead of two.
///
/// The file is tracked, so a parse failure is a build-time mistake rather than
/// anything a user did — `the_shipped_default_parses` is what catches it. At
/// runtime an empty map is the honest fallback: it means "the code decides",
/// exactly as it did before this file existed.
fn shipped_default() -> BTreeMap<ChartLayer, bool> {
    LayerDocument::restore(EMBEDDED_DEFAULT, None, &crate::pane::registered_layers())
}

/// Write the current visibility of every layer this file owns.
pub(crate) fn save(path: &Path, states: &BTreeMap<ChartLayer, bool>) {
    if crate::store_home::guard_write(path).is_err() {
        return;
    }
    let Ok(text) = LayerDocument::encode(states) else {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CHART_LAYERS_WRITE_FAILED",
            action = "layers_not_saved",
            "could not serialize the chart layer visibility"
        );
        return;
    };
    // Temp sibling + rename, for the same reason the indicator state does it:
    // a truncating write that dies halfway leaves a file that reads as garbage.
    let temp = path.with_extension("toml.tmp");
    if let Err(error) = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path)) {
        let _ = std::fs::remove_file(&temp);
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "CHART_LAYERS_WRITE_FAILED",
            path = %path.display(),
            %error,
            action = "layers_not_saved",
            "could not save the chart layer visibility"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of this test's own thread. It used to be one fixed folder
    /// shared by every test in this module *and* by every concurrent run on
    /// the machine — two worktrees running the suite at once wrote each
    /// other's fixtures. `thread_dir` rather than a `ScratchDir` because
    /// every caller spells `scratch().join(..)` in a single statement, where
    /// a value would be dropped before the file it names is written.
    fn scratch() -> PathBuf {
        crate::scratch::thread_dir("chart-layers")
    }

    #[test]
    fn a_missing_file_opens_on_the_shipped_default() {
        let fresh = load(&scratch().join("missing.toml"));
        assert_eq!(
            fresh,
            shipped_default(),
            "a fresh install opens on the config, not on a struct initialiser"
        );
        assert!(!fresh.is_empty(), "the shipped file has to reach the app");
    }

    /// The file is tracked and compiled in, so anything wrong with it is a
    /// build-time mistake — and one that would otherwise degrade silently into
    /// "the code decides", which is the state this file exists to end.
    #[test]
    fn the_shipped_default_parses_and_opens_the_flow_layers() {
        let shipped = shipped_default();
        for layer in [
            ChartLayer::Heatmap,
            ChartLayer::Bubbles,
            ChartLayer::Footprint,
            ChartLayer::LiveStrip,
        ] {
            assert_eq!(
                shipped.get(&layer),
                Some(&true),
                "{} is what the chart is for; a first launch draws it",
                layer.id()
            );
        }
        assert_eq!(
            shipped.get(&ChartLayer::BackfillDivider),
            Some(&false),
            "the one layer a fresh chart holds back"
        );
        // The compass, on. The bars here are cut by ticks and volume, so
        // without it the only way to learn when a candle happened is to arm
        // the crosshair tool — a fresh install that cannot answer "when was
        // this?" while you point at a candle is the gap this project's own
        // bar types create.
        for layer in [ChartLayer::PointerPrice, ChartLayer::PointerTime] {
            assert_eq!(
                shipped.get(&layer),
                Some(&true),
                "{} opens on, and each axis's menu is where it is switched off",
                layer.id()
            );
        }
        // Every layer this file owns is stated, so no reader has to know which
        // ones fall through to a struct initialiser.
        for layer in ChartLayer::ALL.into_iter().filter(|l| l.persisted()) {
            assert!(
                shipped.contains_key(&layer),
                "{} has no entry in config/chart-layers.toml",
                layer.id()
            );
        }
        assert!(
            !shipped.contains_key(&ChartLayer::LaneMarks),
            "the preset is the lane marks' only home"
        );
    }

    /// What `load` must return for a file holding exactly `states`: the
    /// trader's answers over the shipped ones. Written once because three
    /// tests need it and a fourth copy of `shipped_default().extend(...)` is
    /// three chances to encode the *old* contract by accident.
    fn shipped_with(states: &BTreeMap<ChartLayer, bool>) -> BTreeMap<ChartLayer, bool> {
        let mut expected = shipped_default();
        expected.extend(states.iter().filter(|(layer, _)| layer.persisted()));
        expected
    }

    #[test]
    fn visibility_round_trips_through_disk() {
        let path = scratch().join("round-trip.toml");
        let states = BTreeMap::from([
            (ChartLayer::Crosshair, false),
            (ChartLayer::Grid, false),
            (ChartLayer::Heatmap, true),
            (ChartLayer::Drawings, true),
            (ChartLayer::PointerPrice, true),
            (ChartLayer::PointerTime, false),
        ]);
        save(&path, &states);
        assert_eq!(load(&path), shipped_with(&states));
        std::fs::remove_file(&path).ok();
    }
}
