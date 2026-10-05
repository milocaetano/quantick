//! The tape's large reconciliations, run beside the frame instead of in it.
//!
//! A zoom, a new seal lineage, a replay opening, a memory that fell behind
//! or a worker that stalled can hand one tape frame the whole window to
//! reconcile, and on a wide WIN window that held the trader's frames for
//! seconds, once for half a minute. [`TapeRebuilds`] prices each frame's
//! work first ([`tape_frame_work`], [`TapeWork::fits_frame`]); work too large
//! for a frame is handed to a [`TapeRebuildRunner`] as a [`TapeRebuild`], and
//! until it lands the frames draw the groups the pane already holds
//! ([`project_retained_tape_frame`]). Nothing is copied on the frame to hand
//! it over: the memory moves into a shared handle the stand-in reads and the
//! runner takes. The rebuilt memory is the one the frame would have left
//! behind, so every frame reconciled from it draws the tape the complete
//! per-frame path draws; and a rebuild that lands behind the tape is priced
//! again like any frame, so a slow one is followed by a short one, never by a
//! frame that catches up in place.

use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};

use super::{
    TapeDotFrame, TapeDotMemory, TapeFrameInputs, TapeRebuild, TapeWork,
    project_retained_tape_frame, project_tape_frame_with_overlay, tape_frame_work,
};

/// Where a [`TapeRebuild`] runs: somewhere no frame waits for it.
pub trait TapeRebuildRunner {
    /// Hand a rebuild over; it comes back when the runner cannot take it.
    ///
    /// # Errors
    ///
    /// The rebuild itself, when the runner has stopped.
    fn start(&mut self, generation: u64, rebuild: TapeRebuild) -> Result<(), Box<TapeRebuild>>;
    /// A rebuild that finished since the last call, if any; `Err` once the
    /// runner has stopped for good.
    ///
    /// # Errors
    ///
    /// [`RunnerStopped`] when nothing can land any more.
    fn finished(&mut self) -> Result<Option<(u64, TapeDotMemory)>, RunnerStopped>;
}

/// A [`TapeRebuildRunner`] that can take no more rebuilds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerStopped;

/// One pane's tape rebuilds: at most one running, adopted by the first frame
/// after it lands.
#[derive(Debug, Default)]
pub struct TapeRebuilds<R> {
    runner: R,
    /// Advanced by every rebuild and every discarded memory, so a rebuild
    /// that lands after its memory was thrown away is dropped.
    generation: u64,
    /// The running rebuild's generation, and the flag that stops it.
    running: Option<(u64, Arc<AtomicBool>)>,
    /// What the frames draw while a rebuild runs: the memory the rebuild
    /// started from, or the one a display change set aside.
    stand_in: Option<Arc<TapeDotMemory>>,
    /// The work the last frame was priced at; `None` while a rebuild ran.
    last_work: Option<TapeWork>,
}

