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
    /// A name in orderflow.bubbles.tick_presets; null restores regional FLOW; never omitted.
    // Refused when omitted rather than read as null: null strips the pane's
    // tick spheres and, with the asset's save switch on, the saved look too.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(schema_with = "nullable_string")]
    pub preset: Option<String>,
}

/// A string or null that has to be present: `Option<String>` alone reads
/// as optional to both serde and schemars.
fn nullable_string(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type": ["string", "null"]})
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TickBubblesResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub changed: bool,
    pub tick_bubbles: Option<TickBubblesSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TickBubblesSnapshot {
    /// Full-radius quantity in fixed mode; configured floor in visible-max mode.
    pub reference_quantity: String,
    #[serde(default)]
    pub reference_mode: TickBubbleReferenceMode,
    /// Present when final visible quantities and the current camera size the spheres.
    #[serde(default)]
    pub viewport_scale: Option<ViewportBubbleScaleSnapshot>,
    pub max_radius: String,
    pub label_min_radius: String,
    pub opacity: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TickBubbleReferenceMode {
    #[default]
    Fixed,
    VisibleMax,
}

impl From<quantick_orderflow::TickBubbleStyle> for TickBubblesSnapshot {
    fn from(style: quantick_orderflow::TickBubbleStyle) -> Self {
        Self {
            reference_quantity: style.reference_quantity.to_string(),
            reference_mode: match style.reference_mode {
                quantick_orderflow::TickReferenceMode::Fixed => TickBubbleReferenceMode::Fixed,
                quantick_orderflow::TickReferenceMode::VisibleMax => {
                    TickBubbleReferenceMode::VisibleMax
                }
            },
            viewport_scale: style.viewport_scale.map(Into::into),
            max_radius: style.max_radius.to_string(),
            label_min_radius: style.label_min_radius.to_string(),
            opacity: style.opacity.to_string(),
        }
    }
}

/// Optional drawing calibration; factual grouping uses its existing settings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewportBubbleScaleSnapshot {
    /// Bars on candles; milliseconds on native Tape.
    pub x_units_per_pixel: String,
    pub price_units_per_pixel: String,
}

impl From<quantick_orderflow::ViewportBubbleScale> for ViewportBubbleScaleSnapshot {
    fn from(scale: quantick_orderflow::ViewportBubbleScale) -> Self {
        Self {
            x_units_per_pixel: scale.x_units_per_pixel.to_string(),
            price_units_per_pixel: scale.price_units_per_pixel.to_string(),
        }
    }
}

pub fn descriptor() -> quantick_control::registry::CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<TickBubblesInput, TickBubblesResult>(
        TICK_BUBBLES_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Set tick chart sphere preset",
        "Applies only a named tick-chart sphere calibration to a tick pane whose asset the feed config switches an asset mode on for (symbol_asset_modes). Read available names from orderflow.bubbles.tick_presets. A null preset restores regional FLOW on any pane, whatever put the spheres on. Tape sizing, colors, grouping and source settings are preserved. Changes follow the asset save switch and are read back in orderflow.bubbles.tick_bubbles. The Bubbles layer must be enabled to draw the selected style.",
        "Stable tab and pane IDs and preset names are validated, and one rule shared with the settings picker (a tick chart and the asset's config mode) is asked, before any edit. Repeating a selection is a no-op.",
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
    use serde_json::json;

    #[test]
    fn preset_is_required_and_nullable() {
        let base = json!({"tab_id": "1", "pane_id": "2"});
        let error = serde_json::from_value::<TickBubblesInput>(base.clone())
            .expect_err("an omitted preset must not read as a strip");
        assert!(error.to_string().contains("preset"), "{error}");
        let mut strip = base.clone();
        strip["preset"] = serde_json::Value::Null;
        assert_eq!(
            serde_json::from_value::<TickBubblesInput>(strip)
                .unwrap()
                .preset,
            None
        );
        let mut named = base;
        named["preset"] = json!("Binance BTC Spot spheres");
        assert_eq!(
            serde_json::from_value::<TickBubblesInput>(named)
                .unwrap()
                .preset
                .as_deref(),
            Some("Binance BTC Spot spheres")
        );
        let schema = quantick_control::schema::generated_schema::<TickBubblesInput>();
        assert_eq!(schema["required"], json!(["tab_id", "pane_id", "preset"]));
        assert_eq!(
            schema["properties"]["preset"]["type"],
            json!(["string", "null"])
        );
    }
}
