//! The alarms indicator scripts raised this session, newest kept, for the
//! sound/toast delivery and the `alerts.script.history` read.
//!
//! Session-scoped and never persisted: a script alarm is a fact about a bar
//! that closed while the app watched, and it is raised again only by a bar
//! that closes again.
use quantick_alerts::SourceContext;
use quantick_engine::bar_registry::BarConfiguration;
/// The alarm a [`Raised`] carries, for readers the dependency graph keeps
/// off the indicator crate (control-handlers).
pub use quantick_indicators::{FiredAlert, ScriptAlarm};
use std::collections::VecDeque;

/// How many alarms the log keeps; the oldest goes first.
pub const SCRIPT_ALARM_CAPACITY: usize = 256;

/// An alarm labelled with the market and the chart whose bar raised it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raised {
    /// The live feed, or the recording a replay plays.
    pub source: SourceContext,
    pub symbol: String,
    /// The chart's bars in a few words: `Renko 6`, `1m`.
    pub chart: String,
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

/// How many market and chart labels a tab remembers, least recently used
/// out first: alarms arrive from the indicator worker a frame or so after
/// their bar closed, so only the last few can still have alarms in flight.
pub const LABELS_KEPT: usize = 16;

#[derive(Debug)]
struct Label {
    tag: u64,
    source: SourceContext,
    symbol: String,
    chart: BarConfiguration,
}

/// A tab's market and chart labels by tag. Each closed bar is sent to the
/// indicators with the tag of the market and chart it closed on, and each
/// alarm echoes it, so a market switch or a bar change between the close
/// and the alarm's arrival cannot relabel the alarm.
#[derive(Debug, Default)]
pub struct AlarmLabels {
    labels: VecDeque<Label>,
    last_tag: u64,
}

impl AlarmLabels {
    /// The tag for bars `chart` closes on `source` and `symbol` now; a new
    /// one only for a label not already kept.
    pub fn tag(&mut self, source: SourceRef<'_>, symbol: &str, chart: BarConfiguration) -> u64 {
        let kept = self.labels.iter().rposition(|label| {
            label.chart == chart && label.symbol == symbol && source.is(&label.source)
        });
        if let Some(at) = kept {
            // Panes tag print by print in turn; keep every live one recent.
            if at + 1 != self.labels.len() {
                let label = self.labels.remove(at).expect("a kept position");
                self.labels.push_back(label);
            }
            return self.labels.back().expect("just kept").tag;
        }
        self.last_tag += 1;
        if self.labels.len() == LABELS_KEPT {
            self.labels.pop_front();
        }
        self.labels.push_back(Label {
            tag: self.last_tag,
            source: source.to_context(),
            symbol: symbol.to_owned(),
            chart,
        });
        self.last_tag
    }

    /// Tags for the charts of a print landing on `source` and `symbol`.
    pub fn on<'a>(&'a mut self, source: SourceRef<'a>, symbol: &'a str) -> Tagger<'a> {
        Tagger {
            labels: self,
            source,
            symbol,
        }
    }

    /// `alarm` under the market and chart its tag names; `None` for a tag
    /// this tab never handed out or has forgotten.
    #[must_use]
    pub fn raise(&self, alarm: ScriptAlarm) -> Option<Raised> {
        let label = self.labels.iter().find(|label| label.tag == alarm.tag)?;
        Some(Raised {
            source: label.source.clone(),
            symbol: label.symbol.clone(),
            chart: label.chart.caption(),
            alarm,
        })
    }
}

/// A tab's labels bound to one print's market, so each chart the print
/// lands on asks for its tag with its own bars only.
pub struct Tagger<'a> {
    labels: &'a mut AlarmLabels,
    source: SourceRef<'a>,
    symbol: &'a str,
}

impl Tagger<'_> {
    /// The tag for the bars `chart` closes on this print's market.
    pub fn tag(&mut self, chart: BarConfiguration) -> u64 {
        self.labels.tag(self.source, self.symbol, chart)
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
            chart,
            alarm,
        } = &self.raised;
        let alert = &alarm.alert;
        let mut notice = format!("{symbol} {chart} · {}: {}", alarm.indicator, alert.title);
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

