//! What a pane's header line says about the pane and what it lets a hand do:
//! its identity, its bars, its view, its layers and its bubble sizes.
//!
//! Built from that pane's own state and nothing else, so the line reads the
//! same wherever focus sits. The window only paints the result and hands back
//! a [`HeaderIntent`].

use quantick_engine::bar_registry::BarConfiguration;
use quantick_layers::{ChartLayer, LayerBlock};
use quantick_orderflow::{DEFAULT_BUBBLE_DISPLAY_SCALE, LaneView};
use quantick_orderflow::{MAX_BUBBLE_DISPLAY_SCALE, MIN_BUBBLE_DISPLAY_SCALE};

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
/// The gap between the header's groups: bars, view, layers.
pub const GROUP_GAP_PX: f32 = 8.0;

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
}

impl LayerToggle {
    /// Every toggle, left to right.
    pub const ALL: [Self; 4] = [
        Self::Bubbles,
        Self::Heatmap,
        Self::Footprint,
        Self::LiveStrip,
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
        }
    }
}

/// One of the two bubble sizes a flow pane's header carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleSample {
    /// Beside the bubbles glyph: the size of the bubbles over the candles.
    Candle,
    /// At the header's right end: the size of the bubbles on the tape.
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

/// Which control of a header line this is.
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

/// One layer glyph this frame: drawn or not, and what blocks it if anything.
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
    /// Switched off: grey.
    Off,
    /// Cannot move right now: the faintest grey, whatever its switch says.
    Blocked,
}

impl LayerGlyph {
    #[must_use]
    pub fn ink(self) -> GlyphInk {
        match (self.blocked, self.on) {
            (Some(_), _) => GlyphInk::Blocked,
            (None, true) => GlyphInk::Lit,
            (None, false) => GlyphInk::Off,
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

/// What a flow pane's header shows of the pane's tape: its view, and each
/// sample's scale while its target is there to scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowHeaderState {
    pub view: LaneView,
    pub candle_sample: Option<f32>,
    pub tape_sample: Option<f32>,
}

/// What a flow pane's header line keeps at a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderFit {
    pub mark: bool,
    pub samples: bool,
}

/// Width of the view button and the four layer glyphs, with their gaps.
const FLOW_CONTROLS_PX: f32 = SLOT_GAP_PX
    + GROUP_GAP_PX
    + VIEW_SLOT_PX
    + SLOT_GAP_PX
    + GROUP_GAP_PX
    + (LayerToggle::ALL.len() as f32) * (GLYPH_SLOT_PX + SLOT_GAP_PX);
/// The gap the mark leaves before the bars.
const MARK_GAP_PX: f32 = SLOT_GAP_PX + GROUP_GAP_PX;
/// Width of both samples: the slot beside the bubbles glyph and the right end.
const FLOW_SAMPLES_PX: f32 = 2.0 * (SAMPLE_SLOT_PX + SLOT_GAP_PX) + GROUP_GAP_PX;

/// The narrow rule of a flow pane's header: the mark goes first, then the
/// samples. What still does not fit clips at the strip's edge, and with the
/// samples gone nothing stands at the right end to overlap.
#[must_use]
pub fn flow_fit(strip_px: f32, mark_px: f32, bars_px: f32) -> HeaderFit {
    let controls = bars_px + FLOW_CONTROLS_PX;
    let with_samples = controls + FLOW_SAMPLES_PX;
    if with_samples + mark_px + MARK_GAP_PX <= strip_px {
        HeaderFit {
            mark: true,
            samples: true,
        }
    } else {
        HeaderFit {
            mark: false,
            samples: with_samples <= strip_px,
        }
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
    /// The layers a pane with this identity toggles: a time pane has only its
    /// own footprint.
    #[must_use]
    pub fn toggles(identity: PaneIdentity) -> &'static [LayerToggle] {
        match identity {
            PaneIdentity::OrderFlow => &LayerToggle::ALL,
            PaneIdentity::Time => &[LayerToggle::Footprint],
        }
    }

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

    #[test]
    fn a_time_pane_toggles_only_its_own_footprint() {
        assert_eq!(
            PaneHeader::toggles(PaneIdentity::Time),
            [LayerToggle::Footprint]
        );
        assert_eq!(
            PaneHeader::toggles(PaneIdentity::OrderFlow),
            LayerToggle::ALL
        );
    }

    /// The tooltip is the name and nothing else: no sentence to read mid-trade.
    #[test]
    fn every_glyph_is_named_in_a_word_or_two() {
        let words = LayerToggle::ALL.map(LayerToggle::name);
        assert_eq!(words, ["Bubbles", "L2", "Footprint", "Strip"]);
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
            (HeaderControl::Sample(BubbleSample::Tape), "sample.tape"),
            (HeaderControl::Sample(BubbleSample::Candle), "sample.candle"),
        ] {
            assert_eq!(control.id_in_header(), id);
        }
    }

    /// Off is not blocked: a blocked glyph keeps its switch and refuses the
    /// click, and its ink says so whichever way the switch stands.
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
            assert_eq!(glyph(on, Some(block)).ink(), GlyphInk::Blocked);
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

    /// The mark goes first, then the samples; bars and glyphs are never dropped
    /// by the rule, they clip.
    #[test]
    fn a_narrow_strip_drops_the_mark_then_the_samples() {
        let (mark, bars) = (70.0, 80.0);
        let everything = bars + FLOW_CONTROLS_PX + FLOW_SAMPLES_PX + mark + MARK_GAP_PX;
        let both = |mark, samples| HeaderFit { mark, samples };
        assert_eq!(flow_fit(everything, mark, bars), both(true, true));
        assert_eq!(flow_fit(everything - 1.0, mark, bars), both(false, true));
        let samples = bars + FLOW_CONTROLS_PX + FLOW_SAMPLES_PX;
        assert_eq!(flow_fit(samples, mark, bars), both(false, true));
        assert_eq!(flow_fit(samples - 1.0, mark, bars), both(false, false));
        assert_eq!(flow_fit(10.0, mark, bars), both(false, false));
    }

    /// With samples kept the right-end sample starts past the last glyph: the
    /// widths the rule adds are the widths the row lays out.
    #[test]
    fn what_the_rule_keeps_does_not_overlap() {
        let (mark, bars) = (70.0, 80.0);
        for strip in (100..1_200).step_by(7) {
            let strip = strip as f32;
            let fit = flow_fit(strip, mark, bars);
            if fit.samples {
                let left = bars
                    + FLOW_CONTROLS_PX
                    + SAMPLE_SLOT_PX
                    + SLOT_GAP_PX
                    + if fit.mark { mark + MARK_GAP_PX } else { 0.0 };
                assert!(left <= strip - SAMPLE_SLOT_PX, "{strip}");
            }
        }
    }
}
