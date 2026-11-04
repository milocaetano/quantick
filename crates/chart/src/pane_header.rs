//! What a pane's header line says about the pane, what it lets a hand do, and
//! what each chart side's capsule, docked in its header, holds: identity, bars,
//! view, layers and bubble sizes.
//!
//! Built from that pane's own state and nothing else, so the line reads the
//! same wherever focus sits. The window only paints the result and hands back
//! a [`HeaderIntent`].

use quantick_engine::bar_registry::BarConfiguration;
use quantick_layers::{ChartLayer, LayerBlock};
use quantick_orderflow::{DEFAULT_BUBBLE_DISPLAY_SCALE, LaneSide, LaneView, LaneWindow};
use quantick_orderflow::{MAX_BUBBLE_DISPLAY_SCALE, MIN_BUBBLE_DISPLAY_SCALE};
use quantick_workspace::orderflow_panel::{OrderFlowPanel, PanelControl, PanelSection};

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
/// Width of a sample bubble's slot: the largest sample fits inside it.
pub const SAMPLE_SLOT_PX: f32 = 24.0;
/// The gap between neighbouring slots: the clear air bare icons stand in, a 26 px
/// pitch between glyph slots on every docked capsule, flow and time alike.
pub const SLOT_GAP_PX: f32 = 6.0;
/// The gap between the header's groups.
pub const GROUP_GAP_PX: f32 = 8.0;

/// The capsule's room across each end: the same clear air as between its icons,
/// so the first one has a lead-in as wide as the gaps that follow it.
pub(crate) const CAPSULE_FRAME_X_PX: f32 = SLOT_GAP_PX;
const CAPSULE_FRAME_Y_PX: f32 = 4.0;
/// A floating capsule's slots were this tall; the tape's seam plates keep its
/// height.
pub const CAPSULE_SLOT_HEIGHT_PX: f32 = 20.0;
/// A floating capsule's height, which the tape's seam plates keep.
pub const CAPSULE_HEIGHT_PX: f32 = CAPSULE_SLOT_HEIGHT_PX + 2.0 * CAPSULE_FRAME_Y_PX;
/// The most a capsule holds: a sample and four layers.
const CAPSULE_MAX_ITEMS: usize = 5;

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
            Self::Candle => "Bubble size on the trail",
            Self::Tape => "Bubble size on the tape",
        }
    }
}

/// Which control of a pane's header line or capsules this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderControl {
    /// One segment of the view control.
    Side(LaneSide),
    Layer(LayerToggle),
    Sample(BubbleSample),
    /// The gear at the line's right end, or a control of the panel it opens.
    Settings(PanelControl),
    /// The tape header's window selector.
    TapeWindow,
}

impl HeaderControl {
    /// The control's stable name inside its pane's header. Written by the
    /// layer's declared id, never a position.
    #[must_use]
    pub fn id_in_header(self) -> String {
        match self {
            Self::Side(LaneSide::Trail) => "side.trail".to_owned(),
            Self::Side(LaneSide::Tape) => "side.tape".to_owned(),
            Self::Layer(toggle) => format!("layer.{}", toggle.layer().id()),
            Self::Sample(BubbleSample::Candle) => "sample.candle".to_owned(),
            Self::Sample(BubbleSample::Tape) => "sample.tape".to_owned(),
            Self::Settings(control) => control.id_in_header(),
            Self::TapeWindow => "tape.window".to_owned(),
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
    /// The gear's left click: the panel opens or closes.
    ToggleSettings,
    /// The selector's pick, the same write as the tape menu's.
    SetLaneWindow(LaneWindow),
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
    /// Switched on: the primary text colour.
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

/// A view in words.
#[must_use]
pub fn view_name(view: LaneView) -> &'static str {
    match view {
        LaneView::Beside => "Trail and tape",
        LaneView::CandlesOnly => "Trail only",
        LaneView::TapeOnly => "Tape only",
    }
}

/// A side's word beside its glyph, and its switch's name in the scene.
#[must_use]
pub fn side_word(side: LaneSide) -> &'static str {
    match side {
        LaneSide::Trail => "Trail",
        LaneSide::Tape => "Tape",
    }
}

/// How a segment of the view control is inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentInk {
    /// Its side is chosen: white ink on a key raised off the band.
    Lit,
    /// Its side is not chosen: muted ink on a dark key sunk into the band.
    Unlit,
    /// The tape's, while the market holds it folded: the one red control, a sunk
    /// key with a red rim, red ink and a dot.
    MarketClosed,
}