/// The shortest gap between two script-alarm sounds, across every tab: a
/// fast replay or a burst of panes would otherwise ring on every frame.
pub const RING_GAP_MS: u64 = 3_000;

/// When script alarms may sound. The first alarm after a quiet gap rings at
/// once; alarms within [`RING_GAP_MS`] of a ring wait, and once the gap has
/// passed one ring names the newest with `(+N)` for the others. The gate
/// decides the sound and the toast only; every alarm is still recorded and
/// journaled when it arrives. Told the time, never reads it.
#[derive(Debug, Default)]
struct RingGate {
    /// When the last ring sounded, on the caller's monotonic clock.
    last_ring_ms: Option<u64>,
    /// The newest alarm waiting to ring, and how many wait with it.
    held: Option<(ScriptAlarmEntry, usize)>,
}

impl RingGate {
    fn hold(&mut self, entry: &ScriptAlarmEntry) {
        let waiting = self.held.take().map_or(0, |(_, count)| count);
        self.held = Some((entry.clone(), waiting + 1));
    }

    fn ring(&mut self, now_ms: u64) -> Option<String> {
        let quiet = self
            .last_ring_ms
            .is_none_or(|last| now_ms.saturating_sub(last) >= RING_GAP_MS);
        if !quiet {
            return None;
        }
        let (newest, count) = self.held.take()?;
        self.last_ring_ms = Some(now_ms);
        let mut notice = newest.notice();
        if count > 1 {
            notice.push_str(&format!(" (+{})", count - 1));
        }
        Some(notice)
    }
}

