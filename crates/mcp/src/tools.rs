//! The fixed observer tool set, and how each tool maps to a capability.
//!
//! Plan §7.1: a small, stable set of named tools for the paths used constantly,
//! plus `quantick_search_capabilities` and `quantick_invoke` for the long
//! tail. The list never changes shape with application state; availability
//! is reported by `quantick_describe` and `quantick_search_capabilities` from
//! the registry the running instance publishes.
//!
//! Every instance-bound tool takes an optional routing `instance_id` that is
//! removed here before the payload reaches the gateway (contract §8), so the
//! capability's own input schema — committed under `schemas/control/` and
//! embedded below — validates exactly what the application registered.

use std::collections::BTreeMap;

use quantick_control::id::InstanceId;
use serde_json::{Map, Value, json};

use crate::{
    jsonrpc::{INVALID_PARAMS, RpcError},
    link::ControlLink,
    protocol::{Tool, ToolAnnotations, ToolResult},
};

pub const DESCRIBE: &str = "quantick_describe";
pub const GET_SNAPSHOT: &str = "quantick_get_snapshot";
pub const GET_CHART_WINDOW: &str = "quantick_get_chart_window";
pub const GET_DIAGNOSTICS: &str = "quantick_get_diagnostics";
pub const GET_SCENE: &str = "quantick_get_scene";
pub const READ_EVENTS: &str = "quantick_read_events";
pub const WAIT_FOR_CHANGE: &str = "quantick_wait_for_change";
pub const SEARCH_CAPABILITIES: &str = "quantick_search_capabilities";
pub const INVOKE: &str = "quantick_invoke";
/// The annotate tier's tools. They are listed only for a connection whose
/// ceiling is the annotator profile: a tool list that offers what the trader
/// never granted is a promise the instance will refuse.
pub const ANNOTATE: &str = "quantick_annotate";
pub const REMOVE_ANNOTATION: &str = "quantick_remove_annotation";
pub const NOTIFY: &str = "quantick_notify";
pub const ATTACH_SCRIPT: &str = "quantick_attach_script";
pub const DETACH_SCRIPT: &str = "quantick_detach_script";

/// The registered capabilities the named tools resolve to. Same IDs the
/// application's observer contract registers; `quantick_invoke` reaches any
/// other by name.
pub const DESCRIBE_CAPABILITY: &str = "control.describe";
pub const SNAPSHOT_CAPABILITY: &str = "snapshot.read";
pub const CHART_WINDOW_CAPABILITY: &str = "chart.window.read";
pub const DIAGNOSTICS_CAPABILITY: &str = "health.diagnostics.read";
pub const SCENE_CAPABILITY: &str = "scene.read";
pub const EVENTS_READ_CAPABILITY: &str = "events.read";
pub const EVENTS_WAIT_CAPABILITY: &str = "events.wait";
pub const LABEL_CAPABILITY: &str = "annotate.label.create";
pub const ARROW_CAPABILITY: &str = "annotate.arrow.create";
pub const ZONE_CAPABILITY: &str = "annotate.zone.create";
pub const REMOVE_CAPABILITY: &str = "annotate.remove";
pub const POPUP_CAPABILITY: &str = "notify.popup";
pub const TOAST_CAPABILITY: &str = "notify.toast";
pub const SOUND_CAPABILITY: &str = "notify.sound";
pub const ATTACH_SCRIPT_CAPABILITY: &str = "indicator.script.attach";
pub const DETACH_SCRIPT_CAPABILITY: &str = "indicator.script.detach";

/// The profile whose ceiling admits the annotate tier.
pub const ANNOTATOR_PROFILE: &str = "annotator";
/// The read-only floor every other ceiling is treated as.
pub const OBSERVER_PROFILE: &str = "observer";
/// The routing property that picks which object an annotation places, and
/// which channel a notification arrives on. Removed before the payload is
/// validated, exactly as `instance_id` is.
const OBJECT_PROPERTY: &str = "object";
const CHANNEL_PROPERTY: &str = "channel";

/// Every first-generation observer capability is registered at version 1.
const FIRST_CAPABILITY_VERSION: u32 = 1;

const INSTANCE_ID_PROPERTY: &str = "instance_id";
/// Counted in characters, as JSON Schema's `maxLength` is.
const SEARCH_QUERY_MAX_CHARS: usize = 128;

/// The committed contract documents these tools embed, so the tool list
/// describes exactly what the application validates. Regenerated and
/// snapshot-tested on the application side; this crate only carries them.
const SNAPSHOT_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-snapshot-read-input-v1.schema.json");
const CHART_WINDOW_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-chart-window-input-v1.schema.json");
const DESCRIBE_RESULT_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-describe-result-v1.schema.json");
const SNAPSHOT_CAPTURE_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-snapshot-capture-v1.schema.json");
const CHART_WINDOW_PAGE_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-chart-window-page-v1.schema.json");
/// The contract's error document: every tool with an output schema admits
/// it as the error branch, so a tool execution error's structured content
/// validates against the declared schema. MCP clients validate
/// `structuredContent` whenever it is present, error or not.
const CONTROL_ERROR_SCHEMA: &str =
    include_str!("../../../schemas/control/control-error-v1.schema.json");
