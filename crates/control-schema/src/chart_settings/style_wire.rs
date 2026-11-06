//! Decimal geometry uses the control plane's canonical string representation.
use quantick_control::wire::CanonicalDecimal;
use quantick_stores::chart_style::StyleFile;
use serde_json::Value;

const GEOMETRY: &[(&str, &[&str])] = &[
    (
        "candles",
        &[
            "fill_opacity",
            "outline_opacity",
            "outline_width",
            "wick_opacity",
            "wick_width",
            "body_width_frac",
            "gap_px",
            "corner_radius",
            "min_body_height",
            "forming_opacity",
        ],
    ),
    ("canvas", &["grid_opacity"]),
];

pub fn encode(style: &StyleFile) -> Value {
    let mut value = serde_json::to_value(style).expect("finite chart style");
    for (section, fields) in GEOMETRY {
        for field in *fields {
            let number = value[section][field].as_f64().expect("style geometry") as f32;
            let text = if number == 0.0 {
                "0".into()
            } else {
                number.to_string()
            };
            value[section][field] =
                serde_json::to_value(CanonicalDecimal::new(text).expect("bounded style decimal"))
                    .expect("decimal serialization");
        }
    }
    value
}

pub fn decode(mut value: Value) -> Result<StyleFile, &'static str> {
    for (section, fields) in GEOMETRY {
        for field in *fields {
            let text = value[section][field]
                .as_str()
                .ok_or("style geometry must be canonical decimal strings")?;
            let decimal =
                CanonicalDecimal::new(text).map_err(|_| "invalid canonical style decimal")?;
            let number: f32 = decimal
                .as_str()
                .parse()
                .map_err(|_| "invalid style geometry")?;
            if !number.is_finite() {
                return Err("style geometry must be finite");
            }
            value[section][field] =
                serde_json::to_value(number).map_err(|_| "invalid style geometry")?;
        }
    }
    serde_json::from_value(value).map_err(|_| "invalid chart style document")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_style_geometry_roundtrips_and_refuses_noncanonical_inputs() {
        let style: StyleFile = toml_style();
        let document = encode(&style);
        quantick_control::canonical::validate_control_value(&document).unwrap();
        assert_eq!(decode(document.clone()).unwrap(), style);
        for invalid in [
            json!("0.50"),
            json!("-0"),
            json!("NaN"),
            json!("1e2"),
            json!(0.5),
            Value::Null,
        ] {
            let mut candidate = document.clone();
            candidate["candles"]["gap_px"] = invalid;
            assert!(decode(candidate).is_err());
        }
        for invalid in [Value::Null, json!([]), json!({"candles": 1})] {
            assert!(decode(invalid).is_err());
        }
    }

    fn toml_style() -> StyleFile {
        // The persisted chart default covers every decimal field, including fractions.
        serde_json::from_value(json!({
            "candles": {"outline_only":false,"bull_fill":[1,2,3],"bear_fill":[4,5,6],
                "bull_outline":[1,2,3],"bear_outline":[4,5,6],"fill_opacity":0.2,
                "outline_opacity":1.0,"outline_width":0.5,"show_wicks":true,
                "custom_wick_color":false,"wick":[1,2,3],"wick_opacity":1.0,
                "wick_width":1.0,"body_width_frac":0.1,"gap_px":4.5,
                "corner_radius":0.0,"min_body_height":1.0,"forming_opacity":0.75},
            "canvas":{"background_enabled":true,"background":[0,0,0],
                "grid_enabled":true,"grid":[1,2,3],"grid_opacity":0.1}
        }))
        .unwrap()
    }
}
