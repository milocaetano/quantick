//! What a tool is handed when it paints or hit-tests.

use eframe::egui;
use smallvec::SmallVec;

use crate::chart::PriceScale;

use super::{ChartPoint, DrawingPayload, DrawingStyle};

/// The screen-space grab points of one selected object. Six covers every tool
/// in the registry — the channel is the widest, with a corner and a centre on
/// each of its two rails — so the handle pass of the selected object
/// allocates nothing per frame.
pub type Handles = SmallVec<[egui::Pos2; 6]>;

/// The price-axis heights one drawing asks to be tagged at, in screen `y`.
///
/// Two, because the tools that declare a level today name exactly one and the
/// obvious next candidates (a price range's two edges) name two — so the pass
/// over every visible object allocates nothing per frame, the same reason
/// [`Handles`] is sized the way it is.
pub type AxisLevels = SmallVec<[f32; 2]>;

/// What an anchor's second coordinate means in the band being painted.
///
/// A tool that only projects never asks; a tool that *reads a number back to
/// the trader* has to, because `pts` and `%` are price words. A percent over
/// a signed cumulative series is not a smaller truth, it is a false one: a
/// move from -100 to +100 is not "-200%".
#[derive(Clone, Copy)]
pub enum ValueUnit<'a> {
    /// The instrument's price.
    Price,
    /// An indicator's own value, named by that pane's label.
    Indicator(&'a str),
}

/// Everything a tool may need beyond raw screen anchors when painting or
/// hit-testing: its own payload, the chart-space anchors and the price scale
/// that projected them (log-scaled tools compute prices, then project).
#[derive(Clone, Copy)]
pub struct DrawContext<'a> {
    pub payload: &'a dyn DrawingPayload,
    pub anchors: &'a [ChartPoint],
    pub scale: &'a PriceScale,
    /// Screen pixels one bar slot occupies — the time axis's own scale, the
    /// way `scale` is the value axis's. Almost every tool ignores it: their
    /// anchors are already projected. A tool that paints a *series* (one
    /// value per bar, like the anchored VWAP's line) needs it to place slots
    /// its anchors never touched; `<= 0` means the pane could not say, and
    /// such a tool draws nothing rather than guessing.
    pub px_per_bar: f32,
    /// What `scale` measures on this band — see [`ValueUnit`].
    pub unit: ValueUnit<'a>,
    /// Whether this is the first band painting an object that crosses all of
    /// them. A vertical line's stroke belongs in every band; its readout
    /// plate and its selection handles belong in one, or a date range's
    /// "17 bars 4m 21s" is stamped three times down the screen.
    pub primary_band: bool,
    /// The object's own style — hit-testing reads it too (an invisible fill
    /// takes no part in the interior hit-test).
    pub style: DrawingStyle,
    pub selected: bool,
    /// True while the wrapper paints the selection halo pass: tools draw
    /// only their stroke geometry then — no fills, no labels.
    pub halo: bool,
    /// True while this object's *content* is being edited somewhere else on
    /// screen — the on-chart note editor holding the words while they are
    /// typed. A tool whose content is words paints none of it then: the
    /// field is showing the same thing, one line lower, and an empty note
    /// showed its "Note" placeholder stacked over the field's "Add text",
    /// which reads as two objects.
    pub content_editing: bool,
}