/// Why a segment takes no press. Painted as the tooltip's note and listed in the
/// scene as the control's unavailable reason, so the two cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SideRefusal {
    /// It is the last side showing, and the pane is never left empty.
    LastSideShowing,
    /// The market holds the tape folded; the first print brings it back.
    MarketClosed,
}

impl SideRefusal {
    /// The stable reason the scene reports.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::LastSideShowing => "the_last_side_showing_stays",
            Self::MarketClosed => "market_closed",
        }
    }

    /// The words a tooltip adds.
    #[must_use]
    pub fn note(self) -> &'static str {
        match self {
            Self::LastSideShowing => "the last side showing stays",
            Self::MarketClosed => "the first print brings it back",
        }
    }
}

/// One segment of the view control this frame: what a hand sees and what a press
/// on it asks for. The stored view lights the segments, since a fold is not a
/// view; a side the divider folded stays lit, and the tooltip says what is on
/// screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideSegment {
    pub side: LaneSide,
    pub ink: SegmentInk,
    pub refusal: Option<SideRefusal>,
    view: LaneView,
}

impl SideSegment {
    /// The control's two segments, left to right.
    #[must_use]
    pub fn pair(state: &FlowHeaderState) -> [Self; 2] {
        [LaneSide::Trail, LaneSide::Tape].map(|side| Self::of(side, state))
    }

    /// One segment over this state.
    #[must_use]
    pub fn of(side: LaneSide, state: &FlowHeaderState) -> Self {
        let view = state.view;
        let held = state.market_held;
        Self {
            side,
            ink: match (held, side) {
                (true, LaneSide::Tape) => SegmentInk::MarketClosed,
                _ if view.shows(side) => SegmentInk::Lit,
                _ => SegmentInk::Unlit,
            },
            refusal: match (held, side) {
                (true, LaneSide::Tape) => Some(SideRefusal::MarketClosed),
                // The tape is away, so the trail is what is left showing.
                (true, LaneSide::Trail) => Some(SideRefusal::LastSideShowing),
                _ => view
                    .toggled(side)
                    .is_none()
                    .then_some(SideRefusal::LastSideShowing),
            },
            view,
        }
    }

    /// The word beside the glyph, and the control's name in the scene.
    #[must_use]
    pub fn word(self) -> &'static str {
        if self.ink == SegmentInk::MarketClosed {
            MARKET_CLOSED_WORD
        } else {
            side_word(self.side)
        }
    }

    /// What a left click asks for: nothing while the segment refuses.
    #[must_use]
    pub fn click(self) -> Option<HeaderIntent> {
        if self.refusal.is_some() {
            return None;
        }
        self.view.toggled(self.side).map(HeaderIntent::SetView)
    }

    /// What the tooltip adds to the name and the key: why it refuses.
    #[must_use]
    pub fn note(self) -> Option<&'static str> {
        self.refusal.map(SideRefusal::note)
    }
}

/// A rail's name in the scene: what a click on it brings back.
#[must_use]
pub fn rail_word(closed: LaneSide) -> &'static str {
    match closed {
        LaneSide::Trail => "Trail rail",
        LaneSide::Tape => "Tape rail",
    }
}

/// A rail's hover hint: what a click and a drag on it do.
#[must_use]
pub fn rail_hint(closed: LaneSide) -> &'static str {
    match closed {
        LaneSide::Trail => "click to show the candles again, or drag right",
        LaneSide::Tape => "click to show the tape again, or drag left",
    }
}

/// The market-closed setting's words in the settings.
pub const MARKET_FOLD_SETTING: &str = "Fold the tape while the market is closed";
/// What it does, on hover.
pub const MARKET_FOLD_SETTING_HOVER: &str = "beside the candles, while the feed is quiet or offline; the first print brings it back. Off, or with the tape alone, the tape stays up to a seam at the last print";

/// What the tape's segment reads while the market holds it away.
pub const MARKET_CLOSED_WORD: &str = "Market closed";

