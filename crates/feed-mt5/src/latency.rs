//! Where a late tape spends its time.
//!
//! The chart has always been able to say *how* late a print was: subtract its
//! timestamp from the local clock and there is one number. That number cannot
//! be acted on. A tape eighteen seconds behind looks identical whether
//! MetaTrader handed the print over late or quantick read the socket late —
//! two faults with two different fixes, and the trader is told the same thing
//! in both cases.
//!
//! So the chain is cut where the bridge can stamp it. A tick carries `time_ms`
//! (when the venue's clock says it happened) and `sent_ms` (when the bridge
//! handed the line over), both on the server clock. With the reader's own
//! arrival instant that is the whole chain, in two exact halves:
//!
//! ```text
//! venue stamp -----------------> bridge sends -----------------> chart reads
//! |<-------- terminal_lag ------>|<------ transport_lag -------->|
//! |<--------------------- arrival_lag -------------------------->|
//! ```
//!
//! `arrival_lag = terminal_lag + transport_lag` exactly, because both halves
//! come from the same two stamps and one clock read.
//!
//! **Two halves and not three, deliberately.** An earlier version split
//! `terminal_lag` again — what the terminal cost against what the bridge's
//! pump cost — from a `cursor_lag_ms` the heartbeat carried. It could not be
//! made honest. A bridge has no cheap way to ask "what is the newest tick you
//! hold that I have not sent", and every approximation of it collapses to
//! *time since the last print*, which on a stall equals the delay itself and
//! blames the pump for everything. A figure that names the wrong hop is worse
//! than one that names none, so the pump reports its own health where it can
//! actually measure it — `BRIDGE_PUMP_LIMIT` and `BRIDGE_SEND_STALLED`
//! in the Experts tab — and this module reports only what it can subtract.
//!
//! Nothing here reads a clock. The tracker accumulates from arithmetic alone
//! and is handed a clock only when a sample is drawn, which is at most once
//! every [`SAMPLE_EVERY_PRINTS`] prints — so a busy tape pays no per-print
//! system call for being measurable, and every figure below is reproducible
//! from a fixture.

/// Prints one sample may cover before the tracker asks to be read.
///
/// Small enough that a burst is measured while it is still happening, large
/// enough that the clock read behind a sample is not a per-print cost. A thin
/// tape never reaches it and is sampled on the heartbeat instead, so a symbol
/// that prints once a minute still reports.
pub const SAMPLE_EVERY_PRINTS: u32 = 64;

/// Below this, a sample names no hop at all.
///
/// Every delay has a larger half, and reporting it on a tape that is four
/// milliseconds behind would put a culprit's name on noise — on the chart, and
/// worse, in `quantick_get_diagnostics`, where something that is not looking at
/// the screen would read it as a finding. The floor is deliberately far under
/// the thresholds either consumer acts on (`stream::LAG_REPORT_MS`, the app's
/// `metrics::HIGH_LAG_MS`), so it withholds nothing anyone would use.
pub const HOP_FLOOR_MS: i64 = 100;

/// Which hop owns most of the delay in a [`LatencySample`].
///
/// Deliberately one answer rather than a table: the reading a trader acts on is
/// "who is late", and a reader that has to compare four numbers to find out is
/// a reader that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyHop {
    /// The print was already late when the bridge handed it over: the terminal
    /// received it late, or the bridge's pump had not reached it yet. Nothing
    /// on this side of the socket can shorten either, and the Experts tab is
    /// where the two are told apart.
    Terminal,
    /// The bridge handed it over on time and quantick read it late — the
    /// socket, the decoder, or the consumer's own queue.
    Transport,
}

impl LatencyHop {
    /// Every hop this crate can name, for a caller that has to resolve one by
    /// name rather than produce one.
    ///
    /// A registry, not a convenience: the app's `QUANTICK_FAKE_LATENCY_SPLIT`
    /// hook resolves the word a validation script typed through this list, so a
    /// hop that exists is reachable by name and one that does not is refused
    /// rather than photographed. The same bargain `FootprintStyle::ALL` makes
    /// with the style hook.
    pub const ALL: [Self; 2] = [Self::Terminal, Self::Transport];

