//! What a pane's header line says about the pane, what it lets a hand do, and
//! what each chart side's floating capsule holds: identity, bars, view, layers
//! and bubble sizes.
//!
//! Built from that pane's own state and nothing else, so the line reads the
//! same wherever focus sits. The window only paints the result and hands back
//! a [`HeaderIntent`].

use quantick_engine::bar_registry::BarConfiguration;
use quantick_layers::{ChartLayer, LayerBlock};
use quantick_orderflow::{DEFAULT_BUBBLE_DISPLAY_SCALE, LaneView};
use quantick_orderflow::{MAX_BUBBLE_DISPLAY_SCALE, MIN_BUBBLE_DISPLAY_SCALE};

/// The icon of the button on a header line that opens the indicators of that
/// pane.
pub const INDICATORS_GLYPH: &str = "ƒx";
/// The words beside that icon.
pub const INDICATORS_LABEL: &str = "Indicators";

/// The mark naming the order-flow pane.
pub const ORDER_FLOW_MARK: &str = "ORDER FLOW";

/// What a pane is for, as its header line names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneIdentity {
    /// The order-flow pane: heatmap, bubbles and tape.
    OrderFlow,
    /// A timeframe pane: candles, indicators and drawings.
    Time,
}

/// Width of a layer glyph's slot, in points.
pub const GLYPH_SLOT_PX: f32 = 20.0;
/// Width of the view button's slot: its glyph shows both sides of the pane.
pub const VIEW_SLOT_PX: f32 = 28.0;
/// Width of a sample bubble's slot: the largest sample fits inside it.
pub const SAMPLE_SLOT_PX: f32 = 24.0;
/// The gap between neighbouring slots.
pub const SLOT_GAP_PX: f32 = 2.0;
/// The gap between the header's groups.
pub const GROUP_GAP_PX: f32 = 8.0;

/// How far a capsule stands below the header and off its side's right edge.
pub const CAPSULE_INSET_PX: f32 = 8.0;
/// The capsule's padding plus its 1 px border, across and along.
const CAPSULE_FRAME_X_PX: f32 = 5.0;
const CAPSULE_FRAME_Y_PX: f32 = 4.0;
/// A capsule's slots are this tall.
pub const CAPSULE_SLOT_HEIGHT_PX: f32 = 20.0;
/// A capsule's height: one row of slots inside its frame.
pub const CAPSULE_HEIGHT_PX: f32 = CAPSULE_SLOT_HEIGHT_PX + 2.0 * CAPSULE_FRAME_Y_PX;
/// The most a capsule holds: a sample and three layers.
const CAPSULE_MAX_ITEMS: usize = 4;

/// The sample's diameter at the smallest scale, in points.
const SAMPLE_MIN_DIAMETER_PX: f32 = 6.0;
/// How much diameter the whole scale range adds, in points.
const SAMPLE_DIAMETER_RANGE_PX: f32 = 16.0;
/// How far a sample is dragged for one whole `1x` of scale, in points.
const SAMPLE_DRAG_PX_PER_X: f32 = 40.0;

/// The layers a header line toggles, declared once: the header draws from this
/// list and the scene reads what it painted, so a layer cannot wear a glyph an
/// operator cannot read. In reading order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerToggle {
    Bubbles,
    Heatmap,
    Footprint,
    LiveStrip,
    TapeBubbles,
    TapeHeatmap,
}

impl LayerToggle {
    /// Every toggle, left to right.
    pub const ALL: [Self; 6] = [
        Self::Bubbles,
        Self::Heatmap,
        Self::Footprint,
        Self::LiveStrip,
        Self::TapeBubbles,
        Self::TapeHeatmap,
    ];

    /// The canvas layer this glyph switches: a shortcut into the pane's own
    /// layer policy, never a second switch beside it.
    #[must_use]
    pub fn layer(self) -> ChartLayer {
        match self {
            Self::Bubbles => ChartLayer::Bubbles,
            Self::Heatmap => ChartLayer::Heatmap,
            Self::Footprint => ChartLayer::Footprint,
            Self::LiveStrip => ChartLayer::LiveStrip,
            Self::TapeBubbles => ChartLayer::TapeBubbles,
            Self::TapeHeatmap => ChartLayer::TapeHeatmap,
        }
    }

