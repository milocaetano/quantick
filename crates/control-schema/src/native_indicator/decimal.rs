//! Decimal transport for native settings; domain values remain floating point.
use quantick_control::wire::CanonicalDecimal;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser};

fn canonical(
    value: impl ToString,
) -> Result<CanonicalDecimal, quantick_control::wire::WireValueError> {
    let text = value.to_string();
    CanonicalDecimal::new(if text == "-0" { "0".into() } else { text })
}

pub mod float {
    use super::*;
    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        canonical(value)
            .map_err(ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let decimal = CanonicalDecimal::deserialize(deserializer)?;
        let value = decimal.as_str().parse::<f64>().map_err(de::Error::custom)?;
        if !value.is_finite() {
            return Err(de::Error::custom("native float must be finite"));
        }
        Ok(value)
    }
}

pub mod width {
    use super::*;
    pub fn serialize<S: Serializer>(value: &Option<f32>, serializer: S) -> Result<S::Ok, S::Error> {
        value
            .map(canonical)
            .transpose()
            .map_err(ser::Error::custom)?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f32>, D::Error> {
        Option::<CanonicalDecimal>::deserialize(deserializer)?
            .map(|decimal| {
                let value = decimal.as_str().parse::<f32>().map_err(de::Error::custom)?;
                if !value.is_finite() {
                    return Err(de::Error::custom("plot width must be finite"));
                }
                Ok(value)
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::native_indicator::{PlotStyle, Value};
    use serde_json::json;
    #[test]
    fn fractional_settings_round_trip_without_json_floats() {
        for number in [0.0, -0.0, 1.25, -2.5, f64::MIN_POSITIVE, f64::MAX] {
            let encoded = serde_json::to_value(Value::Float(number)).unwrap();
            quantick_control::canonical::validate_control_value(&encoded).unwrap();
            let Value::Float(decoded) = serde_json::from_value(encoded).unwrap() else {
                panic!("expected float")
            };
            assert_eq!(decoded, number);
        }
        let style: PlotStyle = serde_json::from_value(json!({"width":"2.5"})).unwrap();
        style.validate().unwrap();
        assert_eq!(style.width, Some(2.5));
        assert_eq!(serde_json::to_value(style).unwrap()["width"], "2.5");
        let defaults = serde_json::to_value(super::super::catalog()).unwrap();
        quantick_control::canonical::validate_control_value(&defaults).unwrap();
    }
    #[test]
    fn malformed_or_unrepresentable_decimals_are_rejected() {
        for value in [
            json!(2.5),
            json!("NaN"),
            json!("inf"),
            json!("2.50"),
            json!("1e2"),
            json!("9".repeat(400)),
        ] {
            assert!(
                serde_json::from_value::<Value>(json!({"type":"float","value":value})).is_err()
            );
            assert!(serde_json::from_value::<PlotStyle>(json!({"width":value})).is_err());
        }
        for number in [f64::NAN, f64::INFINITY] {
            assert!(serde_json::to_value(Value::Float(number)).is_err());
        }
    }
}
