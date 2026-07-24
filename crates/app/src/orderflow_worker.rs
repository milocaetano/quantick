//! Background thread that owns the [`BookEngine`].
//!
//! The UI thread never touches book state directly: it sends commands through
//! an unbounded channel and reads the latest [`BookPublished`] snapshot from a
//! shared mailbox. Projection requests are coalesced latest-wins — when the
//! worker falls behind (a dense book can take tens of milliseconds per
//! projection), intermediate layouts are dropped and only the newest one is
//! built. The UI keeps drawing the last published frame, so a slow projection
//! can never block a frame.
//!
//! The worker blocks on `recv` while idle and exits when the last sender is
//! dropped. The UI repaints on its own ~60 fps cadence, so no egui handle is
//! needed here.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use quantick_engine::Trade;
use quantick_feed_binance::depth::DepthEvent;
use rust_decimal::Decimal;

use crate::orderflow::HeatmapConfig;
use crate::orderflow_engine::{BookEngine, BookPublished, ProjectionRequest};

/// Commands mirror the [`BookEngine`] mutation surface one-to-one.
pub(crate) enum BookCommand {
    Depth(DepthEvent),
    Trade(Trade),
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
    AcceptGroupingRestart {
        grouping: Decimal,
        generation_floor: u64,
    },
    ResetForSymbol(String),
    ResetSummaryCounters,
    Project(ProjectionRequest),
    /// Test barrier: acknowledged only after every earlier command in the
    /// queue has been applied and its effects published.
    #[allow(dead_code)]
    Flush(Sender<()>),
}

/// UI-side handle: send commands, read the latest published snapshot.
pub(crate) struct BookWorker {
    commands: Sender<BookCommand>,
    published: Arc<Mutex<BookPublished>>,
}

impl BookWorker {
    /// Spawn the book thread for `symbol`.
    #[must_use]
    pub(crate) fn spawn(symbol: &str) -> Self {
        let (tx, rx) = channel::<BookCommand>();
        let published = Arc::new(Mutex::new(BookPublished::initial()));
        let shared = Arc::clone(&published);
        let engine_symbol = symbol.to_owned();
        std::thread::Builder::new()
            .name("quantick-book".to_owned())
            .spawn(move || run(BookEngine::new(engine_symbol), &rx, &shared))
            .expect("spawn book worker thread");
        Self {
            commands: tx,
            published,
        }
    }

    /// Queue one command. A send failure means the worker died (a bug worth a
    /// log line), never a full queue: the channel is unbounded and command
    /// volume is bounded by feed cadence.
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

    /// Latest snapshot published by the worker.
    #[must_use]
    pub(crate) fn published(&self) -> BookPublished {
        self.published
            .lock()
            .expect("book published mailbox poisoned")
            .clone()
    }

    /// Block until every command sent before this call has been applied and
    /// published. Tests use this to make the async pipeline deterministic.
    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let (ack_tx, ack_rx) = channel();
        self.send(BookCommand::Flush(ack_tx));
        let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(10));
    }
}

fn run(mut engine: BookEngine, rx: &Receiver<BookCommand>, shared: &Arc<Mutex<BookPublished>>) {
    // Kept across batches so the worker can re-project after data changes
    // without waiting for the UI to ask again.
    let mut last_request: Option<ProjectionRequest> = None;

    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(next) = rx.try_recv() {
            batch.push(next);
        }

        let mut flushes: Vec<Sender<()>> = Vec::new();
        let mut incoming_request: Option<ProjectionRequest> = None;
        for command in batch {
            match command {
                BookCommand::Depth(event) => engine.handle_depth_event(event),
                BookCommand::Trade(trade) => engine.record_trade(&trade),
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
                BookCommand::AcceptGroupingRestart {
                    grouping,
                    generation_floor,
                } => engine.accept_grouping_restart(grouping, generation_floor),
                BookCommand::ResetForSymbol(symbol) => {
                    // A symbol change orphans any in-flight projection request.
                    last_request = None;
                    engine.reset_for_symbol(symbol);
                }
                BookCommand::ResetSummaryCounters => engine.reset_summary_counters(),
                // Latest-wins: only the newest layout of this batch is built.
                BookCommand::Project(request) => incoming_request = Some(request),
                BookCommand::Flush(ack) => flushes.push(ack),
            }
        }

        if let Some(request) = incoming_request {
            last_request = Some(request);
        }
        // Rebuild against the newest known layout. The engine's own cache
        // (layout equality + depth-cadence interval) decides whether this is
        // a real rebuild or a no-op, so a chatty batch stays cheap.
        if let Some(request) = &last_request
            && engine.enabled()
        {
            engine.project(request);
        }

        {
            let mut mailbox = shared.lock().expect("book published mailbox poisoned");
            *mailbox = engine.published();
        }
        for ack in flushes {
            let _ = ack.send(());
        }
    }
}