#[derive(Debug, Default)]
pub struct ScriptAlarmLog {
    entries: VecDeque<ScriptAlarmEntry>,
    last_sequence: u64,
    gate: RingGate,
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
            self.gate.hold(&entry);
            recorded.push(entry);
        }
        recorded
    }

    /// The toast to show with one alarm sound now, if the alarms recorded
    /// since the last ring may sound at `now_ms` (a monotonic clock, in
    /// milliseconds). Ask every frame: a burst's last alarms ring once the
    /// gap has passed, with no new alarm to prompt them.
    pub fn ring(&mut self, now_ms: u64) -> Option<String> {
        self.gate.ring(now_ms)
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
    use quantick_engine::bar_registry::BUILTIN_BARS;
    use std::path::Path;

    fn bars(config: &str) -> BarConfiguration {
        BUILTIN_BARS.parse(config).expect("a built-in bar")
    }

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
    fn an_alarm_keeps_the_market_and_chart_its_bar_closed_on() {
        let mut labels = AlarmLabels::default();
        let (renko, minute) = (bars("renko:6"), bars("time:1m"));
        let live = SourceRef::Live("mt5");
        let win = labels.tag(live, "WINZ25", renko);
        let win_minute = labels.tag(live, "WINZ25", minute);
        assert_ne!(win, win_minute, "each chart has its own label");
        assert_eq!(labels.tag(live, "WINZ25", renko), win, "same chart again");
        let wdo = labels.tag(live, "WDOZ25", renko);
        let recording = SourceRef::Replay(Path::new("s.qtr"));
        let replay = labels.tag(recording, "WINV25", renko);
        assert_eq!(labels.tag(recording, "WINV25", renko), replay);
        assert_ne!(win, wdo);

        // Raised on WINZ25's renko chart but arriving after two switches.
        let raised = labels.raise(alarm(win, "a")).expect("a recent tag");
        assert_eq!(raised.symbol, "WINZ25");
        assert_eq!(raised.chart, "Renko 6");
        assert_eq!(raised.source, live.to_context());
        let raised = labels.raise(alarm(win_minute, "a")).expect("a recent tag");
        assert_eq!(raised.chart, "1m");
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
    fn only_the_last_used_labels_are_remembered() {
        let mut labels = AlarmLabels::default();
        let live = SourceRef::Live("mt5");
        let (first, kept) = (bars("renko:6"), bars("time:1m"));
        let forgotten = labels.tag(live, "S0", first);
        let in_use = labels.tag(live, "S0", kept);
        for i in 1..LABELS_KEPT {
            labels.tag(live, &format!("S{i}"), first);
            // A pane still printing stays recent however many others came.
            assert_eq!(labels.tag(live, "S0", kept), in_use);
        }
        assert_eq!(labels.raise(alarm(forgotten, "a")), None);
        assert!(labels.raise(alarm(in_use, "a")).is_some());
    }

    fn raised(title: &str) -> Raised {
        Raised {
            source: SourceRef::Live("mt5").to_context(),
            symbol: "WINZ25".to_owned(),
            chart: "Renko 6".to_owned(),
            alarm: alarm(1, title),
        }
    }

    fn entry(log: &mut ScriptAlarmLog, raised: Raised) -> ScriptAlarmEntry {
        log.record([raised], 0).remove(0)
    }

    #[test]
    fn sequences_increase_and_the_oldest_leaves_first() {
        let mut log = ScriptAlarmLog::default();
        let first = entry(&mut log, raised("a"));
        entry(&mut log, raised("b"));
        assert_eq!(first.sequence, 1);
        assert_eq!(first.notice(), "WINZ25 Renko 6 · Renko POC flip: a");
        let after: Vec<_> = log.after(Some(1)).map(|e| e.sequence).collect();
        assert_eq!(after, [2]);
        assert_eq!(log.oldest_sequence(), 1);

        for _ in 0..SCRIPT_ALARM_CAPACITY {
            entry(&mut log, raised("c"));
        }
        assert_eq!(log.after(None).count(), SCRIPT_ALARM_CAPACITY);
        assert_eq!(log.oldest_sequence(), 3);
        assert_eq!(log.last_sequence(), 2 + SCRIPT_ALARM_CAPACITY as u64);
    }

    #[test]
    fn the_notice_names_the_chart_the_message_and_a_replay() {
        let mut log = ScriptAlarmLog::default();
        let mut flip = raised("Bull flip");
        flip.alarm.alert.message = "POC above".to_owned();
        flip.source = SourceRef::Replay(Path::new("session.qtr")).to_context();
        assert_eq!(
            entry(&mut log, flip).notice(),
            "WINZ25 Renko 6 · Renko POC flip: Bull flip — POC above (replay)"
        );
    }

    #[test]
    fn the_first_alarm_rings_at_once_and_a_burst_waits_out_the_gap() {
        let mut log = ScriptAlarmLog::default();
        assert_eq!(log.ring(0), None, "nothing to ring");

        entry(&mut log, raised("a"));
        let lead = "WINZ25 Renko 6 · Renko POC flip: a";
        assert_eq!(
            log.ring(10).as_deref(),
            Some(lead),
            "the first rings at once"
        );
        assert_eq!(log.ring(11), None, "and only once");

        // Three more inside the gap: silent, yet all recorded.
        for (title, at) in [("b", 500), ("c", 1_200), ("d", 2_000)] {
            entry(&mut log, raised(title));
            assert_eq!(log.ring(at), None, "{title} waits");
        }
        assert_eq!(log.after(None).count(), 4, "every alarm is kept");
        assert_eq!(log.ring(10 + RING_GAP_MS - 1), None, "the gap is not over");

        // The gap passes with no new alarm: one ring names the newest.
        let fold = "WINZ25 Renko 6 · Renko POC flip: d (+2)";
        assert_eq!(log.ring(10 + RING_GAP_MS).as_deref(), Some(fold));
        assert_eq!(log.ring(10 + 2 * RING_GAP_MS), None, "nothing left to ring");

        // After a quiet gap the next alarm rings at once again.
        entry(&mut log, raised("e"));
        let next = log.ring(10 + 2 * RING_GAP_MS + 1);
        assert_eq!(next.as_deref(), Some("WINZ25 Renko 6 · Renko POC flip: e"));
    }

    #[test]
    fn alarms_landing_in_one_frame_ring_once_naming_the_newest() {
        let mut log = ScriptAlarmLog::default();
        // Two panes close bars on the same print: one delivery, one ring.
        log.record([raised("Bull flip"), raised("Bear flip")], 0);
        let ring = log.ring(0);
        let fold = "WINZ25 Renko 6 · Renko POC flip: Bear flip (+1)";
        assert_eq!(ring.as_deref(), Some(fold));
        assert_eq!(log.ring(RING_GAP_MS), None, "both were told");
    }
}
