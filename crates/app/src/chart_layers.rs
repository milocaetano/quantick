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
//! The tape's own three — whether it is on the canvas at all, and which of its
//! two flow layers it draws — *are* stored here, and for the opposite reason:
//! the preset explicitly refuses to carry them (a look may not switch a layer),
//! which left them with no home at all and the tape opening on its defaults
//! every launch. They resolve to the fields on
//! [`crate::orderflow::LiveLaneStyle`], same as every other entry resolves to
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

use serde::{Deserialize, Serialize};

/// What a launch opens with when the trader has no file of their own, shipped
/// as config rather than decided by a field initialiser: which layers a fresh
/// chart draws is a product decision someone may want different, and burying
/// it in a `Default` impl puts it where no one can change it without a build.
/// Same discipline as `feeds.toml` and `bubbles.toml`.
const EMBEDDED_DEFAULT: &str = include_str!("../config/chart-layers.toml");

/// Environment override for the layer-visibility file location.
pub(crate) const LAYERS_ENV: &str = "QUANTICK_CHART_LAYERS";
/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub(crate) const LAYERS_FILE: &str = "chart-layers.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;

/// One switchable layer on the chart canvas.
///
/// Two panes' worth. The tape's three come first, then the candles'; inside
/// each group the order is the order that pane's menu shows them — for the
/// candles, what the market drew, then the chart's own chrome, then what the
/// user put on top. [`ChartLayer::on_tape`] is what splits the list between the
/// two menus, so neither ever offers a switch for the canvas beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ChartLayer {
    /// Whether the tape is on the canvas at all. Off, the lane reserves no
    /// width and the candles take the whole canvas.
    TapeChart,
    /// Resting depth on the tape — the candles' [`Self::Heatmap`] is a
    /// different switch, and neither follows the other.
    TapeHeatmap,
    /// Aggression bubbles on the tape. Twin of [`Self::Bubbles`], same rule.
    TapeBubbles,
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
    /// The compact visual key at the canvas's top-left corner.
    FlowLegend,
    /// The book's status badge at the canvas's top-right corner.
    BookStatus,
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
    pub(crate) const ALL: [Self; 19] = [
        Self::TapeChart,
        Self::TapeHeatmap,
        Self::TapeBubbles,
        Self::Heatmap,
        Self::Bubbles,
        Self::Footprint,
        Self::LiveStrip,
        Self::LaneMarks,
        Self::FlowLegend,
        Self::BookStatus,
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

    /// One bit per layer is how visibility change is detected
    /// ([`crate::pane::ChartPane::layer_mask`]), and the bit is the index into
    /// [`Self::ALL`] — so the list may never outgrow that accumulator. Past it
    /// the shift is undefined: a panic in debug, colliding persistence bits in
    /// release. Adding the 33rd layer fails the build here instead.
    #[expect(
        dead_code,
        reason = "a const assertion is evaluated at compile time; nothing reads it at runtime"
    )]
    const MASK_FITS: () = assert!(Self::ALL.len() <= u32::BITS as usize);

    /// Stable identifier used in the state file. Never renamed without a
    /// version bump: the file is hand-editable and an unknown id is dropped.
    pub(crate) const fn id(self) -> &'static str {
        match self {
            Self::TapeChart => "tape_chart",
            Self::TapeHeatmap => "tape_heatmap",
            Self::TapeBubbles => "tape_bubbles",
            Self::Heatmap => "heatmap",
            Self::Bubbles => "bubbles",
            Self::Footprint => "footprint",
            Self::LiveStrip => "live_strip",
            Self::LaneMarks => "lane_marks",
            Self::FlowLegend => "flow_legend",
            Self::BookStatus => "book_status",
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
    ///
    /// The tape's two flow layers deliberately wear the same words as the
    /// candles': under the menu's "tape" heading they can only mean the tape's,
    /// and giving one concept two names would be worse than the repetition.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::TapeChart => "show the tape",
            Self::TapeHeatmap => "L2 heatmap",
            Self::TapeBubbles => "aggression bubbles",
            Self::Heatmap => "L2 heatmap",
            Self::Bubbles => "aggression bubbles",
            Self::Footprint => "candle footprint",
            Self::LiveStrip => "live strip",
            Self::LaneMarks => "live lane marks",
            // The same words the L2 panel's checkbox uses: one switch may not
            // have two names.
            Self::FlowLegend => "chart legend",
            Self::BookStatus => "book status badge",
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
            // The tape's three. The first says what the band costs and what
            // coming back is worth; the other two say, plainly, that the
            // toolbar is not their switch — the question a trader asks after
            // clicking the toolbar's L2 button and watching the tape ignore it.
            Self::TapeChart => {
                "the rolling tape pinned to the right edge: prints landing into the book in real \
                 time, on their own fixed window of market time. Off, the band is not reserved at \
                 all and the candles take the whole canvas; its two layers keep their settings, so \
                 switching it back on returns the tape you switched off"
            }
            Self::TapeHeatmap => {
                "resting depth on the tape. The toolbar's L2 button does not touch this — it is \
                 the candles' switch. Recording never stops either way, so hiding this loses no \
                 history"
            }
            Self::TapeBubbles => {
                "confirmed executions rolling through the tape, drawn where they printed. The \
                 toolbar's bubble button does not touch this — it is the candles' switch"
            }
            // Both of these name the candles explicitly, and say where the
            // other copy is: the tape has switches of its own, so a trader who
            // clears the candles and still sees the book rolling on the tape
            // is looking at a setting, not at a bug.
            Self::Heatmap => {
                "resting depth behind the candles. Recording never stops, so hiding it loses no                  history. The tape has a switch of its own and this one never moves it —                  right-click the tape to reach it"
            }
            Self::Bubbles => {
                "confirmed executions from the trade stream, drawn where they printed, on the                  candles. The tape has a switch of its own and this one never moves it —                  right-click the tape to reach it"
            }
            Self::Footprint => {
                "the buy/sell split at each price inside every candle. Detail follows zoom: \
                 numbers close in, profile and highlight marks further out. The violet line \
                 is the bar's point of control (the price with the most volume); an Nx badge \
                 at a bar's extreme is its aggression ratio there. The legend names the rows' \
                 price width at all times"
            }
            Self::LiveStrip => {
                "the book's resting depth and the forming bar's aggression, beside the price axis"
            }
            Self::LaneMarks => {
                "the dashed line where the bar slots end and the tape begins, and the line on the \
                 live edge itself. Saved with the order-flow preset, not with the other layers"
            }
            // Chrome, not market data: both of these draw *about* the canvas
            // rather than on it, so their entries say what stays true while
            // they are hidden.
            Self::FlowLegend => {
                "the key at the top-left naming every flow layer that is on. Hiding it changes \
                 nothing about what is drawn — the layers keep drawing, and the key comes back \
                 with the same entries. The same switch as the L2 panel's 'show chart legend'"
            }
            Self::BookStatus => {
                "the badge at the top-right reporting on the book feed. Hiding it silences a \
                 label, never the recorder: capture, generation and the ladder carry on, and the \
                 L2 panel still states them. A book that goes down or errors brings the badge \
                 back on its own — hidden chrome may not hide a dead feed"
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

    /// Whether this layer belongs to the tape rather than to the candles.
    ///
    /// The one place the split is declared. Each pane's menu iterates the list
    /// through this, so adding a layer to the wrong group is the only way to
    /// get it onto the wrong menu — there is no second list to keep in step.
    pub(crate) const fn on_tape(self) -> bool {
        matches!(
            self,
            Self::TapeChart | Self::TapeHeatmap | Self::TapeBubbles
        )
    }

    /// Whether this file is the layer's persistence home.
    ///
    /// The lane marks live in the order-flow preset — a feed can declare one,
    /// and the dock saves it as a unit. Storing them here as well would let two
    /// files disagree about one field. The legend and the status badge are
    /// persisted here instead: the order-flow preset does not carry them, and
    /// their state has a single owner (the order-flow config) that this file
    /// merely restores — exactly as it does for the heatmap and the bubbles.
    /// The tape's three are here for the same reason, and because the preset
    /// refuses them by design.
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
    /// A footprint knob moved, so the footprint settings need saving — the
    /// config is the window's (one set of thresholds, every pane), so the
    /// write is the app's, same as the grid.
    pub(crate) footprint_changed: bool,
    /// The menu asked for the footprint settings window; the window is the
    /// app's, like every window.
    pub(crate) open_footprint_settings: bool,
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
        return crate::store_home::test_path(LAYERS_FILE);
    }
    crate::store_home::resolve(LAYERS_ENV, LAYERS_FILE)
}

