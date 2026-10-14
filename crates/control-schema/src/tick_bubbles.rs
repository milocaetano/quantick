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
    /// Optional fill fraction for small volume, reaching full opacity at reference/4.
    pub volume_opacity_floor: Option<String>,
    pub tape_bursts: Option<TickBurstSnapshot>,
    /// Denomination of the preset's reference quantity and displayed labels.
    #[serde(default = "default_quantity_unit")]
    pub quantity_unit: String,
    /// Temporal support of candle regions; does not alter tape grouping.
    #[serde(default = "default_region_window")]
    pub region_window_ms: String,
    /// Instrument quantity at the reference radius. Causal bursts keep it fixed;
    /// legacy regions raise the common reference for larger visible groups.
    pub reference_quantity: String,
    pub max_radius: String,
    pub label_min_radius: String,
    pub opacity: String,
}

fn default_quantity_unit() -> String {
    "BTC".to_owned()
}
fn default_region_window() -> String {
    "1000".to_owned()
}

impl From<quantick_orderflow::TickBubbleStyle> for TickBubblesSnapshot {
    fn from(style: quantick_orderflow::TickBubbleStyle) -> Self {
        Self {
            volume_opacity_floor: style.volume_opacity_floor.map(|value| value.to_string()),
            tape_bursts: style.tape_bursts.map(Into::into),
            quantity_unit: style.quantity_unit.label().to_owned(),
            region_window_ms: style.region_window_ms.to_string(),
            reference_quantity: style.reference_quantity.to_string(),
            max_radius: style.max_radius.to_string(),
            label_min_radius: style.label_min_radius.to_string(),
            opacity: style.opacity.to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TickBurstSnapshot {
    /// Independent UTC blocks; a boundary-crossing burst remains two groups.
    pub window_ms: String,
    pub width_px: String,
    pub price_units_per_pixel: String,
    pub max_radius: String,
}

impl From<quantick_orderflow::TickBurstGrouping> for TickBurstSnapshot {
    fn from(value: quantick_orderflow::TickBurstGrouping) -> Self {
        Self {
            window_ms: value.window_ms.to_string(),
            width_px: value.width_px.to_string(),
            price_units_per_pixel: value.price_units_per_pixel.to_string(),
            max_radius: value.max_radius.to_string(),
        }
    }
}

pub fn descriptor() -> quantick_control::registry::CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<TickBubblesInput, TickBubblesResult>(
        TICK_BUBBLES_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Set tick chart sphere preset",
        "Applies only a named tick-chart sphere calibration to a supported asset's tick pane. Read available names from orderflow.bubbles.tick_presets. A null preset restores regional FLOW. Tape sizing, colors, grouping and source settings are preserved. Changes follow the asset save switch and are read back in orderflow.bubbles.tick_bubbles, including quantity_unit, region_window_ms and optional tape_bursts calibration. WIN candle bursts use independent fixed time blocks; they do not reconstruct a running tape's prior display epoch. The Bubbles layer must be enabled to draw the selected style. The B3 WIN tick spheres preset is chart-only through orderflow.bubbles.preset.apply too; whole-look presets retain their declared scope.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_original_btc_snapshot_remains_a_valid_v1_payload() {
        let original = serde_json::json!({
            "reference_quantity": "40", "max_radius": "32",
            "label_min_radius": "8", "opacity": "0.9"
        });
        let schema = serde_json::to_value(schemars::schema_for!(TickBubblesSnapshot)).unwrap();
        let required = schema["required"].as_array().unwrap();
        for additive in [
            "quantity_unit",
            "region_window_ms",
            "tape_bursts",
            "volume_opacity_floor",
        ] {
            assert!(!required.iter().any(|field| field == additive));
        }
        let decoded: TickBubblesSnapshot = serde_json::from_value(original).unwrap();
        assert_eq!(decoded.quantity_unit, "BTC");
        assert_eq!(decoded.region_window_ms, "1000");
        assert!(decoded.tape_bursts.is_none());
    }
}
