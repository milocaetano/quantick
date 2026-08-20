//! JSON Schema validation, generation, and conservative compatibility checks.

use std::{collections::BTreeSet, fmt};

use schemars::JsonSchema;
use serde_json::Value;

const DRAFT_2020_12_SCHEMA_URI: &str = "https://json-schema.org/draft/2020-12/schema";

pub fn generated_schema<T: JsonSchema>() -> Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("generated JSON Schema is serializable")
}

pub fn validate_schema(schema: &Value) -> Result<(), SchemaError> {
    if let Some(dialect) = schema
        .as_object()
        .and_then(|object| object.get("$schema"))
        .and_then(Value::as_str)
        && dialect != DRAFT_2020_12_SCHEMA_URI
    {
        return Err(SchemaError::UnsupportedDialect(dialect.to_owned()));
    }
    reject_external_references(schema)?;
    jsonschema::draft202012::meta::validate(schema)
        .map_err(|error| SchemaError::InvalidSchema(error.masked().to_string()))?;
    jsonschema::draft202012::options()
        .build(schema)
        .map_err(|error| SchemaError::InvalidSchema(error.masked().to_string()))?;
    Ok(())
}

pub fn validate_instance(schema: &Value, instance: &Value) -> Result<(), SchemaError> {
    validate_schema(schema)?;
    let validator = jsonschema::draft202012::options()
        .build(schema)
        .map_err(|error| SchemaError::InvalidSchema(error.masked().to_string()))?;
    validator.validate(instance).map_err(|error| {
        SchemaError::InvalidInstance(format!(
            "instance at {} does not satisfy schema keyword {}",
            error.instance_path(),
            error.kind().keyword()
        ))
    })
}

