//! Quantick Canonical JSON version 1 and SHA-256 digests.

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalJsonError {
    FloatingPointNumber,
}

impl fmt::Display for CanonicalJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("floating-point JSON numbers are forbidden")
    }
}

impl std::error::Error for CanonicalJsonError {}

pub fn canonical_json(value: &Value) -> Result<String, CanonicalJsonError> {
    let mut output = String::new();
    write_value(value, &mut output)?;
    Ok(output)
}

pub fn validate_control_value(value: &Value) -> Result<(), CanonicalJsonError> {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::Number(number) if number.is_f64() => {
                return Err(CanonicalJsonError::FloatingPointNumber);
            }
            Value::Array(values) => pending.extend(values),
            Value::Object(values) => pending.extend(values.values()),
            _ => {}
        }
    }
    Ok(())
}

pub fn canonical_digest(value: &Value) -> Result<String, CanonicalJsonError> {
    let canonical = canonical_json(value)?;
    Ok(raw_digest(canonical.as_bytes()))
}

pub fn canonical_sha256(value: &Value) -> Result<Sha256Digest, CanonicalJsonError> {
    canonical_digest(value).map(Sha256Digest)
}

pub fn raw_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity("sha256:".len() + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Sha256Digest(
    #[schemars(length(equal = 71), regex(pattern = r"^sha256:[0-9a-f]{64}$"))] String,
);

impl Sha256Digest {
    pub fn new(value: impl Into<String>) -> Result<Self, DigestValidationError> {
        let value = value.into();
        let valid = value.len() == 71
            && value.starts_with("sha256:")
            && value[7..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return Err(DigestValidationError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DigestValidationError;

impl fmt::Display for DigestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("digest must be sha256 followed by 64 lowercase hexadecimal digits")
    }
}

impl std::error::Error for DigestValidationError {}

fn write_value(value: &Value, output: &mut String) -> Result<(), CanonicalJsonError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => {
            if value.is_f64() {
                return Err(CanonicalJsonError::FloatingPointNumber);
            }
            output.push_str(&value.to_string());
        }
        Value::String(value) => write_string(value, output),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_value(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_unstable_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            for (index, key) in keys.into_iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                write_string(key, output);
                output.push(':');
                write_value(&values[key], output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

fn write_string(value: &str, output: &mut String) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\t' => output.push_str("\\t"),
            '\n' => output.push_str("\\n"),
            '\u{0c}' => output.push_str("\\f"),
            '\r' => output.push_str("\\r"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                write!(output, "\\u{:04x}", character as u32)
                    .expect("writing to String cannot fail");
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn objects_are_sorted_and_utf8_is_not_escaped() {
        assert_eq!(
            canonical_json(&json!({"z": 1, "é": "naïve", "a": [2, 1]})).unwrap(),
            "{\"a\":[2,1],\"z\":1,\"é\":\"naïve\"}"
        );
    }

    #[test]
    fn floating_point_numbers_are_rejected() {
        assert_eq!(
            canonical_json(&json!(1.5)),
            Err(CanonicalJsonError::FloatingPointNumber)
        );
    }
}