    /// Longest a [`label`](Self::label) may be, in characters.
    ///
    /// The width of the word `arrival`, and that is exactly where the number
    /// comes from. A consumer's readout has to put this word somewhere, and the
    /// one that does — quantick's status bar — puts it *in place of* `arrival`,
    /// on a row whose three sections share one width with no budget between
    /// them. Measured at 1000 px: a ten-character name overlapped the
    /// neighbouring cell outright, and even a single extra character grazed it.
    /// So the rule is not "short enough", it is **never wider than the word it
    /// replaces** — then no window can be made worse by the chart knowing more.
    /// Owned here and asserted on both sides of that boundary, so neither end
    /// can widen it alone.
    pub const MAX_LABEL_CHARS: usize = 7;

    /// The hop whose [`label`](Self::label) is `name`, if any.
    #[must_use]
    pub fn from_label(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|hop| hop.label() == name)
    }

    /// Short, fixed name for a readout. Stable enough to assert on.
    ///
    /// Held to [`MAX_LABEL_CHARS`](Self::MAX_LABEL_CHARS), which is why these
    /// are the short forms. `MT5` rather than `MetaTrader`: the feed is named
    /// MetaTrader everywhere the trader chose it, so the short form is
    /// unambiguous where it appears. `chart` rather than `quantick`: it is the
    /// word a trader uses for this side of the socket, and the hover says the
    /// precise thing — the queue and the drawing — for anyone who asks.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Terminal => "MT5",
            Self::Transport => "chart",
        }
    }
}

/// One reading of the chain, drawn from the prints seen since the last one.
///
/// Every field is milliseconds. `terminal_lag_ms` and `transport_lag_ms` are
/// `None` together: they both need the bridge's `sent_ms`, and a bridge that
/// predates it leaves the split unavailable rather than reporting a zero it did
/// not measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatencySample {
    /// Newest print: venue stamp to the reader taking it off the socket. The
    /// total the two halves below add up to.
    pub arrival_lag_ms: i64,
    /// Newest print: venue stamp to the bridge handing it over.
    pub terminal_lag_ms: Option<i64>,
    /// Worst `terminal_lag_ms` over the window.
    ///
    /// A true peak, and the only one here: it is two bridge stamps subtracted
    /// per print, so every print in the window contributes its own reading with
    /// no clock involved. The arrival and transport figures cannot have one —
    /// they need the reader's clock, which is read once per sample, and
    /// applying that one instant to a print that arrived earlier measures the
    /// print's *age*, not its delay. An earlier version did exactly that and
    /// reported the heartbeat interval as latency on every healthy slow tape.
    pub terminal_lag_peak_ms: Option<i64>,
    /// Newest print: bridge handing it over to the reader taking it off the
    /// socket.
    pub transport_lag_ms: Option<i64>,
    /// How many live prints this sample covers.
    pub prints: u32,
}

impl LatencySample {
    /// The hop that owns most of this sample's delay.
    ///
    /// `None` when there is nothing to report: an older bridge that sends no
    /// `sent_ms`, or a delay under [`HOP_FLOOR_MS`], where naming a culprit
    /// would be putting a name on noise.
    #[must_use]
    pub fn dominant(&self) -> Option<LatencyHop> {
        let terminal = self.terminal_lag_ms?;
        let transport = self.transport_lag_ms?;
        if self.arrival_lag_ms < HOP_FLOOR_MS {
            return None;
        }
        // Ties go to the terminal side. A tie means the two halves are equally
        // to blame, and naming the one this process controls would send the
        // trader to look at quantick for a delay MetaTrader shares.
        if transport > terminal {
            Some(LatencyHop::Transport)
        } else {
            Some(LatencyHop::Terminal)
        }
    }
}

