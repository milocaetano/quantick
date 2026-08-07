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
//! records those as [`LayerActions`] and the app settles them.
//!
//! One layer is switchable here but persisted elsewhere: the live lane's marks
//! belong to the order-flow preset, which a feed may declare and the dock
//! saves as a unit. Writing them here too would give one field two files to
//! disagree over, so the preset stays their home.
//!
//! The file records each layer's *state*, not a list of hidden ones, because
//! the layers do not share one default — the heatmap, the bubbles, the live
//! strip and the backfill divider open off, everything else opens on. An
//! absent entry therefore means
//! "whatever the app decided", which is what keeps a fresh install, a feed's
//! preset and the autostart env vars behaving exactly as they did before this
//! file existed.
//!
//! Same store discipline as the indicator state: a versioned TOML next to the
//! config (override with `QUANTICK_CHART_LAYERS`), read once at startup,
//! written when a switch flips, temp-file-and-rename so a crash mid-write
//! cannot leave half a file behind. Anything unreadable is ignored entirely.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Environment override for the layer-visibility file location.
const LAYERS_ENV: &str = "QUANTICK_CHART_LAYERS";
/// Default file, next to the working directory's config.
const LAYERS_FILE: &str = "chart-layers.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;

/// One switchable layer on the chart canvas.
///
/// Ordered as the menu shows them: what the market drew, then the chart's own
/// chrome, then what the user put on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ChartLayer {
    /// Resting depth behind the candles.
    Heatmap,
    /// Aggression bubbles from the trade stream.
    Bubbles,
    /// The per-price buy/sell ladder inside each candle, detail following
    /// zoom (see `crate::footprint_render`).
    Footprint,
    /// The book-and-aggression strip beside the price axis.
    LiveStrip,
    /// The live lane's boundary and live-edge lines.
    LaneMarks,
    /// Dashed boundaries around intervals with no depth coverage.
    DepthGaps,
    /// Price/time gridlines.
    Grid,
    /// The dashed last-price line and its chip.
    LastPrice,
    /// The mark where backfilled history ends and live bars begin.
    BackfillDivider,
    /// The mark where venue candles give way to bars built from prints.
    SeamDivider,
    /// The hover crosshair and its axis tags.
    Crosshair,
    /// Simulated orders, the position line and their chips.
    PaperTrading,
    /// Entry/exit marks for closed simulated trades and their connectors.
    TradePaint,
    /// User drawings (lines, boxes, Fibs).
    Drawings,
}

impl ChartLayer {
    /// Every layer, in menu order. A variant missing from here has no switch
    /// and cannot be turned off at all, so a new one belongs in this list and
    /// in the menu test that counts it. The two paper layers sit together:
    /// live orders and closed-trade marks are switched apart, because hiding
    /// history must never hide the position you are in.
    pub(crate) const ALL: [Self; 14] = [
        Self::Heatmap,
        Self::Bubbles,
        Self::Footprint,
        Self::LiveStrip,
        Self::LaneMarks,
        Self::DepthGaps,
        Self::Grid,
        Self::LastPrice,
        Self::BackfillDivider,
        Self::SeamDivider,
        Self::Crosshair,
        Self::PaperTrading,
        Self::TradePaint,
        Self::Drawings,
    ];

