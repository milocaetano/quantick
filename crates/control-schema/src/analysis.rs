//! Indicator and drawing projections — what the trader put *on* the chart.
//!
//! Two scopes, one owner module. Both enumerate per pane, because that is how
//! the trader arranged them and how an agent must address them. Neither
//! evaluates an indicator or re-anchors a drawing: they publish what the
//! application already holds.
//!
//! Two kinds of trader-authored text meet different rules here, and the line
//! between them is deliberate.
//!
//! *Names* are published: an indicator's title, its plot and input titles, and
//! the `script.<name>` kind all come from a script the trader wrote, and they
//! are how an agent addresses the thing at all. Withholding them would leave
//! the scope answering "there are three indicators" and nothing more, which is
//! not the descriptor roadmap 5.1 asked for. Publishing them is therefore
//! gated on `observe.user_text` as well as `observe.indicators` — the contract
//! declares that permission for "user-authored labels, notes, and scripts",
//! keeps it out of the safe defaults, and until now nothing required it.
//!
//! *Content* is withheld, and only its presence reported: a drawing's own name
//! (a name like "the 108k shelf" is a private note, not an address), a
//! script's diagnostic message, and a `string` input the script left
//! unconstrained. A `string` input the script *did* constrain to a fixed
//! option set is an enumeration, not prose, so its value is published like any
//! other name.
//!
//! `observer_resolves_mirrored_drawings_without_leaking_user_text` guards the
//! drawing half, and now captures this module's enumerating scope alongside
//! the pointer scopes it already covered.

use quantick_control_host::wire::{PaneSideDto, canonical_f64, wire_usize};

use quantick_control::{
    limits::{CONTROL_SNAPSHOT_MAX_DRAWINGS_PER_PANE, CONTROL_SNAPSHOT_MAX_INDICATORS_PER_PANE},
    wire::{CanonicalDecimal, WireU64},
};

use quantick_indicators::{InputSpec, InputValue, PlotSpec, PlotStyle, Rgba8, SourceId};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

pub const INDICATORS_SCOPE_ID: &str = "analysis.indicators";

pub const DRAWINGS_SCOPE_ID: &str = "analysis.drawings";

pub const MODULE_ID: &str = "analysis";

pub const SCHEMA_VERSION: u32 = 1;

/// Indicator outputs are `f64` columns of arbitrary magnitude; ten places is
/// what the cursor scope already publishes an axis value at, so a reading and
/// the axis under it round the same way.
pub const READING_DECIMAL_PLACES: u32 = 10;

/// The stable detail a script failure reports instead of its message. The
/// message is the trader's own script talking and never crosses the wire; the
/// same string `health.rs` publishes, so two scopes describe one failure with
/// one word.
pub const RUNTIME_FAILURE_DETAIL: &str = "runtime_evaluation_failed";

