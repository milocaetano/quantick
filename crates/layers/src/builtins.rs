//! The built-in chart layers, each one descriptor registered in [`ALL`].
use crate::{
    ChartLayer, LayerDescriptor, LayerScope, LayerSource, OrderflowSwitch, Persistence, Requirement,
};

#[allow(non_upper_case_globals)]
impl ChartLayer {
    pub const TapeChart: Self = Self(&LayerDescriptor {
        id: "tape_chart",
        label: "show the tape",
        hint: "the rolling tape pinned to the right edge: prints landing into the book in real \
                 time, on their own fixed window of market time. Off, the band is not reserved at \
                 all and the candles take the whole canvas; its two layers keep their settings, so \
                 switching it back on returns the tape you switched off",
        source: LayerSource::Orderflow(OrderflowSwitch::Tape),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: true,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const TapeHeatmap: Self = Self(&LayerDescriptor {
        id: "tape_heatmap",
        label: "L2 heatmap",
        hint: "resting depth on the tape. The toolbar's L2 button does not touch this — it is \
                 the candles' switch. Recording never stops either way, so hiding this loses no \
                 history",
        source: LayerSource::Orderflow(OrderflowSwitch::TapeDepth),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::Book,
        on_tape: true,
        needs_tape: true,
        needs_depth: false,
        capture_gates_visibility: true,
        default_on: true,
        projection_demand: false,
    });
    pub const TapeBubbles: Self = Self(&LayerDescriptor {
        id: "tape_bubbles",
        label: "aggression bubbles",
        hint: "confirmed executions rolling through the tape, drawn where they printed. The \
                 toolbar's bubble button does not touch this — it is the candles' switch",
        source: LayerSource::Orderflow(OrderflowSwitch::TapeBubbles),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::Volume,
        on_tape: true,
        needs_tape: true,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const Heatmap: Self = Self(&LayerDescriptor {
        id: "heatmap",
        label: "L2 heatmap",
        hint: "resting depth behind the candles. Recording never stops, so hiding it loses no \
                 history. The tape has a switch of its own and this one never moves it — \
                 right-click the tape to reach it",
        source: LayerSource::Orderflow(OrderflowSwitch::Depth),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::Book,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: true,
        default_on: true,
        projection_demand: false,
    });
    pub const Bubbles: Self = Self(&LayerDescriptor {
        id: "bubbles",
        label: "aggression bubbles",
        hint: "confirmed executions from the trade stream, drawn where they printed, on the \
                 candles. Beside the native tape, one summary bubble per tick candle, as \
                 candle aggression draws it. The tape has a switch of its own and this one \
                 never moves it — right-click the tape to reach it",
        source: LayerSource::Orderflow(OrderflowSwitch::Bubbles),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::Volume,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const Footprint: Self = Self(&LayerDescriptor {
        id: "footprint",
        label: "candle footprint",
        hint: "the buy/sell split at each price inside every candle. Detail follows zoom: \
                 numbers close in, profile and highlight marks further out. The violet line \
                 is the bar's point of control (the price with the most volume); an Nx badge \
                 at a bar's extreme is its aggression ratio there. The legend names the rows' \
                 price width at all times",
        source: LayerSource::Footprint,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::Volume,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const LiveStrip: Self = Self(&LayerDescriptor {
        id: "live_strip",
        label: "live strip",
        hint: "the book's resting depth and the forming bar's aggression, beside the price axis",
        source: LayerSource::Local,
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::BookOrVolume,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: true,
    });
    pub const LaneMarks: Self = Self(&LayerDescriptor {
        id: "lane_marks",
        label: "live lane marks",
        hint: "the dashed line where the bar slots end and the tape begins, and the line on the \
                 live edge itself. Saved with the order-flow preset, not with the other layers",
        source: LayerSource::Orderflow(OrderflowSwitch::Marks),
        scope: LayerScope::FlowPane,
        persistence: Persistence::OrderflowPreset,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: true,
    });
    pub const FlowLegend: Self = Self(&LayerDescriptor {
        id: "flow_legend",
        label: "chart legend",
        hint: "the key naming visible flow layers, or the compact buy/sell key above a tape-only \
                 plot. Hiding the key leaves the layers drawing. The same switch as the L2 \
                 panel's 'show chart legend'",
        source: LayerSource::Orderflow(OrderflowSwitch::Legend),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const BookStatus: Self = Self(&LayerDescriptor {
        id: "book_status",
        label: "book status badge",
        hint: "the badge at the top-right reporting on the book feed. Hiding it silences a \
                 label, never the recorder: capture, generation and the ladder carry on, and the \
                 L2 panel still states them. A book that goes down or errors brings the badge \
                 back on its own — hidden chrome may not hide a dead feed",
        source: LayerSource::Orderflow(OrderflowSwitch::Status),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::Book,
        on_tape: false,
        needs_tape: false,
        needs_depth: true,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const DepthGaps: Self = Self(&LayerDescriptor {
        id: "depth_gaps",
        label: "L2 gap boundaries",
        hint: "dashed boundaries around intervals with no depth coverage. Off, a stretch the \
                 recorder never saw looks exactly like one it did. Drawn over the heatmap, so \
                 they only show while it is on",
        source: LayerSource::Orderflow(OrderflowSwitch::Gaps),
        scope: LayerScope::FlowPane,
        persistence: Persistence::Layers,
        requirement: Requirement::Book,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const BubbleOverlapMerge: Self = Self(&LayerDescriptor {
        id: "bubble_overlap_merge",
        label: "volume dots (Bookmap style)",
        hint: "every print lands in one dot per bar, window of market time and price level: the \
                 exact summed quantity, a buy/sell pie when both sides traded there, sized by the \
                 volume its cell holds. Windows and levels are anchored in market time and price, \
                 so a closed dot never moves; zooming picks wider or narrower cells. Dots may \
                 overlap, the biggest on top. \
                 Saved with the order-flow preset, not with the other layers",
        source: LayerSource::Orderflow(OrderflowSwitch::OverlapMerge),
        scope: LayerScope::FlowPane,
        persistence: Persistence::OrderflowPreset,
        requirement: Requirement::Volume,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: false,
    });
    pub const TapeOnly: Self = Self(&LayerDescriptor {
        id: "tape_only",
        label: "tape only (hide candles)",
        hint: "the tape takes the whole canvas, Bookmap style: no candles and no candle marks, \
               time runs across the full width on the tape's own window, and the price axis \
               follows the tape's prints. Scrolling zooms the tape's window. \
               Saved with the order-flow preset, not with the other layers",
        source: LayerSource::Orderflow(OrderflowSwitch::TapeOnly),
        scope: LayerScope::FlowPane,
        persistence: Persistence::OrderflowPreset,
        requirement: Requirement::None,
        on_tape: true,
        needs_tape: true,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: false,
    });
    pub const NativeTape: Self = Self(&LayerDescriptor {
        id: "native_tape",
        label: "native tape (execution time and price)",
        hint: "the tape draws each print at its own execution time and price, on the market \
               clock, and the one price axis follows the tape's prints; manual Y still \
               applies. Beside the candles it keeps its share behind the draggable divider and \
               moving or zooming the candles never changes it. Needs volume dots; tape only \
               always draws it. Saved with the order-flow preset, not with the other layers",
        source: LayerSource::Orderflow(OrderflowSwitch::NativeTape),
        scope: LayerScope::FlowPane,
        persistence: Persistence::OrderflowPreset,
        requirement: Requirement::None,
        on_tape: true,
        needs_tape: true,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: false,
    });
    pub const Grid: Self = Self(&LayerDescriptor {
        id: "grid",
        label: "grid",
        hint: "price and time gridlines behind the candles",
        source: LayerSource::Grid,
        scope: LayerScope::Window,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const LastPrice: Self = Self(&LayerDescriptor {
        id: "last_price",
        label: "last price line",
        hint: "the dashed line at the last traded price, and its chip on the axis",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const BackfillDivider: Self = Self(&LayerDescriptor {
        id: "backfill_divider",
        label: "backfill divider",
        hint: "where backfilled history ends and bars built live begin. Off by default: a rule \
                 across every candle for a boundary that is worth reading once",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: false,
    });
    pub const SeamDivider: Self = Self(&LayerDescriptor {
        id: "seam_divider",
        label: "venue/prints seam",
        hint: "where venue candles give way to bars built from prints",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const DaySeparator: Self = Self(&LayerDescriptor {
        id: "day_separator",
        label: "day separator",
        hint: "a tick across the time axis where the display day turns over, dated with the \
                 day it opens, and the leftmost bar's date pinned to the axis's left edge. \
                 Nothing is drawn over the candles. A day with no bars is not drawn: the mark \
                 lands on the first bar that exists",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: false,
        projection_demand: false,
    });
    pub const Crosshair: Self = Self(&LayerDescriptor {
        id: "crosshair",
        label: "crosshair",
        hint: "the hover cross and its price/time tags",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const PointerPrice: Self = Self(&LayerDescriptor {
        id: "pointer_price",
        label: "track pointer price",
        hint: "a tick and the price on the right axis, following the pointer while it is over the chart. Gone the moment it leaves. The crosshair tool draws its own, so the two never stack",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const PointerTime: Self = Self(&LayerDescriptor {
        id: "pointer_time",
        label: "track pointer time",
        hint: "a tick and a clock time on the bottom axis, for the bar under the pointer — how a chart whose bars are cut by ticks or volume answers 'when was this?'. Empty canvas past the newest bar holds no bar, so nothing is marked there rather than a time being extrapolated",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const PaperTrading: Self = Self(&LayerDescriptor {
        id: "paper_trading",
        label: "paper orders & position",
        hint: "simulated entries, exits and the open position. Hiding them draws nothing and \
                 cancels nothing — the orders stay working and the dock still shows them",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const TradePaint: Self = Self(&LayerDescriptor {
        id: "trade_paint",
        label: "closed trade marks",
        hint: "entry and exit marks for closed simulated trades, joined by a faint line. \
                 Hiding them draws nothing and forgets nothing — the trades stay in the \
                 ledger and on disk",
        source: LayerSource::Local,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
    pub const Drawings: Self = Self(&LayerDescriptor {
        id: "drawings",
        label: "drawings",
        hint: "everything drawn by hand. Hidden objects keep their anchors and stop answering \
                 the pointer until the layer is shown again",
        source: LayerSource::Drawings,
        scope: LayerScope::Pane,
        persistence: Persistence::Layers,
        requirement: Requirement::None,
        on_tape: false,
        needs_tape: false,
        needs_depth: false,
        capture_gates_visibility: false,
        default_on: true,
        projection_demand: false,
    });
}
pub const ALL: [ChartLayer; 26] = [
    ChartLayer::TapeChart,
    ChartLayer::TapeHeatmap,
    ChartLayer::TapeBubbles,
    ChartLayer::Heatmap,
    ChartLayer::Bubbles,
    ChartLayer::Footprint,
    ChartLayer::LiveStrip,
    ChartLayer::LaneMarks,
    ChartLayer::FlowLegend,
    ChartLayer::BookStatus,
    ChartLayer::DepthGaps,
    ChartLayer::BubbleOverlapMerge,
    ChartLayer::TapeOnly,
    ChartLayer::NativeTape,
    ChartLayer::Grid,
    ChartLayer::LastPrice,
    ChartLayer::BackfillDivider,
    ChartLayer::SeamDivider,
    ChartLayer::DaySeparator,
    ChartLayer::Crosshair,
    ChartLayer::PointerPrice,
    ChartLayer::PointerTime,
    ChartLayer::PaperTrading,
    ChartLayer::TradePaint,
    ChartLayer::Drawings,
    ChartLayer::CandleAggression,
];
