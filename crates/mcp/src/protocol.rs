//! The MCP shapes this server emits, and the protocol versions it speaks.
//!
//! Only what the observer surface needs is modelled: tool descriptors with
//! their annotations, tool results with text and structured content, and the
//! initialize result. Nothing here knows about Quantick; that is [`crate::tools`].

use quantick_control::error::ControlError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Protocol versions this server can negotiate, newest first. A client that
/// asks for one of these gets it back; any other request gets the newest.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// What the server answers when the client's version is not one it supports.
pub const LATEST_PROTOCOL_VERSION: &str = SUPPORTED_PROTOCOL_VERSIONS[0];

pub const SERVER_NAME: &str = "quantick-mcp";
pub const SERVER_TITLE: &str = "Quantick control plane";

/// MCP version negotiation (lifecycle §"Version Negotiation"): the client's
/// version if supported, otherwise the latest this server supports.
pub fn negotiate(requested: &str) -> &'static str {
    SUPPORTED_PROTOCOL_VERSIONS
        .iter()
        .find(|version| **version == requested)
        .copied()
        .unwrap_or(LATEST_PROTOCOL_VERSION)
}

/// Tool annotations as MCP defines them. They are a client hint, not an
/// authorization boundary (contract §8): the gateway enforces authority.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub read_only_hint: bool,
    pub destructive_hint: bool,
    pub idempotent_hint: bool,
    pub open_world_hint: bool,
}

impl ToolAnnotations {
    /// The annotations of a named observer read: read-only, not destructive,
    /// idempotent, closed world.
    pub fn observer_read(title: &str) -> Self {
        Self {
            title: Some(title.to_owned()),
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        }
    }
}

/// One tool as `tools/list` returns it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    pub annotations: ToolAnnotations,
}

/// One item of a tool result's unstructured content. Only text is produced
/// here: the structured content carries the data, and the text block repeats
/// it for clients that read nothing else.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    Text { text: String },
}

/// A `tools/call` result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub content: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    pub is_error: bool,
}

impl ToolResult {
    /// A successful result: the structured value, repeated as serialized JSON
    /// in a text block as the specification recommends.
    pub fn structured(value: Value) -> Self {
        let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_owned());
        Self {
            content: vec![Content::Text { text }],
            structured_content: Some(value),
            is_error: false,
        }
    }

    /// A tool execution error carrying a structured control error: the code
    /// to branch on, the next steps to follow, and a one-line summary for
    /// clients that show only text.
    pub fn control_error(error: &ControlError) -> Self {
        let mut summary = format!("{}: {}", error.code, error.message);
        if !error.context.next_steps.is_empty() {
            summary.push_str(" Next: ");
            summary.push_str(&error.context.next_steps.join(" "));
        }
        let value = serde_json::to_value(error).unwrap_or(Value::Null);
        Self {
            content: vec![Content::Text { text: summary }],
            structured_content: Some(serde_json::json!({ "error": value })),
            is_error: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation_returns_the_requested_version_when_supported() {
        assert_eq!(negotiate("2025-03-26"), "2025-03-26");
        assert_eq!(negotiate("2024-11-05"), "2024-11-05");
        assert_eq!(negotiate("2099-01-01"), LATEST_PROTOCOL_VERSION);
        assert_eq!(negotiate(""), LATEST_PROTOCOL_VERSION);
    }

    #[test]
    fn annotations_serialize_in_the_wire_casing() {
        let value = serde_json::to_value(ToolAnnotations::observer_read("x")).unwrap();
        assert_eq!(value["readOnlyHint"], true);
        assert_eq!(value["destructiveHint"], false);
        assert_eq!(value["idempotentHint"], true);
        assert_eq!(value["openWorldHint"], false);
    }

    #[test]
    fn a_control_error_becomes_a_tool_execution_error_with_its_code() {
        let error = ControlError::invalid_request("nope");
        let result = ToolResult::control_error(&error);
        assert!(result.is_error);
        assert_eq!(
            result.structured_content.as_ref().unwrap()["error"]["code"],
            "control.invalid_request"
        );
        let Content::Text { text } = &result.content[0];
        assert!(text.starts_with("control.invalid_request: nope"));
    }
}
