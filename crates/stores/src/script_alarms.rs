//! The alarms indicator scripts raised this session, newest kept, for the
//! sound/toast delivery and the `alerts.script.history` read.
//!
//! Session-scoped and never persisted: a script alarm is a fact about a bar
//! that closed while the app watched, and it is raised again only by a bar
//! that closes again.
use quantick_alerts::SourceContext;
/// The alarm a [`Raised`] carries, for readers the dependency graph keeps
/// off the indicator crate (control-handlers).
pub use quantick_indicators::{FiredAlert, ScriptAlarm};
use std::collections::VecDeque;

/// How many alarms the log keeps; the oldest goes first.
pub const SCRIPT_ALARM_CAPACITY: usize = 256;

/// An alarm labelled with the market it was raised on, as the tab read it
/// when the alarm arrived — before a market switch on the same frame could
/// relabel it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The live feed, or the recording a replay plays.
    pub source: SourceContext,
    pub symbol: String,
    pub alarm: ScriptAlarm,
}

/// `alarms`, each labelled with `source` and `symbol`.
pub fn label(
    source: &SourceContext,
    symbol: &str,
    alarms: impl IntoIterator<Item = ScriptAlarm>,
) -> impl Iterator<Item = Raised> {
    let (source, symbol) = (source.clone(), symbol.to_owned());
    alarms.into_iter().map(move |alarm| Raised {
        source: source.clone(),
        symbol: symbol.clone(),
        alarm,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptAlarmEntry {
    /// Increasing from 1 for the session; never reused.
    pub sequence: u64,
    /// When the window delivered it, UTC milliseconds.
    pub utc_ms: i64,
    pub raised: Raised,
}

impl ScriptAlarmEntry {
    /// What the toast says; a replay's alarm says so.
    #[must_use]
    pub fn notice(&self) -> String {
        let Raised {
            source,
            symbol,
            alarm,
        } = &self.raised;
        let alert = &alarm.alert;
        let mut notice = format!("{symbol} {}: {}", alarm.indicator, alert.title);
        if alert.message != alert.title {
            notice.push_str(" — ");
            notice.push_str(&alert.message);
        }
        if matches!(source, SourceContext::Replay { .. }) {
            notice.push_str(" (replay)");
        }
        notice
    }
}

#[derive(Debug, Default)]
pub struct ScriptAlarmLog {
    entries: VecDeque<ScriptAlarmEntry>,
    last_sequence: u64,
}

impl ScriptAlarmLog {
    /// Record `alarms`, in order, delivered at `utc_ms`; answers the new
    /// entries.
    pub fn record(
        &mut self,
        alarms: impl IntoIterator<Item = Raised>,
        utc_ms: i64,
    ) -> Vec<ScriptAlarmEntry> {
        let mut recorded = Vec::new();
        for raised in alarms {
            self.last_sequence += 1;
            let entry = ScriptAlarmEntry {
                sequence: self.last_sequence,
                utc_ms,
                raised,
            };
            if self.entries.len() == SCRIPT_ALARM_CAPACITY {
                self.entries.pop_front();
            }
            self.entries.push_back(entry.clone());
            recorded.push(entry);
        }
        recorded
    }

    /// The kept entries with a sequence above `after` (all with `None`),
    /// oldest first.
    pub fn after(&self, after: Option<u64>) -> impl Iterator<Item = &ScriptAlarmEntry> {
        let after = after.unwrap_or(0);
        self.entries
            .iter()
            .filter(move |entry| entry.sequence > after)
    }

    /// The sequence of the newest alarm ever recorded (0: none yet).
    #[must_use]
    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    /// The sequence of the oldest alarm still kept (0: none kept). A reader
    /// whose `after` is below this minus one has missed the alarms between.
    #[must_use]
    pub fn oldest_sequence(&self) -> u64 {
        self.entries.front().map_or(0, |entry| entry.sequence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alarm(title: &str) -> ScriptAlarm {
        ScriptAlarm {
            indicator: "Renko POC flip".to_owned(),
            bar_close_ms: 1,
            alert: FiredAlert {
                title: title.to_owned(),
                message: title.to_owned(),
            },
        }
    }

    fn live() -> SourceContext {
        SourceContext::Live {
            feed: "mt5".to_owned(),
        }
    }

    #[test]
    fn sequences_increase_and_the_oldest_leaves_first() {
        let mut log = ScriptAlarmLog::default();
        let first = log.record(label(&live(), "WINZ25", [alarm("a"), alarm("b")]), 10);
        assert_eq!(first.iter().map(|e| e.sequence).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(first[0].notice(), "WINZ25 Renko POC flip: a");
        let after: Vec<_> = log.after(Some(1)).map(|e| e.sequence).collect();
        assert_eq!(after, [2]);
        assert_eq!(log.oldest_sequence(), 1);

        let flood = (0..SCRIPT_ALARM_CAPACITY).map(|_| alarm("c"));
        log.record(label(&live(), "WINZ25", flood), 20);
        assert_eq!(log.after(None).count(), SCRIPT_ALARM_CAPACITY);
        assert_eq!(log.oldest_sequence(), 3);
        assert_eq!(log.last_sequence(), 2 + SCRIPT_ALARM_CAPACITY as u64);
    }

    #[test]
    fn the_notice_names_the_message_and_a_replay() {
        let mut log = ScriptAlarmLog::default();
        let mut flip = alarm("Bull flip");
        flip.alert.message = "POC above".to_owned();
        let replay = SourceContext::Replay {
            recording: "session.qtr".to_owned(),
        };
        let entry = log.record(label(&replay, "WINZ25", [flip]), 0).remove(0);
        assert_eq!(
            entry.notice(),
            "WINZ25 Renko POC flip: Bull flip — POC above (replay)"
        );
    }
}
