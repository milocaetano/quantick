//! Where a late tape spends its time.
//!
//! The chart has always been able to say *how* late a print was: subtract its
//! timestamp from the local clock and there is one number. That number cannot
//! be acted on. A tape eighteen seconds behind looks identical whether the
//! terminal received the print late, the bridge's pump is trailing the
//! terminal, or quantick read the socket late — three faults with three
//! different fixes, and the trader is told the same thing in every case.
//!
//! So the chain is cut where the bridge can stamp it. A tick carries `time_ms`
//! (when the venue's clock says it happened) and `sent_ms` (when the bridge
//! handed the line over), both on the server clock, and the heartbeat carries
//! `cursor_lag_ms` (how far the pump trails the newest tick the terminal
//! holds). Those three, plus the reader's own arrival instant, account for the
//! whole chain:
//!
//! ```text
//! venue stamp --upstream--> terminal --bridge--> wire --transport--> chart
//! |                                  |                              |
//! |<--------- terminal_lag ---------->|<------ transport_lag ------->|
//! |<------------------------ arrival_lag ---------------------------->|
//! ```
//!
//! `arrival_lag = terminal_lag + transport_lag` exactly, because both sides
//! come from the same two stamps and one clock read. `cursor_lag` splits
//! `terminal_lag` again — into what the terminal cost and what the pump cost —
//! and that split is an estimate, because the two are sampled at different
//! instants. It is labelled as one everywhere it is used.
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

/// Which hop owns most of the delay in a [`LatencySample`].
///
/// Deliberately one answer rather than a table: the reading a trader acts on
/// is "who is late", and a reader that has to compare four numbers to find out
/// is a reader that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencyHop {
    /// The terminal itself received the print late — nothing on this side of
    /// the socket can shorten it. Reported only when the bridge told us how far
    /// its own pump trails, so this hop is what is left after that.
    Upstream,
    /// The bridge's pump trails the ticks the terminal already holds.
    Bridge,
    /// Somewhere inside MetaTrader, with no `cursor_lag_ms` to say which half:
    /// an older bridge that does not report it, or a session that has not sent
    /// a heartbeat yet.
    Terminal,
    /// The line left the bridge on time and quantick read it late — the socket,
    /// the decoder, or the chart's own drain.
    Transport,
}

impl LatencyHop {
    /// Short, fixed name for a readout. Stable enough to assert on.
    ///
    /// Kept to eight characters. A consumer's readout has to put this word
    /// somewhere, and the one that does — quantick's status bar — swaps it for
    /// the word `arrival`, so a name longer than that is a name that pushes a
    /// neighbouring cell off a narrow window. `MT5` rather than `MetaTrader`
    /// for exactly that reason: the feed is named MetaTrader everywhere the
    /// trader chose it, so the short form is unambiguous where it appears.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Upstream => "terminal",
            Self::Bridge => "bridge",
            Self::Terminal => "MT5",
            Self::Transport => "quantick",
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
    /// Newest print: venue stamp to this chart. The figure the status bar has
    /// always shown, kept so the split can be checked against it.
    pub arrival_lag_ms: i64,
    /// Oldest print in this window: the same measurement for whichever print
    /// waited longest. A burst that arrived in one read has one arrival instant
    /// and many send instants, so this is a true peak, not an estimate.
    pub arrival_lag_peak_ms: i64,
    /// Newest print: venue stamp to the bridge handing it over.
    pub terminal_lag_ms: Option<i64>,
    /// Worst `terminal_lag_ms` over the window.
    pub terminal_lag_peak_ms: Option<i64>,
    /// Newest print: bridge handing it over to this chart reading it.
    pub transport_lag_ms: Option<i64>,
    /// Worst `transport_lag_ms` over the window, from the oldest print in it.
    pub transport_lag_peak_ms: Option<i64>,
    /// Last `cursor_lag_ms` the bridge reported on a heartbeat: how far its
    /// pump trails the newest tick the terminal holds.
    pub cursor_lag_ms: Option<i64>,
    /// How many live prints this sample covers.
    pub prints: u32,
}