/// Width of a key's glyph.
pub const SEGMENT_GLYPH_PX: f32 = 14.0;
/// A key's height on the band: the Trail and Tape keys and the gear alike.
pub const SEGMENT_HEIGHT_PX: f32 = 20.0;
/// The gap between the Trail and Tape keys; no frame encloses the pair.
pub const SEGMENT_GAP_PX: f32 = 1.0;
/// A key's padding either side of its glyph, with its word or without it.
const SEGMENT_PAD_PX: f32 = 10.0;
/// The gap between a key's glyph and its word.
pub const SEGMENT_WORD_GAP_PX: f32 = 5.0;
/// The gear's key: its glyph and 5 px either side.
pub const GEAR_KEY_PX: f32 = 2.0 * 5.0 + SEGMENT_GLYPH_PX;

/// A key's width: its glyph alone, or with a word this wide beside it.
#[must_use]
pub fn segment_px(word_px: Option<f32>) -> f32 {
    word_px.map_or(SEGMENT_BARE_PX, |word| {
        SEGMENT_BARE_PX + SEGMENT_WORD_GAP_PX + word
    })
}

const SEGMENT_BARE_PX: f32 = 2.0 * SEGMENT_PAD_PX + SEGMENT_GLYPH_PX;

/// What the words beside the glyphs add to the right end, these words being
/// this wide.
#[must_use]
pub fn side_words_px(words: [f32; 2]) -> f32 {
    words
        .map(|word| segment_px(Some(word)) - SEGMENT_BARE_PX)
        .iter()
        .sum()
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
    /// The stored view, which the segments light and switch.
    pub view: LaneView,
    /// The side on screen, the market's hold included, which the capsules and
    /// labels follow.
    pub shown_view: LaneView,
    /// The market holds the tape away: it reads so, and both segments refuse.
    pub market_held: bool,
    pub candle_sample: Option<f32>,
    pub tape_sample: Option<f32>,
    /// How much market time the tape shows, and what following the bars works
    /// out to while the frame knows.
    pub window: LaneWindow,
    pub window_reference_ms: Option<i64>,
    /// The window the asset files, which a double click returns to and a pick
    /// is judged against ([`quantick_orderflow::lane_window_pick`]).
    pub window_setting: LaneWindow,
}

impl FlowHeaderState {
    /// The scale of the sample this side's capsule carries, if it has one.
    #[must_use]
    pub fn sample_of(&self, side: CapsuleSide) -> Option<f32> {
        match side {
            CapsuleSide::Candles => self.candle_sample,
            CapsuleSide::Tape => self.tape_sample,
            CapsuleSide::Time => None,
        }
    }

    /// What this lane shows, with each sample's scale while it may be scaled.
    #[must_use]
    pub fn of(
        lane: &quantick_orderflow::LiveLaneStyle,
        candle_sample: Option<f32>,
        tape_sample: Option<f32>,
        window_reference_ms: Option<i64>,
        window_setting: LaneWindow,
    ) -> Self {
        Self {
            view: lane.view(),
            shown_view: lane.shown_view(),
            market_held: lane.market_held(),
            candle_sample,
            tape_sample,
            window: lane.window,
            window_reference_ms,
            window_setting,
        }
    }
}

/// What a flow pane's header line keeps at a given width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderFit {
    pub mark: bool,
    /// The job slot, left of the chip.
    pub slot: bool,
    /// The chip at all: three dots.
    pub chip: bool,
    /// The chip's words beside its dots.
    pub chip_words: bool,
    /// The words beside the view control's glyphs.
    pub side_words: bool,
}

/// Width of the header's right end at its barest: the two keys without their
/// words, and the gear's. The strip glyph lives in the trail's capsule, the one
/// side it draws on.
pub const FLOW_CONTROLS_PX: f32 =
    2.0 * SEGMENT_BARE_PX + SEGMENT_GAP_PX + GROUP_GAP_PX + GEAR_KEY_PX;

impl HeaderControl {
    /// A section's header, as the flow pane's line lists it, on screen or not.
    #[must_use]
    pub const fn section(section: PanelSection) -> Self {
        Self::Settings(PanelControl::Section(section))
    }
}

/// The layer glyph a section of the panel configures, for the gear's menu: the
/// right click that opens that section from a capsule's glyph.
#[must_use]
pub const fn section_layer(section: PanelSection) -> LayerToggle {
    match section {
        PanelSection::L2 => LayerToggle::Heatmap,
        PanelSection::Bubbles => LayerToggle::Bubbles,
    }
}

