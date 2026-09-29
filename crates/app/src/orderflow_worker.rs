//! Background thread that owns the [`BookEngine`].
//!
//! The UI thread never touches book state: it sends commands through a channel
//! bounded by [`BOOK_COMMAND_QUEUE`] and reads the latest [`BookPublished`] from
//! a shared mailbox. A full queue parks commands UI-side, in order, folding only
//! superseded layouts ([`fold_parked`]); nothing blocks or is dropped. Projection
//! requests coalesce latest-wins, so a slow projection never blocks a frame.
//! The worker blocks on `recv` while idle and exits with the last sender.

use crate::live_envelope::BOOK_COMMAND_QUEUE;
use crate::worker_progress::{
    Coalescing, ObservedSender, ProgressSnapshot, SharedProgress, WorkerProgress,
};
use std::sync::mpsc::{Receiver, Sender, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use quantick_engine::Trade;
use quantick_orderbook::DepthEvent;
use rust_decimal::Decimal;

use quantick_orderflow::HeatmapConfig;
use quantick_orderflow::engine::{BookEngine, BookPublished, ProjectionRequest};

/// Commands mirror the [`BookEngine`] mutation surface one-to-one.
pub(crate) enum BookCommand {
    Depth {
        event: DepthEvent,
        received_at_ms: i64,
    },
    Trade(Trade),
    /// View-owned admission ordinal, independent of venue IDs/reconnections.
    TapeTrade {
        ordinal: u64,
        trade: Trade,
    },
    /// A source/mode boundary; earlier requests cannot acknowledge this epoch.
    TapeEpoch(u64),
    SetEnabled {
        enabled: bool,
        generation_floor: u64,
    },
    PrepareRestart {
        generation_floor: u64,
        reason: &'static str,
    },
    ApplyVisualConfig(HeatmapConfig),
    ApplyGroupingNow(Decimal),
    /// The price grid the tape itself prints on and the magnitude it prints
    /// at, for a chart whose feed never states an instrument tick and whose
    /// book never states a price. Loses to a venue-stated step; see
    /// [`BookEngine::size_from_tape`](quantick_orderflow::engine::BookEngine::size_from_tape).
    TapePriceGrid {
        step: Decimal,
        reference_price: Option<Decimal>,
    },
    AcceptGroupingRestart {
        grouping: Decimal,
        generation_floor: u64,
    },
    ResetForSymbol(String),
    ResetSummaryCounters,
    /// Hold the native tape at a past instant, or `None` for live.
    TapeEnd(Option<i64>),
    Project(ProjectionRequest),
    /// Test barrier: acknowledged only after every earlier command in the
    /// queue has been applied and its effects published.
    #[allow(dead_code)]
    Flush(Sender<()>),
}

/// UI-side handle: send commands, read the latest published snapshot.
#[derive(Clone, Copy)]
pub(crate) struct TapeFrameReceipt {
    pub epoch: u64,
    pub through_ordinal: u64,
}

#[derive(Clone)]
pub(crate) struct BookPublication {
    pub book: BookPublished,
    pub tape_receipt: Option<TapeFrameReceipt>,
}

impl BookPublication {
    fn initial() -> Self {
        Self {
            book: BookPublished::initial(),
            tape_receipt: None,
        }
    }
}

pub(crate) struct BookWorker {
    commands: ObservedSender<BookCommand>,
    published: Arc<Mutex<BookPublication>>,
}

impl BookWorker {
    /// Spawn the book thread for `symbol`.
    #[must_use]
    pub(crate) fn spawn(symbol: &str) -> Self {
        Self::spawn_with_progress(symbol, crate::worker_progress::monotonic())
    }

    pub(crate) fn spawn_with_progress(symbol: &str, progress: WorkerProgress) -> Self {
        let (tx, rx) = sync_channel::<BookCommand>(BOOK_COMMAND_QUEUE);
        let published = Arc::new(Mutex::new(BookPublication::initial()));
        let shared = Arc::clone(&published);
        let engine_symbol = symbol.to_owned();
        let observed = progress.consumer();
        std::thread::Builder::new()
            .name("quantick-book".to_owned())
            .spawn(move || run(BookEngine::new(engine_symbol), &rx, &shared, observed))
            .expect("spawn book worker thread");
        Self {
            commands: progress.bind_merging(tx, fold_parked),
            published,
        }
    }

    /// Queue one command, or park it when the bounded queue is full; either
    /// way it reaches the worker and the caller never waits. A send failure
    /// means the worker died (a bug worth a log line).
    pub(crate) fn send(&self, command: BookCommand) {
        if self.commands.send(command).is_err() {
            tracing::error!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "HEATMAP_WORKER_DOWN",
                action = "heatmap_frozen_until_restart",
                "book worker thread is gone; heatmap commands are being dropped"
            );
        }
    }

    pub(crate) fn progress(&self) -> ProgressSnapshot {
        self.commands.snapshot()
    }

    /// Latest snapshot published by the worker.
    ///
    /// Read every frame, so it is also the retry point for commands parked
    /// behind a full queue: one length read when nothing is parked.
    #[must_use]
    pub(crate) fn publication(&self) -> BookPublication {
        self.commands.pump();
        self.published
            .lock()
            .expect("book published mailbox poisoned")
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn published(&self) -> BookPublished {
        self.publication().book
    }

    /// Just the capture bucket from the published mailbox: the footprint needs
    /// it every frame, even with every layer off, without cloning the frame.
    pub(crate) fn published_base_grouping(&self) -> Decimal {
        // Read every frame even with every layer off, so it pumps too.
        self.commands.pump();
        self.published
            .lock()
            .expect("book published mailbox poisoned")
            .book
            .base_price_grouping
    }

    /// Block until every command sent before this call has been applied and
    /// published. Tests use this to make the async pipeline deterministic.
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = std::sync::mpsc::channel();
        self.send(BookCommand::Flush(ack_tx));
        // A test is its own frame loop: pump what a full queue parked, or a
        // parked barrier would wait on a queue nobody refills.
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        while Instant::now() < deadline {
            self.commands.pump();
            if ack_rx
                .recv_timeout(std::time::Duration::from_millis(5))
                .is_ok()
            {
                return;
            }
        }
    }
}

