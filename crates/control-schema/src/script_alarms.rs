//! `alerts.script.history`: the alarms indicator scripts raised this session
//! (`alertcondition` on a bar that closed live), and `alerts.script.fired`,
//! the journal event each one writes as it is delivered.
use quantick_control::registry::CapabilityDescriptor;
use quantick_control::wire::WireU64;
use quantick_control_host::authority::{ReadSpec, read_descriptor};
use quantick_stores::script_alarms::ScriptAlarmEntry;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const HISTORY_CAPABILITY_ID: &str = "alerts.script.history";

/// Journaled once per script alarm as the window delivers it, under the
/// trader's hand; its payload is one [`ScriptAlarm`].
pub const FIRED_EVENT_KIND: &str = "alerts.script.fired";

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Only alarms with a greater sequence; absent for every kept alarm.
    pub after: Option<WireU64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct ScriptAlarm {
    /// Increasing from 1 for the session; never reused.
    pub sequence: WireU64,
    /// When the window delivered it, UTC milliseconds.
    pub utc_ms: i64,
    /// The market of the tab whose chart raised it.
    pub symbol: String,
    /// The indicator's title, as its legend shows it.
    pub indicator: String,
    /// The `alertcondition` title.
    pub title: String,
    /// The `alertcondition` message; the title when the script gave none.
    pub message: String,
    /// Close time of the bar that raised it, exchange milliseconds.
    pub bar_close_ms: i64,
}

impl From<&ScriptAlarmEntry> for ScriptAlarm {
    fn from(entry: &ScriptAlarmEntry) -> Self {
        Self {
            sequence: WireU64::new(entry.sequence),
            utc_ms: entry.utc_ms,
            symbol: entry.symbol.clone(),
            indicator: entry.alarm.indicator.clone(),
            title: entry.alarm.alert.title.clone(),
            message: entry.alarm.alert.message.clone(),
            bar_close_ms: entry.alarm.bar_close_ms,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct History {
    /// Oldest first; the session keeps the newest 256.
    pub alarms: Vec<ScriptAlarm>,
    /// The newest sequence ever raised (0: none yet). Pass it as `after` to
    /// read only what comes next.
    pub last_sequence: WireU64,
}

pub fn descriptor() -> CapabilityDescriptor {
    read_descriptor::<Input, History>(&ReadSpec {
        id: HISTORY_CAPABILITY_ID,
        module: crate::alerts::MODULE_ID,
        title: "Read script alarms",
        description: "Reads the alarms indicator scripts raised this session, oldest first: each Pine alertcondition that held on a bar that closed live, once per bar, never on a forming bar or replayed history. Each carries sequence, delivery time, symbol, indicator title, alert title and message, and the bar's close time. The newest 256 are kept, not persisted. Pass after = a previous last_sequence for only newer ones; each is also journaled as alerts.script.fired. Changes nothing.",
        permissions: &[
            "observe",
            "observe.market",
            "observe.indicators",
            "observe.user_text",
        ],
        pagination: None,
    })
}

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as s;
    vec![
        ("alerts-script-history-input-v1.schema.json", s::<Input>()),
        (
            "alerts-script-history-result-v1.schema.json",
            s::<History>(),
        ),
        (
            "alerts-script-fired-event-v1.schema.json",
            s::<ScriptAlarm>(),
        ),
    ]
}
