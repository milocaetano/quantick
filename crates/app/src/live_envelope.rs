//! The live workload quantick supports, as numbers the code reads.
//!
//! Every figure here is measured or derived, and says which. The trade
//! figures come from the thirteen WINV26 sessions recorded on the trader's
//! own terminal (B3 mini index, 2026-08-17 to 2026-09-04, 1.40–1.82 M prints
//! each) plus one WDOU26 session, read by `tools/live_envelope/tape_rates.py`
//! (output: `docs/quality/live-envelope/tape-rates.txt`). A print is one
//! MetaTrader tick — the unit the app ingests. The depth figures are derived
//! from the venues' own bounds because no recording carries a book.
//! `docs/quality/live-envelope.md` renders the same numbers with the runs
//! behind them, and `envelope_doc_tests` fails the build when the two drift
//! apart.
//!
//! The caps below are what the envelope *implies*; the policy that holds
//! them — park, fold, count, never drop — is [`crate::worker_backlog`], and
//! [`crate::worker_progress`] publishes the counts that show it holding. The
//! health summary sets the window's measured rates against these figures
//! (`live_rate`) and warns when a pane retains more than the envelope
//! (`LIVE_ENVELOPE_EXCEEDED`).

/// Sustained live trade rate, prints per second, one pane.
///
/// The p99 one-second rate over the thirteen WINV26 sessions is 224–286;
/// this is that figure rounded up. The session means are 41–53.
pub(crate) const SUSTAINED_TRADES_PER_S: u64 = 300;

/// Burst trade rate, prints per second, held for at most a few seconds.
///
/// The busiest single second in any recorded session held 1,882 prints
/// (WINV26, 2026-09-03); rounded up. B3's opening auction and news prints
/// are the source of such seconds.
pub(crate) const BURST_TRADES_PER_S: u64 = 2_000;

/// Prints one 60 fps frame may have to take in at the burst's peak.
///
/// The busiest 16 ms window in any recorded session held 267 prints
/// (WINV26, 2026-08-17); the p99 frame holds 6–7. Doubled and rounded up to
/// a power of two.
pub(crate) const BURST_TRADES_PER_FRAME: usize = 512;

/// Mean trade rate a full session is sized from, prints per second.
///
/// The session means are 41.1–53.4 prints/s (WINV26) and 3.5 (WDOU26);
/// rounded up. The retained-history figure below multiplies this by the
/// session length.
pub(crate) const MEAN_TRADES_PER_S: u64 = 55;

/// Depth updates per second, one book. **Derived, not measured**: every
/// recording on this host is a trade tape without a book. Binance publishes
/// `depth@100ms` (10/s) and Hyperliquid pushes on change; the MetaTrader
/// bridge's rate is unmeasured. The feed-side depth channel holds 8,192
/// events (`crates/feed/src/binance.rs`, `hyperliquid.rs`,
/// `BOOK_EVENT_CHANNEL_CAPACITY`) and a frame drains up to
/// [`BURST_DEPTH_UPDATES_PER_FRAME`], so this rate leaves the drain a
/// hundredfold headroom; the burst test drives it synthetically.
pub(crate) const DEPTH_UPDATES_PER_S: u64 = 1_000;

/// Depth events one frame drains from the feed, at most. The tab's own drain
/// budget reads this (`crate::tab`'s `BOOK_DRAIN_BUDGET`), so the book queue
/// below is sized from the figure the drain actually uses.
pub(crate) const BURST_DEPTH_UPDATES_PER_FRAME: usize = 2_048;

/// Hours one live session runs. B3's session on the recordings spans
/// 9.47–9.52 h from first to last print (09:00–18:31 local); rounded up.
pub(crate) const SESSION_HOURS: u64 = 10;

/// Sessions one pane retains at once: the day on screen and the day before
/// it, which the replay browser and the MetaTrader session recovery both
/// join in front of the live tape.
pub(crate) const RETAINED_SESSIONS: u64 = 2;

/// Prints one pane is expected to retain at the envelope's edge.
///
/// `MEAN_TRADES_PER_S × 3,600 × SESSION_HOURS × RETAINED_SESSIONS`
/// = 3,960,000. Every pane of a tab retains its own copy of the tape; the
/// bytes that makes are measured in `docs/quality/live-envelope.md`.
/// Nothing evicts a print below or above this figure; a pane that holds more
/// is reported as outside the envelope (`LIVE_ENVELOPE_EXCEEDED`), and what
/// to do about it is a product decision recorded in that document.
pub(crate) const RETAINED_TRADES_PER_PANE: usize =
    (MEAN_TRADES_PER_S * 3_600 * SESSION_HOURS * RETAINED_SESSIONS) as usize;

/// Commands the indicator worker's queue holds before the sender parks.
///
/// Per frame a pane sends at most one closed bar per print (a `tick:1`
/// chart) plus one forming-bar update: `BURST_TRADES_PER_FRAME + 1` = 513
/// at the burst's peak. Twice the burst frame, so one peak frame fits with
/// nearly a second one's worth of room while the worker is busy.
pub(crate) const INDICATOR_COMMAND_QUEUE: usize = 2 * BURST_TRADES_PER_FRAME;

/// Commands the book worker's queue holds before the sender parks.
///
/// Per frame a pane sends one command per depth event drained (at most
/// `BURST_DEPTH_UPDATES_PER_FRAME`), one per print (`BURST_TRADES_PER_FRAME`)
/// and one projection request: 2,561. Rounded up to the next power of two,
/// 4,096 — also the trade channel every venue feed already sizes itself to
/// (`crates/feed/src/binance.rs`, `metatrader.rs`), so the worker never holds
/// less than the feed can hand it.
pub(crate) const BOOK_COMMAND_QUEUE: usize = 4_096;

// The caps hold the worst frame they were derived from, and the rates keep
// their order — checked when the crate compiles, so a constant edited alone
// cannot build.
const _: () = {
    // One closed bar per print (tick:1) plus the forming-bar update.
    assert!(INDICATOR_COMMAND_QUEUE > BURST_TRADES_PER_FRAME + 1);
    // One command per depth event and per print, plus one layout request.
    assert!(BOOK_COMMAND_QUEUE > BURST_DEPTH_UPDATES_PER_FRAME + BURST_TRADES_PER_FRAME + 1);
    // The per-frame figure covers a second of the burst spread evenly.
    assert!(BURST_TRADES_PER_FRAME as u64 >= BURST_TRADES_PER_S / 60);
    assert!(BURST_TRADES_PER_S > SUSTAINED_TRADES_PER_S);
    assert!(SUSTAINED_TRADES_PER_S > MEAN_TRADES_PER_S);
};

#[cfg(test)]
mod envelope_doc_tests;