/// What the flow pane's gear and capsules asked its panel for: looking is not
/// enabling, so no layer moves.
pub fn settings_actions(panel: &mut OrderFlowPanel, actions: &quantick_layers::LayerActions) {
    if actions.toggle_settings {
        panel.toggle();
    }
    if let Some(layer) = actions.open_layer_tab {
        panel.open_on(Some(layer_section(layer)));
    }
}

/// A status label's first words, for a section's header: `book live`, with no
/// generation or reason.
#[must_use]
pub fn book_headline(status: &quantick_orderflow::engine::CaptureStatus) -> String {
    let label = status.label();
    label.split(" · ").next().unwrap_or_default().to_owned()
}

/// The panel section that configures a layer. The strip reads the same book the
/// depth map draws, so one section configures both.
#[must_use]
pub fn layer_section(layer: ChartLayer) -> PanelSection {
    match layer {
        ChartLayer::Bubbles | ChartLayer::TapeBubbles => PanelSection::Bubbles,
        _ => PanelSection::L2,
    }
}

/// The widths a flow pane's band has to fit. An absent item is zero.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HeaderWidths {
    pub strip: f32,
    pub mark: f32,
    /// The right end at its barest, its own gap included; zero without a tape.
    pub controls: f32,
    /// What the words beside the view control's glyphs add to the right end.
    pub side_words: f32,
    pub slot: f32,
    pub chip_with_words: f32,
    pub chip_dots: f32,
    /// The market holds the tape: the side words are the fold's only trace.
    pub market_held: bool,
}

/// What an item takes on a line where items stand `gap` apart: its width and the
/// gap after it, or nothing for an item that is not there.
#[must_use]
pub(crate) fn item_px(width: f32, gap: f32) -> f32 {
    if width > 0.0 { width + gap } else { 0.0 }
}

