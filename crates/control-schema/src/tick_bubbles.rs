//! A scoped tick-chart appearance operation; the tape retains its own settings.
use quantick_control::{registry::IdempotencyPolicy, wire::WireU64};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const TICK_BUBBLES_CAPABILITY_ID: &str = "orderflow.bubbles.tick.set";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TickBubblesInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// A name in orderflow.bubbles.tick_presets; null restores regional FLOW.
    pub preset: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TickBubblesResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub changed: bool,
    pub tick_bubbles: Option<TickBubblesSnapshot>,
}

pub fn result(
    input: &TickBubblesInput,
    changed: bool,
    config: &quantick_orderflow::HeatmapConfig,
) -> TickBubblesResult {
    TickBubblesResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        changed,
        tick_bubbles: config.volume_dots.tick_bubbles.map(Into::into),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TickBubblesSnapshot {
    /// Instrument quantity at max_radius. Larger visible groups increase the
    /// common reference uniformly, so all areas remain proportional.
    pub reference_quantity: String,
    pub max_radius: String,
    pub label_min_radius: String,
    pub opacity: String,
}

impl From<quantick_orderflow::TickBubbleStyle> for TickBubblesSnapshot {
    fn from(style: quantick_orderflow::TickBubbleStyle) -> Self {
        Self {
            reference_quantity: style.reference_quantity.to_string(),
            max_radius: style.max_radius.to_string(),
            label_min_radius: style.label_min_radius.to_string(),
            opacity: style.opacity.to_string(),
        }
    }
}

pub fn descriptor() -> quantick_control::registry::CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<TickBubblesInput, TickBubblesResult>(
        TICK_BUBBLES_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Set tick chart sphere preset",
        "Applies only a named tick-chart sphere calibration to a Binance BTC tick pane. Read available names from orderflow.bubbles.tick_presets. A null preset restores regional FLOW. Tape sizing, colors, grouping and source settings are preserved. Changes follow the asset save switch and are read back in orderflow.bubbles.tick_bubbles. The Bubbles layer must be enabled to draw the selected style.",
        "Stable tab and pane IDs, tick membership, asset ownership and preset names are validated before any edit. Repeating a selection is a no-op.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::snapshot(
    TICK_BUBBLES_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.tick_bubbles",
    "the addressed pane reports the selected tick sphere calibration independently of the tape",
    &[
        "tick_spheres_are_scoped_retry_safe_and_preserve_tape_settings",
        crate::readback::EVERY_OPTIONAL_TEST,
    ],
)];
