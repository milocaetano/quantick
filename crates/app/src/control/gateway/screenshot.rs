//! The screenshot service: what a client may be shown of this window.
//!
//! A screenshot is the one read that cannot be answered from state alone -- it
//! needs a painted frame -- so the request is parked, armed against the next
//! repaint, harvested when the pixels arrive, and answered late. That whole
//! arc lives here. The frame service it is armed against (`execute_on_ui`,
//! `begin_frame`) stays with the host: those two run every request, not only
//! the ones that want pixels.

use std::sync::Arc;
use std::time::{Duration, Instant};

use quantick_control::error::codes;
use quantick_control::limits::{CONTROL_UI_BUDGET_US, CONTROL_UI_MAX_REQUESTS_PER_FRAME};

use crate::app::QuantickApp;

use super::{
    CONTROL_MAX_SCREENSHOT_WAITERS, CONTROL_SCREENSHOT_GRACE_MS, ControlAccess, RawScreenshot,
    SCREENSHOT_NOTICE, UiRequest, known_error,
};
// The one thing this file borrows from the other side of the seam: an
// arithmetic helper, so both threads report a duration the same way.
use super::server::elapsed_us_since;

impl ControlAccess {
    /// Hand the gateway a rasterised frame the way the window would.
    ///
    /// A headless `egui::Context` answers no `ViewportCommand::Screenshot`, so
    /// the correlation between an image and the scene captured beside it can
    /// only be proved by supplying the pixels here. Everything downstream —
    /// the stamping, the point-to-pixel scaling, the encoding, the digest — is
    /// the shipped path, unchanged.
    #[cfg(test)]
    pub(crate) fn publish_screenshot_for_test(
        &mut self,
        app: &mut QuantickApp,
        raw: RawScreenshot,
    ) {
        self.accept_screenshot(app, raw);
    }

    /// Whether a capture is still waiting for the window to be rasterised.
    #[cfg(test)]
    pub(crate) fn awaiting_screenshot_for_test(&self) -> usize {
        self.awaiting_screenshot.len()
    }

    /// How many bundles this instance is holding.
    #[cfg(test)]
    pub(crate) fn retained_evidence_for_test(&self) -> usize {
        self.evidence.retained()
    }

    /// Whether this window is holding a rasterised frame a read could use.
    pub(crate) fn has_screenshot(&self) -> bool {
        self.screenshot.is_some()
    }

    /// The registered snapshot scopes the configured grant already reaches.
    pub(crate) fn readable_scopes(&self) -> Vec<quantick_control::id::SnapshotScopeId> {
        self.contract.readable_scopes(&self.configured_scopes)
    }

    /// Whether the configured grant permits rasterising the window.
    ///
    /// Asked *before* anything arms a rasterise, because taking the picture is
    /// what raises the notice: a caller that will be refused the scope one
    /// step later must not first tell the trader their window was captured.
    /// The indicator only means something if it is never wrong.
    pub(crate) fn grants_screenshot(&self) -> bool {
        self.configured_scopes
            .iter()
            .any(|permission| permission.as_str() == super::evidence::SCREENSHOT_PERMISSION_ID)
    }

    /// Ask the window to rasterise itself for the next read that wants it,
    /// and take the frame if one has already arrived.
    ///
    /// The same arming and the same harvest a deferred remote capture gets.
    /// Both halves, because a local read runs whether or not the gateway is
    /// enabled while the frame service that normally harvests runs only when
    /// it is — so a hook that armed but could not harvest would wait out its
    /// whole budget for a frame sitting in the input queue.
    pub(crate) fn service_screenshot(
        &mut self,
        app: &mut QuantickApp,
        ctx: &eframe::egui::Context,
    ) {
        self.harvest_screenshot(app, ctx);
        if self.screenshot.is_none() {
            self.arm_screenshot(ctx);
        }
    }

    /// Take the rasterised frame the window was asked for, if it has arrived.
    ///
    /// Costs a frame nothing until something arms it: with no capture waiting
    /// the input scan does not run at all. When one does arrive the trader is
    /// told, because a picture of their window leaving the process is not
    /// something that should happen quietly (threat model O-18).
    pub(super) fn harvest_screenshot(
        &mut self,
        app: &mut QuantickApp,
        ctx: &eframe::egui::Context,
    ) {
        if !self.screenshot_armed {
            return;
        }
        // Inside the input lock: clone the image's handle and nothing else.
        // The rows are converted by the closure below, on the response worker
        // — a 4K framebuffer is eight million pixels, and paying for that here
        // would blow a 250 microsecond frame budget by two orders of
        // magnitude every time an agent asks for a picture.
        let taken = ctx.input(|input| {
            let pixels_per_point = input.pixels_per_point();
            input.events.iter().find_map(|event| match event {
                eframe::egui::Event::Screenshot { image, .. } => {
                    let image = Arc::clone(image);
                    Some(RawScreenshot {
                        width_px: u32::try_from(image.size[0]).unwrap_or(0),
                        height_px: u32::try_from(image.size[1]).unwrap_or(0),
                        pixels_per_point,
                        rgba: super::evidence::ScreenshotPixels::new(move || {
                            image
                                .pixels
                                .iter()
                                // Unmultiplied: the toolkit stores colours
                                // premultiplied by alpha and a PNG's are not,
                                // so a translucent surface would darken on the
                                // way out if the raw bytes were copied across.
                                .flat_map(eframe::egui::Color32::to_srgba_unmultiplied)
                                .collect()
                        }),
                    })
                }
                _ => None,
            })
        });
        if let Some(raw) = taken {
            self.accept_screenshot(app, raw);
        }
    }