const EVENTS_READ_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-events-read-input-v1.schema.json");
const EVENTS_WAIT_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-events-wait-input-v1.schema.json");
const EVENT_PAGE_SCHEMA: &str =
    include_str!("../../../schemas/control/observer-event-page-v1.schema.json");
const ANNOTATION_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/annotate-object-input-v1.schema.json");
const ANNOTATION_RESULT_SCHEMA: &str =
    include_str!("../../../schemas/control/annotate-object-result-v1.schema.json");
const ANNOTATION_REMOVE_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/annotate-remove-input-v1.schema.json");
const ANNOTATION_REMOVE_RESULT_SCHEMA: &str =
    include_str!("../../../schemas/control/annotate-remove-result-v1.schema.json");
const NOTIFY_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/notify-input-v1.schema.json");
const NOTIFY_RESULT_SCHEMA: &str =
    include_str!("../../../schemas/control/notify-result-v1.schema.json");
const ATTACH_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/indicator-script-attach-input-v1.schema.json");
const ATTACH_RESULT_SCHEMA: &str =
    include_str!("../../../schemas/control/indicator-script-attach-result-v1.schema.json");
const DETACH_INPUT_SCHEMA: &str =
    include_str!("../../../schemas/control/indicator-script-detach-input-v1.schema.json");
const DETACH_RESULT_SCHEMA: &str =
    include_str!("../../../schemas/control/indicator-script-detach-result-v1.schema.json");

/// The tool list for one profile ceiling. The named reads are read-only
/// whatever the ceiling; `quantick_invoke` takes the conservative hints of
/// contract §8 for the ceiling, because a cached tool list must never promise
/// less caution than the strongest capability it could reach.
pub fn tools(profile_ceiling: &str) -> Vec<Tool> {
    let invoke_annotations = match profile_ceiling {
        "observer" => ToolAnnotations {
            title: Some("Invoke a registered capability".to_owned()),
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        },
        "annotator" => ToolAnnotations {
            title: Some("Invoke a registered capability".to_owned()),
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        },
        _ => ToolAnnotations {
            title: Some("Invoke a registered capability".to_owned()),
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: false,
            open_world_hint: true,
        },
    };
    let mut tools = vec![
        Tool {
            name: DESCRIBE.to_owned(),
            title: "Describe the running Quantick instance".to_owned(),
            description: "Without instance_id: list the live Quantick instances this adapter can reach (empty, with a next step, when none is running — the adapter never starts one). With instance_id: describe that instance — application version, negotiated protocol, effective profile and scopes, registered modules, capabilities with their availability, snapshot scopes and limits. Call this first.".to_owned(),
            input_schema: instance_only_schema(),
            output_schema: Some(describe_output_schema()),
            annotations: ToolAnnotations::observer_read("Describe"),
        },
        Tool {
            name: GET_SNAPSHOT.to_owned(),
            title: "Capture a coherent snapshot of selected scopes".to_owned(),
            description: "One coherent capture of the requested snapshot scopes (for example system.info, workspace.summary, feed.status, chart.summary, health.summary, interaction.cursor, interaction.selection) taken in a single pass on the application thread, with one capture revision and every module revision it observed. Scopes not requested are listed as omitted. Use quantick_describe to learn the registered scope IDs.".to_owned(),
            input_schema: with_instance_routing(parse_schema(SNAPSHOT_INPUT_SCHEMA)),
            output_schema: Some(capability_output_schema(parse_schema(SNAPSHOT_CAPTURE_SCHEMA))),
            annotations: ToolAnnotations::observer_read("Snapshot"),
        },
        Tool {
            name: GET_CHART_WINDOW.to_owned(),
            title: "Read a page of closed bars from one chart pane".to_owned(),
            description: "A paginated, append-only read of closed bars for one tab and pane: OHLC, volume, delta, trade count and timestamps as exact decimal strings. The first call sends a query; later pages send the same query plus the cursor it returned. The in-progress bar belongs to the snapshot's chart.summary scope, not to this series.".to_owned(),
            input_schema: with_instance_routing(parse_schema(CHART_WINDOW_INPUT_SCHEMA)),
            output_schema: Some(capability_output_schema(parse_schema(CHART_WINDOW_PAGE_SCHEMA))),
            annotations: ToolAnnotations::observer_read("Chart window"),
        },
        Tool {
            name: GET_DIAGNOSTICS.to_owned(),
            title: "Read health and diagnostics".to_owned(),
            description: "The bounded structured health view of the running instance: frame timing, feed arrival, order-flow engine state, worker and queue metrics, and recent error counts — the scopes a stalled-feed or slow-frame investigation starts from.".to_owned(),
            input_schema: instance_only_schema(),
            output_schema: Some(capability_output_schema(parse_schema(SNAPSHOT_CAPTURE_SCHEMA))),
            annotations: ToolAnnotations::observer_read("Diagnostics"),
        },
        Tool {
            name: GET_SCENE.to_owned(),
            title: "Read what is on screen, as named controls".to_owned(),
            description: "The semantic scene: every control the trader can see — chart tabs, the toolbar's layer toggles, the drawing tool rail, the dock's tabs and each chart canvas — with an ID stable across frames, its owner, whether it is selected, and a coded reason when it cannot be operated. Chart canvases carry their rectangle in window pixels; the chrome reports that its bounds are not recorded rather than guessing. The cursor scope answers with the same control IDs, so a pointer position and this list name the same button.".to_owned(),
            input_schema: instance_only_schema(),
            output_schema: Some(capability_output_schema(parse_schema(SNAPSHOT_CAPTURE_SCHEMA))),
            annotations: ToolAnnotations::observer_read("Scene"),
        },
        Tool {
            name: READ_EVENTS.to_owned(),
            title: "Read the semantic event journal".to_owned(),
            description: "A page of the bounded semantic event journal — tab, focus and selection changes, feed connection and market changes, replay state, human marks — after a cursor or from an explicit start (oldest or latest). Each page returns the next cursor and says when older events were dropped. Marks carry the fully resolved target the user pointed at.".to_owned(),
            input_schema: with_instance_routing(parse_schema(EVENTS_READ_INPUT_SCHEMA)),
            output_schema: Some(capability_output_schema(parse_schema(EVENT_PAGE_SCHEMA))),
            annotations: ToolAnnotations::observer_read("Read events"),
        },
        Tool {
            name: WAIT_FOR_CHANGE.to_owned(),
            title: "Wait for the journal to move".to_owned(),
            description: "Parks until the event journal moves past the cursor or timeout_ms elapses (at most 30 s), then returns the page that completes the call; timed_out says which. This is how a client watches the user point in real time instead of polling: wait, read the mark, answer about that bar and no other.".to_owned(),
            input_schema: with_instance_routing(parse_schema(EVENTS_WAIT_INPUT_SCHEMA)),
            output_schema: Some(capability_output_schema(parse_schema(EVENT_PAGE_SCHEMA))),
            annotations: ToolAnnotations::observer_read("Wait for change"),
        },
        Tool {
            name: SEARCH_CAPABILITIES.to_owned(),
            title: "Search the registered capabilities and snapshot scopes".to_owned(),
            description: "Find capabilities and snapshot scopes by a substring of their ID, title, description or module, as the running instance currently registers them — with availability and the reason when one is unavailable. The long tail is reached with quantick_invoke.".to_owned(),
            input_schema: search_schema(),
            output_schema: None,
            annotations: ToolAnnotations::observer_read("Search capabilities"),
        },
        Tool {
            name: INVOKE.to_owned(),
            title: "Invoke a registered capability by ID".to_owned(),
            description: "Execute one registered capability by ID and version with its declared input. Availability, permission, revision and idempotency rules are enforced by the instance exactly as for the named tools, whatever this connection's ceiling: a capability ID the trader did not grant is refused with control.permission_denied. Use quantick_describe or quantick_search_capabilities to learn which IDs this instance registers.".to_owned(),
            input_schema: invoke_schema(),
            output_schema: None,
            annotations: invoke_annotations,
        },
    ];
    if profile_ceiling == ANNOTATOR_PROFILE {
        tools.extend(annotate_tools());
    }
    tools
}

