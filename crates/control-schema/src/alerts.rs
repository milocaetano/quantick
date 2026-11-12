//! Alert control contracts. Definitions use the domain's serialized vocabulary.
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use serde_json::{Value, json};
pub const OPEN_CAPABILITY_ID: &str = "alerts.open";
pub const OPEN: &str = OPEN_CAPABILITY_ID;
pub const RAIL_CONTROL: &str = "dock.alerts";
pub const MODULE_ID: &str = "alerts";
pub const CHANGED_EVENT: &str = "alerts.changed";
pub const LIST_CAPABILITY_ID: &str = "alerts.list";
pub const LIST: &str = LIST_CAPABILITY_ID;
pub const READ_CAPABILITY_ID: &str = "alerts.read";
pub const READ: &str = READ_CAPABILITY_ID;
pub const VALIDATE_CAPABILITY_ID: &str = "alerts.validate";
pub const VALIDATE: &str = VALIDATE_CAPABILITY_ID;
pub const CREATE_CAPABILITY_ID: &str = "alerts.create";
pub const CREATE: &str = CREATE_CAPABILITY_ID;
pub const EDIT_CAPABILITY_ID: &str = "alerts.edit";
pub const EDIT: &str = EDIT_CAPABILITY_ID;
pub const PAUSE_CAPABILITY_ID: &str = "alerts.pause";
pub const PAUSE: &str = PAUSE_CAPABILITY_ID;
pub const RESUME_CAPABILITY_ID: &str = "alerts.resume";
pub const RESUME: &str = RESUME_CAPABILITY_ID;
pub const REARM_CAPABILITY_ID: &str = "alerts.rearm";
pub const REARM: &str = REARM_CAPABILITY_ID;
pub const DELETE_CAPABILITY_ID: &str = "alerts.delete";
pub const DELETE: &str = DELETE_CAPABILITY_ID;
pub const HISTORY_CAPABILITY_ID: &str = "alerts.history";
pub const HISTORY: &str = HISTORY_CAPABILITY_ID;
pub const CLEAR_HISTORY_CAPABILITY_ID: &str = "alerts.clear_history";
pub const CLEAR_HISTORY: &str = CLEAR_HISTORY_CAPABILITY_ID;
pub const TARGET_RESOLVE_CAPABILITY_ID: &str = "alerts.target.resolve";
pub const TARGET_RESOLVE: &str = TARGET_RESOLVE_CAPABILITY_ID;
pub const OPERATIONS: &[&str] = &[
    OPEN,
    LIST,
    READ,
    VALIDATE,
    CREATE,
    EDIT,
    PAUSE,
    RESUME,
    REARM,
    DELETE,
    HISTORY,
    CLEAR_HISTORY,
    TARGET_RESOLVE,
];

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct DrawingSelector {
    pub tab_id: u64,
    pub pane_id: u64,
    pub drawing_id: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetResolveError {
    Unavailable,
    Missing,
    Ambiguous,
    Busy,
    SourceChanged,
    WriteFailed(String),
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpecInput {
    pub spec: Value,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    pub spec: Value,
    pub expected_revision: u64,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevisionInput {
    pub id: u64,
    pub expected_revision: u64,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    pub id: u64,
}
#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PageInput {
    #[serde(default)]
    pub after: u64,
    #[serde(default = "page_limit")]
    pub limit: usize,
}
fn page_limit() -> usize {
    32
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object", "additionalProperties":false, "properties":properties, "required":required})
}
pub fn input_schema(id: &str) -> Value {
    let integer = json!({"type":"integer", "minimum":0});
    match id {
        CREATE | VALIDATE => object(json!({"spec":spec_schema()}), &["spec"]),
        EDIT => object(
            json!({"spec":spec_schema(),"expected_revision":integer}),
            &["spec", "expected_revision"],
        ),
        PAUSE | RESUME | REARM | DELETE => object(
            json!({"id":integer,"expected_revision":integer}),
            &["id", "expected_revision"],
        ),
        READ => object(json!({"id":integer}), &["id"]),
        LIST | HISTORY => object(
            json!({"after":integer,"limit":{"type":"integer","minimum":1,"maximum":32}}),
            &[],
        ),
        TARGET_RESOLVE => quantick_control::schema::generated_schema::<DrawingSelector>(),
        _ => object(json!({}), &[]),
    }
}
fn variant(name: &str, value: Value) -> Value {
    object(json!({name:value}), &[name])
}
pub fn spec_schema() -> Value {
    let text = json!({"type":"string","maxLength":2048});
    let positive = json!({"type":"integer","minimum":1});
    let target = json!({"oneOf":[variant("Value",decimal_schema()),variant("Drawing",object(json!({"workspace":text,"origin":text,"pane":text,"drawing":positive,"incarnation":text}), &["workspace","origin","pane","drawing","incarnation"]))]});
    let source = json!({"oneOf":[variant("Live",object(json!({"feed":text}), &["feed"])),variant("Replay",object(json!({"recording":text}), &["recording"]))]});
    let condition = object(
        json!({"operator":{"enum":["Crossing","CrossingUp","CrossingDown","GreaterThan","LessThan","Entering","Exiting","Inside","Outside"]},"target":target}),
        &["operator", "target"],
    );
    let mut schema = object(
        json!({"version":{"const":1},"id":{"type":"integer","minimum":0},"name":text,
            "market":object(json!({"source":source,"symbol":text,"aggregation":text}), &["source","symbol","aggregation"]),
            "conditions":{"type":"array","minItems":1,"maxItems":8,"items":condition},
            "frequency":{"enum":["Once","PerBar","PerBarClose","PerMinute"]},
            "expiration":{"oneOf":[{"const":"Never"},variant("At",object(json!({"utc_ms":{"type":"integer"},"display_offset_minutes":{"type":"integer","minimum":-720,"maximum":840}}), &["utc_ms","display_offset_minutes"]))]},
            "message":text,"notification":object(json!({"popup":{"type":"boolean"},"toast":{"type":"boolean"},"sound":sound_schema(),"sound_seconds":{"type":["integer","null"],"minimum":1}}), &["popup","toast"])
        }),
        &[
            "version",
            "id",
            "name",
            "market",
            "conditions",
            "frequency",
            "expiration",
            "message",
            "notification",
        ],
    );
    schema["description"] = json!(
        "Version 1. All conditions evaluate together against actual price; no OR, indicators or orders. Decimal prices are canonical strings that round-trip through finite f64 precision; values needing rounding are refused. Expiration is an absolute UTC millisecond cutoff and display offset, so reconnect never extends it. Bar aggregation is the canonical chart token, e.g. tick:50 or time:60000."
    );
    schema["x-operator-targets"] = json!({"Crossing":["Value","Line"],"CrossingUp":["Value","Line"],"CrossingDown":["Value","Line"],"GreaterThan":["Value","Line","Rectangle"],"LessThan":["Value","Line","Rectangle"],"Entering":["Channel","Rectangle"],"Exiting":["Channel","Rectangle"],"Inside":["Channel","Rectangle"],"Outside":["Channel","Rectangle"]});
    schema["x-frequency-semantics"] = json!({"Once":"Complete after the first qualifying event; rearm explicitly.","PerBar":"At most once per actual bar identity.","PerBarClose":"Only each distinct committed close.","PerMinute":"At most once per monotonic session minute."});
    schema
}

pub fn descriptor(id: &'static str) -> CapabilityDescriptor {
    if id == OPEN {
        let mut descriptor = crate::layout::transient_descriptor::<Value, Value>(
            id,
            MODULE_ID,
            "Open Alerts",
            "Open the local Alerts manager beside the chart.",
            "Opening an open manager leaves it open; inspect dock.alerts selected state.",
        );
        descriptor.input_schema = input_schema(id);
        descriptor.output_schema = output_schema(id);
        return descriptor;
    }
    let read = matches!(id, LIST | READ | VALIDATE | HISTORY);
    let mut descriptor = if read {
        quantick_control_host::authority::read_descriptor::<Value, Value>(
            &quantick_control_host::authority::ReadSpec {
                id,
                module: MODULE_ID,
                title: id,
                description: "Read or validate bounded local alert state. No notification or mutation.",
                permissions: &[quantick_control_host::authority::OBSERVE_PERMISSION_ID],
                pagination: None,
            },
        )
    } else {
        let mut d = crate::layout::descriptor(
            id,
            id,
            "Persist local alert definitions or a durable drawing binding. Never places orders or sends a notification during this command.",
            input_schema(id),
        );
        d.module = quantick_control::id::ModuleId::new(MODULE_ID).expect("static module");
        d.required_permissions = [quantick_control::id::PermissionId::new(
            quantick_control_host::authority::COCKPIT_PERMISSION_ID,
        )
        .expect("static permission")]
        .into();
        d.idempotency = IdempotencyPolicy::Required;
        d.expected_cost.max_response_bytes =
            Some(quantick_control::limits::CONTROL_IDEMPOTENCY_RECORD_MAX_BYTES);
        d.stale_input_safety = Some("Idempotency keys deduplicate within the same connection and bounded retention window only. After reconnect or unknown outcome, read back before retrying. Mutations require idempotency keys. Edits, deletion and lifecycle changes require the definition revision; target resolution persists the same identity before returning it.".into());
        d
    };
    if matches!(id, DELETE | CLEAR_HISTORY) {
        use quantick_control::id::{PermissionId, RiskFlagId};
        use quantick_control_host::authority;
        descriptor.effect = authority::effect(authority::ALERT_DELETE_EFFECT_ID);
        descriptor.required_permissions.insert(
            PermissionId::new(authority::COCKPIT_ALERT_DELETE_PERMISSION_ID)
                .expect("static permission"),
        );
        descriptor
            .risk_flags
            .insert(RiskFlagId::new(authority::ALERT_REMOVED_RISK_FLAG).expect("static risk flag"));
        descriptor.reversible = false;
        // Like journal deletion, destructive:false avoids the unsupported envelope
        // expected-revisions policy. It does not imply that removal is reversible.
        descriptor.description = if id == DELETE {
            "Permanently remove one alert definition. Recorded history remains. Requires cockpit.alert.delete; no command restores the definition."
        } else {
            "Permanently clear all recorded alert history at execution time. Definitions remain. Requires cockpit.alert.delete; no command restores events."
        }.into();
        descriptor.stale_input_safety = Some(
            "Required idempotency keys deduplicate only within the same connection and bounded retention window. After reconnect or unknown outcome, reconcile alerts.list/read/history and alerts.changed before retrying. Delete requires payload.expected_revision for the selected definition; this is distinct from unsupported envelope.expected_revisions. Clear history has no revision guard and removes all events present at execution, including events recorded after the last read.".into(),
        );
    }
    descriptor.input_schema = input_schema(id);
    descriptor.output_schema = output_schema(id);
    descriptor
}

#[must_use]
pub fn documents() -> Vec<(&'static str, Value)> {
    vec![
        ("observer-alert-spec-v1.schema.json", spec_schema()),
        (
            "observer-alert-target-resolve-input-v1.schema.json",
            input_schema(TARGET_RESOLVE),
        ),
    ]
}

fn decimal_schema() -> Value {
    quantick_control::schema::generated_schema::<quantick_control::wire::CanonicalDecimal>()
}
fn target_schema() -> Value {
    let text = json!({"type":"string","minLength":1,"maxLength":2048});
    object(
        json!({"workspace":text,"origin":text,"pane":text,"drawing":{"type":"integer","minimum":1},"incarnation":text}),
        &["workspace", "origin", "pane", "drawing", "incarnation"],
    )
}
fn view_schema() -> Value {
    object(
        json!({"spec":spec_schema(),"revision":{"type":"integer","minimum":1},
        "status":{"enum":["monitoring","paused","completed","expired","suspended"]},
        "reason":{"enum":[null,"SourceInactive","MissingDrawing","AmbiguousDrawing","LegacyIdentity","UnsupportedDrawing","OutsideTimeDomain","InvalidGeometry","Hydrating","ReplayPaused","Scaffold"]},
        "summary":{"type":"string","maxLength":65536}}),
        &["spec", "revision", "status", "reason", "summary"],
    )
}
fn event_schema() -> Value {
    let integer = json!({"type":"integer","minimum":0});
    object(
        json!({"sequence":integer,"definition_revision":integer,"spec":spec_schema(),
        "utc_ms":{"type":"integer","description":"Desktop receipt/evaluation time in UTC milliseconds, not replay market time."},"session_ms":integer,"bar_epoch":integer,"bar_sequence":integer,
        "price":decimal_schema(),"delivery":{"enum":["Pending","Delivered","Suppressed","Failed"]}}),
        &[
            "sequence",
            "definition_revision",
            "spec",
            "utc_ms",
            "session_ms",
            "bar_epoch",
            "bar_sequence",
            "price",
            "delivery",
        ],
    )
}
/// Every admitted result is bounded and describes the same wire vocabulary.
pub fn output_schema(id: &str) -> Value {
    match id {
        OPEN => object(json!({"open":{"const":true}}), &["open"]),
        READ => view_schema(),
        LIST => object(
            json!({"alerts":{"type":"array","maxItems":32,"items":view_schema()},"next_after":{"type":["integer","null"],"minimum":0}}),
            &["alerts", "next_after"],
        ),
        HISTORY => object(
            json!({"events":{"type":"array","maxItems":32,"items":event_schema()},"next_after":{"type":["integer","null"],"minimum":0}}),
            &["events", "next_after"],
        ),
        VALIDATE => object(json!({"valid":{"const":true}}), &["valid"]),
        TARGET_RESOLVE => object(json!({"target":target_schema()}), &["target"]),
        _ => object(
            json!({"id":{"type":["integer","null"],"minimum":0},"revision":{"type":"integer","minimum":0}}),
            &["id", "revision"],
        ),
    }
}

fn sound_schema() -> Value {
    json!({"enum":[null,"information","question","exclamation","critical","beep","american-phone","business-phone","cuckoo","english-phone","high-pitched-beep","low-beep","short-beep","aviary","brook","city","dock","dockside","ebb-tide","everglades","foghorn","hail","northwoods","oceanside","rain","rainforest","steam-train","summer-night","surfs-up","thunderstorm","white-noise","wind-chimes","yosemite-falls"]})
}

const fn readback(capability: &'static str) -> crate::readback::Readback {
    crate::readback::journal(
        capability,
        IdempotencyPolicy::Required,
        CHANGED_EVENT,
        "payload.receipt",
        "an event after the pre-call cursor names the operation and original request_id; receipt reports the committed alert id/revision or exact durable target. Read alerts.list/read/history for current state. A dropped-before cursor requires reconciliation before a new command.",
        &[
            "alert_socket_crud_retry_and_readback",
            "alert_socket_target_binding_and_reopen",
        ],
    )
}
pub const READBACKS: &[crate::readback::Readback] = &[
    crate::readback::snapshot(
        OPEN,
        IdempotencyPolicy::Optional,
        crate::scene::CONTROLS_SCOPE_ID,
        "controls[].selected",
        "dock.alerts is selected",
        &["alerts_right_rail_entry"],
    ),
    readback(CREATE),
    readback(EDIT),
    readback(PAUSE),
    readback(RESUME),
    readback(REARM),
    readback(DELETE),
    readback(CLEAR_HISTORY),
    readback(TARGET_RESOLVE),
];