    /// What the glyph is called: its tooltip, with the shortcut if it has one.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Bubbles => "Bubbles",
            Self::Heatmap => "L2",
            Self::Footprint => "Footprint",
            Self::LiveStrip => "Strip",
            Self::TapeBubbles => "Tape bubbles",
            Self::TapeHeatmap => "Tape L2",
        }
    }
}

/// One of the two bubble sizes a flow pane's capsules carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleSample {
    /// In the candles' capsule: the size of the bubbles over the candles.
    Candle,
    /// In the tape's capsule: the size of the bubbles on the tape.
    Tape,
}

impl BubbleSample {
    /// What the sample is called: its tooltip and its name in the scene.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Candle => "Bubble size on the candles",
            Self::Tape => "Bubble size on the tape",
        }
    }
}

/// Which control of a pane's header line or capsules this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderControl {
    View,
    Layer(LayerToggle),
    Sample(BubbleSample),
}

impl HeaderControl {
    /// The control's stable name inside its pane's header. Written by the
    /// layer's declared id, never a position.
    #[must_use]
    pub fn id_in_header(self) -> String {
        match self {
            Self::View => "view".to_owned(),
            Self::Layer(toggle) => format!("layer.{}", toggle.layer().id()),
            Self::Sample(BubbleSample::Candle) => "sample.candle".to_owned(),
            Self::Sample(BubbleSample::Tape) => "sample.tape".to_owned(),
        }
    }
}

/// What a press on a header control asks the pane for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeaderIntent {
    SetLayer(LayerToggle, bool),
    /// Looking is not enabling: this never toggles on the way.
    OpenLayerSettings(LayerToggle),
    SetView(LaneView),
    SetSample(BubbleSample, f32),
}

/// One layer glyph this frame: drawn or not, and what blocks it if anything. A
/// blocked glyph is never painted; the layer read still says why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerGlyph {
    pub toggle: LayerToggle,
    pub on: bool,
    pub blocked: Option<LayerBlock>,
}

/// How a glyph is inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphInk {
    /// Switched on: the accent colour.
    Lit,
    /// Switched off by hand: an outline with a slash through it.
    Off,
}

impl LayerGlyph {
    #[must_use]
    pub fn ink(self) -> GlyphInk {
        if self.on {
            GlyphInk::Lit
        } else {
            GlyphInk::Off
        }
    }

    /// What a left click asks for: nothing while something blocks the layer.
    #[must_use]
    pub fn click(self) -> Option<HeaderIntent> {
        self.blocked
            .is_none()
            .then_some(HeaderIntent::SetLayer(self.toggle, !self.on))
    }
}

/// The view button's name: its tooltip and its name in the scene.
#[must_use]
pub fn view_name(view: LaneView) -> &'static str {
    match view {
        LaneView::Beside => "Candles and tape",
        LaneView::CandlesOnly => "Candles only",
        LaneView::TapeOnly => "Tape only",
    }
}

/// A sample's diameter at this scale: it grows with what it controls.
#[must_use]
pub fn sample_diameter_px(scale: f32) -> f32 {
    let range = MAX_BUBBLE_DISPLAY_SCALE - MIN_BUBBLE_DISPLAY_SCALE;
    SAMPLE_MIN_DIAMETER_PX + (scale - MIN_BUBBLE_DISPLAY_SCALE) / range * SAMPLE_DIAMETER_RANGE_PX
}

/// The scale a sample dragged `up_px` above where it was pressed shows, from
/// the scale it had then: up grows, and the range holds.
#[must_use]
pub fn dragged_scale(pressed_at: f32, up_px: f32) -> f32 {
    (pressed_at + up_px / SAMPLE_DRAG_PX_PER_X)
        .clamp(MIN_BUBBLE_DISPLAY_SCALE, MAX_BUBBLE_DISPLAY_SCALE)
}