impl LatencySample {
    /// What the terminal cost before the bridge could see the print.
    ///
    /// **An estimate.** `terminal_lag_ms` is measured on one print and
    /// `cursor_lag_ms` on the last heartbeat, so subtracting one from the other
    /// compares two instants up to a heartbeat apart. It is the right estimate
    /// to make — the alternative is not splitting the terminal at all — and it
    /// is clamped at zero, because a negative reading here means the two
    /// samples disagreed, never that a print arrived before it happened.
    #[must_use]
    pub fn upstream_lag_ms(&self) -> Option<i64> {
        let terminal = self.terminal_lag_ms?;
        let cursor = self.cursor_lag_ms?;
        Some((terminal - cursor).max(0))
    }

    /// What the bridge's pump cost, bounded by the terminal figure it is part
    /// of. Same estimate, same caveat, as [`Self::upstream_lag_ms`].
    #[must_use]
    pub fn bridge_lag_ms(&self) -> Option<i64> {
        let terminal = self.terminal_lag_ms?;
        let cursor = self.cursor_lag_ms?;
        Some(cursor.clamp(0, terminal.max(0)))
    }

    /// The hop that owns most of this sample's delay.
    ///
    /// `None` when there is no split to report — an older bridge, or a session
    /// that has not delivered a live print yet.
    #[must_use]
    pub fn dominant(&self) -> Option<LatencyHop> {
        let terminal = self.terminal_lag_ms?;
        let transport = self.transport_lag_ms?;
        // Ties go to the terminal side. A tie means the two halves are equally
        // to blame, and naming the one this process controls would send the
        // trader to look at quantick for a delay MetaTrader shares.
        if transport > terminal {
            return Some(LatencyHop::Transport);
        }
        match (self.upstream_lag_ms(), self.bridge_lag_ms()) {
            (Some(upstream), Some(bridge)) if bridge > upstream => Some(LatencyHop::Bridge),
            (Some(_), Some(_)) => Some(LatencyHop::Upstream),
            // No heartbeat has split the terminal yet: say "MetaTrader" rather
            // than guess which half of it.
            _ => Some(LatencyHop::Terminal),
        }
    }
}