/// Accumulates the chain from live prints.
///
/// Per print this does one subtraction and one comparison: no clock, no
/// allocation, nothing that grows with the tape. The clock is read once, by the
/// caller, when [`Self::sample`] is called.
#[derive(Debug, Clone, Default)]
pub struct LatencyTracker {
    /// Newest live print seen since the last sample.
    newest: Option<PrintStamps>,
    /// Worst `sent_ms - time_ms` over the window.
    terminal_peak_ms: Option<i64>,
    prints: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PrintStamps {
    /// Venue stamp, server time.
    time_ms: i64,
    /// When the bridge handed the line over, server time. `None` on a bridge
    /// that predates the field.
    sent_ms: Option<i64>,
}

/// `sent_ms - time_ms`, floored at zero.
///
/// Both stamps arrive from a bridge any local process on this machine can
/// impersonate, so the arithmetic saturates rather than overflowing. The floor
/// is not paranoia either: a bridge stamps a batch once and the terminal can
/// hand it a tick *during* that batch, so a stamp fractionally older than the
/// tick it carries is an ordinary event, not a hostile one. A negative here
/// would be handed on as an inflated transport figure and blame quantick for a
/// delay that never happened.
fn terminal_lag(stamps: PrintStamps) -> Option<i64> {
    let sent = stamps.sent_ms?;
    Some(sent.saturating_sub(stamps.time_ms).max(0))
}

impl LatencyTracker {
    /// A tracker with nothing observed yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one **live** print.
    ///
    /// Backfill and paged history are deliberately not observed: their stamps
    /// are true and their age is the age of the history, so measuring latency
    /// from them would report minutes of delay on a chart that is current.
    pub fn observe_live(&mut self, time_ms: i64, sent_ms: Option<i64>) {
        let stamps = PrintStamps { time_ms, sent_ms };
        self.newest = Some(stamps);
        if let Some(terminal) = terminal_lag(stamps) {
            self.terminal_peak_ms = Some(match self.terminal_peak_ms {
                Some(peak) if peak >= terminal => peak,
                _ => terminal,
            });
        }
        self.prints = self.prints.saturating_add(1);
    }

    /// Whether enough prints have arrived to be worth a clock read.
    #[must_use]
    pub fn due(&self) -> bool {
        self.prints >= SAMPLE_EVERY_PRINTS
    }

