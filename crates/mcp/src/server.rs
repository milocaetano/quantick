//! The request loop: lines in, answers out.
//!
//! [`McpServer`] owns one [`ControlLink`] and the fixed tool list, and turns
//! each inbound JSON-RPC line into at most one outbound frame. It writes
//! nothing but frames to its output: diagnostics belong on standard error,
//! and the binary in `main.rs` is what wires the two streams.

use std::io::{self, BufRead, Write};

use quantick_control::limits::CONTROL_MAX_REQUEST_BYTES;

use serde_json::{Map, Value, json};

use crate::{
    jsonrpc::{self, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, Message, RpcError},
    link::ControlLink,
    protocol::{LATEST_PROTOCOL_VERSION, SERVER_NAME, SERVER_TITLE, Tool, negotiate},
    tools,
};

/// The instructions a client shows its model. ADR 0001 §7: the connection
/// rule, the read-before-act rule, the instance-selection rule and the
/// authority boundary all sit inside the first 512 characters, because that
/// is what a client is sure to surface.
pub const INSTRUCTIONS: &str = "Quantick observer. Connects to a Quantick desktop instance that is ALREADY running with Local agent access enabled; it never starts Quantick. Read before you act: call quantick_describe first. Instances: with one live instance every tool targets it; with several, pass instance_id (quantick_describe without it lists them) - nothing is chosen silently. Authority: the observer profile is read-only; no tool changes the chart, orders or settings, and write capability IDs are refused. Values are exact decimal strings; timestamps name their unit (_unix_ms). Errors carry a stable control.* code and next steps: branch on the code.";

/// How many leading characters of [`INSTRUCTIONS`] must already state the
/// four rules; pinned by a test.
pub const INSTRUCTIONS_LEAD_CHARS: usize = 512;

pub struct McpServer {
    link: Box<dyn ControlLink>,
    tools: Vec<Tool>,
    protocol_version: Option<&'static str>,
    initialized: bool,
}

impl McpServer {
    /// A server over one link, advertising the tool list for one profile
    /// ceiling. The ceiling is known when the adapter starts (contract §8).
    pub fn new(link: Box<dyn ControlLink>, profile_ceiling: &str) -> Self {
        Self {
            link,
            tools: tools::tools(profile_ceiling),
            protocol_version: None,
            initialized: false,
        }
    }

    /// The negotiated protocol version, once `initialize` has been answered.
    pub fn protocol_version(&self) -> Option<&'static str> {
        self.protocol_version
    }

    /// Whether the client has sent `notifications/initialized`.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Handle one inbound line. `None` means nothing is written back: an
    /// empty line, a notification, or a stray response.
    pub fn handle_line(&mut self, line: &str) -> Option<Value> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        match jsonrpc::parse(line) {
            Err(error) => Some(jsonrpc::failure(None, &error)),
            Ok(Message::Notification { method, .. }) => {
                self.on_notification(&method);
                None
            }
            Ok(Message::Response { .. }) => None,
            Ok(Message::Malformed { id, reason }) => Some(jsonrpc::failure(
                Some(&id),
                &RpcError::new(jsonrpc::INVALID_REQUEST, reason),
            )),
            Ok(Message::Request { id, method, params }) => {
                Some(match self.on_request(&method, params) {
                    Ok(result) => jsonrpc::success(&id, result),
                    Err(error) => jsonrpc::failure(Some(&id), &error),
                })
            }
        }
    }

    /// Run until the input closes. Every outbound frame is one line; the
    /// writer is flushed after each so a client waiting on a reply never
    /// waits on a buffer. A line is read into a bounded buffer: one that
    /// overflows the request limit, or is not UTF-8, is answered with a parse
    /// error and skipped — it never ends the session and never allocates
    /// without bound, the same discipline as the control plane's framing.
    pub fn serve(&mut self, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
        let mut buffer = Vec::with_capacity(4096);
        loop {
            buffer.clear();
            let read = read_bounded_line(&mut input, &mut buffer, MCP_FRAME_MAX_BYTES)?;
            let frame = match read {
                LineRead::Closed => return Ok(()),
                LineRead::TooLong => Some(jsonrpc::failure(
                    None,
                    &RpcError::new(
                        jsonrpc::PARSE_ERROR,
                        format!("line exceeds {MCP_FRAME_MAX_BYTES} bytes"),
                    ),
                )),
                LineRead::Line => match std::str::from_utf8(&buffer) {
                    Ok(line) => self.handle_line(line),
                    Err(_) => Some(jsonrpc::failure(
                        None,
                        &RpcError::new(jsonrpc::PARSE_ERROR, "line is not UTF-8"),
                    )),
                },
            };
            if let Some(frame) = frame {
                serde_json::to_writer(&mut output, &frame)?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
    }

    fn on_notification(&mut self, method: &str) {
        if method == "notifications/initialized" {
            self.initialized = true;
        }
        // Cancellation and progress notifications carry nothing this server
        // can act on: every call is answered synchronously.
    }

    fn on_request(&mut self, method: &str, params: Option<Value>) -> Result<Value, RpcError> {
        match method {
            "initialize" => {
                let requested = params
                    .as_ref()
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(Value::as_str)
                    .unwrap_or(LATEST_PROTOCOL_VERSION);
                let version = negotiate(requested);
                self.protocol_version = Some(version);
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": {
                        "name": SERVER_NAME,
                        "title": SERVER_TITLE,
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            _ if self.protocol_version.is_none() => Err(RpcError::new(
                INVALID_REQUEST,
                "initialize must be the first request on this connection",
            )),
            "tools/list" => Ok(json!({ "tools": self.tools })),
            "tools/call" => {
                let params = params.ok_or_else(|| {
                    RpcError::new(INVALID_PARAMS, "tools/call needs params with a tool name")
                })?;
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(INVALID_PARAMS, "tools/call needs a tool name"))?;
                let arguments = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Map::new()));
                let result = tools::call(self.link.as_mut(), name, arguments)?;
                serde_json::to_value(result).map_err(|error| {
                    RpcError::new(
                        jsonrpc::INTERNAL_ERROR,
                        format!("tool result serialization failed: {error}"),
                    )
                })
            }
            _ => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("method not found: {method}"),
            )),
        }
    }
}