    /// Stable identifier used in the state file. Never renamed without a
    /// version bump: the file is hand-editable and an unknown id is dropped.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::Heatmap => "heatmap",
            Self::Bubbles => "bubbles",
            Self::Footprint => "footprint",
            Self::LiveStrip => "live_strip",
            Self::LaneMarks => "lane_marks",
            Self::DepthGaps => "depth_gaps",
            Self::Grid => "grid",
            Self::LastPrice => "last_price",
            Self::BackfillDivider => "backfill_divider",
            Self::SeamDivider => "seam_divider",
            Self::Crosshair => "crosshair",
            Self::PaperTrading => "paper_trading",
            Self::TradePaint => "trade_paint",
            Self::Drawings => "drawings",
        }
    }

    /// The reverse of [`Self::id`]; `None` for anything this build does not
    /// know, so a file written by a newer version degrades instead of failing.
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|layer| layer.id() == id)
    }

    /// Menu label.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Heatmap => "L2 heatmap",
            Self::Bubbles => "aggression bubbles",
            Self::Footprint => "candle footprint",
            Self::LiveStrip => "live strip",
            Self::LaneMarks => "live lane marks",
            Self::DepthGaps => "L2 gap boundaries",
            Self::Grid => "grid",
            Self::LastPrice => "last price line",
            Self::BackfillDivider => "backfill divider",
            Self::SeamDivider => "venue/prints seam",
            Self::Crosshair => "crosshair",
            Self::PaperTrading => "paper orders & position",
            Self::TradePaint => "closed trade marks",
            Self::Drawings => "drawings",
        }
    }

    /// Hover text: what disappears, and what keeps running while it is hidden.
    pub(crate) const fn hint(self) -> &'static str {
        match self {
            Self::Heatmap => {
                "resting depth behind the candles. Recording never stops, so hiding it loses no history"
            }
            Self::Bubbles => "confirmed executions from the trade stream, drawn where they printed",
            Self::Footprint => {
                "the buy/sell split at each price inside every candle. Detail follows zoom: \
                 numbers close in, profile and highlight marks further out. The legend names \
                 the rows' price width at all times"
            }
            Self::LiveStrip => {
                "the book's resting depth and the forming bar's aggression, beside the price axis"
            }
            Self::LaneMarks => {
                "the dashed line where the bar slots end and the tape begins, and the line on the \
                 live edge itself. Saved with the order-flow preset, not with the other layers"
            }
            // Data honesty: this is the one switch that hides a statement
            // about missing data rather than data itself, so the entry says
            // what its absence will mean.
            Self::DepthGaps => {
                "dashed boundaries around intervals with no depth coverage. Off, a stretch the \
                 recorder never saw looks exactly like one it did. Drawn over the heatmap, so \
                 they only show while it is on"
            }
            Self::Grid => "price and time gridlines behind the candles",
            Self::LastPrice => "the dashed line at the last traded price, and its chip on the axis",
            Self::BackfillDivider => {
                "where backfilled history ends and bars built live begin. Off by default: a rule \
                 across every candle for a boundary that is worth reading once"
            }
            Self::SeamDivider => "where venue candles give way to bars built from prints",
            Self::Crosshair => "the hover cross and its price/time tags",
            // Same honesty rule as the gap boundaries: what is hidden here is a
            // drawing of live state, so the entry says the state is still live.
            Self::PaperTrading => {
                "simulated entries, exits and the open position. Hiding them draws nothing and \
                 cancels nothing — the orders stay working and the dock still shows them"
            }
            Self::TradePaint => {
                "entry and exit marks for closed simulated trades, joined by a faint line. \
                 Hiding them draws nothing and forgets nothing — the trades stay in the \
                 ledger and on disk"
            }
            Self::Drawings => {
                "everything drawn by hand. Hidden objects keep their anchors and stop answering \
                 the pointer until the layer is shown again"
            }
        }
    }

    /// Whether this file is the layer's persistence home.
    ///
    /// The lane marks live in the order-flow preset — a feed can declare one,
    /// and the dock saves it as a unit. Storing them here as well would let two
    /// files disagree about one field.
    pub(crate) const fn persisted(self) -> bool {
        !matches!(self, Self::LaneMarks)
    }
}

/// What a pane's layer menu asked of the app, drained once the canvas is done.
///
/// Two entries are not the pane's to switch. The grid belongs to the window's
/// shared [`crate::style::ChartStyle`], which the appearance panel also edits;
/// hiding an indicator has to reach the state file the app persists. Rather
/// than hand the pane a second copy of either — the one thing this menu refuses
/// to do — it records the wish and the app applies it to the real owner.
#[derive(Debug, Default)]
pub(crate) struct LayerActions {
    /// The grid was switched to this.
    pub(crate) grid: Option<bool>,
    /// An indicator was hidden or shown, so the indicator state needs saving.
    pub(crate) indicators_changed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct LayersFile {
    version: u32,
    /// Each layer's visibility by [`ChartLayer::id`]. An absent layer keeps
    /// whatever the app decided this launch (default, preset or autostart).
    #[serde(default)]
    layers: BTreeMap<String, bool>,
}

/// The layer-visibility file the app opens with and writes back to.
///
/// Under test it is a scratch file of its own per app instead. The app's own
/// tests build many apps and draw many frames in one process, and a store
/// rooted in the working directory would have them restoring one another's
/// canvas — and rewriting the repo's copy while they did it.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return scratch_path();
    }
    std::env::var_os(LAYERS_ENV).map_or_else(|| PathBuf::from(LAYERS_FILE), PathBuf::from)
}