/// The tools a connection holding the annotator profile also gets: the half
/// of the loop that answers on the chart. Each maps to exactly one registered
/// capability — the routing property picks which — so nothing here is a
/// second vocabulary beside the instance's own IDs.
fn annotate_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: ANNOTATE.to_owned(),
            title: "Place a label, an arrow or a zone on the chart".to_owned(),
            description: "Add one object to a chart at market-time and price coordinates — the ones chart.window.read and the cursor report. `object` picks what to place: a label (one anchor, carries text), an arrow or a zone (two anchors each). The object is visibly attributed to this client wherever the trader sees it, and either of you can remove it in one action.".to_owned(),
            input_schema: with_routing_choice(
                parse_schema(ANNOTATION_INPUT_SCHEMA),
                OBJECT_PROPERTY,
                &["label", "arrow", "zone"],
                "Which object to place.",
            ),
            output_schema: Some(capability_output_schema(parse_schema(ANNOTATION_RESULT_SCHEMA))),
            annotations: annotate_write("Annotate the chart"),
        },
        Tool {
            name: REMOVE_ANNOTATION.to_owned(),
            title: "Remove an annotation this client placed".to_owned(),
            description: "Remove one object by the annotation_id an annotate call returned. Only objects placed by an operator can be removed this way: an object the trader drew by hand is refused, whatever ID is asked for.".to_owned(),
            input_schema: with_instance_routing(parse_schema(ANNOTATION_REMOVE_INPUT_SCHEMA)),
            output_schema: Some(capability_output_schema(parse_schema(
                ANNOTATION_REMOVE_RESULT_SCHEMA,
            ))),
            annotations: annotate_write("Remove an annotation"),
        },
        Tool {
            name: NOTIFY.to_owned(),
            title: "Get the trader's attention".to_owned(),
            description: "Raise one notification: `popup` opens a small window over the chart, `toast` posts a line to the acknowledgement lane, `sound` asks the platform for its alert sound and needs a scope of its own that is off by default. Every channel is attributed to this client and is rate limited; none of them can be taken back, so use them when the chart alone will not do.".to_owned(),
            input_schema: with_routing_choice(
                parse_schema(NOTIFY_INPUT_SCHEMA),
                CHANNEL_PROPERTY,
                &["popup", "toast", "sound"],
                "Which channel the notification arrives on.",
            ),
            output_schema: Some(capability_output_schema(parse_schema(NOTIFY_RESULT_SCHEMA))),
            annotations: annotate_write("Notify the trader"),
        },
        Tool {
            name: ATTACH_SCRIPT.to_owned(),
            title: "Compile and attach a Quantick Pine indicator".to_owned(),
            description: "Compile Quantick Pine source and attach the indicator it produces to the focused pane. A script that does not compile is refused with its diagnostics as structured data — stable code, byte span, line, column, message and notes — so the next attempt can fix the exact span. Returns the slot id to detach with.".to_owned(),
            input_schema: with_instance_routing(parse_schema(ATTACH_INPUT_SCHEMA)),
            output_schema: Some(capability_output_schema(parse_schema(ATTACH_RESULT_SCHEMA))),
            annotations: annotate_write("Attach a script"),
        },
        Tool {
            name: DETACH_SCRIPT.to_owned(),
            title: "Detach a script indicator".to_owned(),
            description: "Remove one indicator slot this client attached, leaving the pane as it was before.".to_owned(),
            input_schema: with_instance_routing(parse_schema(DETACH_INPUT_SCHEMA)),
            output_schema: Some(capability_output_schema(parse_schema(DETACH_RESULT_SCHEMA))),
            annotations: annotate_write("Detach a script"),
        },
    ]
}