enum LineRead {
    Line,
    TooLong,
    Closed,
}

/// The JSON-RPC envelope around a control payload — `jsonrpc`, `id`,
/// `method`, `params.name`, `params.arguments` and the routing property — so
/// a payload the gateway itself accepts is never refused here for its
/// wrapping.
const MCP_FRAME_WRAPPER_SLACK_BYTES: usize = 4096;
/// One standard-input line at most: the control request bound plus the
/// wrapper.
const MCP_FRAME_MAX_BYTES: usize = CONTROL_MAX_REQUEST_BYTES + MCP_FRAME_WRAPPER_SLACK_BYTES;

/// Read one `\n`-terminated line into `buffer`, at most `limit` bytes. A
/// longer line is consumed to its end and reported as too long, so the next
/// line starts clean; end of input with nothing read is `Closed`.
fn read_bounded_line(
    input: &mut impl BufRead,
    buffer: &mut Vec<u8>,
    limit: usize,
) -> io::Result<LineRead> {
    let mut too_long = false;
    loop {
        let available = match input.fill_buf() {
            Ok(available) => available,
            // A signal during the blocking read does not end the session;
            // std's own line readers retry it too.
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(if buffer.is_empty() && !too_long {
                LineRead::Closed
            } else if too_long {
                LineRead::TooLong
            } else {
                LineRead::Line
            });
        }
        let (chunk, done) = match available.iter().position(|byte| *byte == b'\n') {
            Some(newline) => (&available[..newline], true),
            None => (available, false),
        };
        let consumed = chunk.len() + usize::from(done);
        if !too_long {
            if buffer.len() + chunk.len() > limit {
                too_long = true;
                buffer.clear();
            } else {
                buffer.extend_from_slice(chunk);
            }
        }
        input.consume(consumed);
        if done {
            return Ok(if too_long {
                LineRead::TooLong
            } else {
                LineRead::Line
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use quantick_control::id::InstanceId;

    use super::*;
    use crate::fake::FakeLink;

    fn server_over(link: FakeLink) -> McpServer {
        McpServer::new(Box::new(link), "observer")
    }

    fn request(id: u64, method: &str, params: Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    fn initialized(mut server: McpServer) -> McpServer {
        let reply = server
            .handle_line(&request(1, "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})))
            .unwrap();
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert!(
            server
                .handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        assert!(server.is_initialized());
        server
    }

    #[test]
    fn the_instructions_lead_with_the_four_rules() {
        let lead: String = INSTRUCTIONS.chars().take(INSTRUCTIONS_LEAD_CHARS).collect();
        for rule in [
            "ALREADY running",
            "never starts Quantick",
            "quantick_describe first",
            "instance_id",
            "nothing is chosen silently",
            "read-only",
        ] {
            assert!(
                lead.contains(rule),
                "the first 512 characters must state: {rule}"
            );
        }
    }

    #[test]
    fn initialize_negotiates_and_requests_before_it_are_refused() {
        let mut server = server_over(FakeLink::default());
        let refused = server
            .handle_line(&request(5, "tools/list", json!({})))
            .unwrap();
        assert_eq!(refused["error"]["code"], INVALID_REQUEST);
        // A ping is allowed at any time.
        assert_eq!(
            server.handle_line(&request(6, "ping", json!({}))).unwrap()["result"],
            json!({})
        );
        let reply = server
            .handle_line(&request(
                1,
                "initialize",
                json!({"protocolVersion": "2099-01-01"}),
            ))
            .unwrap();
        assert_eq!(reply["result"]["protocolVersion"], LATEST_PROTOCOL_VERSION);
        assert_eq!(reply["result"]["serverInfo"]["name"], SERVER_NAME);
        assert_eq!(
            reply["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
        assert!(
            reply["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("never starts Quantick")
        );
    }

    #[test]
    fn tools_list_is_the_fixed_set_and_unknown_methods_are_not_found() {
        let mut server = initialized(server_over(FakeLink::default()));
        let reply = server
            .handle_line(&request(2, "tools/list", json!({})))
            .unwrap();
        let names = reply["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 6);
        assert_eq!(names[0], tools::DESCRIBE);
        let missing = server
            .handle_line(&request(3, "resources/list", json!({})))
            .unwrap();
        assert_eq!(missing["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn describe_lists_instances_and_forwards_to_one_when_named() {
        let id = InstanceId::from_bytes([9; 16]);
        let mut link = FakeLink::default();
        link.add_instance(id.clone());
        let mut server = initialized(server_over(link));
        let listed = server
            .handle_line(&request(2, "tools/call", json!({"name": tools::DESCRIBE})))
            .unwrap();
        let result = &listed["result"];
        assert_eq!(result["isError"], false);
        assert_eq!(
            result["structuredContent"]["instances"][0]["instance_id"],
            id.to_string()
        );
        let described = server
            .handle_line(&request(
                3,
                "tools/call",
                json!({"name": tools::DESCRIBE, "arguments": {"instance_id": id.to_string()}}),
            ))
            .unwrap();
        assert_eq!(described["result"]["isError"], false);
        assert_eq!(
            described["result"]["structuredContent"]["result"]["capabilities"][0]["id"],
            tools::DESCRIBE_CAPABILITY
        );
        let content = described["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            content.contains(tools::DESCRIBE_CAPABILITY),
            "the text block repeats the JSON"
        );
    }

    #[test]
    fn the_routing_id_never_reaches_the_instance_and_an_unknown_tool_is_a_protocol_error() {
        let id = InstanceId::from_bytes([9; 16]);
        let mut link = FakeLink::default();
        link.add_instance(id.clone());
        let mut server = initialized(server_over(link));
        let reply = server
            .handle_line(&request(
                2,
                "tools/call",
                json!({"name": tools::GET_SNAPSHOT, "arguments": {"instance_id": id.to_string(), "scopes": ["system.info"]}}),
            ))
            .unwrap();
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(
            reply["result"]["structuredContent"]["result"]["echo"]["scopes"][0],
            "system.info"
        );
        assert!(
            reply["result"]["structuredContent"]["result"]["echo"]
                .get("instance_id")
                .is_none(),
            "the routing id is stripped before the payload is forwarded"
        );
        let unknown = server
            .handle_line(&request(
                3,
                "tools/call",
                json!({"name": "quantick_delete_everything"}),
            ))
            .unwrap();
        assert_eq!(unknown["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn routing_failures_and_refused_writes_are_tool_errors_with_their_codes() {
        let mut server = initialized(server_over(FakeLink::default()));
        let none = server
            .handle_line(&request(
                2,
                "tools/call",
                json!({"name": tools::GET_DIAGNOSTICS}),
            ))
            .unwrap();
        assert_eq!(none["result"]["isError"], true);
        assert_eq!(
            none["result"]["structuredContent"]["error"]["code"],
            "control.instance_gone"
        );

        let mut link = FakeLink::default();
        link.add_instance(InstanceId::from_bytes([1; 16]));
        link.add_instance(InstanceId::from_bytes([2; 16]));
        let mut server = initialized(server_over(link));
        let many = server
            .handle_line(&request(
                3,
                "tools/call",
                json!({"name": tools::GET_DIAGNOSTICS}),
            ))
            .unwrap();
        assert_eq!(
            many["result"]["structuredContent"]["error"]["code"],
            "control.instance_ambiguous"
        );
        assert_eq!(
            many["result"]["structuredContent"]["error"]["details"]["instance_ids"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let write = server
            .handle_line(&request(
                4,
                "tools/call",
                json!({"name": tools::INVOKE, "arguments": {"instance_id": InstanceId::from_bytes([1; 16]).to_string(), "capability_id": "paper.order.place", "payload": {}}}),
            ))
            .unwrap();
        assert_eq!(write["result"]["isError"], true);
        assert_eq!(
            write["result"]["structuredContent"]["error"]["code"],
            "control.permission_denied"
        );
    }

    #[test]
    fn search_filters_the_described_registry() {
        let id = InstanceId::from_bytes([9; 16]);
        let mut link = FakeLink::default();
        link.add_instance(id);
        let mut server = initialized(server_over(link));
        let reply = server
            .handle_line(&request(
                2,
                "tools/call",
                json!({"name": tools::SEARCH_CAPABILITIES, "arguments": {"query": "chart"}}),
            ))
            .unwrap();
        let found = &reply["result"]["structuredContent"];
        assert_eq!(found["capability_count"], 1);
        assert_eq!(
            found["capabilities"][0]["id"],
            tools::CHART_WINDOW_CAPABILITY
        );
        assert_eq!(found["snapshot_scopes"][0]["id"], "chart.summary");
        assert!(
            found["snapshot_scopes"][0].get("schema").is_none(),
            "the search names a scope; describe carries its schema"
        );
        // A scope is found by its own ID — the field the contract's describe
        // document actually carries.
        let by_id = server
            .handle_line(&request(
                4,
                "tools/call",
                json!({"name": tools::SEARCH_CAPABILITIES, "arguments": {"query": "system.info"}}),
            ))
            .unwrap();
        assert_eq!(
            by_id["result"]["structuredContent"]["snapshot_scope_count"],
            1
        );
        assert_eq!(
            by_id["result"]["structuredContent"]["snapshot_scopes"][0]["id"],
            "system.info"
        );
        let all = server
            .handle_line(&request(
                3,
                "tools/call",
                json!({"name": tools::SEARCH_CAPABILITIES}),
            ))
            .unwrap();
        assert_eq!(all["result"]["structuredContent"]["capability_count"], 2);
    }

    #[test]
    fn a_frame_with_an_id_but_no_method_result_or_error_is_answered_not_dropped() {
        let mut server = initialized(server_over(FakeLink::default()));
        let reply = server
            .handle_line(r#"{"jsonrpc":"2.0","id":7,"params":{}}"#)
            .expect("a malformed request with an id is answered");
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["error"]["code"], jsonrpc::INVALID_REQUEST);
        let reply = server
            .handle_line(r#"{"jsonrpc":"2.0","id":"m","method":5}"#)
            .expect("a non-string method with an id is answered");
        assert_eq!(reply["id"], "m");
        assert_eq!(reply["error"]["code"], jsonrpc::INVALID_REQUEST);
        // A real response to nothing this server sent is still dropped.
        assert!(
            server
                .handle_line(r#"{"jsonrpc":"2.0","id":"x","result":{}}"#)
                .is_none()
        );
    }

    #[test]
    fn an_overlong_or_non_utf8_line_is_answered_not_fatal() {
        let mut server = server_over(FakeLink::default());
        let mut input = Vec::new();
        input.extend_from_slice(
            request(1, "initialize", json!({"protocolVersion": "2025-06-18"})).as_bytes(),
        );
        input.push(b'\n');
        // A line longer than the request limit, then a valid ping, then a
        // line with a stray non-UTF-8 byte, then another ping.
        input.extend(std::iter::repeat_n(b'x', MCP_FRAME_MAX_BYTES + 10));
        input.push(b'\n');
        input.extend_from_slice(request(2, "ping", json!({})).as_bytes());
        input.push(b'\n');
        input.extend_from_slice(
            b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\",\"x\":\"\xff\"}\n",
        );
        input.extend_from_slice(request(4, "ping", json!({})).as_bytes());
        input.push(b'\n');
        let mut output = Vec::new();
        server.serve(input.as_slice(), &mut output).unwrap();
        let frames = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            frames.len(),
            5,
            "initialize, too-long error, ping, utf-8 error, ping"
        );
        assert_eq!(frames[1]["error"]["code"], jsonrpc::PARSE_ERROR);
        assert!(frames[1]["id"].is_null());
        assert_eq!(frames[2]["id"], 2);
        assert_eq!(frames[3]["error"]["code"], jsonrpc::PARSE_ERROR);
        assert_eq!(frames[4]["id"], 4);
    }

    #[test]
    fn the_serve_loop_writes_only_frames_and_answers_garbage_with_a_null_id() {
        let mut server = server_over(FakeLink::default());
        let input = [
            request(1, "initialize", json!({"protocolVersion": "2025-06-18"})),
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_owned(),
            "this is not json".to_owned(),
            String::new(),
            request(2, "ping", json!({})),
        ]
        .join("\n");
        let mut output = Vec::new();
        server.serve(input.as_bytes(), &mut output).unwrap();
        let text = String::from_utf8(output).unwrap();
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3, "one frame per answered line, nothing else");
        for line in &lines {
            let frame: Value = serde_json::from_str(line).expect("every line is JSON");
            assert_eq!(frame["jsonrpc"], "2.0");
        }
        let garbage: Value = serde_json::from_str(lines[1]).unwrap();
        assert!(garbage["id"].is_null());
        assert_eq!(garbage["error"]["code"], jsonrpc::PARSE_ERROR);
    }
}