/// The scale a double click on a sample returns to.
pub const SAMPLE_RESET_SCALE: f32 = DEFAULT_BUBBLE_DISPLAY_SCALE;

/// What the sample shows beside itself while it is dragged, and only then.
#[must_use]
pub fn sample_value_text(scale: f32) -> String {
    format!("{scale:.2}x")
}

/// What a flow pane shows of its tape: its view, and each sample's scale while
/// its target is there to scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowHeaderState {
    /// The stored view, which the button walks.
    pub view: LaneView,
    /// The side on screen, which the button's glyph and name read.
    pub shown_view: LaneView,
    pub candle_sample: Option<f32>,
    pub tape_sample: Option<f32>,
}

/// What a flow pane's header line keeps at a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderFit {
    pub mark: bool,
}

/// Width of the header's right end: the view button and the strip glyph.
pub const FLOW_CONTROLS_PX: f32 = VIEW_SLOT_PX + GROUP_GAP_PX + GLYPH_SLOT_PX;
/// The gap the mark leaves before the bars.
const MARK_GAP_PX: f32 = GROUP_GAP_PX;

/// The narrow rule of a flow pane's header: only the mark goes. What still does
/// not fit is the bars text, clipped short of the indicators button.
#[must_use]
pub fn flow_fit(strip_px: f32, mark_px: f32, bars_px: f32, indicators_px: f32) -> HeaderFit {
    let line = mark_px + MARK_GAP_PX + bars_px + GROUP_GAP_PX + indicators_px;
    HeaderFit {
        mark: line + GROUP_GAP_PX + FLOW_CONTROLS_PX <= strip_px,
    }
}

/// The chart side a capsule floats over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapsuleSide {
    Candles,
    Tape,
    /// A timeframe pane: one side, candles only.
    Time,
}

impl CapsuleSide {
    /// The layers this side's capsule switches, left to right.
    #[must_use]
    pub fn toggles(self) -> &'static [LayerToggle] {
        match self {
            Self::Candles => &[
                LayerToggle::Bubbles,
                LayerToggle::Heatmap,
                LayerToggle::Footprint,
            ],
            Self::Tape => &[LayerToggle::TapeBubbles, LayerToggle::TapeHeatmap],
            Self::Time => &[LayerToggle::Footprint],
        }
    }

    /// The sample that stands right of this side's bubbles glyph, if any.
    fn sample(self, toggle: LayerToggle) -> Option<BubbleSample> {
        match (self, toggle) {
            (Self::Candles, LayerToggle::Bubbles) => Some(BubbleSample::Candle),
            (Self::Tape, LayerToggle::TapeBubbles) => Some(BubbleSample::Tape),
            _ => None,
        }
    }
}

/// One slot of a capsule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CapsuleItem {
    Layer(LayerGlyph),
    /// A live sample at this scale.
    Sample(BubbleSample, f32),
    /// A sample whose bubbles are switched off by hand: nothing to scale, so it
    /// takes no hand and no scene entry, and its ink says why.
    SampleOff(BubbleSample),
}

impl CapsuleItem {
    /// The slot's width.
    #[must_use]
    pub fn slot_px(self) -> f32 {
        match self {
            Self::Layer(_) => GLYPH_SLOT_PX,
            Self::Sample(..) | Self::SampleOff(_) => SAMPLE_SLOT_PX,
        }
    }
}

/// What one chart side's capsule holds this frame: only what a hand can switch.
/// A layer the data blocks is left out with its sample, and comes back when the
/// data arrives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capsule {
    items: [Option<CapsuleItem>; CAPSULE_MAX_ITEMS],
}