/// The annotations of a tier that writes but never destroys and never
/// repeats: not read-only, not destructive, not idempotent (two identical
/// calls place two objects), closed world.
fn annotate_write(title: &str) -> ToolAnnotations {
    ToolAnnotations {
        title: Some(title.to_owned()),
        read_only_hint: false,
        destructive_hint: false,
        idempotent_hint: false,
        open_world_hint: false,
    }
}

/// A committed input document plus the instance routing and one required
/// choice property that picks the capability behind the tool.
fn with_routing_choice(
    schema: Value,
    property: &str,
    choices: &[&str],
    description: &str,
) -> Value {
    let mut schema = with_instance_routing(schema);
    if let Some(object) = schema.as_object_mut() {
        if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
            properties.insert(
                property.to_owned(),
                json!({
                    "type": "string",
                    "enum": choices,
                    "description": description,
                }),
            );
        }
        if let Some(required) = object.get_mut("required").and_then(Value::as_array_mut) {
            required.push(Value::String(property.to_owned()));
        } else {
            object.insert(
                "required".to_owned(),
                Value::Array(vec![Value::String(property.to_owned())]),
            );
        }
    }
    schema
}

/// Execute one tool call. Protocol-level problems (unknown tool, arguments
/// that are not an object, an unusable instance_id) are JSON-RPC errors;
/// everything the instance or the transport refuses is a tool execution
/// error carrying the structured control error.
pub fn call(
    link: &mut dyn ControlLink,
    name: &str,
    arguments: Value,
) -> Result<ToolResult, RpcError> {
    let mut arguments = match arguments {
        Value::Object(arguments) => arguments,
        Value::Null => Map::new(),
        _ => {
            return Err(RpcError::new(
                INVALID_PARAMS,
                "tool arguments must be a JSON object",
            ));
        }
    };
    let instance = take_instance_id(&mut arguments)?;
    match name {
        DESCRIBE => match instance {
            None => match link.instances() {
                Ok(instances) => Ok(ToolResult::structured(
                    serde_json::to_value(instances).map_err(internal)?,
                )),
                Err(error) => Ok(ToolResult::control_error(&error)),
            },
            Some(id) => forward(link, Some(&id), DESCRIBE_CAPABILITY, json!({})),
        },
        GET_SNAPSHOT => forward(
            link,
            instance.as_ref(),
            SNAPSHOT_CAPABILITY,
            Value::Object(arguments),
        ),
        GET_CHART_WINDOW => forward(
            link,
            instance.as_ref(),
            CHART_WINDOW_CAPABILITY,
            Value::Object(arguments),
        ),
        GET_DIAGNOSTICS => forward(
            link,
            instance.as_ref(),
            DIAGNOSTICS_CAPABILITY,
            Value::Object(arguments),
        ),
        GET_SCENE => forward(
            link,
            instance.as_ref(),
            SCENE_CAPABILITY,
            Value::Object(arguments),
        ),
        READ_EVENTS => forward(
            link,
            instance.as_ref(),
            EVENTS_READ_CAPABILITY,
            Value::Object(arguments),
        ),
        WAIT_FOR_CHANGE => forward(
            link,
            instance.as_ref(),
            EVENTS_WAIT_CAPABILITY,
            Value::Object(arguments),
        ),
        ANNOTATE => {
            let capability = match take_choice(&mut arguments, OBJECT_PROPERTY)?.as_str() {
                "label" => LABEL_CAPABILITY,
                "arrow" => ARROW_CAPABILITY,
                "zone" => ZONE_CAPABILITY,
                other => {
                    return Err(RpcError::new(
                        INVALID_PARAMS,
                        format!("object must be label, arrow or zone, not `{other}`"),
                    ));
                }
            };
            forward(
                link,
                instance.as_ref(),
                capability,
                Value::Object(arguments),
            )
        }
        REMOVE_ANNOTATION => forward(
            link,
            instance.as_ref(),
            REMOVE_CAPABILITY,
            Value::Object(arguments),
        ),
        NOTIFY => {
            let capability = match take_choice(&mut arguments, CHANNEL_PROPERTY)?.as_str() {
                "popup" => POPUP_CAPABILITY,
                "toast" => TOAST_CAPABILITY,
                "sound" => SOUND_CAPABILITY,
                other => {
                    return Err(RpcError::new(
                        INVALID_PARAMS,
                        format!("channel must be popup, toast or sound, not `{other}`"),
                    ));
                }
            };
            forward(
                link,
                instance.as_ref(),
                capability,
                Value::Object(arguments),
            )
        }
        ATTACH_SCRIPT => forward(
            link,
            instance.as_ref(),
            ATTACH_SCRIPT_CAPABILITY,
            Value::Object(arguments),
        ),
        DETACH_SCRIPT => forward(
            link,
            instance.as_ref(),
            DETACH_SCRIPT_CAPABILITY,
            Value::Object(arguments),
        ),
        SEARCH_CAPABILITIES => search(link, instance.as_ref(), &arguments),
        INVOKE => {
            let capability_id = arguments
                .get("capability_id")
                .and_then(Value::as_str)
                .ok_or_else(|| RpcError::new(INVALID_PARAMS, "capability_id is required"))?
                .to_owned();
            let capability_version = match arguments.get("capability_version") {
                None => FIRST_CAPABILITY_VERSION,
                Some(value) => value
                    .as_u64()
                    .and_then(|version| u32::try_from(version).ok())
                    .filter(|version| *version >= 1)
                    .ok_or_else(|| {
                        RpcError::new(INVALID_PARAMS, "capability_version is a positive integer")
                    })?,
            };
            let payload = arguments
                .get("payload")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            match link.invoke(
                instance.as_ref(),
                &capability_id,
                capability_version,
                payload,
            ) {
                Ok(response) => Ok(result_of(response)),
                Err(error) => Ok(ToolResult::control_error(&error)),
            }
        }
        _ => Err(RpcError::new(
            INVALID_PARAMS,
            format!("unknown tool: {name}"),
        )),
    }
}