/// The narrow rule of a flow pane's band, in the order things go: the slot, then
/// the words beside the view control's glyphs, then the mark, then the chip's
/// words, then the chip's dots. While the market holds the tape its words outlast
/// the status: "Market closed" is all that says where the tape went.
#[must_use]
pub fn flow_fit(widths: &HeaderWidths) -> HeaderFit {
    let item = |width: f32| item_px(width, GROUP_GAP_PX);
    let mark = item(widths.mark);
    let fits = |extra: f32| widths.controls + extra <= widths.strip;
    let (full_chip, dots) = (item(widths.chip_with_words), item(widths.chip_dots));
    let slot = fits(widths.side_words + mark + full_chip + item(widths.slot));
    let (side_words, kept) = if widths.market_held {
        let keep = fits(widths.side_words);
        (keep, if keep { widths.side_words } else { 0.0 })
    } else {
        (fits(widths.side_words + mark + full_chip), 0.0)
    };
    let keep_mark = fits(kept + mark + full_chip);
    let chip_words = fits(kept + full_chip);
    let chip = chip_words || fits(kept + dots);
    HeaderFit {
        // The mark goes before the chip's words, but with no chip to make room
        // for it answers only to its own width.
        mark: keep_mark,
        slot: slot && widths.slot > 0.0,
        chip: chip && widths.chip_dots > 0.0,
        chip_words: chip_words && widths.chip_with_words > 0.0,
        side_words: side_words && widths.side_words > 0.0,
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
    /// The layers this side's capsule switches, left to right. The trail's ends
    /// with the strip, against the divider the strip stands beside.
    #[must_use]
    pub fn toggles(self) -> &'static [LayerToggle] {
        match self {
            Self::Candles => &[
                LayerToggle::Bubbles,
                LayerToggle::Heatmap,
                LayerToggle::Footprint,
                LayerToggle::LiveStrip,
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
            (HeaderControl::Side(LaneSide::Trail), "side.trail"),
            (HeaderControl::Side(LaneSide::Tape), "side.tape"),
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

    fn widths(strip: f32) -> HeaderWidths {
        HeaderWidths {
            strip,
            mark: 70.0,
            controls: FLOW_CONTROLS_PX + GROUP_GAP_PX,
            side_words: 0.0,
            slot: 120.0,
            chip_with_words: 150.0,
            chip_dots: 48.0,
            market_held: false,
        }
    }

    /// Only the mark goes; the right end never does.
    #[test]
    fn a_narrow_strip_with_no_chip_drops_only_the_mark() {
        let line = |strip| HeaderWidths {
            slot: 0.0,
            chip_with_words: 0.0,
            chip_dots: 0.0,
            ..widths(strip)
        };
        let w = line(0.0);
        let everything = w.mark + GROUP_GAP_PX + w.controls;
        let fit = flow_fit(&line(everything));
        assert!(fit.mark);
        assert!(!fit.chip && !fit.slot && !fit.chip_words);
        assert!(!flow_fit(&line(everything - 1.0)).mark);
    }

    /// The trader's narrow pane, step by step: the slot, the mark, the words,
    /// the dots, in that order, and never an overlap.
    #[test]
    fn a_narrowing_strip_gives_things_up_in_order() {
        let w = widths(0.0);
        let core = w.controls;
        let gap = GROUP_GAP_PX;
        let mark = w.mark + gap;
        let words = w.chip_with_words + gap;
        let dots = w.chip_dots + gap;
        let slot = w.slot + gap;
        let keeps = |strip: f32| {
            let fit = flow_fit(&widths(strip));
            (fit.slot, fit.mark, fit.chip_words, fit.chip)
        };
        assert_eq!(keeps(core + mark + words + slot), (true, true, true, true));
        assert_eq!(keeps(core + mark + words), (false, true, true, true));
        assert_eq!(keeps(core + words), (false, false, true, true));
        assert_eq!(keeps(core + dots), (false, false, false, true));
        assert_eq!(keeps(core + dots - 1.0), (false, false, false, false));
    }

    /// The words go before the mark and the chip: the control keeps
    /// working on its glyphs alone.
    #[test]
    fn a_narrowing_strip_drops_the_side_words_before_the_mark() {
        let line = |strip| HeaderWidths {
            slot: 0.0,
            chip_with_words: 0.0,
            chip_dots: 0.0,
            side_words: 60.0,
            ..widths(strip)
        };
        let w = line(0.0);
        let without = w.mark + GROUP_GAP_PX + w.controls;
        let keeps = |strip| {
            let fit = flow_fit(&line(strip));
            (fit.side_words, fit.mark)
        };
        assert_eq!(keeps(without + 60.0), (true, true));
        assert_eq!(keeps(without + 59.0), (false, true));
        assert_eq!(keeps(without), (false, true));
        assert_eq!(keeps(without - 1.0), (false, false));
        assert!(!flow_fit(&widths(10_000.0)).side_words, "no words, no flag");
    }

    /// Held by the market the order turns over: the pills and the mark go first and
    /// the words "Market closed" last.
    #[test]
    fn a_held_tape_keeps_its_words_after_the_status_is_gone() {
        let words = 100.0;
        let line = |strip| HeaderWidths {
            side_words: words,
            market_held: true,
            ..widths(strip)
        };
        let w = line(0.0);
        let core = w.controls;
        let gap = GROUP_GAP_PX;
        let (mark, pills, dots) = (w.mark + gap, w.chip_with_words + gap, w.chip_dots + gap);
        let keeps = |strip: f32| {
            let fit = flow_fit(&line(strip));
            (fit.mark, fit.chip_words, fit.chip, fit.side_words)
        };
        assert_eq!(keeps(core + words + mark + pills), (true, true, true, true));
        assert_eq!(keeps(core + words + pills), (false, true, true, true));
        assert_eq!(keeps(core + words + dots), (false, false, true, true));
        assert_eq!(keeps(core + words), (false, false, false, true));
        assert!(!keeps(core + words - 1.0).3, "and only then the dot");
        let unheld = HeaderWidths {
            market_held: false,
            ..line(core + words)
        };
        let fit = flow_fit(&unheld);
        assert!(fit.chip && !fit.side_words, "an ordinary word goes first");
    }

    fn state(view: LaneView) -> FlowHeaderState {
        FlowHeaderState {
            view,
            shown_view: view,
            market_held: false,
            candle_sample: None,
            tape_sample: None,
            window: LaneWindow::default(),
            window_reference_ms: None,
            window_setting: LaneWindow::default(),
        }
    }

    /// Both lit is beside; a click on one puts that side away; the last lit
    /// segment stays lit, refuses the click and says why, under one code.
    #[test]
    fn the_last_lit_segment_stays_lit_and_refuses_the_click() {
        let [trail, tape] = SideSegment::pair(&state(LaneView::Beside));
        assert_eq!((trail.ink, tape.ink), (SegmentInk::Lit, SegmentInk::Lit));
        assert_eq!(
            trail.click(),
            Some(HeaderIntent::SetView(LaneView::TapeOnly))
        );
        assert_eq!(
            tape.click(),
            Some(HeaderIntent::SetView(LaneView::CandlesOnly))
        );
        assert_eq!((trail.note(), tape.note()), (None, None));

        let [trail, tape] = SideSegment::pair(&state(LaneView::CandlesOnly));
        assert_eq!((trail.ink, tape.ink), (SegmentInk::Lit, SegmentInk::Unlit));
        assert_eq!(trail.click(), None);
        assert_eq!(trail.refusal, Some(SideRefusal::LastSideShowing));
        assert_eq!(trail.note(), Some("the last side showing stays"));
        assert_eq!(
            trail.refusal.map(SideRefusal::code),
            Some("the_last_side_showing_stays")
        );
        assert_eq!(tape.click(), Some(HeaderIntent::SetView(LaneView::Beside)));

        let [trail, tape] = SideSegment::pair(&state(LaneView::TapeOnly));
        assert_eq!((trail.ink, tape.ink), (SegmentInk::Unlit, SegmentInk::Lit));
        assert_eq!(tape.click(), None);
        assert_eq!(trail.click(), Some(HeaderIntent::SetView(LaneView::Beside)));
    }

    /// While the market holds the tape: Tape reads so in its own ink and refuses
    /// under its own code, and Trail is the last side showing. Neither click asks
    /// for anything, and the view stays what the trader chose.
    #[test]
    fn a_market_hold_turns_the_tape_segment_and_refuses_both() {
        let held = FlowHeaderState {
            shown_view: LaneView::CandlesOnly,
            market_held: true,
            ..state(LaneView::Beside)
        };
        let [trail, tape] = SideSegment::pair(&held);
        assert_eq!(tape.ink, SegmentInk::MarketClosed);
        assert_eq!(tape.word(), "Market closed");
        assert_eq!(tape.click(), None);
        assert_eq!(tape.refusal.map(SideRefusal::code), Some("market_closed"));
        assert_eq!(trail.ink, SegmentInk::Lit);
        assert_eq!(trail.word(), "Trail");
        assert_eq!(trail.click(), None);
        assert_eq!(
            trail.refusal.map(SideRefusal::code),
            Some("the_last_side_showing_stays")
        );
    }

    /// The screen says Trail; the wire keeps its words elsewhere.
    #[test]
    fn the_screen_words_say_trail() {
        assert_eq!(view_name(LaneView::Beside), "Trail and tape");
        assert_eq!(view_name(LaneView::CandlesOnly), "Trail only");
        assert_eq!(view_name(LaneView::TapeOnly), "Tape only");
        assert_eq!(BubbleSample::Candle.name(), "Bubble size on the trail");
    }

    /// The bare right end is what the words add to: the two keys 1 px apart with
    /// no frame, then the gear's key, and no strip glyph. A key is padded 3 x 10.
    #[test]
    fn the_right_end_measures_its_parts() {
        assert_eq!(segment_px(None), 10.0 + 14.0 + 10.0);
        assert_eq!(segment_px(Some(30.0)), 34.0 + 5.0 + 30.0);
        assert_eq!(side_words_px([30.0, 40.0]), 35.0 + 45.0);
        assert_eq!(FLOW_CONTROLS_PX, 68.0 + 1.0 + 8.0 + 24.0);
        assert_eq!(SEGMENT_HEIGHT_PX, 3.0 + 14.0 + 3.0);
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

    /// Each side lists its own layers, with its sample right of its bubbles; the
    /// trail's ends with the strip, next to the divider.
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
                "layer.footprint",
                "layer.live_strip"
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
            [
                "layer.bubbles",
                "sample.candle",
                "layer.footprint",
                "layer.live_strip"
            ]
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

    /// The trail's capsule holds four glyphs and the sample with clear air between
    /// them, and a lead-in as wide at either end: 20 px glyph slots on a 26 px pitch.
    #[test]
    fn a_capsule_measures_its_slots() {
        let on = |toggle| glyph(toggle, true, None);
        let capsule = Capsule::of(CapsuleSide::Candles, on, Some(1.0));
        let width = 4.0 * GLYPH_SLOT_PX + SAMPLE_SLOT_PX + 6.0 * SLOT_GAP_PX;
        assert_eq!(capsule.width_px(), width);
        assert_eq!(GLYPH_SLOT_PX + SLOT_GAP_PX, 26.0, "the pitch");
        assert_eq!(capsule.width_px(), 140.0);
    }
}
