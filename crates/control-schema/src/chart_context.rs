//! `chart.context.read`: one chart pane as a document an agent can reason
//! over — its market and bar rule, the indicators on it with their inputs,
//! its drawings by name, and the strategies armed on them. Each list is
//! capped; its total says how many there were.
use crate::native_indicator::Value;
use quantick_control::annotation::AnnotationBand;
use quantick_control::registry::CapabilityDescriptor;
use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_control_host::authority::{ReadSpec, read_descriptor};
use quantick_control_host::wire::BarSpecDto;
use quantick_stores::indicator_state::{SavedIndicator, SavedInputExt, SavedKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const CONTEXT_CAPABILITY_ID: &str = "chart.context.read";
pub const MAX_INDICATORS: usize = 64;
pub const MAX_DRAWINGS: usize = 256;
pub const MAX_STRATEGIES: usize = 64;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Document {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub feed_id: String,
    pub symbol: String,
    pub bar_spec: BarSpecDto,
    /// The first [`MAX_INDICATORS`], in the pane's order.
    pub indicators: Vec<Indicator>,
    pub indicators_total: WireU64,
    /// The first [`MAX_DRAWINGS`] this chart's layout shows, back to front;
    /// a mark held only for another strip layout is left out.
    pub drawings: Vec<Drawing>,
    pub drawings_total: WireU64,
    /// The first [`MAX_STRATEGIES`], in arming order.
    pub strategies: Vec<Strategy>,
    pub strategies_total: WireU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IndicatorKind {
    Native,
    Script,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct Indicator {
    pub slot_id: WireU64,
    pub kind: IndicatorKind,
    /// The native catalog id (`native.ema`) or the script's library name.
    pub id: String,
    pub hidden: bool,
    /// Whether the pane runs it without an error.
    pub loaded: bool,
    /// In declaration order.
    pub inputs: Vec<IndicatorInput>,
}

/// One input value. A script's saved inputs carry no names, so `name` is
/// absent for them; their order is the script's declaration order.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct IndicatorInput {
    pub name: Option<String>,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct Drawing {
    pub id: WireU64,
    /// The trader's name for it, or the one generated when it was placed
    /// (`zone-1`, `line-2`).
    pub name: Option<String>,
    /// The drawing tool id (`rectangle`, `trend-line`).
    pub kind: String,
    /// The value axis the prices are on: the candles' price axis, or an
    /// indicator pane's (then a price is that indicator's value). Absent for
    /// a mark of an instant, which has no value axis.
    pub band: Option<AnnotationBand>,
    /// One per anchor, in anchor order; null for an anchor whose price is
    /// not a finite number or not representable as a canonical decimal.
    pub prices: Vec<Option<CanonicalDecimal>>,
    pub selected: bool,
    pub locked: bool,
    /// Hidden by its own eye or by the chart's hide-all.
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct Strategy {
    pub drawing_id: WireU64,
    /// The name of the drawing it rides; absent when that drawing is gone or
    /// held only for another strip layout, so not listed in `drawings`.
    pub drawing_name: Option<String>,
    pub preset: String,
    /// `buy` or `sell`.
    pub side: String,
    pub quantity: CanonicalDecimal,
    /// Take profit at this multiple of the trigger bar's projection; zero or
    /// less, no take-profit leg.
    pub tp_mult: CanonicalDecimal,
    /// Stop at this multiple of the trigger bar's projection; zero or less,
    /// no stop leg.
    pub sl_mult: CanonicalDecimal,
    /// `ignore` or `retest_limit`: what a bar cutting through the region does.
    pub on_break: String,
    /// `paper` places orders; `alarm_only` only watches.
    pub execution: String,
    /// `one_shot` or `auto`.
    pub rearm: String,
    /// `armed`, `fired`, `retest_resting`, `in_position`, `done` or `disarmed`.
    pub state: String,
    pub disarm_reason: Option<String>,
    /// The badge the chart prints beside the drawing.
    pub badge: String,
}

pub fn descriptor() -> CapabilityDescriptor {
    read_descriptor::<Input, Document>(&ReadSpec {
        id: CONTEXT_CAPABILITY_ID,
        module: "chart",
        title: "Read one chart's context",
        description: "Reads one exact pane as a single document: feed, symbol and bar type with its parameter; indicators (up to 64) with kind, id, hidden and loaded state and their committed input values; drawings this chart's layout shows (up to 256) with id, name, tool kind, value band, anchor prices (null where not finite), and selected, locked and hidden (own or hide-all) flags; strategies armed on its drawings (up to 64) with the drawing they ride, preset, side, quantity, take-profit and stop multiples, break policy, execution, rearm, state and badge. Every drawing carries a name: the trader's, or one generated per kind on placement and on load (zone-1, line-2), stable across save and load. Each list reports its total. Script inputs list values in declaration order without names. Changes nothing.",
        permissions: &[
            "observe",
            "observe.market",
            "observe.chart",
            "observe.indicators",
            "observe.drawings",
            "observe.paper",
            "observe.user_text",
        ],
        pagination: None,
    })
}

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as s;
    vec![
        ("chart-context-read-input-v1.schema.json", s::<Input>()),
        ("chart-context-read-result-v1.schema.json", s::<Document>()),
    ]
}

/// The first [`MAX_INDICATORS`] of the pane's saved slots, each with whether
/// it runs; natives name their inputs from the catalog.
pub fn indicators(saved: Vec<(u64, SavedIndicator, bool)>) -> Vec<Indicator> {
    saved
        .into_iter()
        .take(MAX_INDICATORS)
        .map(|(slot, entry, loaded)| indicator(slot, entry, loaded))
        .collect()
}

fn indicator(slot: u64, entry: SavedIndicator, loaded: bool) -> Indicator {
    let (kind, id) = match entry.kind {
        SavedKind::Native { id } => (IndicatorKind::Native, id),
        SavedKind::Script { name } => (IndicatorKind::Script, name),
    };
    let saved = |i: usize| entry.inputs.get(i).and_then(SavedInputExt::to_value);
    let native = (kind == IndicatorKind::Native)
        .then(|| quantick_indicators::native::native(&id))
        .flatten();
    let inputs = match native {
        Some(native) => (native.default_inputs().iter().enumerate())
            .map(|(i, spec)| IndicatorInput {
                name: Some(spec.name().to_owned()),
                value: Value::from_input(&saved(i).unwrap_or_else(|| spec.default_value())),
            })
            .collect(),
        None => (0..entry.inputs.len())
            .filter_map(saved)
            .map(|value| IndicatorInput {
                name: None,
                value: Value::from_input(&value),
            })
            .collect(),
    };
    Indicator {
        slot_id: WireU64::new(slot),
        kind,
        id,
        hidden: entry.hidden,
        loaded,
        inputs,
    }
}

#[cfg(test)]
#[path = "chart_context_tests.rs"]
mod tests;