/// Remove the routing `instance_id` from the arguments, validating it as the
/// contract's instance identity. The gateway must never see it: the
/// capability payload is what the application registered, nothing more.
fn take_instance_id(arguments: &mut Map<String, Value>) -> Result<Option<InstanceId>, RpcError> {
    match arguments.remove(INSTANCE_ID_PROPERTY) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => InstanceId::new(raw).map(Some).map_err(|_| {
            RpcError::new(
                INVALID_PARAMS,
                "instance_id is not a Quantick instance identity; take it from quantick_describe",
            )
        }),
        Some(_) => Err(RpcError::new(INVALID_PARAMS, "instance_id is a string")),
    }
}

fn forward(
    link: &mut dyn ControlLink,
    instance: Option<&InstanceId>,
    capability_id: &str,
    payload: Value,
) -> Result<ToolResult, RpcError> {
    match link.invoke(instance, capability_id, FIRST_CAPABILITY_VERSION, payload) {
        Ok(response) => Ok(result_of(response)),
        Err(error) => Ok(ToolResult::control_error(&error)),
    }
}

/// A capability response as a tool result: the result itself plus the
/// revisions that let a follow-up call reason about staleness.
fn result_of(response: quantick_control::wire::ResponseEnvelope) -> ToolResult {
    match response.outcome {
        quantick_control::wire::ResponseOutcome::Success { result } => {
            ToolResult::structured(json!({
                "instance_id": response.instance_id,
                "capture_revision": response.capture_revision,
                "module_revisions": response.module_revisions,
                "warnings": response.warnings,
                "result": result,
            }))
        }
        quantick_control::wire::ResponseOutcome::Failure { error } => {
            ToolResult::control_error(&error)
        }
    }
}

