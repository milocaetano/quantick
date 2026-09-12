//! The envelope's measurement harness — a maintained artifact, not a test
//! that gates anything. Run it with
//!
//! ```sh
//! env -u QUANTICK_BUBBLES cargo test --release -p quantick-app \
//!     live_envelope_tests::measure -- --ignored --nocapture --test-threads=1
//! ```
//!
//! and commit its output as `docs/quality/live-envelope/measure.txt`. It
//! prints four blocks:
//!
//! 1. element sizes and the bytes each bounded queue preallocates;
//! 2. one pane's retained tape and bars at the envelope's edge
//!    ([`RETAINED_TRADES_PER_PANE`] prints at the mean rate), with the
//!    per-print ingest cost at the start and at the end of the session, the
//!    single slowest ingest, and the process working set (read from the OS,
//!    so it is labelled as such and includes everything the test binary
//!    holds);
//! 3. the heatmap's retained history at its own caps;
//! 4. worker queue depth second by second through [`super::burst::Rig`] at the
//!    sustained rate, the burst rate and the peak frame.

use super::burst::{FRAMES_PER_S, Rig, Tape, play};
use crate::indicator_worker::IndicatorCommand;
use crate::live_envelope::*;
use crate::orderflow_worker::BookCommand;
use crate::state::{BarSpec, ChartState};
use crate::worker_progress::WorkerProgress;
use quantick_engine::{Bar, Side, Trade};
use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::engine::BookEngine;
use rust_decimal::Decimal;
use std::time::{Duration, Instant};

/// The process working set in bytes, from the operating system. `None` where
/// the platform command is missing: the figure is then reported as unknown,
/// never estimated.
fn working_set() -> Option<u64> {
    let pid = std::process::id().to_string();
    let output = if cfg!(windows) {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!("(Get-Process -Id {pid}).WorkingSet64"),
            ])
            .output()
            .ok()?
    } else {
        let rss_kib = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid])
            .output()
            .ok()?;
        let kib: u64 = String::from_utf8_lossy(&rss_kib.stdout)
            .trim()
            .parse()
            .ok()?;
        return Some(kib * 1024);
    };
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn session_print(i: u64) -> Trade {
    // One print every 1/MEAN_TRADES_PER_S seconds, a B3-like price walk.
    Trade {
        agg_id: i + 1,
        timestamp_ms: 1_700_000_000_000 + (i * 1_000 / MEAN_TRADES_PER_S) as i64,
        price: Decimal::from(176_000 + (i % 97) as i64 * 5),
        quantity: Decimal::from(1 + (i % 5) as i64),
        side: if i % 7 < 4 { Side::Buy } else { Side::Sell },
    }
}

fn sizes() {
    let slot = std::mem::size_of::<usize>();
    let indicator = std::mem::size_of::<IndicatorCommand>();
    let book = std::mem::size_of::<BookCommand>();
    println!("## 1. element sizes and queue preallocation");
    println!(
        "Trade {} B, Bar {} B",
        std::mem::size_of::<Trade>(),
        std::mem::size_of::<Bar>()
    );
    println!(
        "IndicatorCommand {indicator} B x {INDICATOR_COMMAND_QUEUE} = {} per pane (slot + stamp, std array channel)",
        mib(((indicator + slot) * INDICATOR_COMMAND_QUEUE) as u64)
    );
    println!(
        "BookCommand {book} B x {BOOK_COMMAND_QUEUE} = {} per pane with order flow",
        mib(((book + slot) * BOOK_COMMAND_QUEUE) as u64)
    );
}

fn retained(footprint: bool) {
    let before = working_set();
    let mut state = ChartState::new(BarSpec::Tick(50));
    state.set_footprint_enabled(footprint);
    let total = RETAINED_TRADES_PER_PANE as u64;
    let chunk = 100_000;
    let mut chunks = Vec::new();
    let mut slowest = (Duration::ZERO, 0);
    let started = Instant::now();
    let mut chunk_start = Instant::now();
    for i in 0..total {
        let trade = session_print(i);
        let one = Instant::now();
        state.ingest_live(&trade);
        let took = one.elapsed();
        if took > slowest.0 {
            slowest = (took, i);
        }
        if (i + 1) % chunk == 0 {
            chunks.push(chunk_start.elapsed().as_nanos() as f64 / chunk as f64);
            chunk_start = Instant::now();
        }
    }
    let elapsed = started.elapsed();
    let after = working_set();
    let trades = state.trades().len();
    let bars = state.bars().len();
    let trade_bytes = std::mem::size_of_val(state.trades()) as u64;
    let bar_bytes = std::mem::size_of_val(state.bars()) as u64;
    println!(
        "footprint={footprint}: {trades} prints, {bars} bars (tick:50) in {:.2} s; \
         tape {} + bars {} (computed, len x size; capacity may be up to 2x)",
        elapsed.as_secs_f64(),
        mib(trade_bytes),
        mib(bar_bytes),
    );
    println!(
        "  ingest ns/print: first 100k {:.0}, last 100k {:.0}, median chunk {:.0}; slowest single ingest {:.2} ms at print {}",
        chunks.first().copied().unwrap_or(0.0),
        chunks.last().copied().unwrap_or(0.0),
        {
            let mut sorted = chunks.clone();
            sorted.sort_by(f64::total_cmp);
            sorted[sorted.len() / 2]
        },
        slowest.0.as_secs_f64() * 1_000.0,
        slowest.1,
    );
    match (before, after) {
        (Some(before), Some(after)) => println!(
            "  working set (OS): {} before, {} after, +{}",
            mib(before),
            mib(after),
            mib(after.saturating_sub(before))
        ),
        _ => println!("  working set (OS): unknown on this platform"),
    }
}

