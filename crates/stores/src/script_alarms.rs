//! The alarms indicator scripts raised this session, newest kept, for the
//! sound/toast delivery and the `alerts.script.history` read.
//!
//! Session-scoped and never persisted: a script alarm is a fact about a bar
//! that closed while the app watched, and it is raised again only by a bar
//! that closes again.
/// What the log records, re-exported so a reader needs no indicator crate.
pub use quantick_indicators::{FiredAlert, ScriptAlarm};
use std::collections::VecDeque;

/// How many alarms the log keeps; the oldest goes first.
pub const SCRIPT_ALARM_CAPACITY: usize = 256;

/// One recorded alarm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptAlarmEntry {
    /// Increasing from 1 for the session; never reused.
    pub sequence: u64,
    /// When the window delivered it, UTC milliseconds.
    pub utc_ms: i64,
    /// The market of the tab whose chart raised it.
    pub symbol: String,
    pub alarm: ScriptAlarm,
}

impl ScriptAlarmEntry {
    /// What the toast says.
    #[must_use]
    pub fn notice(&self) -> String {
        let alert = &self.alarm.alert;
        if alert.message == alert.title {
            format!("{} {}: {}", self.symbol, self.alarm.indicator, alert.title)
        } else {
            format!(
                "{} {}: {} — {}",
                self.symbol, self.alarm.indicator, alert.title, alert.message
            )
        }
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
        alarms: impl IntoIterator<Item = (String, ScriptAlarm)>,
        utc_ms: i64,
    ) -> Vec<ScriptAlarmEntry> {
        let mut recorded = Vec::new();
        for (symbol, alarm) in alarms {
            self.last_sequence += 1;
            let entry = ScriptAlarmEntry {
                sequence: self.last_sequence,
                utc_ms,
                symbol,
                alarm,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alarm(title: &str) -> (String, ScriptAlarm) {
        let alert = FiredAlert {
            title: title.to_owned(),
            message: title.to_owned(),
        };
        let alarm = ScriptAlarm {
            indicator: "Renko POC flip".to_owned(),
            bar_close_ms: 1,
            alert,
        };
        ("WINZ25".to_owned(), alarm)
    }

    #[test]
    fn sequences_increase_and_the_oldest_leaves_first() {
        let mut log = ScriptAlarmLog::default();
        let first = log.record([alarm("a"), alarm("b")], 10);
        assert_eq!(first.iter().map(|e| e.sequence).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(first[0].notice(), "WINZ25 Renko POC flip: a");
        let after: Vec<_> = log.after(Some(1)).map(|e| e.sequence).collect();
        assert_eq!(after, [2]);

        log.record((0..SCRIPT_ALARM_CAPACITY).map(|_| alarm("c")), 20);
        assert_eq!(log.after(None).count(), SCRIPT_ALARM_CAPACITY);
        assert_eq!(log.after(None).next().map(|e| e.sequence), Some(3));
        assert_eq!(log.last_sequence(), 2 + SCRIPT_ALARM_CAPACITY as u64);
    }

    #[test]
    fn a_message_beside_the_title_is_shown() {
        let mut log = ScriptAlarmLog::default();
        let (symbol, mut alarm) = alarm("Bull flip");
        alarm.alert.message = "POC above".to_owned();
        let entry = log.record([(symbol, alarm)], 0).remove(0);
        assert_eq!(
            entry.notice(),
            "WINZ25 Renko POC flip: Bull flip — POC above"
        );
    }
}