fn search(
    link: &mut dyn ControlLink,
    instance: Option<&InstanceId>,
    arguments: &Map<String, Value>,
) -> Result<ToolResult, RpcError> {
    let query = optional_string(arguments, "query")?.map(|query| query.to_lowercase());
    let module = optional_string(arguments, "module")?;
    let response = match link.invoke(
        instance,
        DESCRIBE_CAPABILITY,
        FIRST_CAPABILITY_VERSION,
        json!({}),
    ) {
        Ok(response) => response,
        Err(error) => return Ok(ToolResult::control_error(&error)),
    };
    let described = match response.outcome {
        quantick_control::wire::ResponseOutcome::Success { result } => result,
        quantick_control::wire::ResponseOutcome::Failure { error } => {
            return Ok(ToolResult::control_error(&error));
        }
    };
    let matches = |candidate: &Value, fields: &[&str]| -> bool {
        if let Some(module) = &module
            && candidate.get("module").and_then(Value::as_str) != Some(module.as_str())
            && candidate.get("module_id").and_then(Value::as_str) != Some(module.as_str())
        {
            return false;
        }
        match &query {
            None => true,
            Some(query) => fields.iter().any(|field| {
                candidate
                    .get(*field)
                    .and_then(Value::as_str)
                    .is_some_and(|text| text.to_lowercase().contains(query.as_str()))
            }),
        }
    };
    let capabilities = described
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|capabilities| {
            capabilities
                .iter()
                .filter(|capability| matches(capability, &["id", "title", "description", "module"]))
                .map(|capability| {
                    let mut summary = Map::new();
                    for field in [
                        "id",
                        "version",
                        "title",
                        "description",
                        "module",
                        "effect",
                        "read_only",
                        "availability",
                        "required_permissions",
                        "pagination",
                    ] {
                        if let Some(value) = capability.get(field) {
                            summary.insert(field.to_owned(), value.clone());
                        }
                    }
                    Value::Object(summary)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let snapshot_scopes = described
        .get("snapshot_scopes")
        .and_then(Value::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .filter(|scope| matches(scope, &["id", "title", "description", "module_id"]))
                .map(|scope| {
                    // The describe document carries each scope's whole
                    // schema; the search names the scope and leaves the
                    // document to `quantick_describe`.
                    let mut summary = Map::new();
                    for field in [
                        "id",
                        "module_id",
                        "title",
                        "description",
                        "schema_version",
                        "required_permissions",
                    ] {
                        if let Some(value) = scope.get(field) {
                            summary.insert(field.to_owned(), value.clone());
                        }
                    }
                    Value::Object(summary)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(ToolResult::structured(json!({
        "instance_id": response.instance_id,
        "capability_count": capabilities.len(),
        "capabilities": capabilities,
        "snapshot_scope_count": snapshot_scopes.len(),
        "snapshot_scopes": snapshot_scopes,
    })))
}

fn optional_string(arguments: &Map<String, Value>, key: &str) -> Result<Option<String>, RpcError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.chars().count() <= SEARCH_QUERY_MAX_CHARS => {
            Ok(Some(text.clone()))
        }
        Some(Value::String(_)) => Err(RpcError::new(
            INVALID_PARAMS,
            format!("{key} is at most {SEARCH_QUERY_MAX_CHARS} characters"),
        )),
        Some(_) => Err(RpcError::new(INVALID_PARAMS, format!("{key} is a string"))),
    }
}

fn internal(error: serde_json::Error) -> RpcError {
    RpcError::new(
        crate::jsonrpc::INTERNAL_ERROR,
        format!("result serialization failed: {error}"),
    )
}

fn parse_schema(document: &str) -> Value {
    serde_json::from_str(document).expect("committed contract documents are valid JSON")
}

fn instance_id_schema() -> Value {
    json!({
        "type": "string",
        "description": "Routing only, removed before the instance sees the call: which running instance answers. Omit it when exactly one instance is live; take it from quantick_describe otherwise."
    })
}

fn instance_only_schema() -> Value {
    json!({
        "type": "object",
        "properties": { INSTANCE_ID_PROPERTY: instance_id_schema() },
        "additionalProperties": false
    })
}

/// Add the routing property to a committed capability input schema. The
/// committed document forbids unknown properties, so the routing one has to
/// be declared here and stripped again before the payload is forwarded.
/// Take one required routing choice out of the arguments, so the payload the
/// instance validates is exactly its own committed input document.
fn take_choice(arguments: &mut Map<String, Value>, property: &str) -> Result<String, RpcError> {
    match arguments.remove(property) {
        Some(Value::String(choice)) => Ok(choice),
        Some(_) => Err(RpcError::new(
            INVALID_PARAMS,
            format!("{property} must be a string"),
        )),
        None => Err(RpcError::new(
            INVALID_PARAMS,
            format!("{property} is required"),
        )),
    }
}

fn with_instance_routing(mut schema: Value) -> Value {
    if let Some(object) = schema.as_object_mut() {
        object.remove("$schema");
        let properties = object
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(properties) = properties.as_object_mut() {
            properties.insert(INSTANCE_ID_PROPERTY.to_owned(), instance_id_schema());
        }
    }
    schema
}

fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            INSTANCE_ID_PROPERTY: instance_id_schema(),
            "query": {
                "type": "string",
                "maxLength": SEARCH_QUERY_MAX_CHARS,
                "description": "Case-insensitive substring matched against capability and scope IDs, titles, descriptions and modules. Omit to list everything."
            },
            "module": {
                "type": "string",
                "maxLength": SEARCH_QUERY_MAX_CHARS,
                "description": "Restrict to one owning module ID, such as chart or health."
            }
        },
        "additionalProperties": false
    })
}

