//! The thread a pane's large tape reconciliations run on: the one part of
//! [`TapeRebuilds`] a headless crate cannot hold.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Instant;

use quantick_orderflow::projection::{
    RunnerStopped, TapeDotMemory, TapeRebuild, TapeRebuildRunner, TapeRebuilds,
};

/// A pane's tape rebuilds, run on its own thread.
pub(crate) type PaneTapeRebuilds = TapeRebuilds<RebuildThread>;

type Channels = (Sender<(u64, TapeRebuild)>, Receiver<(u64, TapeDotMemory)>);

/// The pane's rebuild thread, spawned by the first reconciliation too large
/// for a frame. Latest wins: a rebuild queued behind another is newer.
#[derive(Debug, Default)]
pub(crate) struct RebuildThread {
    channels: Option<Channels>,
}

impl TapeRebuildRunner for RebuildThread {
    fn start(&mut self, generation: u64, rebuild: TapeRebuild) -> Result<(), Box<TapeRebuild>> {
        let (jobs, _) = self.channels.get_or_insert_with(spawn);
        jobs.send((generation, rebuild)).map_err(|refused| {
            self.channels = None;
            Box::new(refused.0.1)
        })
    }

    fn finished(&mut self) -> Result<Option<(u64, TapeDotMemory)>, RunnerStopped> {
        let Some((_, done)) = &self.channels else {
            return Ok(None);
        };
        match done.try_recv() {
            Ok(landed) => Ok(Some(landed)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                tracing::error!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "TAPE_REBUILD_WORKER_DOWN",
                    action = "respawn_on_next_reread",
                    "the tape rebuild thread stopped"
                );
                self.channels = None;
                Err(RunnerStopped)
            }
        }
    }
}

fn spawn() -> Channels {
    let (jobs, inbox) = channel::<(u64, TapeRebuild)>();
    let (outbox, done) = channel();
    std::thread::Builder::new()
        .name("quantick-tape-rebuild".to_owned())
        .spawn(move || {
            while let Ok(mut job) = inbox.recv() {
                while let Ok(newer) = inbox.try_recv() {
                    job = newer;
                }
                let (generation, rebuild) = job;
                let abandoned = std::sync::Arc::clone(&rebuild.abandoned);
                let started = Instant::now();
                let memory = rebuild.run();
                if abandoned.load(std::sync::atomic::Ordering::Relaxed) {
                    // Dropped here, beside the frame, not on the painting thread.
                    tracing::info!(
                        target: "quantick::app",
                        schema_version = 1_u8,
                        event_code = "TAPE_REBUILD_ABANDONED",
                        generation,
                        ms = started.elapsed().as_secs_f64() * 1_000.0,
                        action = "drop_superseded_rebuild",
                        "a newer display change superseded the tape reconciliation"
                    );
                    continue;
                }
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "TAPE_REBUILD_RAN",
                    generation,
                    ms = started.elapsed().as_secs_f64() * 1_000.0,
                    action = "hand_to_pane",
                    "the tape reconciliation finished beside the frame"
                );
                if outbox.send((generation, memory)).is_err() {
                    break;
                }
            }
        })
        .expect("spawn tape rebuild thread");
    (jobs, done)
}

#[cfg(test)]
mod tape_rebuild_tests {
    use super::*;
    use quantick_orderflow::config::theme::OrderflowRenderStyle;
    use quantick_orderflow::projection::TapeDotGeometry;

    #[test]
    fn a_rebuild_lands_from_the_thread() {
        let mut thread = RebuildThread::default();
        let style = OrderflowRenderStyle::from_config(&Default::default(), [0, 0, 0, 255]);
        let rebuild = TapeRebuild {
            memory: Default::default(),
            abandoned: Default::default(),
            projection: std::sync::Arc::new(quantick_orderflow::HeatmapProjection::empty(
                true,
                quantick_orderflow::EffectiveGrouping::resolve(
                    quantick_orderflow::DisplayGrouping::Native,
                    rust_decimal::Decimal::ONE,
                    rust_decimal::Decimal::from(100),
                ),
            )),
            bubbles: style.clone(),
            style,
            geometry: TapeDotGeometry {
                left_x: 0.0,
                right_x: 1.0,
                width_px: 100.0,
                height_px: 100.0,
            },
            time: None,
            prices: None,
            overlay: None,
        };
        assert!(thread.start(7, rebuild).is_ok());
        let deadline = Instant::now() + std::time::Duration::from_secs(30);
        let landed = loop {
            if let Some((generation, _)) = thread.finished().expect("the thread runs") {
                break Some(generation);
            }
            if Instant::now() > deadline {
                break None;
            }
            std::thread::yield_now();
        };
        assert_eq!(landed, Some(7));
    }
}