/// Parse a layer-visibility file, reporting why it is not one. The gate a
/// bundle section goes through — see [`crate::workspace_bundle`].
pub(crate) fn validate(text: &str) -> Result<(), String> {
    let file: LayersFile = toml::from_str(text).map_err(|error| error.to_string())?;
    if file.version == FORMAT_VERSION {
        Ok(())
    } else {
        Err(format!(
            "chart-layers format version {} (this build reads {FORMAT_VERSION})",
            file.version
        ))
    }
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
    let stored = match toml::from_str::<LayersFile>(&text) {
        Ok(file) if file.version == FORMAT_VERSION => file.layers,
        Ok(file) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CHART_LAYERS_VERSION",
                path = %path.display(),
                version = file.version,
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
    states.extend(resolve(stored));
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
    match toml::from_str::<LayersFile>(EMBEDDED_DEFAULT) {
        Ok(file) if file.version == FORMAT_VERSION => resolve(file.layers),
        _ => BTreeMap::new(),
    }
}

/// Map stored ids onto the layers this build knows and this file owns.
///
/// An unknown id is dropped rather than failing the read, so a file written by
/// a newer build still restores the layers this one has.
fn resolve(stored: BTreeMap<String, bool>) -> BTreeMap<ChartLayer, bool> {
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
    fn a_missing_file_opens_on_the_shipped_default() {
        let fresh = load(&temp_dir().join("missing.toml"));
        assert_eq!(
            fresh,
            shipped_default(),
            "a fresh install opens on the config, not on a struct initialiser"
        );
        assert!(!fresh.is_empty(), "the shipped file has to reach the app");
    }

    /// The promise the shipped default is only acceptable because of: it
    /// decides for someone who has never touched a switch, and stops deciding
    /// the moment they do. A trader who switched the heatmap off and found it
    /// back on next launch would have lost the setting to a default, which is
    /// worse than the bare chart this whole change is about.
    #[test]
    fn the_traders_own_choice_outranks_the_shipped_default() {
        let path = temp_dir().join("trader-choice.toml");
        assert_eq!(
            shipped_default().get(&ChartLayer::Heatmap),
            Some(&true),
            "the layer has to be one the shipped file opens, or this proves nothing"
        );
        // A file with one switch in it, the way it lands after one click.
        std::fs::write(&path, "version = 1\n[layers]\nheatmap = false\n").unwrap();
        let loaded = load(&path);
        assert_eq!(
            loaded.get(&ChartLayer::Heatmap),
            Some(&false),
            "an explicit no stays a no"
        );
        // And the answer they did not give is the shipped one, not the code's
        // off baseline — see `load`. Switching the heatmap off must not take
        // the bubbles down with it.
        assert_eq!(
            loaded.get(&ChartLayer::Bubbles),
            Some(&true),
            "a layer the trader never touched opens on the shipped answer"
        );
        // A layer absent from *both* files is still the app's to decide, which
        // is what keeps the order-flow preset the only home of `lane_marks`.
        assert!(
            !loaded.contains_key(&ChartLayer::LaneMarks),
            "a layer the shipped file holds no opinion on stays the app's"
        );
        std::fs::remove_file(&path).ok();
    }

    /// The bug that made shipping the defaults change nothing for the trader
    /// who reported the bare chart in the first place.
    ///
    /// Their `chart-layers.toml` was written by a build from before the flow
    /// layers had shipped defaults, so it names the chrome and says nothing
    /// about the four layers the chart is for. Read as "whatever the code
    /// decides", that silence is *off* on all four — a file that has never
    /// been asked the question answering it anyway, on every launch, forever.
    ///
    /// This is the file, key for key, that `chart-layers.toml.bak` on the
    /// reporter's machine turned out to hold.
    #[test]
    fn a_file_from_before_the_defaults_still_opens_the_flow_layers() {
        let path = temp_dir().join("pre-defaults.toml");
        std::fs::write(
            &path,
            "version = 1
[layers]
             grid = true
             crosshair = true
             last_price = true
             backfill_divider = false
",
        )
        .unwrap();
        let loaded = load(&path);
        for layer in [
            ChartLayer::Heatmap,
            ChartLayer::Bubbles,
            ChartLayer::Footprint,
            ChartLayer::LiveStrip,
        ] {
            assert_eq!(
                loaded.get(&layer),
                Some(&true),
                "{} is unanswered in this file, so it opens on the shipped answer",
                layer.id()
            );
        }
        // What the file *does* say still wins, including its one no.
        assert_eq!(
            loaded.get(&ChartLayer::BackfillDivider),
            Some(&false),
            "the file's own answers are untouched"
        );
        std::fs::remove_file(&path).ok();
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
        let path = temp_dir().join("round-trip.toml");
        let states = BTreeMap::from([
            (ChartLayer::Crosshair, false),
            (ChartLayer::Grid, false),
            (ChartLayer::Heatmap, true),
            (ChartLayer::Drawings, true),
        ]);
        save(&path, &states);
        // Every answer written comes back, and the layers this file says
        // nothing about come back as the shipped answer rather than as the
        // code's off baseline — see `load`.
        assert_eq!(load(&path), shipped_with(&states));
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
            shipped_with(&BTreeMap::from([(ChartLayer::Grid, false)])),
            "everything else still round-trips, over the shipped answer"
        );
        assert!(
            !load(&path).contains_key(&ChartLayer::LaneMarks),
            "and the lane marks are in neither file, so they stay the app's"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unknown_versions_ids_and_garbage_degrade_instead_of_failing() {
        let path = temp_dir().join("bad-layers.toml");
        // A file this build cannot read means "we do not know what they
        // chose" — the same question a first launch asks, so it gets the same
        // answer rather than a second one.
        std::fs::write(&path, "version = 99\n[layers]\ngrid = false\n").unwrap();
        assert_eq!(
            load(&path),
            shipped_default(),
            "unknown version falls back to the shipped default"
        );
        std::fs::write(&path, "not even toml [").unwrap();
        assert_eq!(
            load(&path),
            shipped_default(),
            "garbage falls back to the shipped default"
        );
        std::fs::write(
            &path,
            "version = 1\n[layers]\ngrid = false\nfrom_the_future = true\n",
        )
        .unwrap();
        assert_eq!(
            load(&path),
            shipped_with(&BTreeMap::from([(ChartLayer::Grid, false)])),
            "an id this build does not know is dropped, the rest still restores"
        );
        std::fs::remove_file(&path).ok();
    }
}
