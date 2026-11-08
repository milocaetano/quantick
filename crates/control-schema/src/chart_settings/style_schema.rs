//! Bounded wire schema for the stores-owned chart style document.
use quantick_stores::chart_style::{
    MAX_BODY_HEIGHT, MAX_BODY_WIDTH_FRAC, MAX_CANDLE_GAP, MAX_CORNER_RADIUS, MAX_LINE_WIDTH,
    MIN_BODY_HEIGHT, MIN_BODY_WIDTH_FRAC, MIN_CANDLE_GAP, MIN_LINE_WIDTH,
};
use serde_json::{Value, json};

/// The one ownership marker every field's provenance uses, named once under
/// `$defs` with its default so the many markers stay one reference each.
const OWNERSHIP_MARKER: &str = "StyleOwnership";

pub(super) fn chart_style(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    generator.definitions_mut().insert(
        OWNERSHIP_MARKER.to_owned(),
        json!({"type": "string", "enum": ["theme", "explicit"], "default": "explicit"}),
    );
    let inherited = json!({"$ref": format!("#/$defs/{OWNERSHIP_MARKER}")});
    let ownership = json!({
        "type": "object",
        "additionalProperties": false,
        "description": "Theme follows the chart's theme; explicit keeps the value across themes. Omitted markers are explicit; inspect returns all.",
        "properties": {
            "fill_opacity": inherited.clone(),
            "bull_fill": inherited.clone(),
            "bear_fill": inherited.clone(),
            "bull_outline": inherited.clone(),
            "bear_outline": inherited.clone(),
            "wick": inherited.clone(),
            "background": inherited.clone(),
            "grid": inherited.clone(),
            "outline_width": inherited.clone(),
            "outline_opacity": inherited.clone(),
            "wick_opacity": inherited.clone(),
            "time_grid": inherited
        }
    });
    let candles = json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "outline_only", "bull_fill", "bear_fill", "bull_outline", "bear_outline",
            "fill_opacity", "outline_opacity", "outline_width", "show_wicks",
            "custom_wick_color", "wick", "wick_opacity", "wick_width",
            "body_width_frac", "gap_px", "corner_radius", "min_body_height",
            "forming_opacity"
        ],
        "properties": {
            "outline_only": {"type": "boolean"},
            "bull_fill": rgb(),
            "bear_fill": rgb(),
            "bull_outline": rgb(),
            "bear_outline": rgb(),
            "fill_opacity": range(0.0, 1.0),
            "outline_opacity": range(0.0, 1.0),
            "outline_width": range(MIN_LINE_WIDTH, MAX_LINE_WIDTH),
            "show_wicks": {"type": "boolean"},
            "custom_wick_color": {"type": "boolean"},
            "wick": rgb(),
            "wick_opacity": range(0.0, 1.0),
            "wick_width": range(MIN_LINE_WIDTH, MAX_LINE_WIDTH),
            "body_width_frac": range(MIN_BODY_WIDTH_FRAC, MAX_BODY_WIDTH_FRAC),
            "gap_px": range(MIN_CANDLE_GAP, MAX_CANDLE_GAP),
            "corner_radius": range(0.0, MAX_CORNER_RADIUS),
            "min_body_height": range(MIN_BODY_HEIGHT, MAX_BODY_HEIGHT),
            "forming_opacity": range(0.0, 1.0)
        }
    });
    let canvas = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["background_enabled", "background", "grid_enabled", "grid", "grid_opacity"],
        "properties": {
            "background_enabled": {"type": "boolean"},
            "background": rgb(),
            "grid_enabled": {"type": "boolean"},
            "time_grid_enabled": {"type": "boolean"},
            "grid": rgb(),
            "grid_opacity": range(0.0, 1.0)
        }
    });
    schemars::json_schema!({
        "type": "object",
        "additionalProperties": false,
        "description": "One pane's candle/canvas style: read effective_style, edit values and ownership, submit it whole to update_candles. A changed value on a theme-owned field is refused until its ownership is explicit. Geometry and opacity are canonical decimal strings in inclusive bounds.",
        "required": ["candles", "canvas"],
        "properties": {
            "ownership": ownership,
            "candles": candles,
            "canvas": canvas
        }
    })
}

pub(super) fn optional_chart_style(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let style = chart_style(generator);
    schemars::json_schema!({"anyOf": [style, {"type": "null"}]})
}

fn rgb() -> Value {
    json!({
        "type": "array",
        "description": "sRGB red, green and blue byte values, in that order.",
        "minItems": 3,
        "maxItems": 3,
        "items": {"type": "integer", "minimum": 0, "maximum": 255}
    })
}

fn range(min: f32, max: f32) -> Value {
    json!({"type": "string", "pattern": r"^-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?$",
        "description": format!("Decimal [{min}, {max}], inclusive.")})
}

#[cfg(test)]
mod tests {
    use super::super::{ChartSettingsInput, ChartSettingsResult, descriptor};
    use quantick_control::limits::CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES;
    use quantick_control::schema::{CompiledSchema, generated_schema};
    use serde_json::{Value, json};

    /// The style document as each published schema carries it.
    fn style_of(schema: &Value) -> &Value {
        let update = schema["oneOf"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|variant| variant["properties"]["action"]["const"] == "update_candles");
        match update {
            Some(variant) => &variant["properties"]["style"],
            None => &schema["properties"]["effective_style"]["anyOf"][0],
        }
    }

    #[test]
    fn every_ownership_marker_states_its_explicit_default_and_the_descriptor_fits() {
        for schema in [
            generated_schema::<ChartSettingsInput>(),
            generated_schema::<ChartSettingsResult>(),
        ] {
            let marker = &schema["$defs"][super::OWNERSHIP_MARKER];
            assert_eq!(marker["default"], json!("explicit"), "{marker}");
            assert_eq!(marker["enum"], json!(["theme", "explicit"]));
            let markers = style_of(&schema)["properties"]["ownership"]["properties"]
                .as_object()
                .expect("an ownership object");
            assert_eq!(markers.len(), 12, "{markers:?}");
            for (field, value) in markers {
                assert_eq!(
                    value["$ref"],
                    json!(format!("#/$defs/{}", super::OWNERSHIP_MARKER)),
                    "{field}"
                );
            }
            CompiledSchema::new(&schema).expect("the schema compiles with its reference");
        }
        let bytes = serde_json::to_vec(&descriptor()).unwrap().len();
        assert!(
            bytes <= CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES,
            "chart settings descriptor is {bytes} bytes"
        );
    }
}
