//! `alerts.script.history`: the alarms indicator scripts raised this session
//! (`alertcondition` on a bar that closed on the tab's source, live feed or
//! replay), and `alerts.script.fired`, the journal event each one writes as
//! it is delivered.
use quantick_control::registry::CapabilityDescriptor;
use quantick_control::wire::WireU64;
use quantick_control_host::authority::{ReadSpec, read_descriptor};
use quantick_stores::alerts::SourceContext;
use quantick_stores::script_alarms::ScriptAlarmEntry;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const HISTORY_CAPABILITY_ID: &str = "alerts.script.history";

/// Journaled once per script alarm as the window delivers it; its payload is
/// one [`ScriptAlarm`]. The window journals it under its own local actor, as
/// it does an automatic contract roll: no trader gesture is implied.
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
    /// Where the bar closed: the live feed, or the recording a replay plays.
    pub source: AlarmSource,
    /// The market of the tab whose chart raised it, read when it arrived.
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

/// The wire shape of [`SourceContext`], as `alerts.*` market sources are.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum AlarmSource {
    Live { feed: String },
    Replay { recording: String },
}

impl From<&SourceContext> for AlarmSource {
    fn from(source: &SourceContext) -> Self {
        match source {
            SourceContext::Live { feed } => Self::Live { feed: feed.clone() },
            SourceContext::Replay { recording } => Self::Replay {
                recording: recording.clone(),
            },
        }
    }
}

impl From<&ScriptAlarmEntry> for ScriptAlarm {
    fn from(entry: &ScriptAlarmEntry) -> Self {
        let raised = &entry.raised;
        Self {
            sequence: WireU64::new(entry.sequence),
            utc_ms: entry.utc_ms,
            source: AlarmSource::from(&raised.source),
            symbol: raised.symbol.clone(),
            indicator: raised.alarm.indicator.clone(),
            title: raised.alarm.alert.title.clone(),
            message: raised.alarm.alert.message.clone(),
            bar_close_ms: raised.alarm.bar_close_ms,
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
    /// The oldest sequence still kept (0: none kept). An `after` below this
    /// minus one means the alarms between were dropped unread.
    pub oldest_sequence: WireU64,
}

pub fn descriptor() -> CapabilityDescriptor {
    read_descriptor::<Input, History>(&ReadSpec {
        id: HISTORY_CAPABILITY_ID,
        module: crate::alerts::MODULE_ID,
        title: "Read script alarms",
        description: "Reads the alarms indicator scripts raised this session, oldest first: each Pine alertcondition that held on a bar that closed on the tab's source, once per bar; never on a forming bar, nor for history recomputed on load, backfill, input change or reload. A replay is a source too: its alarms carry source Replay with the recording. Each carries sequence, delivery time, source, symbol, indicator title, alert title and message, and the bar's close time. The newest 256 are kept, not persisted; oldest_sequence shows what was dropped. Pass after = a previous last_sequence for only newer ones; each is also journaled as alerts.script.fired. Changes nothing.",
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