impl<R: TapeRebuildRunner> TapeRebuilds<R> {
    #[must_use]
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            generation: 0,
            running: None,
            stand_in: None,
            last_work: None,
        }
    }

    /// Whether a rebuild is running, so the frames draw a stand-in.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    #[must_use]
    pub fn runner(&self) -> &R {
        &self.runner
    }

    /// The work the last frame was priced at, whether it ran in the frame or
    /// beside it; `None` when a rebuild was already running.
    #[must_use]
    pub fn last_work(&self) -> Option<TapeWork> {
        self.last_work
    }

    /// The tape frame for `frame`: reconciled in the frame when that fits
    /// it, drawn from the retained groups while larger work runs.
    pub fn project(
        &mut self,
        memory: &mut TapeDotMemory,
        frame: TapeFrameInputs<'_>,
    ) -> Option<TapeDotFrame> {
        self.adopt(memory);
        let marks = frame.marks();
        let (facts, overlay) = (frame.facts(), frame.overlay.map(Arc::as_ref));
        self.last_work = None;
        if self.running.is_none() {
            let work = tape_frame_work(
                memory,
                &marks,
                frame.style,
                frame.geometry,
                frame.time,
                frame.prices,
                facts,
                overlay,
            );
            self.last_work = Some(work);
            if !work.fits_frame() {
                self.hand_over(memory, frame, work);
            }
            if self.running.is_none() {
                self.stand_in = None;
                return project_tape_frame_with_overlay(
                    marks,
                    Some(memory),
                    frame.style,
                    frame.geometry,
                    frame.time,
                    frame.prices,
                    facts,
                    overlay,
                );
            }
        }
        let empty = TapeDotMemory::default();
        project_retained_tape_frame(
            &marks,
            self.stand_in.as_deref().unwrap_or(&empty),
            frame.style,
            frame.geometry,
            frame.time,
            frame.prices,
            facts,
            overlay,
        )
    }

    /// Start the rebuild of `memory` for `frame`, `memory` moving into it;
    /// where no runner takes it, it runs here, once.
    fn hand_over(
        &mut self,
        memory: &mut TapeDotMemory,
        frame: TapeFrameInputs<'_>,
        work: TapeWork,
    ) {
        let held = Arc::new(std::mem::take(memory));
        let base = if work.starts_over {
            Arc::default()
        } else {
            Arc::clone(&held)
        };
        let rebuild = frame.rebuild(base);
        let abandoned = Arc::clone(&rebuild.abandoned);
        let groups = held.retained_group_count();
        self.generation += 1;
        match self.runner.start(self.generation, rebuild) {
            Ok(()) => {
                self.running = Some((self.generation, abandoned));
                if groups > 0 || self.stand_in.is_none() {
                    self.stand_in = Some(held);
                }
                tracing::info!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "TAPE_REBUILD_STARTED",
                    generation = self.generation,
                    units = work.units(),
                    cells = work.cells,
                    windows = work.windows,
                    starts_over = work.starts_over,
                    action = "draw_retained_tape",
                    "the tape reconciles beside the frame"
                );
            }
            // Nowhere to run it: this frame pays for it, and the next one
            // hands over again.
            Err(rebuild) => {
                drop(held);
                *memory = (*rebuild).run();
            }
        }
    }

    /// The tape's price range for the axis fit: the stand-in's while a
    /// rebuild runs, `memory`'s otherwise.
    #[must_use]
    pub fn price_range(
        &self,
        memory: &TapeDotMemory,
        now_ms: i64,
        window_ms: i64,
    ) -> Option<(f64, f64)> {
        self.stand_in
            .as_deref()
            .filter(|_| self.running.is_some())
            .unwrap_or(memory)
            .price_range(now_ms, window_ms)
    }

    /// Set aside the memory a display change is about to clear, to draw from
    /// until the rebuild the change forces lands; a rebuild of the old
    /// display is dropped when it does.
    pub fn start_over(&mut self, memory: &mut TapeDotMemory) {
        self.forget_running();
        let held = std::mem::take(memory);
        if held.retained_group_count() > 0 {
            self.stand_in = Some(Arc::new(held));
        }
    }

    /// Forget the tape entirely: it belongs to a market or an epoch that is
    /// gone, so none of it may stand in either.
    pub fn discard(&mut self) {
        self.forget_running();
        self.stand_in = None;
    }

    fn forget_running(&mut self) {
        self.generation += 1;
        if let Some((_, abandoned)) = self.running.take() {
            abandoned.store(true, atomic::Ordering::Relaxed);
        }
    }

    fn adopt(&mut self, memory: &mut TapeDotMemory) {
        loop {
            match self.runner.finished() {
                Ok(Some((generation, rebuilt))) => {
                    if self
                        .running
                        .as_ref()
                        .is_some_and(|(running, _)| *running == generation)
                    {
                        *memory = rebuilt;
                        self.running = None;
                        self.stand_in = None;
                        tracing::info!(
                            target: "quantick::app",
                            schema_version = 1_u8,
                            event_code = "TAPE_REBUILD_ADOPTED",
                            generation,
                            groups = memory.retained_group_count(),
                            action = "draw_reconciled_tape",
                            "the tape reconciliation landed"
                        );
                    }
                }
                Ok(None) => break,
                Err(RunnerStopped) => {
                    // Nothing will land: the next frame prices its work again.
                    self.running = None;
                    break;
                }
            }
        }
    }
}
