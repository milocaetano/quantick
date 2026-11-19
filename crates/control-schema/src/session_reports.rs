//! Opening a Trades report by stable session identity or a clicked civil day.
use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence},
    schema::generated_schema,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const REPORT_OPEN_CAPABILITY_ID: &str = "session.report.open";
pub const EVENT_KIND: &str = "session.report.opened";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportInput {
    Session {
        #[schemars(length(max = 512))]
        session: String,
    },
    Day {
        #[schemars(length(max = 512))]
        session: String,
        #[schemars(length(min = 10, max = 10))]
        day: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReportResult {
    pub action: String,
    pub session: String,
    pub origin: String,
    pub day: Option<String>,
    pub title: String,
    pub trade_count: quantick_control::wire::WireU64,
    pub net_points: quantick_control::wire::CanonicalDecimal,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        REPORT_OPEN_CAPABILITY_ID,
        "Open a session or day report",
        "Open the same performance report as the Trades session context menu. action=session selects only that session; action=day requires YYYY-MM-DD in the display timezone and selects every instrument and session of that origin on the clicked day. The session must hold a trade on that day. Session keys are <symbol>/<file>.csv for the simulator and broker/<symbol>/<file>.csv for real-trades, independent of custom names. Replaces prior symbol, period, source and session filters. Returns the opened scope, trade count and net points; events.read reconciles a lost response through session.report.opened. Refusals: invalid_session, session_not_found, session_unreadable, day_not_in_session, no_ledger. Never places orders or writes history.",
        false,
        generated_schema::<ReportInput>(),
    );
    descriptor.module =
        quantick_control::id::ModuleId::new(crate::session::MODULE_ID).expect("static module ID");
    descriptor.output_schema = generated_schema::<ReportResult>();
    descriptor.persistence = EffectPersistence::Transient;
    descriptor.stale_input_safety = Some("Repeating a request opens the same scope from current history; deleted sessions are refused before changing the report.".to_owned());
    descriptor.expected_cost.max_response_bytes = Some(4096);
    descriptor
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::journal(
    REPORT_OPEN_CAPABILITY_ID,
    quantick_control::registry::IdempotencyPolicy::Optional,
    EVENT_KIND,
    "payload.report",
    "After a successful report open",
    &[crate::readback::EVERY_OPTIONAL_TEST],
)];

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "session-report-input-v1.schema.json",
            generated_schema::<ReportInput>(),
        ),
        (
            "session-report-result-v1.schema.json",
            generated_schema::<ReportResult>(),
        ),
    ]
}