    /// Take one rasterised frame, and tell the person at the window.
    ///
    /// The single door for pixels entering the control plane, so the notice
    /// cannot be bypassed by whatever hands them over — the window's own
    /// screenshot event today, a test's fixture in the same breath.
    pub(super) fn accept_screenshot(&mut self, app: &mut QuantickApp, raw: RawScreenshot) {
        self.screenshot_armed = false;
        self.screenshot = Some(raw);
        app.show_agent_toast(SCREENSHOT_NOTICE.to_owned());
    }

    /// Run the captures that were waiting for an image, or give up on them,
    /// and report how much of the frame's request budget they spent.
    ///
    /// Returns before touching anything when none is waiting, which is every
    /// frame of every session where no client asked for a picture.
    pub(super) fn serve_awaiting_screenshot(
        &mut self,
        app: &mut QuantickApp,
        generation: u64,
        ctx: &eframe::egui::Context,
        frame_started: Instant,
    ) -> usize {
        if self.awaiting_screenshot.is_empty() {
            return 0;
        }
        let now = Instant::now();
        let mut served = 0usize;
        let mut waiting = std::mem::take(&mut self.awaiting_screenshot);
        while let Some(request) = waiting.pop_front() {
            // Revocation applies to work already in flight, and a parked
            // capture is in flight. Answered here rather than left to park,
            // because parking is what keeps asking the window to rasterise —
            // and a revoked connection must not be able to make the trader's
            // window flash a capture notice for a request that will be refused
            // anyway.
            if request.grant_generation != generation
                || self.revoked_connections.contains(&request.connection_id)
            {
                let _ = request.response.try_send(Err(known_error(
                    codes::PERMISSION_DENIED,
                    "connection authority was revoked while the capture waited for a frame",
                    false,
                )));
                continue;
            }
            // These captures spend the same frame budget the drain does, and
            // they run before it. Left uncounted, four waiters plus the
            // drain's own four would put eight projection passes in one frame
            // against a documented ceiling of four, and the drain would find
            // its budget already gone. A waiter this frame cannot serve stays
            // queued and is served by the next one.
            if served >= CONTROL_UI_MAX_REQUESTS_PER_FRAME
                || elapsed_us_since(frame_started) > CONTROL_UI_BUDGET_US
            {
                self.awaiting_screenshot.push_back(request);
                self.awaiting_screenshot.extend(waiting);
                ctx.request_repaint();
                return served;
            }
            // Given up on *before* the deadline, not at it. `execute_on_ui`
            // refuses an expired request with `control.timeout` as its very
            // first act, so waiting to the last moment would answer a window
            // that never presented with a bare timeout — no bundle, no
            // `screenshot: frame_not_delivered`, and none of the text, events
            // and configuration that were collectable all along. The grace is
            // what buys the honest answer room to be built.
            let give_up_at = request
                .deadline
                .checked_sub(Duration::from_millis(CONTROL_SCREENSHOT_GRACE_MS))
                .unwrap_or(request.deadline);
            if self.screenshot.is_none() && now < give_up_at {
                self.arm_screenshot(ctx);
                self.awaiting_screenshot.push_back(request);
                continue;
            }
            let result = self.execute_on_ui(app, generation, &request);
            let _ = request.response.try_send(result);
            served = served.saturating_add(1);
        }
        // Nothing is waiting any more, so nothing is owed a rasterise. Left
        // set, the flag would suppress every future arming for the rest of the
        // session (see `arm_screenshot`).
        if self.awaiting_screenshot.is_empty() {
            self.screenshot_armed = false;
        }
        served
    }

    /// Whether this request has to wait a frame for the window to be
    /// rasterised, arming the rasterise if so.
    pub(super) fn defer_for_screenshot(
        &mut self,
        request: &UiRequest,
        ctx: &eframe::egui::Context,
    ) -> bool {
        if !request.prepared.dispatch.needs_screenshot() || self.screenshot.is_some() {
            return false;
        }
        if self.awaiting_screenshot.len() >= CONTROL_MAX_SCREENSHOT_WAITERS {
            return false;
        }
        self.arm_screenshot(ctx);
        true
    }

    /// Ask the window to rasterise itself, and keep asking while someone is
    /// waiting.
    ///
    /// The command is sent every time rather than once per arming. A window
    /// that is minimised, occluded, or between viewport states can swallow the
    /// request and produce no event; an arming that latched would then park
    /// every future capture behind a command nobody would ever send again, for
    /// the rest of the session. Repeating it costs one viewport command per
    /// frame, and only while a capture is actually waiting for a picture.
    pub(super) fn arm_screenshot(&mut self, ctx: &eframe::egui::Context) {
        self.screenshot_armed = true;
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Screenshot);
        ctx.request_repaint();
    }

    /// Forget every retained bundle and everything staged around one.
    ///
    /// One function, called by both teardown paths, so the two cannot disagree
    /// about what a withdrawal clears — which is exactly the kind of omission
    /// nobody notices until a screenshot flag outlives the door it came
    /// through.
    pub(super) fn forget_evidence(&mut self) {
        self.evidence.clear();
        self.screenshot = None;
        self.screenshot_armed = false;
        self.awaiting_screenshot.clear();
    }
}