impl Capsule {
    /// `glyph` reads each layer of the side; `scale` is the side's sample scale
    /// while its target may be scaled.
    #[must_use]
    pub fn of(
        side: CapsuleSide,
        glyph: impl Fn(LayerToggle) -> LayerGlyph,
        scale: Option<f32>,
    ) -> Self {
        let mut items = [None; CAPSULE_MAX_ITEMS];
        let mut slots = items.iter_mut();
        for &toggle in side.toggles() {
            let glyph = glyph(toggle);
            if glyph.blocked.is_some() {
                continue;
            }
            if let Some(slot) = slots.next() {
                *slot = Some(CapsuleItem::Layer(glyph));
            }
            let sample = side
                .sample(toggle)
                .and_then(|which| match (scale, glyph.on) {
                    (Some(scale), _) => Some(CapsuleItem::Sample(which, scale)),
                    (None, false) => Some(CapsuleItem::SampleOff(which)),
                    (None, true) => None,
                });
            if let Some(sample) = sample
                && let Some(slot) = slots.next()
            {
                *slot = Some(sample);
            }
        }
        Self { items }
    }

    /// The slots, left to right.
    pub fn items(&self) -> impl Iterator<Item = CapsuleItem> + '_ {
        self.items.iter().copied().flatten()
    }

    /// With nothing left to switch, the capsule is not painted at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items().next().is_none()
    }

    /// The whole capsule's width, frame included.
    #[must_use]
    pub fn width_px(&self) -> f32 {
        let slots = self.items().map(CapsuleItem::slot_px).sum::<f32>();
        let gaps = self.items().count().saturating_sub(1) as f32 * SLOT_GAP_PX;
        slots + gaps + 2.0 * CAPSULE_FRAME_X_PX
    }

    /// Where the first slot starts, from the capsule's own corner.
    #[must_use]
    pub fn slot_origin(min_x: f32, min_y: f32) -> (f32, f32) {
        (min_x + CAPSULE_FRAME_X_PX, min_y + CAPSULE_FRAME_Y_PX)
    }

    /// The capsule's corner over a side spanning `left..right` of a chart spanning
    /// `top..bottom`; `None` when the side is too narrow or short to hold it.
    #[must_use]
    pub fn place(&self, left: f32, right: f32, top: f32, bottom: f32) -> Option<(f32, f32)> {
        let min_x = right - CAPSULE_INSET_PX - self.width_px();
        let min_y = top + CAPSULE_INSET_PX;
        (min_x >= left && min_y + CAPSULE_HEIGHT_PX <= bottom).then_some((min_x, min_y))
    }
}

/// The sides on screen for this view over a chart spanning `left..right`, with
/// the tape's divider if there is one, leftmost first. A hidden side has no
/// capsule; a pane without a lane is all candles.
#[must_use]
pub fn flow_sides(
    shown: LaneView,
    left: f32,
    right: f32,
    divider: Option<f32>,
) -> [Option<(CapsuleSide, f32, f32)>; 2] {
    let candles = |end| Some((CapsuleSide::Candles, left, end));
    let tape = |start| Some((CapsuleSide::Tape, start, right));
    match (shown, divider) {
        (LaneView::Beside, Some(divider)) => [candles(divider), tape(divider)],
        (LaneView::Beside | LaneView::CandlesOnly, None) => [candles(right), None],
        (LaneView::CandlesOnly, Some(divider)) => [candles(divider), None],
        (LaneView::TapeOnly, divider) => [tape(divider.unwrap_or(left)), None],
    }
}

/// One pane's header line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneHeader {
    /// The pane's name, when it needs one. A time pane's presets already say
    /// what it is.
    pub mark: Option<&'static str>,
    /// The bars this pane builds, as the bars control reads.
    pub bars_text: String,
    /// Whether the timeframe presets stand beside the bars control.
    pub presets: bool,
}

impl PaneHeader {
    /// The header line of a pane with this identity building these bars.
    #[must_use]
    pub fn of(identity: PaneIdentity, bars: BarConfiguration) -> Self {
        let presets = identity == PaneIdentity::Time && bars.time_interval_ms().is_some();
        Self {
            mark: (identity == PaneIdentity::OrderFlow).then_some(ORDER_FLOW_MARK),
            // The presets beside it name the interval, so the kind says enough.
            bars_text: if presets {
                kind_name(bars)
            } else {
                format!("{} · {}", kind_name(bars), bars.parameter_summary())
            },
            presets,
        }
    }
}

