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

/// An alarm labelled with the market whose bar raised it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The live feed, or the recording a replay plays.
    pub source: SourceContext,
    pub symbol: String,
    pub alarm: ScriptAlarm,
}

/// Where a tab's bars close, borrowed so a print can be tagged without
/// allocating.
#[derive(Debug, Clone, Copy)]
pub enum SourceRef<'a> {
    Live(&'a str),
    Replay(&'a std::path::Path),
}

impl SourceRef<'_> {
    #[must_use]
    pub fn to_context(self) -> SourceContext {
        match self {
            Self::Live(feed) => SourceContext::Live {
                feed: feed.to_owned(),
            },
            Self::Replay(path) => SourceContext::Replay {
                recording: path.to_string_lossy().into_owned(),
            },
        }
    }

    fn is(self, source: &SourceContext) -> bool {
        match (self, source) {
            (Self::Live(feed), SourceContext::Live { feed: kept }) => feed == kept,
            (Self::Replay(path), SourceContext::Replay { recording }) => {
                path.to_string_lossy() == recording.as_str()
            }
            _ => false,
        }
    }
}

/// How many market labels a tab remembers: alarms arrive from the indicator
/// worker a frame or so after their bar closed, so only the last few
/// markets can still have alarms in flight.
pub const LABELS_KEPT: usize = 16;

/// A tab's market labels by tag. Each closed bar is sent to the indicators
/// with the tag of the market it closed on, and each alarm echoes it, so a
/// market switch between the close and the alarm's arrival cannot relabel
/// the alarm.
#[derive(Debug, Default)]
pub struct AlarmLabels {
    labels: VecDeque<(u64, SourceContext, String)>,
    last_tag: u64,
}

impl AlarmLabels {
    /// The tag for bars closing on `source` and `symbol` now; a new one
    /// only when the market differs from the last tagged.
    pub fn tag(&mut self, source: SourceRef<'_>, symbol: &str) -> u64 {
        if let Some((tag, kept, kept_symbol)) = self.labels.back()
            && source.is(kept)
            && kept_symbol == symbol
        {
            return *tag;
        }
        self.last_tag += 1;
        if self.labels.len() == LABELS_KEPT {
            self.labels.pop_front();
        }
        let label = (self.last_tag, source.to_context(), symbol.to_owned());
        self.labels.push_back(label);
        self.last_tag
    }

    /// `alarm` under the market its tag names; `None` for a tag this tab
    /// never handed out or has forgotten.
    #[must_use]
    pub fn raise(&self, alarm: ScriptAlarm) -> Option<Raised> {
        let (_, source, symbol) = self.labels.iter().find(|(tag, ..)| *tag == alarm.tag)?;
        Some(Raised {
            source: source.clone(),
            symbol: symbol.clone(),
            alarm,
        })
    }
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
    use std::path::Path;

    fn alarm(tag: u64, title: &str) -> ScriptAlarm {
        ScriptAlarm {
            tag,
            indicator: "Renko POC flip".to_owned(),
            bar_close_ms: 1,
            alert: FiredAlert {
                title: title.to_owned(),
                message: title.to_owned(),
            },
        }
    }

    #[test]
    fn an_alarm_keeps_the_market_its_bar_closed_on() {
        let mut labels = AlarmLabels::default();
        let win = labels.tag(SourceRef::Live("mt5"), "WINZ25");
        assert_eq!(
            labels.tag(SourceRef::Live("mt5"), "WINZ25"),
            win,
            "same market"
        );
        let wdo = labels.tag(SourceRef::Live("mt5"), "WDOZ25");
        let replay = labels.tag(SourceRef::Replay(Path::new("s.qtr")), "WINV25");
        assert_eq!(
            labels.tag(SourceRef::Replay(Path::new("s.qtr")), "WINV25"),
            replay
        );
        assert_ne!(win, wdo);

        // Raised on WINZ25 but arriving after two switches.
        let raised = labels.raise(alarm(win, "a")).expect("a recent tag");
        assert_eq!(raised.symbol, "WINZ25");
        assert_eq!(raised.source, SourceRef::Live("mt5").to_context());
        let raised = labels.raise(alarm(replay, "b")).expect("a recent tag");
        assert_eq!(
            (raised.symbol.as_str(), raised.source),
            (
                "WINV25",
                SourceContext::Replay {
                    recording: "s.qtr".to_owned()
                }
            )
        );
        assert_eq!(labels.raise(alarm(99, "c")), None, "an unknown tag");
    }

    #[test]
    fn only_the_last_markets_are_remembered() {
        let mut labels = AlarmLabels::default();
        let first = labels.tag(SourceRef::Live("mt5"), "S0");
        for i in 1..=LABELS_KEPT {
            labels.tag(SourceRef::Live("mt5"), &format!("S{i}"));
        }
        assert_eq!(labels.raise(alarm(first, "a")), None);
    }

    fn entry(
        log: &mut ScriptAlarmLog,
        source: SourceContext,
        alarm: ScriptAlarm,
    ) -> ScriptAlarmEntry {
        let raised = Raised {
            source,
            symbol: "WINZ25".to_owned(),
            alarm,
        };
        log.record([raised], 0).remove(0)
    }

    #[test]
    fn sequences_increase_and_the_oldest_leaves_first() {
        let mut log = ScriptAlarmLog::default();
        let live = SourceRef::Live("mt5").to_context();
        let first = entry(&mut log, live.clone(), alarm(1, "a"));
        entry(&mut log, live.clone(), alarm(1, "b"));
        assert_eq!(first.sequence, 1);
        assert_eq!(first.notice(), "WINZ25 Renko POC flip: a");
        let after: Vec<_> = log.after(Some(1)).map(|e| e.sequence).collect();
        assert_eq!(after, [2]);
        assert_eq!(log.oldest_sequence(), 1);

        for _ in 0..SCRIPT_ALARM_CAPACITY {
            entry(&mut log, live.clone(), alarm(1, "c"));
        }
        assert_eq!(log.after(None).count(), SCRIPT_ALARM_CAPACITY);
        assert_eq!(log.oldest_sequence(), 3);
        assert_eq!(log.last_sequence(), 2 + SCRIPT_ALARM_CAPACITY as u64);
    }

    #[test]
    fn the_notice_names_the_message_and_a_replay() {
        let mut log = ScriptAlarmLog::default();
        let mut flip = alarm(1, "Bull flip");
        flip.alert.message = "POC above".to_owned();
        let replay = SourceRef::Replay(Path::new("session.qtr")).to_context();
        assert_eq!(
            entry(&mut log, replay, flip).notice(),
            "WINZ25 Renko POC flip: Bull flip — POC above (replay)"
        );
    }
}