/// The stable detail for a hot reload that failed while the running version
/// kept evaluating.
pub const STALE_RELOAD_DETAIL: &str = "reload_failed_running_version_retained";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndicatorsSnapshot {
    pub tabs: Vec<TabIndicatorsSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabIndicatorsSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneIndicatorsSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneIndicatorsSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    #[schemars(length(max = CONTROL_SNAPSHOT_MAX_INDICATORS_PER_PANE))]
    pub indicators: Vec<IndicatorSnapshot>,
    pub indicator_count: WireU64,
    pub indicators_truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndicatorSnapshot {
    /// The slot the running instance answers to. Reused after a remove, so it
    /// addresses an instance *now* and is not an identity across a session.
    pub slot_id: WireU64,
    /// The constructor it was added through (`native.cvd`, `script.zigzag`),
    /// durable across remove and re-add in a way the slot is not.
    pub kind: String,
    /// `native` or `script` — whether we wrote it or the trader did. It
    /// decides whether any diagnostic text is theirs, and so redacted.
    pub source_kind: String,
    /// Which instance of that kind this is, assigned once at birth. Two EMAs
    /// on one pane are told apart by this, never by position.
    pub ordinal: u32,
    pub title: String,
    pub short_title: Option<String>,
    /// True = drawn over the price chart; false = its own sub-pane below it.
    pub overlay: bool,
    /// The eye toggle. A hidden indicator keeps evaluating, so its readings
    /// below are still current.
    pub hidden: bool,
    /// Whether price-chart hover is mirrored into this non-price pane.
    #[serde(default)]
    pub mouse_vertical_line: bool,
    pub plots: Vec<PlotSnapshot>,
    /// Every declared input paired with the value currently bound to it —
    /// what the settings dialog would open with.
    pub inputs: Vec<IndicatorInputSnapshot>,
    /// Committed bars this indicator has evaluated. Tracked rather than
    /// derived from the plot columns: an indicator whose whole output is
    /// candle paint declares no plots at all.
    pub committed_bar_count: WireU64,
    /// Whether a forming-bar preview frame exists right now.
    pub preview_present: bool,
    /// Why this indicator is not answering, when it is not. Absent means it
    /// evaluated cleanly.
    pub failure: Option<IndicatorFailureSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlotSnapshot {
    /// Index of the column this spec describes, in declaration order.
    pub plot_index: WireU64,
    pub title: String,
    pub style: String,
    /// Horizontal shift in bars; positive is rightward.
    #[schemars(extend("x-unit" = "bars"))]
    pub offset_bars: i32,
    /// True when the column renders as markers instead of through its style.
    pub renders_as_marker: bool,
    pub base_color: String,
    /// The most recent committed value of this column. Absent when the column
    /// has no rows yet, or when the value is not finite — a gap in a series is
    /// a gap, never a zero.
    pub latest_value: Option<CanonicalDecimal>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndicatorInputSnapshot {
    /// Stable identifier — the persistence key and the script argument name.
    pub name: String,
    pub title: String,
    /// `int`, `float`, `bool`, `color`, `string` or `source`.
    pub kind: String,
    /// The bound value, rendered in the input's own vocabulary. Absent for a
    /// free-text input: see `text_present`.
    pub value: Option<String>,
    pub default: Option<String>,
    /// True when this is a `string` input the script left unconstrained, and a
    /// value is bound. Free text is the trader's own words, so the value is
    /// withheld and only its presence reported; a `string` input constrained
    /// to a fixed option set is an enumeration, not prose, and its value is
    /// published above.
    pub text_present: bool,
    /// The fixed choices, when the input declares any.
    pub options: Vec<String>,
}

/// Why an indicator is not answering. The stable `detail` is a code a client
/// branches on; the underlying message is withheld whenever the trader's own
/// script wrote it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndicatorFailureSnapshot {
    /// `error` — disabled until rebuilt — or `stale`: a hot reload failed and
    /// the previously running version is still evaluating.
    pub state: String,
    pub detail: String,
    /// True when a message exists but is the trader's own text.
    pub user_text_redacted: bool,
    /// The bar the evaluation failed on, when the failure names one.
    pub bar_index: Option<WireU64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DrawingsSnapshot {
    pub tabs: Vec<TabDrawingsSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabDrawingsSnapshot {
    pub tab_id: WireU64,
    pub panes: Vec<PaneDrawingsSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaneDrawingsSnapshot {
    pub pane_id: WireU64,
    pub side: PaneSideDto,
    #[schemars(length(max = CONTROL_SNAPSHOT_MAX_DRAWINGS_PER_PANE))]
    pub drawings: Vec<DrawingSnapshot>,
    pub drawing_count: WireU64,
    pub drawings_truncated: bool,
    /// The toolrail's "Hide all" is on, so nothing below is drawn whatever its
    /// own eye says. A separate switch from the per-object `hidden` — "show
    /// all" restores exactly the per-object visibility it found — and stated
    /// here rather than folded into each row, so an agent that turns the layer
    /// back on knows which marks it will get.
    pub layer_hidden: bool,
    /// Position of the selected drawing within this pane's own list — which is
    /// `drawing_count` long and not the possibly truncated `drawings` page
    /// above. Present only when the selection is on this pane.
    pub selected_index: Option<WireU64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DrawingSnapshot {
    /// Identity, stable while the object lives. Position in the list is not
    /// identity: removing an earlier object renumbers positions and never
    /// this.
    pub drawing_id: WireU64,
    pub tool_id: String,
    /// The value axis the anchors were placed against.
    pub band: String,
    /// `this_chart` or `all_charts` — whether the tab's other panes show it.
    pub scope: String,
    pub locked: bool,
    /// This object's own eye. It is not the whole answer to "is it on screen":
    /// the pane's `layer_hidden` above hides every object regardless.
    pub hidden: bool,
    /// The tab changed the instrument under this mark. Time survives a symbol
    /// switch and price does not, so the anchors still resolve while the level
    /// means nothing on the chart it is now over. The object stays and says
    /// so, rather than pretending to be a level on this market.
    pub foreign_market: bool,
    /// This pane's series does not reach the market instant an anchor was
    /// placed at — the mark survived a re-cut, a rewind or a symbol switch,
    /// but no longer sits on the data it was drawn against.
    pub off_series: bool,
    pub anchor_count: WireU64,
    /// True when the trader gave the object their own name. The name itself is
    /// their words and is never published.
    pub user_label_present: bool,
    /// Set when something other than the trader's hand placed this object.
    /// Absent is the trader's own. An object an agent placed must never be
    /// indistinguishable from one the trader placed.
    pub author: Option<DrawingAuthorSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DrawingAuthorSnapshot {
    /// The wire's actor kind: `agent`, `automation` or `human_ui`.
    pub actor_kind: String,
    /// The client's own name, as it introduced itself at the handshake.
    pub client_name: String,
}

pub fn plot_snapshot(index: usize, spec: &PlotSpec, column: Option<&Vec<f64>>) -> PlotSnapshot {
    PlotSnapshot {
        plot_index: wire_usize(index),
        title: spec.title.clone(),
        style: plot_style_name(spec.style).to_owned(),
        offset_bars: spec.offset,
        renders_as_marker: spec.marker.is_some(),
        base_color: colour(spec.base_color),
        latest_value: column
            .and_then(|values| values.last().copied())
            .and_then(|value| canonical_f64(value, READING_DECIMAL_PLACES)),
    }
}

pub fn input_snapshot(spec: &InputSpec, bound: Option<&InputValue>) -> IndicatorInputSnapshot {
    let options = match spec {
        InputSpec::Int { options, .. } => options.iter().map(i64::to_string).collect(),
        InputSpec::Float { options, .. } => options.iter().map(f64::to_string).collect(),
        InputSpec::Str { options, .. } => options.clone(),
        InputSpec::Bool { .. } | InputSpec::Color { .. } => Vec::new(),
        InputSpec::Source { .. } => SourceId::ALL
            .iter()
            .map(|id| source_name(*id).to_owned())
            .collect(),
    };
    // A `string` input the script left unconstrained is free text the trader
    // typed. Its presence is reported; its content is not.
    let free_text = matches!(spec, InputSpec::Str { options, .. } if options.is_empty());
    let value = bound.and_then(|value| input_value_text(value, free_text));
    IndicatorInputSnapshot {
        name: spec.name().to_owned(),
        title: spec.title().to_owned(),
        kind: input_kind_name(spec).to_owned(),
        value,
        default: input_value_text(&spec.default_value(), free_text),
        text_present: free_text && matches!(bound, Some(InputValue::Str(text)) if !text.is_empty()),
        options,
    }
}

/// One bound value in its input's own vocabulary, or `None` when it is free
/// text the trader typed.
pub fn input_value_text(value: &InputValue, free_text: bool) -> Option<String> {
    match value {
        InputValue::Int(value) => Some(value.to_string()),
        InputValue::Float(value) => Some(value.to_string()),
        InputValue::Bool(value) => Some(value.to_string()),
        InputValue::Color(colour_value) => Some(colour(*colour_value)),
        InputValue::Source(source) => Some(source_name(*source).to_owned()),
        InputValue::Str(text) => (!free_text).then(|| text.clone()),
    }
}

/// The wire name of a plot style. Owned here rather than on `PlotStyle`: the
/// vocabulary belongs to the control plane's contract, and the indicator crate
/// has no business carrying a name only the wire uses.
pub const fn plot_style_name(style: PlotStyle) -> &'static str {
    match style {
        PlotStyle::Line => "line",
        PlotStyle::StepLine => "step_line",
        PlotStyle::Histogram => "histogram",
        PlotStyle::Columns => "columns",
        PlotStyle::Circles => "circles",
        PlotStyle::Cross => "cross",
        PlotStyle::Area => "area",
    }
}

/// The wire name of a source series, owned here for the same reason as
/// [`plot_style_name`].
pub const fn source_name(source: SourceId) -> &'static str {
    match source {
        SourceId::Open => "open",
        SourceId::High => "high",
        SourceId::Low => "low",
        SourceId::Close => "close",
        SourceId::Hl2 => "hl2",
        SourceId::Hlc3 => "hlc3",
        SourceId::Ohlc4 => "ohlc4",
        SourceId::Hlcc4 => "hlcc4",
        SourceId::Volume => "volume",
        SourceId::Delta => "delta",
        SourceId::BuyVolume => "buy_volume",
        SourceId::SellVolume => "sell_volume",
        SourceId::TradeCount => "trade_count",
        SourceId::Cvd => "cvd",
    }
}

pub const fn input_kind_name(spec: &InputSpec) -> &'static str {
    match spec {
        InputSpec::Int { .. } => "int",
        InputSpec::Float { .. } => "float",
        InputSpec::Bool { .. } => "bool",
        InputSpec::Color { .. } => "color",
        InputSpec::Str { .. } => "string",
        InputSpec::Source { .. } => "source",
    }
}

/// `#rrggbbaa`, the one colour spelling the control plane publishes.
pub fn colour(value: Rgba8) -> String {
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        value.r, value.g, value.b, value.a
    )
}
