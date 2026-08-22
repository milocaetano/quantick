//! The slice of JSON-RPC 2.0 an MCP server over STDIO needs.
//!
//! One message per line. A request carries an `id` and is answered; a
//! notification carries none and is not. Batches (JSON arrays) are refused:
//! the 2025-06-18 protocol removed them, and the clients this adapter serves
//! never sent them.

use serde_json::{Value, json};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// One inbound message, classified.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Request {
        id: Value,
        method: String,
        params: Option<Value>,
    },
    Notification {
        method: String,
        params: Option<Value>,
    },
    /// A response to something the server sent. This server sends no
    /// requests, so a response is read and dropped.
    Response { id: Value },
}

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    pub data: Option<Value>,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Parse one line into a message. Everything that is not a well-formed
/// JSON-RPC 2.0 object is an error the caller answers with a null id.
pub fn parse(line: &str) -> Result<Message, RpcError> {
    let value: Value = serde_json::from_str(line)
        .map_err(|error| RpcError::new(PARSE_ERROR, format!("parse error: {error}")))?;
    let object = match value {
        Value::Object(object) => object,
        Value::Array(_) => {
            return Err(RpcError::new(
                INVALID_REQUEST,
                "JSON-RPC batches are not supported by this server",
            ));
        }
        _ => {
            return Err(RpcError::new(
                INVALID_REQUEST,
                "a JSON-RPC message is a JSON object",
            ));
        }
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(RpcError::new(INVALID_REQUEST, "jsonrpc must be \"2.0\""));
    }
    let id = object.get("id").cloned();
    match object.get("method").and_then(Value::as_str) {
        Some(method) => {
            let params = object.get("params").cloned();
            match id {
                Some(id) if !id.is_null() => Ok(Message::Request {
                    id,
                    method: method.to_owned(),
                    params,
                }),
                _ => Ok(Message::Notification {
                    method: method.to_owned(),
                    params,
                }),
            }
        }
        None => match id {
            Some(id) => Ok(Message::Response { id }),
            None => Err(RpcError::new(
                INVALID_REQUEST,
                "a JSON-RPC message carries a method or an id",
            )),
        },
    }
}

/// A successful response frame.
pub fn success(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// An error response frame. A parse error has no id to echo; JSON-RPC says to
/// send `null` then.
pub fn failure(id: Option<&Value>, error: &RpcError) -> Value {
    let mut body = json!({ "code": error.code, "message": error.message });
    if let Some(data) = &error.data {
        body["data"] = data.clone();
    }
    json!({
        "jsonrpc": "2.0",
        "id": id.cloned().unwrap_or(Value::Null),
        "error": body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_notifications_and_responses_are_told_apart() {
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).unwrap(),
            Message::Request {
                id: json!(1),
                method: "ping".to_owned(),
                params: None
            }
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap(),
            Message::Notification {
                method: "notifications/initialized".to_owned(),
                params: None
            }
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0","id":"x","result":{}}"#).unwrap(),
            Message::Response { id: json!("x") }
        );
    }

    #[test]
    fn malformed_lines_are_errors_with_the_standard_codes() {
        assert_eq!(parse("not json").unwrap_err().code, PARSE_ERROR);
        assert_eq!(parse("[]").unwrap_err().code, INVALID_REQUEST);
        assert_eq!(parse("42").unwrap_err().code, INVALID_REQUEST);
        assert_eq!(
            parse(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#)
                .unwrap_err()
                .code,
            INVALID_REQUEST
        );
        assert_eq!(
            parse(r#"{"jsonrpc":"2.0"}"#).unwrap_err().code,
            INVALID_REQUEST
        );
    }

    #[test]
    fn frames_carry_the_id_back_and_null_when_there_is_none() {
        let ok = success(&json!(7), json!({"a": 1}));
        assert_eq!(ok["id"], 7);
        assert_eq!(ok["result"]["a"], 1);
        let err = failure(None, &RpcError::new(PARSE_ERROR, "bad"));
        assert!(err["id"].is_null());
        assert_eq!(err["error"]["code"], PARSE_ERROR);
        let with_data = failure(
            Some(&json!("r")),
            &RpcError::new(INVALID_PARAMS, "bad").with_data(json!({"field": "x"})),
        );
        assert_eq!(with_data["id"], "r");
        assert_eq!(with_data["error"]["data"]["field"], "x");
    }
}