fn invoke_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            INSTANCE_ID_PROPERTY: instance_id_schema(),
            "capability_id": {
                "type": "string",
                "pattern": "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*){1,7}$",
                "description": "A registered capability ID from quantick_describe or quantick_search_capabilities."
            },
            "capability_version": {
                "type": "integer",
                "minimum": 1,
                "default": 1
            },
            "payload": {
                "type": "object",
                "description": "The capability's declared input, validated by the instance against the schema quantick_describe publishes."
            }
        },
        "required": ["capability_id"],
        "additionalProperties": false
    })
}

/// The `quantick_describe` output: either the instance list (no ID given) or
/// the instance's describe result.
fn describe_output_schema() -> Value {
    let describe = parse_schema(DESCRIBE_RESULT_SCHEMA);
    let mut definitions = definitions_of(&describe);
    let mut described = describe;
    if let Some(object) = described.as_object_mut() {
        object.remove("$defs");
        object.remove("$schema");
    }
    definitions.insert(
        "InstanceList".to_owned(),
        json!({
            "type": "object",
            "properties": {
                "instances": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "instance_id": { "type": "string" },
                            "application_version": { "type": "string" },
                            "application_commit": { "type": "string" },
                            "process_id": { "type": "integer", "minimum": 0 },
                            "published_at_unix_ms": { "type": "integer" }
                        },
                        "required": ["instance_id", "application_version", "application_commit", "process_id", "published_at_unix_ms"]
                    }
                },
                "issues": { "type": "array", "items": { "type": "string" } },
                "next_steps": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["instances", "issues", "next_steps"]
        }),
    );
    definitions.insert("DescribeResult".to_owned(), described);
    // The capability response wraps the describe result like every other
    // forwarded capability result does; the error branch is the same.
    definitions.insert(
        "CapabilityResponse".to_owned(),
        response_wrapper_schema(json!({ "$ref": "#/$defs/DescribeResult" })),
    );
    add_error_branch(&mut definitions);
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": definitions,
        "oneOf": [
            { "$ref": "#/$defs/InstanceList" },
            { "$ref": "#/$defs/CapabilityResponse" },
            { "$ref": "#/$defs/ErrorResponse" }
        ]
    })
}

/// The output schema of a forwarded capability: the envelope fields the
/// adapter returns around the capability's own result document.
fn capability_output_schema(capability_result: Value) -> Value {
    let mut definitions = definitions_of(&capability_result);
    let mut result = capability_result;
    if let Some(object) = result.as_object_mut() {
        object.remove("$defs");
        object.remove("$schema");
    }
    definitions.insert("CapabilityResult".to_owned(), result);
    definitions.insert(
        "CapabilityResponse".to_owned(),
        response_wrapper_schema(json!({ "$ref": "#/$defs/CapabilityResult" })),
    );
    add_error_branch(&mut definitions);
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": definitions,
        "oneOf": [
            { "$ref": "#/$defs/CapabilityResponse" },
            { "$ref": "#/$defs/ErrorResponse" }
        ]
    })
}

/// The error branch of every output schema: `{ "error": <control error> }`,
/// the shape [`ToolResult::control_error`] produces. The control error's own
/// definitions are hoisted beside the capability's; the names do not collide
/// because both come from the same contract generator.
fn add_error_branch(definitions: &mut BTreeMap<String, Value>) {
    let error = parse_schema(CONTROL_ERROR_SCHEMA);
    for (name, schema) in definitions_of(&error) {
        definitions.entry(name).or_insert(schema);
    }
    let mut error_body = error;
    if let Some(object) = error_body.as_object_mut() {
        object.remove("$defs");
        object.remove("$schema");
    }
    definitions.insert("ControlError".to_owned(), error_body);
    definitions.insert(
        "ErrorResponse".to_owned(),
        json!({
            "type": "object",
            "properties": { "error": { "$ref": "#/$defs/ControlError" } },
            "required": ["error"],
            "additionalProperties": false
        }),
    );
}

fn response_wrapper_schema(result: Value) -> Value {
    json!({
        "type": "object",
        "properties": {
            "instance_id": { "type": "string" },
            "capture_revision": { "type": ["string", "null"], "description": "Present for a coherent captured read; a decimal string." },
            "module_revisions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "module_id": { "type": "string" },
                        "revision": { "type": "string" }
                    },
                    "required": ["module_id", "revision"]
                }
            },
            "warnings": { "type": "array" },
            "result": result
        },
        "required": ["instance_id", "module_revisions", "warnings", "result"]
    })
}