/// The bar kind as a header line writes it: its registered label, capitalised.
fn kind_name(bars: BarConfiguration) -> String {
    let label = bars.definition().label();
    let mut letters = label.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::BarSpec;
    use rust_decimal::Decimal;

    fn header(identity: PaneIdentity, spec: BarSpec) -> PaneHeader {
        PaneHeader::of(identity, spec.into())
    }

    fn expected(mark: Option<&'static str>, bars_text: &str, presets: bool) -> PaneHeader {
        PaneHeader {
            mark,
            bars_text: bars_text.to_owned(),
            presets,
        }
    }

    #[test]
    fn the_flow_pane_is_marked_and_writes_its_bars_in_full() {
        assert_eq!(
            header(PaneIdentity::OrderFlow, BarSpec::Tick(2000)),
            expected(Some("ORDER FLOW"), "Tick · 2000", false)
        );
        assert_eq!(
            header(PaneIdentity::OrderFlow, BarSpec::Time(60_000)),
            expected(Some("ORDER FLOW"), "Time · 1m", false),
            "the flow pane has no presets to name the interval"
        );
        assert_eq!(
            header(PaneIdentity::OrderFlow, BarSpec::Volume(Decimal::from(50))),
            expected(Some("ORDER FLOW"), "Volume · 50.0", false)
        );
    }

    #[test]
    fn a_time_pane_on_time_bars_keeps_its_presets_and_says_only_the_kind() {
        assert_eq!(
            header(PaneIdentity::Time, BarSpec::Time(60_000)),
            expected(None, "Time", true)
        );
        assert_eq!(
            header(PaneIdentity::Time, BarSpec::Time(3_600_000)),
            expected(None, "Time", true),
            "the interval belongs to the presets"
        );
    }

    #[test]
    fn a_time_pane_on_other_bars_drops_the_presets_it_would_misstate() {
        assert_eq!(
            header(PaneIdentity::Time, BarSpec::Tick(4)),
            expected(None, "Tick · 4", false)
        );
    }

    /// The tooltip is the name and nothing else: no sentence to read mid-trade.
    #[test]
    fn every_glyph_is_named_in_a_word_or_two() {
        let words = LayerToggle::ALL.map(LayerToggle::name);
        assert_eq!(
            words,
            [
                "Bubbles",
                "L2",
                "Footprint",
                "Strip",
                "Tape bubbles",
                "Tape L2"
            ]
        );
        for view in [LaneView::Beside, LaneView::CandlesOnly, LaneView::TapeOnly] {
            assert!(view_name(view).split(' ').count() <= 3, "{view:?}");
        }
    }

    /// Ids come from the declared layer, so the scene can point at one twice.
    #[test]
    fn a_control_is_named_by_what_it_is() {
        for (control, id) in [
            (HeaderControl::View, "view"),
            (
                HeaderControl::Layer(LayerToggle::LiveStrip),
                "layer.live_strip",
            ),
            (
                HeaderControl::Layer(LayerToggle::TapeBubbles),
                "layer.tape_bubbles",
            ),
            (
                HeaderControl::Layer(LayerToggle::TapeHeatmap),
                "layer.tape_heatmap",
            ),
            (HeaderControl::Sample(BubbleSample::Tape), "sample.tape"),
            (HeaderControl::Sample(BubbleSample::Candle), "sample.candle"),
        ] {
            assert_eq!(control.id_in_header(), id);
        }
    }

    /// Off is not blocked: a blocked glyph keeps its switch and refuses the
    /// click, whichever way the switch stands.
    #[test]
    fn a_blocked_glyph_keeps_its_state_and_refuses_the_click() {
        let glyph = |on, blocked| LayerGlyph {
            toggle: LayerToggle::Heatmap,
            on,
            blocked,
        };
        assert_eq!(glyph(true, None).ink(), GlyphInk::Lit);
        assert_eq!(glyph(false, None).ink(), GlyphInk::Off);
        assert_eq!(
            glyph(true, None).click(),
            Some(HeaderIntent::SetLayer(LayerToggle::Heatmap, false))
        );
        assert_eq!(
            glyph(false, None).click(),
            Some(HeaderIntent::SetLayer(LayerToggle::Heatmap, true))
        );
        let block = quantick_layers::blocks::TAPE_ONLY_CANDLES;
        for on in [true, false] {
            assert_eq!(glyph(on, Some(block)).click(), None);
        }
    }

    /// The sample's size is the proposal's formula, end to end.
    #[test]
    fn a_samples_diameter_spans_six_to_twenty_two_points() {
        assert!((sample_diameter_px(0.25) - 6.0).abs() < 1e-4);
        assert!((sample_diameter_px(3.0) - 22.0).abs() < 1e-4);
        assert!((sample_diameter_px(1.0) - (6.0 + 0.75 / 2.75 * 16.0)).abs() < 1e-4);
        assert!(sample_diameter_px(3.0) <= SAMPLE_SLOT_PX, "fits its slot");
    }

    /// Up grows at one `1x` per 40 points, and the range holds both ways.
    #[test]
    fn dragging_a_sample_up_grows_it_within_the_range() {
        assert!((dragged_scale(1.0, 40.0) - 2.0).abs() < 1e-5);
        assert!((dragged_scale(1.0, -20.0) - 0.5).abs() < 1e-5);
        assert_eq!(dragged_scale(1.0, 4_000.0), MAX_BUBBLE_DISPLAY_SCALE);
        assert_eq!(dragged_scale(1.0, -4_000.0), MIN_BUBBLE_DISPLAY_SCALE);
        assert_eq!(SAMPLE_RESET_SCALE, 1.0);
        assert_eq!(sample_value_text(1.5), "1.50x");
    }

    /// Only the mark goes; the right end and the indicators button never do.
    #[test]
    fn a_narrow_strip_drops_only_the_mark() {
        let (mark, bars, indicators) = (70.0, 80.0, 90.0);
        let everything =
            mark + MARK_GAP_PX + bars + GROUP_GAP_PX + indicators + GROUP_GAP_PX + FLOW_CONTROLS_PX;
        assert!(flow_fit(everything, mark, bars, indicators).mark);
        assert!(!flow_fit(everything - 1.0, mark, bars, indicators).mark);
    }

    fn glyph(toggle: LayerToggle, on: bool, blocked: Option<LayerBlock>) -> LayerGlyph {
        LayerGlyph {
            toggle,
            on,
            blocked,
        }
    }

    fn ids(capsule: &Capsule) -> Vec<String> {
        capsule
            .items()
            .map(|item| match item {
                CapsuleItem::Layer(glyph) => HeaderControl::Layer(glyph.toggle).id_in_header(),
                CapsuleItem::Sample(which, _) => HeaderControl::Sample(which).id_in_header(),
                CapsuleItem::SampleOff(which) => {
                    format!("off.{}", HeaderControl::Sample(which).id_in_header())
                }
            })
            .collect()
    }

    /// Each side lists its own layers, with its sample right of its bubbles.
    #[test]
    fn each_side_holds_its_own_controls() {
        let on = |toggle| glyph(toggle, true, None);
        let candles = Capsule::of(CapsuleSide::Candles, on, Some(1.0));
        assert_eq!(
            ids(&candles),
            [
                "layer.bubbles",
                "sample.candle",
                "layer.heatmap",
                "layer.footprint"
            ]
        );
        let tape = Capsule::of(CapsuleSide::Tape, on, Some(1.0));
        assert_eq!(
            ids(&tape),
            ["layer.tape_bubbles", "sample.tape", "layer.tape_heatmap"]
        );
        let time = Capsule::of(CapsuleSide::Time, on, None);
        assert_eq!(ids(&time), ["layer.footprint"]);
    }

    /// Book offline takes L2 only; no traded volume takes bubbles with their
    /// sample and the footprint; both gone, the capsule is empty.
    #[test]
    fn what_the_data_blocks_is_left_out() {
        let book = quantick_layers::blocks::NO_BOOK;
        let volume = quantick_layers::blocks::NO_TRADED_VOLUME;
        let no_book = |toggle| {
            let l2 = matches!(toggle, LayerToggle::Heatmap | LayerToggle::TapeHeatmap);
            glyph(toggle, true, l2.then_some(book))
        };
        assert_eq!(
            ids(&Capsule::of(CapsuleSide::Candles, no_book, Some(1.0))),
            ["layer.bubbles", "sample.candle", "layer.footprint"]
        );
        let no_volume = |toggle| {
            let l2 = matches!(toggle, LayerToggle::Heatmap | LayerToggle::TapeHeatmap);
            glyph(toggle, true, (!l2).then_some(volume))
        };
        assert_eq!(
            ids(&Capsule::of(CapsuleSide::Tape, no_volume, Some(1.0))),
            ["layer.tape_heatmap"]
        );
        let nothing = |toggle| glyph(toggle, true, Some(book));
        assert!(Capsule::of(CapsuleSide::Tape, nothing, Some(1.0)).is_empty());
        // The data back, everything is back: the capsule holds no memory.
        let back = |toggle| glyph(toggle, true, None);
        assert_eq!(
            ids(&Capsule::of(CapsuleSide::Tape, back, Some(1.0))).len(),
            3
        );
    }

    /// Off by hand, the bubbles keep their place and their sample's slot; on but
    /// with nothing to scale, the sample has no slot.
    #[test]
    fn a_sample_follows_its_bubbles() {
        let off = |toggle| glyph(toggle, false, None);
        assert_eq!(
            ids(&Capsule::of(CapsuleSide::Tape, off, None)),
            [
                "layer.tape_bubbles",
                "off.sample.tape",
                "layer.tape_heatmap"
            ]
        );
        let on = |toggle| glyph(toggle, true, None);
        assert_eq!(
            ids(&Capsule::of(CapsuleSide::Tape, on, None)),
            ["layer.tape_bubbles", "layer.tape_heatmap"]
        );
    }

    /// The capsule stands 8 points below the header and off the side's right
    /// edge, with its slots inside its frame, and a side too narrow has none.
    #[test]
    fn a_capsule_sits_in_its_sides_corner() {
        let on = |toggle| glyph(toggle, true, None);
        let capsule = Capsule::of(CapsuleSide::Candles, on, Some(1.0));
        let width = 3.0 * GLYPH_SLOT_PX + SAMPLE_SLOT_PX + 3.0 * SLOT_GAP_PX + 10.0;
        assert_eq!(capsule.width_px(), width);
        let place = |right, bottom| capsule.place(0.0, right, 40.0, bottom);
        assert_eq!(place(500.0, 300.0), Some((492.0 - width, 48.0)));
        assert_eq!(place(width + 8.0, 300.0), Some((0.0, 48.0)));
        assert_eq!(place(width + 7.0, 300.0), None);
        assert_eq!(place(500.0, 76.0), Some((492.0 - width, 48.0)));
        assert_eq!(place(500.0, 75.0), None, "a plot too short holds none");
    }

    /// A hidden side has no capsule, and a side keeps its own edge.
    #[test]
    fn the_view_decides_the_sides() {
        let candles = |end| Some((CapsuleSide::Candles, 0.0, end));
        let tape = |start| Some((CapsuleSide::Tape, start, 100.0));
        let sides = |view, divider| flow_sides(view, 0.0, 100.0, divider);
        assert_eq!(
            sides(LaneView::Beside, Some(70.0)),
            [candles(70.0), tape(70.0)]
        );
        assert_eq!(sides(LaneView::Beside, None), [candles(100.0), None]);
        assert_eq!(sides(LaneView::CandlesOnly, None), [candles(100.0), None]);
        assert_eq!(
            sides(LaneView::CandlesOnly, Some(92.0)),
            [candles(92.0), None]
        );
        assert_eq!(sides(LaneView::TapeOnly, Some(0.0)), [tape(0.0), None]);
        assert_eq!(sides(LaneView::TapeOnly, None), [tape(0.0), None]);
    }
}