    /// Draw a sample and start a new window.
    ///
    /// `now_utc_ms` is the caller's clock — read once, here, and nowhere in the
    /// per-print path. It must be read where the newest print came off the
    /// wire, not after handing that print downstream: a consumer that is slow
    /// to take it would otherwise have its own queueing charged to the wire.
    /// `offset_ms` is `server_time - utc` in milliseconds, the same conversion
    /// the tick mapper applies.
    ///
    /// `None` when no live print has arrived since the last sample: there is
    /// nothing to measure, and reporting the previous window again would let a
    /// wedged socket show a healthy split forever.
    pub fn sample(&mut self, now_utc_ms: i64, offset_ms: i64) -> Option<LatencySample> {
        let newest = self.newest?;
        let arrival_lag_ms = now_utc_ms
            .saturating_sub(newest.time_ms.saturating_sub(offset_ms))
            .max(0);
        let terminal_lag_ms = terminal_lag(newest);
        // Derived, not measured a second time: transport is whatever the
        // arrival figure has left after the terminal's share, so the two always
        // add up to the total above. Floored for the same reason `terminal_lag`
        // is — a terminal share larger than the total is two clocks
        // disagreeing, never a print that arrived before it was sent.
        let transport_lag_ms =
            terminal_lag_ms.map(|terminal| arrival_lag_ms.saturating_sub(terminal).max(0));
        let sample = LatencySample {
            arrival_lag_ms,
            terminal_lag_ms,
            terminal_lag_peak_ms: self.terminal_peak_ms,
            transport_lag_ms,
            prints: self.prints,
        };
        self.newest = None;
        self.terminal_peak_ms = None;
        self.prints = 0;
        Some(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// B3: server = UTC-3, so a server stamp is three hours ahead of the UTC
    /// instant it names.
    const OFFSET_MS: i64 = -3 * 60 * 60 * 1000;

    /// A server-time instant, from a UTC one.
    fn server(utc_ms: i64) -> i64 {
        utc_ms + OFFSET_MS
    }

    #[test]
    fn every_hop_name_fits_the_cell_that_has_to_show_it() {
        // A consumer puts this word *in place of* `arrival`, so anything wider
        // pushes a neighbouring status-bar cell off a narrow window — a defect
        // that only shows up on someone's laptop. Measured: ten characters
        // overlapped outright, eight grazed.
        for hop in LatencyHop::ALL {
            let label = hop.label();
            assert!(
                label.chars().count() <= LatencyHop::MAX_LABEL_CHARS,
                "{label:?} is too long for the readout"
            );
        }
    }

    #[test]
    fn every_hop_is_reachable_by_the_name_it_reports() {
        // The hook that fakes a split resolves the word a script typed through
        // this list, so a name that round-trips is a state a capture can reach
        // and a typo is a refusal rather than a picture of the wrong hop.
        for hop in LatencyHop::ALL {
            assert_eq!(LatencyHop::from_label(hop.label()), Some(hop));
        }
        assert_eq!(LatencyHop::from_label("MT5 "), None, "names are exact");
        assert_eq!(LatencyHop::from_label(""), None);
    }

    #[test]
    fn the_split_adds_up_to_the_arrival_figure_the_status_bar_shows() {
        // The whole point of the split is that it is the same number, cut. If
        // the halves ever stopped summing to the total, the trader would be
        // reading two contradictory accounts of one delay.
        let mut tracker = LatencyTracker::new();
        tracker.observe_live(server(1_000), Some(server(1_400)));
        let s = tracker.sample(2_000, OFFSET_MS).unwrap();
        assert_eq!(s.arrival_lag_ms, 1_000);
        assert_eq!(s.terminal_lag_ms, Some(400));
        assert_eq!(s.transport_lag_ms, Some(600));
        assert_eq!(
            s.terminal_lag_ms.unwrap() + s.transport_lag_ms.unwrap(),
            s.arrival_lag_ms
        );
    }

    #[test]
    fn a_bridge_that_does_not_stamp_leaves_the_split_unavailable() {
        // Data honesty: an older bridge sends no `sent_ms`, and a zero here
        // would read as "the terminal is instant", which is a claim nobody
        // measured.
        let mut tracker = LatencyTracker::new();
        tracker.observe_live(server(1_000), None);
        let s = tracker.sample(2_000, OFFSET_MS).unwrap();
        assert_eq!(s.arrival_lag_ms, 1_000);
        assert_eq!(s.terminal_lag_ms, None);
        assert_eq!(s.transport_lag_ms, None);
        assert_eq!(s.dominant(), None);
    }

    #[test]
    fn a_slow_tape_is_not_reported_late_for_being_slow() {
        // The regression this shape exists to prevent. A window is the prints
        // since the last sample, and on a thin tape sampled by heartbeat those
        // span seconds. Measuring the oldest of them against the sample's own
        // clock reports how *old* it is, not how late it was — an earlier
        // version did that and fired a warn on every healthy quiet symbol,
        // edge-triggered, so the matching recovery could never follow.
        let mut tracker = LatencyTracker::new();
        for i in 0..5 {
            // One print a second, each handed over 20 ms after it happened.
            let at = i * 1_000;
            tracker.observe_live(server(at), Some(server(at + 20)));
        }
        // The sample is drawn five seconds after the first print.
        let s = tracker.sample(4_030, OFFSET_MS).unwrap();
        assert_eq!(s.prints, 5);
        assert_eq!(s.arrival_lag_ms, 30, "the newest print is 30 ms behind");
        assert_eq!(s.terminal_lag_peak_ms, Some(20), "and none took longer");
        assert!(
            s.arrival_lag_ms < 1_000,
            "nothing here may report the sampling interval as delay"
        );
    }

    #[test]
    fn the_worst_print_in_the_window_is_reported_from_the_bridge_stamps_alone() {
        // The one true peak: both stamps come from the bridge, per print, so
        // every print contributes without a clock being read.
        let mut tracker = LatencyTracker::new();
        tracker.observe_live(server(0), Some(server(700)));
        tracker.observe_live(server(1_000), Some(server(1_020)));
        let s = tracker.sample(1_030, OFFSET_MS).unwrap();
        assert_eq!(s.terminal_lag_ms, Some(20), "the newest print");
        assert_eq!(s.terminal_lag_peak_ms, Some(700), "the worst of the two");
    }

    #[test]
    fn a_blocked_socket_names_quantick_and_a_slow_handover_names_mt5() {
        let mut wire = LatencyTracker::new();
        wire.observe_live(server(0), Some(server(5)));
        let s = wire.sample(4_000, OFFSET_MS).unwrap();
        assert_eq!(s.dominant(), Some(LatencyHop::Transport));
        assert_eq!(s.dominant().unwrap().label(), "chart");

        let mut terminal = LatencyTracker::new();
        terminal.observe_live(server(0), Some(server(4_000)));
        let s = terminal.sample(4_010, OFFSET_MS).unwrap();
        assert_eq!(s.dominant(), Some(LatencyHop::Terminal));
        assert_eq!(s.dominant().unwrap().label(), "MT5");
    }

    #[test]
    fn a_healthy_tape_names_nobody() {
        // Every delay has a larger half. On a four-millisecond tape, saying
        // which one would put a culprit's name on noise — on the chart, and in
        // the diagnostics an agent reads as a finding.
        let mut tracker = LatencyTracker::new();
        tracker.observe_live(server(0), Some(server(2)));
        let s = tracker.sample(4, OFFSET_MS).unwrap();
        assert_eq!(s.arrival_lag_ms, 4);
        assert_eq!(s.dominant(), None);
        assert_eq!(
            s.terminal_lag_ms,
            Some(2),
            "still measured, just not blamed"
        );
    }

    #[test]
    fn a_window_with_no_print_reports_nothing_at_all() {
        // A wedged socket must not be able to keep showing the last healthy
        // split: the readout has to go stale with the tape it describes.
        let mut tracker = LatencyTracker::new();
        assert!(tracker.sample(1_000, OFFSET_MS).is_none());
        tracker.observe_live(server(0), Some(server(1)));
        assert!(tracker.sample(1_000, OFFSET_MS).is_some());
        assert!(
            tracker.sample(9_000, OFFSET_MS).is_none(),
            "the window emptied with the sample"
        );
    }

    #[test]
    fn a_sample_is_due_only_after_a_bounded_number_of_prints() {
        // The bound is what keeps the clock read off the per-print path.
        let mut tracker = LatencyTracker::new();
        for _ in 0..SAMPLE_EVERY_PRINTS - 1 {
            tracker.observe_live(server(0), Some(server(1)));
            assert!(!tracker.due());
        }
        tracker.observe_live(server(0), Some(server(1)));
        assert!(tracker.due());
        tracker.sample(1_000, OFFSET_MS).unwrap();
        assert!(!tracker.due(), "the sample starts a new window");
    }

    #[test]
    fn a_crafted_stamp_cannot_overflow_or_read_backwards() {
        // Any local process can impersonate the bridge on the loopback port, so
        // these two subtractions are the ones a hostile line reaches. Nothing
        // here may panic in debug or wrap in release, and no reading may come
        // out negative — a negative terminal share inflates the transport share
        // and blames quantick for a delay that never happened.
        let mut tracker = LatencyTracker::new();
        tracker.observe_live(0, Some(i64::MIN));
        let s = tracker.sample(1_000, OFFSET_MS).unwrap();
        assert_eq!(s.terminal_lag_ms, Some(0));
        assert!(s.transport_lag_ms.unwrap() >= 0);

        let mut ahead = LatencyTracker::new();
        ahead.observe_live(i64::MIN, Some(i64::MAX));
        let s = ahead.sample(i64::MAX, i64::MIN).unwrap();
        assert!(s.arrival_lag_ms >= 0);
        assert!(s.transport_lag_ms.unwrap() >= 0);

        // The ordinary case, not a hostile one: a bridge stamps a batch once
        // and the terminal hands it a tick during that batch, so the stamp is
        // fractionally older than the tick it carries.
        let mut during = LatencyTracker::new();
        during.observe_live(server(500), Some(server(480)));
        let s = during.sample(600, OFFSET_MS).unwrap();
        assert_eq!(s.terminal_lag_ms, Some(0), "floored, never negative");
        assert_eq!(s.transport_lag_ms, Some(100));
    }
}