fn reject_external_references(value: &Value) -> Result<(), SchemaError> {
    match value {
        Value::Array(values) => {
            for value in values {
                reject_external_references(value)?;
            }
        }
        Value::Object(values) => {
            for keyword in ["$ref", "$dynamicRef"] {
                if let Some(reference) = values.get(keyword).and_then(Value::as_str)
                    && !reference.starts_with('#')
                {
                    return Err(SchemaError::ExternalReference(reference.to_owned()));
                }
            }
            for value in values.values() {
                reject_external_references(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Compatibility {
    Additive,
    Breaking(Vec<String>),
}

impl Compatibility {
    pub fn is_breaking(&self) -> bool {
        matches!(self, Self::Breaking(_))
    }
}

pub fn compare_schemas(previous: &Value, next: &Value) -> Compatibility {
    let mut reasons = Vec::new();
    compare_at(previous, next, "$", &mut reasons);
    if reasons.is_empty() {
        Compatibility::Additive
    } else {
        reasons.sort();
        reasons.dedup();
        Compatibility::Breaking(reasons)
    }
}

pub fn require_compatible_version(
    previous_version: u32,
    previous_schema: &Value,
    next_version: u32,
    next_schema: &Value,
) -> Result<Compatibility, SchemaError> {
    if previous_version == 0 || next_version == 0 || next_version < previous_version {
        return Err(SchemaError::InvalidVersion);
    }
    validate_schema(previous_schema)?;
    validate_schema(next_schema)?;
    let compatibility = compare_schemas(previous_schema, next_schema);
    if next_version == previous_version && compatibility.is_breaking() {
        return Err(SchemaError::BreakingChangeWithoutVersionBump(
            match &compatibility {
                Compatibility::Breaking(reasons) => reasons.clone(),
                Compatibility::Additive => unreachable!(),
            },
        ));
    }
    Ok(compatibility)
}

fn compare_at(previous: &Value, next: &Value, path: &str, reasons: &mut Vec<String>) {
    let (Some(previous), Some(next)) = (previous.as_object(), next.as_object()) else {
        if previous != next {
            reasons.push(format!("{path}: boolean or non-object schema changed"));
        }
        return;
    };

    compare_type(previous.get("type"), next.get("type"), path, reasons);
    compare_enum(previous.get("enum"), next.get("enum"), path, reasons);
    compare_restriction_keyword(previous, next, "const", path, reasons);
    compare_restriction_keyword(previous, next, "pattern", path, reasons);
    compare_restriction_keyword(previous, next, "format", path, reasons);
    compare_restriction_keyword(previous, next, "contentEncoding", path, reasons);
    compare_restriction_keyword(previous, next, "contentMediaType", path, reasons);
    compare_restriction_keyword(previous, next, "multipleOf", path, reasons);
    compare_semantic_keyword(previous, next, "x-unit", path, reasons);
    compare_lower_bound(previous, next, "minimum", path, reasons);
    compare_lower_bound(previous, next, "exclusiveMinimum", path, reasons);
    compare_lower_bound(previous, next, "minLength", path, reasons);
    compare_lower_bound(previous, next, "minItems", path, reasons);
    compare_lower_bound(previous, next, "minProperties", path, reasons);
    compare_lower_bound(previous, next, "minContains", path, reasons);
    compare_upper_bound(previous, next, "maximum", path, reasons);
    compare_upper_bound(previous, next, "exclusiveMaximum", path, reasons);
    compare_upper_bound(previous, next, "maxLength", path, reasons);
    compare_upper_bound(previous, next, "maxItems", path, reasons);
    compare_upper_bound(previous, next, "maxProperties", path, reasons);
    compare_upper_bound(previous, next, "maxContains", path, reasons);

    for keyword in [
        "oneOf",
        "anyOf",
        "allOf",
        "not",
        "$ref",
        "$dynamicRef",
        "if",
        "then",
        "else",
        "propertyNames",
        "patternProperties",
        "dependentRequired",
        "dependentSchemas",
        "unevaluatedProperties",
        "unevaluatedItems",
        "prefixItems",
    ] {
        compare_restriction_keyword(previous, next, keyword, path, reasons);
    }
    compare_false_to_true_keyword(previous, next, "uniqueItems", path, reasons);
    for keyword in ["$id", "$anchor", "$dynamicAnchor"] {
        compare_semantic_keyword(previous, next, keyword, path, reasons);
    }

    let previous_required = string_set(previous.get("required"));
    let next_required = string_set(next.get("required"));
    for required in next_required.difference(&previous_required) {
        reasons.push(format!(
            "{path}: optional property `{required}` became required"
        ));
    }

    let previous_properties = previous.get("properties").and_then(Value::as_object);
    let next_properties = next.get("properties").and_then(Value::as_object);
    if let Some(previous_properties) = previous_properties {
        for (name, previous_property) in previous_properties {
            let property_path = format!("{path}/properties/{name}");
            match next_properties.and_then(|properties| properties.get(name)) {
                Some(next_property) => {
                    compare_at(previous_property, next_property, &property_path, reasons)
                }
                None => reasons.push(format!("{property_path}: property was removed")),
            }
        }
    }

    compare_subschema_keyword(previous, next, "items", path, reasons);
    compare_subschema_keyword(previous, next, "contains", path, reasons);
    compare_named_subschemas(previous, next, "$defs", path, reasons);
    compare_named_subschemas(previous, next, "definitions", path, reasons);
    compare_additional_properties(previous, next, path, reasons);
}

fn compare_type(
    previous: Option<&Value>,
    next: Option<&Value>,
    path: &str,
    reasons: &mut Vec<String>,
) {
    match (previous, next) {
        (None, Some(_)) => reasons.push(format!("{path}: accepted JSON types were narrowed")),
        (Some(_), None) | (None, None) => {}
        (Some(previous), Some(next)) => {
            let previous = type_set(previous);
            let next = type_set(next);
            if !previous.is_subset(&next) {
                reasons.push(format!("{path}: accepted JSON types were narrowed"));
            }
        }
    }
}

fn type_set(value: &Value) -> BTreeSet<String> {
    match value {
        Value::String(value) => BTreeSet::from([value.clone()]),
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => BTreeSet::new(),
    }
}

fn compare_enum(
    previous: Option<&Value>,
    next: Option<&Value>,
    path: &str,
    reasons: &mut Vec<String>,
) {
    match (
        previous.and_then(Value::as_array),
        next.and_then(Value::as_array),
    ) {
        (None, Some(_)) => reasons.push(format!("{path}: an enum restriction was added")),
        (Some(previous), Some(next))
            if previous.iter().any(|value| !next.contains(value))
                || next.iter().any(|value| !previous.contains(value)) =>
        {
            reasons.push(format!("{path}: enum values changed"));
        }
        _ => {}
    }
}

fn compare_restriction_keyword(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    keyword: &str,
    path: &str,
    reasons: &mut Vec<String>,
) {
    if let Some(next) = next.get(keyword)
        && previous.get(keyword) != Some(next)
    {
        reasons.push(format!("{path}: `{keyword}` changed"));
    }
}

fn compare_semantic_keyword(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    keyword: &str,
    path: &str,
    reasons: &mut Vec<String>,
) {
    if previous.get(keyword) != next.get(keyword)
        && (previous.contains_key(keyword) || next.contains_key(keyword))
    {
        reasons.push(format!("{path}: `{keyword}` changed"));
    }
}

fn compare_false_to_true_keyword(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    keyword: &str,
    path: &str,
    reasons: &mut Vec<String>,
) {
    let previous = previous
        .get(keyword)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let next = next.get(keyword).and_then(Value::as_bool).unwrap_or(false);
    if !previous && next {
        reasons.push(format!("{path}: `{keyword}` added a restriction"));
    }
}

fn compare_lower_bound(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    keyword: &str,
    path: &str,
    reasons: &mut Vec<String>,
) {
    match (previous.get(keyword), next.get(keyword)) {
        (None, Some(_)) => {
            reasons.push(format!("{path}: `{keyword}` narrowed the accepted range"));
        }
        (Some(previous), Some(next)) if previous != next => {
            match (integer_value(previous), integer_value(next)) {
                (Some(previous), Some(next)) if next <= previous => {}
                _ => reasons.push(format!(
                    "{path}: `{keyword}` narrowed or ambiguously changed the accepted range"
                )),
            }
        }
        _ => {}
    }
}

fn compare_upper_bound(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    keyword: &str,
    path: &str,
    reasons: &mut Vec<String>,
) {
    match (previous.get(keyword), next.get(keyword)) {
        (None, Some(_)) => {
            reasons.push(format!("{path}: `{keyword}` narrowed the accepted range"));
        }
        (Some(previous), Some(next)) if previous != next => {
            match (integer_value(previous), integer_value(next)) {
                (Some(previous), Some(next)) if next >= previous => {}
                _ => reasons.push(format!(
                    "{path}: `{keyword}` narrowed or ambiguously changed the accepted range"
                )),
            }
        }
        _ => {}
    }
}

fn integer_value(value: &Value) -> Option<i128> {
    value
        .as_i64()
        .map(i128::from)
        .or_else(|| value.as_u64().map(i128::from))
}

fn compare_subschema_keyword(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    keyword: &str,
    path: &str,
    reasons: &mut Vec<String>,
) {
    match (previous.get(keyword), next.get(keyword)) {
        (None, Some(_)) => reasons.push(format!("{path}: `{keyword}` added a restriction")),
        (Some(previous), Some(next)) => {
            compare_at(previous, next, &format!("{path}/{keyword}"), reasons);
        }
        _ => {}
    }
}

fn compare_additional_properties(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    path: &str,
    reasons: &mut Vec<String>,
) {
    let previous = previous.get("additionalProperties");
    let next = next.get("additionalProperties");
    match (previous, next) {
        (None | Some(Value::Bool(true)), Some(Value::Bool(false) | Value::Object(_))) => reasons
            .push(format!(
                "{path}: `additionalProperties` became more restrictive"
            )),
        (Some(previous @ Value::Object(_)), Some(next @ Value::Object(_))) => compare_at(
            previous,
            next,
            &format!("{path}/additionalProperties"),
            reasons,
        ),
        (Some(Value::Object(_)), Some(Value::Bool(false))) => reasons.push(format!(
            "{path}: `additionalProperties` changed from schema-constrained to forbidden"
        )),
        _ => {}
    }
}

fn compare_named_subschemas(
    previous: &serde_json::Map<String, Value>,
    next: &serde_json::Map<String, Value>,
    keyword: &str,
    path: &str,
    reasons: &mut Vec<String>,
) {
    let Some(previous) = previous.get(keyword).and_then(Value::as_object) else {
        return;
    };
    let next = next.get(keyword).and_then(Value::as_object);
    for (name, previous_schema) in previous {
        let nested_path = format!("{path}/{keyword}/{name}");
        match next.and_then(|schemas| schemas.get(name)) {
            Some(next_schema) => compare_at(previous_schema, next_schema, &nested_path, reasons),
            None => reasons.push(format!("{nested_path}: referenced schema was removed")),
        }
    }
}

fn string_set(value: Option<&Value>) -> BTreeSet<String> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SchemaError {
    ExternalReference(String),
    UnsupportedDialect(String),
    InvalidSchema(String),
    InvalidInstance(String),
    InvalidVersion,
    BreakingChangeWithoutVersionBump(Vec<String>),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExternalReference(reference) => {
                write!(
                    formatter,
                    "external schema reference is forbidden: {reference}"
                )
            }
            Self::UnsupportedDialect(dialect) => {
                write!(formatter, "unsupported JSON Schema dialect: {dialect}")
            }
            Self::InvalidSchema(message) => write!(formatter, "invalid JSON Schema: {message}"),
            Self::InvalidInstance(message) => write!(formatter, "invalid example: {message}"),
            Self::InvalidVersion => formatter.write_str("schema version is invalid"),
            Self::BreakingChangeWithoutVersionBump(reasons) => write!(
                formatter,
                "breaking schema change requires a version bump: {}",
                reasons.join("; ")
            ),
        }
    }
}

impl std::error::Error for SchemaError {}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn optional_property_is_additive_but_required_property_is_breaking() {
        let previous = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"a": {"type": "string"}},
            "required": ["a"]
        });
        let optional = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"a": {"type": "string"}, "b": {"type": "integer"}},
            "required": ["a"]
        });
        assert_eq!(
            compare_schemas(&previous, &optional),
            Compatibility::Additive
        );
        let required = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"a": {"type": "string"}, "b": {"type": "integer"}},
            "required": ["a", "b"]
        });
        assert!(compare_schemas(&previous, &required).is_breaking());
        assert!(require_compatible_version(1, &previous, 1, &required).is_err());
        assert!(require_compatible_version(1, &previous, 2, &required).is_ok());
    }

    #[test]
    fn external_refs_are_rejected() {
        for keyword in ["$ref", "$dynamicRef"] {
            assert!(matches!(
                validate_schema(&json!({(keyword): "https://example.invalid/schema"})),
                Err(SchemaError::ExternalReference(_))
            ));
        }
    }

    #[test]
    fn an_explicit_schema_dialect_must_be_draft_2020_12() {
        assert!(matches!(
            validate_schema(&json!({
                "$schema": "http://json-schema.org/draft-07/schema#",
                "type": "object"
            })),
            Err(SchemaError::UnsupportedDialect(_))
        ));
        assert!(validate_schema(&json!({"type": "object"})).is_ok());
    }
}