/// A store of its own, for an app built by a test. See [`default_path`].
fn scratch_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "quantick-chart-layers-{}-{}.toml",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Load the stored visibility; empty (change nothing) when the file is
/// missing, unreadable or from an unknown version. Unknown ids are dropped one
/// by one — a file from a newer build still restores the layers this one has.
#[must_use]
pub(crate) fn load(path: &Path) -> BTreeMap<ChartLayer, bool> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let stored = match toml::from_str::<LayersFile>(&text) {
        Ok(file) if file.version == FORMAT_VERSION => file.layers,
        Ok(file) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYERS_VERSION",
                path = %path.display(),
                version = file.version,
                action = "keeping_default_visibility",
                "chart layer file is from an unknown version"
            );
            return BTreeMap::new();
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYERS_UNREADABLE",
                path = %path.display(),
                %error,
                action = "keeping_default_visibility",
                "chart layer file is unreadable"
            );
            return BTreeMap::new();
        }
    };
    stored
        .into_iter()
        .filter_map(|(id, visible)| {
            ChartLayer::from_id(&id)
                .filter(|layer| layer.persisted())
                .map(|layer| (layer, visible))
        })
        .collect()
}

/// Write the current visibility of every layer this file owns.
pub(crate) fn save(path: &Path, states: &BTreeMap<ChartLayer, bool>) {
    let file = LayersFile {
        version: FORMAT_VERSION,
        layers: states
            .iter()
            .filter(|(layer, _)| layer.persisted())
            .map(|(layer, visible)| (layer.id().to_owned(), *visible))
            .collect(),
    };
    let Ok(text) = toml::to_string_pretty(&file) else {
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

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("quantick-chart-layers-test");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ids_are_unique_and_round_trip_through_from_id() {
        let mut ids: Vec<&str> = ChartLayer::ALL.iter().map(|layer| layer.id()).collect();
        ids.sort_unstable();
        let count = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), count, "two layers share a persisted id");
        for layer in ChartLayer::ALL {
            assert_eq!(ChartLayer::from_id(layer.id()), Some(layer));
        }
        assert_eq!(ChartLayer::from_id("no_such_layer"), None);
    }

    #[test]
    fn a_missing_file_changes_nothing() {
        assert!(
            load(&temp_dir().join("missing.toml")).is_empty(),
            "a fresh install keeps every layer's own default"
        );
    }

    #[test]
    fn visibility_round_trips_through_disk() {
        let path = temp_dir().join("round-trip.toml");
        let states = BTreeMap::from([
            (ChartLayer::Crosshair, false),
            (ChartLayer::Grid, false),
            (ChartLayer::Heatmap, true),
            (ChartLayer::Drawings, true),
        ]);
        save(&path, &states);
        assert_eq!(load(&path), states);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn preset_owned_layers_are_never_written_here() {
        let path = temp_dir().join("preset-owned.toml");
        save(
            &path,
            &BTreeMap::from([(ChartLayer::LaneMarks, false), (ChartLayer::Grid, false)]),
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("lane_marks"),
            "the order-flow preset owns the lane marks; this file must not claim them too:\n{text}"
        );
        assert_eq!(
            load(&path),
            BTreeMap::from([(ChartLayer::Grid, false)]),
            "everything else still round-trips"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unknown_versions_ids_and_garbage_degrade_instead_of_failing() {
        let path = temp_dir().join("bad-layers.toml");
        std::fs::write(&path, "version = 99\n[layers]\ngrid = false\n").unwrap();
        assert!(load(&path).is_empty(), "unknown version changes nothing");
        std::fs::write(&path, "not even toml [").unwrap();
        assert!(load(&path).is_empty(), "garbage changes nothing");
        std::fs::write(
            &path,
            "version = 1\n[layers]\ngrid = false\nfrom_the_future = true\n",
        )
        .unwrap();
        assert_eq!(
            load(&path),
            BTreeMap::from([(ChartLayer::Grid, false)]),
            "an id this build does not know is dropped, the rest still restores"
        );
        std::fs::remove_file(&path).ok();
    }
}