/// Accumulates the chain from live prints and heartbeats.
///
/// Per print this does two subtractions and two comparisons: no clock, no
/// allocation, nothing that grows with the tape. The clock is read once, by the
/// caller, when [`Self::sample`] is called.
#[derive(Debug, Clone, Default)]
pub struct LatencyTracker {
    /// Newest live print seen since the last sample.
    newest: Option<PrintStamps>,
    /// Oldest live print seen since the last sample. Its send stamp is the
    /// earliest, so at one arrival instant it carries the window's worst wait.
    oldest: Option<PrintStamps>,
    /// Worst `sent_ms - time_ms` over the window.
    terminal_peak_ms: Option<i64>,
    /// Last `cursor_lag_ms` a heartbeat reported. Survives a sample: it
    /// describes the pump, not the window.
    cursor_lag_ms: Option<i64>,
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
        self.oldest.get_or_insert(stamps);
        self.newest = Some(stamps);
        if let Some(sent) = sent_ms {
            let terminal = sent.saturating_sub(time_ms);
            self.terminal_peak_ms = Some(match self.terminal_peak_ms {
                Some(peak) if peak >= terminal => peak,
                _ => terminal,
            });
        }
        self.prints = self.prints.saturating_add(1);
    }

    /// Record what a heartbeat said about the pump's cursor.
    ///
    /// A heartbeat without the field leaves the previous reading in place: the
    /// bridge either reports it on every heartbeat or on none, so a missing one
    /// is an older bridge, not a pump that just caught up.
    pub fn observe_cursor_lag(&mut self, cursor_lag_ms: Option<i64>) {
        if let Some(lag) = cursor_lag_ms {
            self.cursor_lag_ms = Some(lag.max(0));
        }
    }

    /// Whether enough prints have arrived to be worth a clock read.
    #[must_use]
    pub fn due(&self) -> bool {
        self.prints >= SAMPLE_EVERY_PRINTS
    }

    /// Draw a sample and start a new window.
    ///
    /// `now_utc_ms` is the caller's clock — read once, here, and nowhere in the
    /// per-print path. `offset_ms` is `server_time - utc` in milliseconds, the
    /// same conversion the tick mapper applies.
    ///
    /// `None` when no live print has arrived since the last sample: there is
    /// nothing to measure, and reporting the previous window again would let a
    /// wedged socket show a healthy split forever.
    pub fn sample(&mut self, now_utc_ms: i64, offset_ms: i64) -> Option<LatencySample> {
        let newest = self.newest?;
        let oldest = self.oldest.unwrap_or(newest);
        let lag_of = |stamps: PrintStamps| {
            now_utc_ms.saturating_sub(stamps.time_ms.saturating_sub(offset_ms))
        };
        let arrival_lag_ms = lag_of(newest);
        let arrival_lag_peak_ms = lag_of(oldest);
        // Derived, not measured a second time: transport is whatever the
        // arrival figure has left after the terminal's share, so the two always
        // add up to the number the status bar already showed.
        let terminal_lag_ms = newest
            .sent_ms
            .map(|sent| sent.saturating_sub(newest.time_ms));
        let transport_lag_ms = terminal_lag_ms.map(|terminal| arrival_lag_ms - terminal);
        let transport_lag_peak_ms = oldest
            .sent_ms
            .map(|sent| arrival_lag_peak_ms - sent.saturating_sub(oldest.time_ms));
        let sample = LatencySample {
            arrival_lag_ms,
            arrival_lag_peak_ms,
            terminal_lag_ms,
            terminal_lag_peak_ms: self.terminal_peak_ms,
            transport_lag_ms,
            transport_lag_peak_ms,
            cursor_lag_ms: self.cursor_lag_ms,
            prints: self.prints,
        };
        self.newest = None;
        self.oldest = None;
        self.terminal_peak_ms = None;
        self.prints = 0;
        Some(sample)
    }

    /// Forget everything the current session observed.
    ///
    /// The cursor reading goes too: it belongs to the bridge that reported it,
    /// and carrying it into the next session would describe a pump that is no
    /// longer running.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hop_name_fits_the_cell_that_has_to_show_it() {
        // A consumer swaps this word for `arrival`, whose seven characters are
        // the budget. Eight is the agreed ceiling; a longer one silently pushes
        // a neighbouring status-bar cell off a narrow window, which is a defect
        // that only shows up on someone's laptop.
        for hop in [
            LatencyHop::Upstream,
            LatencyHop::Bridge,
            LatencyHop::Terminal,
            LatencyHop::Transport,
        ] {
            let label = hop.label();
            assert!(
                label.chars().count() <= 8,
                "{label:?} is too long for the readout"
            );
        }
    }

    /// B3: server = UTC-3, so a server stamp is three hours ahead of the UTC
    /// instant it names.
    const OFFSET_MS: i64 = -3 * 60 * 60 * 1000;

    /// A server-time instant, from a UTC one.
    fn server(utc_ms: i64) -> i64 {
        utc_ms + OFFSET_MS
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
    fn the_oldest_print_in_a_burst_carries_the_peak() {
        // One read delivers a burst: every print in it arrives at the same
        // instant, so the one sent earliest waited longest. Reporting only the
        // newest would show a healthy figure for a burst that was not.
        let mut tracker = LatencyTracker::new();
        tracker.observe_live(server(0), Some(server(100)));
        tracker.observe_live(server(500), Some(server(600)));
        tracker.observe_live(server(900), Some(server(950)));
        let s = tracker.sample(1_000, OFFSET_MS).unwrap();
        assert_eq!(s.prints, 3);
        assert_eq!(s.arrival_lag_ms, 100, "newest print");
        assert_eq!(s.arrival_lag_peak_ms, 1_000, "oldest print");
        assert_eq!(s.transport_lag_ms, Some(50));
        assert_eq!(s.transport_lag_peak_ms, Some(900));
        assert_eq!(s.terminal_lag_peak_ms, Some(100));
    }

    #[test]
    fn a_blocked_socket_names_quantick_not_metatrader() {
        // The bridge stamped it promptly and it still arrived late: whatever
        // ate the time is on this side of the wire.
        let mut tracker = LatencyTracker::new();
        tracker.observe_cursor_lag(Some(2));
        tracker.observe_live(server(0), Some(server(5)));
        let s = tracker.sample(4_000, OFFSET_MS).unwrap();
        assert_eq!(s.dominant(), Some(LatencyHop::Transport));
        assert_eq!(s.dominant().unwrap().label(), "quantick");
    }

    #[test]
    fn a_trailing_pump_names_the_bridge_and_a_slow_terminal_does_not() {
        // Same terminal lag, two different causes, told apart by how far the
        // bridge says its own cursor is behind.
        let mut trailing = LatencyTracker::new();
        trailing.observe_cursor_lag(Some(3_000));
        trailing.observe_live(server(0), Some(server(4_000)));
        let s = trailing.sample(4_010, OFFSET_MS).unwrap();
        assert_eq!(s.dominant(), Some(LatencyHop::Bridge));
        assert_eq!(s.bridge_lag_ms(), Some(3_000));
        assert_eq!(s.upstream_lag_ms(), Some(1_000));

        let mut upstream = LatencyTracker::new();
        upstream.observe_cursor_lag(Some(5));
        upstream.observe_live(server(0), Some(server(4_000)));
        let s = upstream.sample(4_010, OFFSET_MS).unwrap();
        assert_eq!(s.dominant(), Some(LatencyHop::Upstream));
        assert_eq!(s.dominant().unwrap().label(), "terminal");
    }

    #[test]
    fn without_a_heartbeat_the_terminal_is_named_whole() {
        // Refusing to guess which half is the honest answer, and it still
        // points the trader at MetaTrader rather than at the chart.
        let mut tracker = LatencyTracker::new();
        tracker.observe_live(server(0), Some(server(4_000)));
        let s = tracker.sample(4_010, OFFSET_MS).unwrap();
        assert_eq!(s.dominant(), Some(LatencyHop::Terminal));
        assert_eq!(s.dominant().unwrap().label(), "MT5");
        assert_eq!(s.upstream_lag_ms(), None);
        assert_eq!(s.bridge_lag_ms(), None);
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
    fn the_cursor_reading_outlives_a_window_but_not_a_session() {
        // It describes the pump, not the prints, so a sample must not clear it
        // — and a reconnect must, because the pump it described is gone.
        let mut tracker = LatencyTracker::new();
        tracker.observe_cursor_lag(Some(40));
        tracker.observe_live(server(0), Some(server(10)));
        assert_eq!(
            tracker.sample(1_000, OFFSET_MS).unwrap().cursor_lag_ms,
            Some(40)
        );
        tracker.observe_live(server(0), Some(server(10)));
        assert_eq!(
            tracker.sample(1_000, OFFSET_MS).unwrap().cursor_lag_ms,
            Some(40)
        );
        tracker.reset();
        tracker.observe_live(server(0), Some(server(10)));
        assert_eq!(
            tracker.sample(1_000, OFFSET_MS).unwrap().cursor_lag_ms,
            None
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
    fn a_negative_cursor_reading_is_clamped_rather_than_believed() {
        // `cursor_lag_ms` arrives from a bridge any local process can
        // impersonate, and a pump cannot be ahead of the terminal it reads.
        let mut tracker = LatencyTracker::new();
        tracker.observe_cursor_lag(Some(-500));
        tracker.observe_live(server(0), Some(server(100)));
        let s = tracker.sample(1_000, OFFSET_MS).unwrap();
        assert_eq!(s.cursor_lag_ms, Some(0));
        assert_eq!(s.bridge_lag_ms(), Some(0));
        assert_eq!(s.upstream_lag_ms(), Some(100));
    }
}