fn heatmap_history() {
    let config = HeatmapConfig {
        show_aggressions: true,
        ..HeatmapConfig::default()
    };
    println!(
        "## 3. heatmap retained history (existing caps): retention {} min, max_aggressions {}, \
         max_history_runs {}, max_history_bytes {}",
        config.retention_ms / 60_000,
        config.max_aggressions,
        config.max_history_runs,
        mib(config.max_history_bytes as u64),
    );
    let mut engine = BookEngine::new("WINV26".to_owned());
    engine.apply_visual_config(config.clone());
    // Thirty minutes at the mean rate, then as many again at the sustained
    // rate: the first fits the retention window, the second hits the count.
    let mean_prints = MEAN_TRADES_PER_S * 30 * 60;
    let mut at_ms = 1_700_000_000_000_i64;
    for i in 0..mean_prints {
        at_ms += (1_000 / MEAN_TRADES_PER_S) as i64;
        let mut trade = session_print(i);
        trade.timestamp_ms = at_ms;
        engine.record_trade(&trade);
    }
    let health = engine.published().health;
    println!(
        "  30 min at {MEAN_TRADES_PER_S}/s ({mean_prints} prints): {} aggressions retained, history {}",
        health.aggression_count,
        mib(health.history_bytes as u64)
    );
    let sustained_prints = SUSTAINED_TRADES_PER_S * 30 * 60;
    for i in 0..sustained_prints {
        at_ms += (1_000 / SUSTAINED_TRADES_PER_S) as i64;
        let mut trade = session_print(mean_prints + i);
        trade.timestamp_ms = at_ms;
        engine.record_trade(&trade);
    }
    let health = engine.published().health;
    println!(
        "  then 30 min at {SUSTAINED_TRADES_PER_S}/s ({sustained_prints} prints): {} aggressions retained \
         (the count cap binds; older bubbles leave the canvas), history {}",
        health.aggression_count,
        mib(health.history_bytes as u64)
    );
}

fn queue_depths() {
    println!(
        "## 4. worker queue depth, deepest per second (tick:1: one indicator command per print)"
    );
    let mut rig = Rig::new(
        BarSpec::Tick(1),
        WorkerProgress::new(),
        WorkerProgress::new(),
    );
    let mut tape = Tape::default();
    let phases = [
        ("sustained", SUSTAINED_TRADES_PER_S, 20),
        ("burst", BURST_TRADES_PER_S, 5),
    ];
    let mut depth_sent = 0;
    for (name, rate, seconds) in phases {
        for second in 0..seconds {
            rig.depths = Default::default();
            depth_sent += play(&mut rig, &mut tape, rate, DEPTH_UPDATES_PER_S, 1);
            println!(
                "  {name:<9} t={second:>2}s rate={rate}/s  indicator queued max {:>4}/{INDICATOR_COMMAND_QUEUE}  \
                 book queued max {:>4}/{BOOK_COMMAND_QUEUE}  parked {}/{}",
                rig.depths.indicator_queued,
                rig.depths.book_queued,
                rig.depths.indicator_parked,
                rig.depths.book_parked,
            );
        }
    }
    rig.depths = Default::default();
    let peak = Instant::now();
    rig.frame(
        &mut tape,
        BURST_TRADES_PER_FRAME,
        BURST_TRADES_PER_S,
        BURST_DEPTH_UPDATES_PER_FRAME,
    );
    let send_ms = peak.elapsed().as_secs_f64() * 1_000.0;
    depth_sent += BURST_DEPTH_UPDATES_PER_FRAME as u64;
    println!(
        "  peak frame: {BURST_TRADES_PER_FRAME} prints + {BURST_DEPTH_UPDATES_PER_FRAME} depth sent in {send_ms:.2} ms on the UI side; \
         indicator queued max {}  book queued max {}  parked {}/{}",
        rig.depths.indicator_queued,
        rig.depths.book_queued,
        rig.depths.indicator_parked,
        rig.depths.book_parked,
    );
    rig.settle();
    rig.assert_nothing_lost(&tape, depth_sent);
    let (indicator, book) = (rig.indicator_counts(), rig.book_counts());
    println!(
        "  settled: {} prints, {} frames at {FRAMES_PER_S} fps; deferred {}/{}, lost 0",
        tape.prints.len(),
        25 * FRAMES_PER_S + 1,
        indicator.deferred,
        book.deferred,
    );
}

#[test]
#[ignore = "measurement harness; run with --ignored --nocapture, see the module docs"]
fn measure() {
    sizes();
    println!(
        "## 2. one pane's retained state at the envelope's edge: {RETAINED_TRADES_PER_PANE} prints \
         ({MEAN_TRADES_PER_S}/s x {SESSION_HOURS} h x {RETAINED_SESSIONS} sessions)"
    );
    retained(false);
    retained(true);
    heatmap_history();
    queue_depths();
}
