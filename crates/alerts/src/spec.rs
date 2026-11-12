//! Versioned definitions. Mutation is rare; observations borrow these values.
pub use quantick_workspace::drawing_identity::DurableDrawingRef;
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;
pub const MAX_ALERTS: usize = 256;
pub const MAX_CONDITIONS: usize = 8;
pub const MAX_HISTORY: usize = 512;
pub const MAX_MESSAGE_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AlertId(pub u64);

/// Replay is a separate recording/session, never the live feed's symbol alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceContext {
    Live { feed: String },
    Replay { recording: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketRef {
    pub source: SourceContext,
    pub symbol: String,
    /// Canonical aggregation token, including its parameters.
    pub aggregation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TargetRef {
    Value(f64),
    Drawing(DurableDrawingRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operator {
    Crossing,
    CrossingUp,
    CrossingDown,
    GreaterThan,
    LessThan,
    Entering,
    Exiting,
    Inside,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetKind {
    Value,
    Line,
    Channel,
    Rectangle,
}

/// The editor consumes this catalog instead of implementing operator policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorDescriptor {
    pub operator: Operator,
    pub label: &'static str,
    pub targets: &'static [TargetKind],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Predicate {
    pub operator: Operator,
    pub target: TargetRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frequency {
    Once,
    PerBar,
    PerBarClose,
    PerMinute,
}

impl Frequency {
    /// The trader-facing name the editor and the summary both show.
    pub fn label(self) -> &'static str {
        match self {
            Self::Once => "Only once",
            Self::PerBar => "Once per bar",
            Self::PerBarClose => "Once per bar close",
            Self::PerMinute => "Once per minute",
        }
    }
}

/// Absolute cutoff resolved at definition time using the supplied display
/// offset. Persisting the cutoff prevents a restart from extending expiration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expiration {
    Never,
    At {
        utc_ms: i64,
        display_offset_minutes: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpirationChoice {
    Never,
    EndOfDay,
    Week,
    CalendarMonth,
    Custom { utc_ms: i64 },
}

/// Sound tokens name only the existing neutral catalog; validation rejects
/// unknown tokens. The persistence form does not serialize catalog indexes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub popup: bool,
    pub toast: bool,
    pub sound: Option<String>,
    pub sound_seconds: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSpec {
    pub version: u32,
    pub id: AlertId,
    pub name: String,
    pub market: MarketRef,
    /// Explicit ALL at one snapshot. No historical conjunct latch.
    pub conditions: Vec<Predicate>,
    pub frequency: Frequency,
    pub expiration: Expiration,
    pub message: String,
    pub notification: Notification,
}

impl AlertSpec {
    /// What a toast says when this alert fires: always its name, then the
    /// trader's message when there is one.
    pub fn notice(&self) -> String {
        match self.message.trim() {
            "" => self.name.clone(),
            message => format!("{}: {message}", self.name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Native C11: an alert without a message raised an empty toast, so the
    /// trader could not tell which alert fired. The notice always names it.
    #[test]
    fn the_notice_names_the_alert_even_without_a_message() {
        let mut spec = AlertSpec {
            version: FORMAT_VERSION,
            id: AlertId(1),
            name: "C11 value down 101".into(),
            market: crate::MarketRef {
                source: crate::SourceContext::Live {
                    feed: "fixture".into(),
                },
                symbol: "TEST".into(),
                aggregation: "tick:10".into(),
            },
            conditions: Vec::new(),
            frequency: Frequency::Once,
            expiration: Expiration::Never,
            message: "  ".into(),
            notification: Notification {
                popup: false,
                toast: true,
                sound: None,
                sound_seconds: None,
            },
        };
        assert_eq!(spec.notice(), "C11 value down 101");
        spec.message = "Price crossed".into();
        assert_eq!(spec.notice(), "C11 value down 101: Price crossed");
    }
}