/// The `$defs` of a committed document, hoisted so `#/$defs/...` references
/// keep resolving when the document is nested under a wrapper. JSON Schema
/// references resolve against the root document, never the subschema.
fn definitions_of(document: &Value) -> BTreeMap<String, Value> {
    document
        .get("$defs")
        .and_then(Value::as_object)
        .map(|definitions| {
            definitions
                .iter()
                .map(|(name, schema)| (name.clone(), schema.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use quantick_control::schema::validate_schema;

    use super::*;

    #[test]
    fn every_tool_schema_is_a_valid_draft_2020_12_document_with_routing() {
        for tool in tools("observer") {
            let mut input = tool.input_schema.clone();
            input["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
            validate_schema(&input)
                .unwrap_or_else(|error| panic!("{} input schema is invalid: {error}", tool.name));
            assert!(
                tool.input_schema["properties"][INSTANCE_ID_PROPERTY].is_object(),
                "{} declares the routing instance_id",
                tool.name
            );
            if let Some(output) = &tool.output_schema {
                validate_schema(output).unwrap_or_else(|error| {
                    panic!("{} output schema is invalid: {error}", tool.name)
                });
                // A tool execution error carries structured content too, and
                // clients validate it against the same schema: the error
                // branch must admit it.
                let error = ToolResult::control_error(
                    &quantick_control::error::ControlError::invalid_request("nope"),
                );
                quantick_control::schema::validate_instance(
                    output,
                    error.structured_content.as_ref().unwrap(),
                )
                .unwrap_or_else(|problem| {
                    panic!(
                        "{} output schema refuses its error branch: {problem}",
                        tool.name
                    )
                });
            }
        }
    }

    #[test]
    fn the_fake_describe_document_keeps_the_contracts_scope_shape() {
        // The search reads the scope's `id`; a fake that drifted to another
        // field name would make the search pass in tests and fail against
        // a running instance. Pin the fakes to the committed document.
        let describe = parse_schema(DESCRIBE_RESULT_SCHEMA);
        let scope_schema = json!({
            "$schema": describe["$schema"],
            "$ref": "#/$defs/SnapshotScopeDescriptor",
            "$defs": describe["$defs"],
        });
        let mut link = crate::fake::FakeLink::default();
        let id = InstanceId::from_bytes([9; 16]);
        link.add_instance(id.clone());
        let response = link
            .invoke(Some(&id), DESCRIBE_CAPABILITY, 1, json!({}))
            .unwrap();
        let quantick_control::wire::ResponseOutcome::Success { result, .. } = response.outcome
        else {
            panic!("the fake describes itself");
        };
        let scopes = result["snapshot_scopes"].as_array().expect("scopes");
        assert!(!scopes.is_empty());
        for scope in scopes {
            quantick_control::schema::validate_instance(&scope_schema, scope).unwrap_or_else(
                |problem| panic!("fake scope {scope} drifted from the contract: {problem}"),
            );
        }
    }

    #[test]
    fn the_named_reads_are_read_only_and_invoke_follows_the_ceiling() {
        for tool in tools("observer") {
            assert!(
                tool.annotations.read_only_hint,
                "{} is read-only",
                tool.name
            );
            assert!(!tool.annotations.destructive_hint);
            assert!(!tool.annotations.open_world_hint);
        }
        let invoke = tools("developer")
            .into_iter()
            .find(|tool| tool.name == INVOKE)
            .unwrap();
        assert!(!invoke.annotations.read_only_hint);
        assert!(invoke.annotations.destructive_hint);
        assert!(invoke.annotations.open_world_hint);
        let invoke = tools("annotator")
            .into_iter()
            .find(|tool| tool.name == INVOKE)
            .unwrap();
        assert!(!invoke.annotations.read_only_hint);
        assert!(!invoke.annotations.destructive_hint);
        assert!(!invoke.annotations.open_world_hint);
    }

    #[test]
    fn the_tool_list_is_fixed_and_named_as_the_contract_says() {
        let names = tools("observer")
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                DESCRIBE,
                GET_SNAPSHOT,
                GET_CHART_WINDOW,
                GET_DIAGNOSTICS,
                GET_SCENE,
                READ_EVENTS,
                WAIT_FOR_CHANGE,
                SEARCH_CAPABILITIES,
                INVOKE
            ]
        );
    }

    #[test]
    fn the_routing_id_is_validated_and_removed_before_forwarding() {
        let mut arguments = Map::new();
        arguments.insert("instance_id".to_owned(), json!("not-an-id"));
        assert_eq!(
            take_instance_id(&mut arguments).unwrap_err().code,
            INVALID_PARAMS
        );
        let id = InstanceId::from_bytes([3; 16]);
        let mut arguments = Map::new();
        arguments.insert("instance_id".to_owned(), json!(id.to_string()));
        arguments.insert("scopes".to_owned(), json!(["system.info"]));
        assert_eq!(take_instance_id(&mut arguments).unwrap(), Some(id));
        assert!(!arguments.contains_key("instance_id"));
        assert!(arguments.contains_key("scopes"));
    }
}