/// The superseding rule for commands parked behind a full queue: only a layout
/// request folds, into one parked right before it (only the newest is built).
/// Prints, depth and configuration keep their place and order: applying a
/// configuration can prune history the next one would not have.
pub(crate) fn fold_parked(older: &mut BookCommand, newer: BookCommand) -> Option<BookCommand> {
    match (&mut *older, newer) {
        (BookCommand::Project(request), BookCommand::Project(next)) => {
            *request = next;
            None
        }
        (_, newer) => Some(newer),
    }
}

fn run(
    mut engine: BookEngine,
    rx: &Receiver<BookCommand>,
    shared: &Arc<Mutex<BookPublication>>,
    progress: Arc<SharedProgress>,
) {
    let _lifecycle = progress.lifecycle(std::thread::panicking);
    // Kept across batches so the worker can re-project after data changes
    // without waiting for the UI to ask again.
    let mut last_request: Option<ProjectionRequest> = None;
    let mut tape_epoch = 0;
    let mut through_ordinal = 0;
    let mut tape_receipt = None;

    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }

        progress.begin(batch.len());
        let mut coalescing = Coalescing::new(&progress);
        let mut flushes: Vec<Sender<()>> = Vec::new();
        let mut incoming_request: Option<ProjectionRequest> = None;
        for command in batch {
            match command {
                BookCommand::Depth {
                    event,
                    received_at_ms,
                } => engine.handle_depth_event_at(event, received_at_ms),
                BookCommand::Trade(trade) => engine.record_trade(&trade),
                BookCommand::TapeTrade { ordinal, trade } => {
                    engine.record_trade(&trade);
                    through_ordinal = ordinal;
                }
                BookCommand::TapeEpoch(epoch) => {
                    tape_epoch = epoch;
                    through_ordinal = 0;
                    tape_receipt = None;
                    last_request = None;
                    incoming_request = None;
                }
                BookCommand::SetEnabled {
                    enabled,
                    generation_floor,
                } => engine.set_enabled(enabled, generation_floor),
                BookCommand::PrepareRestart {
                    generation_floor,
                    reason,
                } => engine.prepare_restart(generation_floor, reason),
                BookCommand::ApplyVisualConfig(config) => engine.apply_visual_config(config),
                BookCommand::ApplyGroupingNow(grouping) => engine.apply_grouping_now(grouping),
                BookCommand::TapePriceGrid {
                    step,
                    reference_price,
                } => engine.size_from_tape(step, reference_price),
                BookCommand::AcceptGroupingRestart {
                    grouping,
                    generation_floor,
                } => engine.accept_grouping_restart(grouping, generation_floor),
                BookCommand::ResetForSymbol(symbol) => {
                    // A symbol change orphans any in-flight projection request.
                    last_request = None;
                    incoming_request = None;
                    tape_receipt = None;
                    through_ordinal = 0;
                    engine.reset_for_symbol(symbol);
                }
                BookCommand::ResetSummaryCounters => engine.reset_summary_counters(),
                BookCommand::TapeEnd(end_ms) => engine.set_tape_end(end_ms),
                // Latest-wins: only the newest layout of this batch is built.
                BookCommand::Project(request) => {
                    coalescing.projects += usize::from(incoming_request.is_some());
                    incoming_request = Some(request);
                }
                BookCommand::Flush(ack) => flushes.push(ack),
            }
        }

        if let Some(request) = incoming_request {
            // Noted outside the layer gate below: the published ladder keeps
            // following the view even when no heatmap layer wants frames.
            engine.note_price_window(request.price_range);
            last_request = Some(request);
        }
        // Rebuild against the newest known layout. The engine's own cache
        // (layout + bar/history revisions + minimum cadence) decides whether
        // this is a real rebuild or a no-op, so a chatty batch stays cheap.
        if let Some(request) = &last_request
            && engine.any_layer_enabled()
            && engine.project_at(request, Instant::now()).is_some()
        {
            tape_receipt = Some(TapeFrameReceipt {
                epoch: tape_epoch,
                through_ordinal,
            });
        }

        coalescing.publishing();
        {
            let mut mailbox = shared.lock().expect("book published mailbox poisoned");
            *mailbox = BookPublication {
                book: engine.published(),
                tape_receipt,
            };
        }
        progress.finish(true);
        for ack in flushes {
            let _ = ack.send(());
        }
    }
}

#[cfg(test)]
mod fold_tests;

#[cfg(test)]
mod progress_tests;
