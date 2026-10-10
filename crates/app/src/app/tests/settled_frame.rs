//! A frame the off-thread workers and the wall clock cannot move.
//!
//! The book worker publishes between frames whenever the host gets round
//! to it, and that publication moves the lane divider, the price scale and
//! whatever is anchored to them. The frame clock is real time, so a loaded
//! host also changes what the time-fed readouts measure. A test that reads
//! a position in one frame and acts on it in the next is then a test of
//! the scheduler. These helpers take both out: every tab's tape is flushed
//! before the frame, and a pinned context's frames advance a fixed step.
use super::*;

/// The fixed step a pinned context's frames advance: one 60 Hz frame.
const SETTLED_FRAME_STEP: std::time::Duration = std::time::Duration::from_millis(16);

fn frame_clock_id() -> egui::Id {
    egui::Id::new("quantick-test-fixed-frame-clock")
}

/// From now on, every frame driven on `ctx` by the shared drivers sees a
/// clock that advances [`SETTLED_FRAME_STEP`] per frame instead of
/// `Instant::now()`. Idempotent.
pub(super) fn pin_frame_clock(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_insert_with(frame_clock_id(), Instant::now);
    });
}

/// The instant the next frame on `ctx` runs at: the pinned clock's next
/// step, or the wall clock when the context was never pinned.
pub(super) fn frame_instant(ctx: &egui::Context) -> Instant {
    ctx.data_mut(|data| {
        let pinned = data.get_temp::<Instant>(frame_clock_id())?;
        let next = pinned + SETTLED_FRAME_STEP;
        data.insert_temp(frame_clock_id(), next);
        Some(next)
    })
    .unwrap_or_else(Instant::now)
}

/// Wait for every tab's book worker and adopt what it published, so the
/// next frame lays out from a publication the test chose, not the host.
pub(super) fn settle_workers(app: &mut QuantickApp) {
    for tab in app.tabs.iter_mut() {
        tab.tape_mut().flush_for_test();
    }
}

/// Pin the clock, settle the workers, frame, then settle and frame once
/// more so anything the first frame sent the workers is on screen too.
/// Returns the settle frame's output.
pub(super) fn settled_frame(app: &mut QuantickApp, ctx: &egui::Context) -> egui::FullOutput {
    pin_frame_clock(ctx);
    settle_workers(app);
    run_frame(app, ctx);
    settle_workers(app);
    run_frame(app, ctx)
}
