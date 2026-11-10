//! Native frame admission and delivery, pinned to one market window.
//!
//! Immediate Glow child viewports do not service Screenshot commands in eframe
//! 0.29. A child target captures only its requested final paint, using the
//! same renderer readback as the root Screenshot path. No callbacks or GPU
//! reads run idle.
use super::server::elapsed_us_since;
use super::{
    CONTROL_MAX_SCREENSHOT_WAITERS, CONTROL_SCREENSHOT_GRACE_MS, ControlAccess, RawScreenshot,
    SCREENSHOT_NOTICE, UiRequest,
};
use crate::app::ControlWindow;
use eframe::egui;
use quantick_control::{
    limits::{
        CONTROL_SCREENSHOT_MAX_PIXELS, CONTROL_UI_BUDGET_US, CONTROL_UI_MAX_REQUESTS_PER_FRAME,
    },
    wire::WireU64,
};
use quantick_control_host::window_capture::CaptureTicket;
use quantick_control_schema::scene::SceneSnapshot;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "tests/rendered_capture_tests.rs"]
mod rendered_capture_tests;

/// One UI-frame owner, docked on ControlAccess as a single field.
pub(super) struct CaptureService {
    pub screenshot: Option<RawScreenshot>,
    pub frames: quantick_control_host::window_capture::WindowFrames<PaintedScene, RawScreenshot>,
    pub rendered_tx: crossbeam_channel::Sender<RenderedFrame>,
    pub rendered_rx: crossbeam_channel::Receiver<RenderedFrame>,
    pub served: usize,
    pub service_us: u64,
    pub awaiting: std::collections::VecDeque<UiRequest>,
    #[cfg(any(feature = "control-harness", test))]
    pub local_window: Option<WireU64>,
}
impl Default for CaptureService {
    fn default() -> Self {
        let (rendered_tx, rendered_rx) = crossbeam_channel::bounded(CONTROL_MAX_SCREENSHOT_WAITERS);
        Self {
            screenshot: None,
            frames: Default::default(),
            rendered_tx,
            rendered_rx,
            served: 0,
            service_us: 0,
            awaiting: Default::default(),
            #[cfg(any(feature = "control-harness", test))]
            local_window: None,
        }
    }
}

pub(super) struct PaintedScene {
    pub scene: SceneSnapshot,
    pixels_per_point: f32,
}

pub(super) fn unpainted_scene(app: &ControlWindow, window: WireU64) -> SceneSnapshot {
    SceneSnapshot {
        window_id: Some(window),
        active_tab_id: WireU64::new(app.tab_reads().tabs().active_id()),
        active_surface: Default::default(),
        focused_pane_id: None,
        focused_pane_side: crate::control::pane_side_dto(
            app.tab_reads().tabs()[app.tab_reads().active_tab_index()].focused_side(),
        ),
        controls: Vec::new(),
        covered_regions: Vec::new(),
        coverage: quantick_control_host::wire::unavailable("window_frame_not_delivered"),
    }
}
impl ControlAccess {
    #[cfg(any(feature = "control-harness", test))]
    pub(crate) fn has_screenshot(&self) -> bool {
        self.captures.frames.has_image(WireU64::new(0)) || self.captures.screenshot.is_some()
    }
    #[cfg(any(feature = "control-harness", test))]
    pub(crate) fn readable_scopes(&self) -> Vec<quantick_control::id::SnapshotScopeId> {
        self.contract.readable_scopes(&self.configured_scopes)
    }
    #[cfg(any(feature = "control-harness", test))]
    pub(crate) fn grants_screenshot(&self) -> bool {
        self.configured_scopes
            .iter()
            .any(|permission| permission.as_str() == super::evidence::SCREENSHOT_PERMISSION_ID)
    }
    #[cfg(any(feature = "control-harness", test))]
    pub(crate) fn service_screenshot(&mut self, app: &mut ControlWindow, ctx: &egui::Context) {
        self.harvest_screenshot(app, ctx);
        let window = *self
            .captures
            .local_window
            .get_or_insert_with(|| WireU64::new(app.tab_reads().tabs().windows().presented().0));
        if !self.captures.frames.has_image(window) {
            self.arm_screenshot(app, ctx, window);
        }
    }
    /// Cheap on an idle viewport: no tree, strings, or event scan without a waiter.
    pub(crate) fn needs_window_frame(&self) -> bool {
        !self.captures.awaiting.is_empty() || self.captures.frames.any_armed()
    }
    /// Called inside the matching native viewport before its new paint changes geometry.
    pub(crate) fn harvest_screenshot(&mut self, app: &mut ControlWindow, ctx: &egui::Context) {
        let window = WireU64::new(app.tab_reads().tabs().windows().presented().0);
        if !self.captures.frames.armed(window) {
            return;
        }
        let viewport = ctx.viewport_id();
        let scale = self
            .captures
            .frames
            .scene(window)
            .map_or(ctx.pixels_per_point(), |painted| painted.pixels_per_point);
        let taken = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot {
                    viewport_id, image, ..
                } if *viewport_id == viewport => {
                    let image = Arc::clone(image);
                    Some(RawScreenshot {
                        window_id: Some(window),
                        source_window_id: Some(window),
                        width_px: u32::try_from(image.size[0]).unwrap_or(0),
                        height_px: u32::try_from(image.size[1]).unwrap_or(0),
                        pixels_per_point: scale,
                        rgba: super::evidence::ScreenshotPixels::new(move || {
                            image
                                .pixels
                                .iter()
                                .flat_map(egui::Color32::to_srgba_unmultiplied)
                                .collect()
                        }),
                    })
                }
                _ => None,
            })
        });
        if let Some(raw) = taken {
            self.accept_screenshot(app, window, window, raw);
        }
    }
    pub(super) fn accept_screenshot(
        &mut self,
        app: &mut ControlWindow,
        target: WireU64,
        source: WireU64,
        raw: RawScreenshot,
    ) {
        if self.captures.frames.accept(target, source, raw) {
            app.alerts().show_toast(SCREENSHOT_NOTICE.to_owned());
        }
    }
    fn request_authorized(&self, request: &UiRequest, generation: u64) -> bool {
        request.grant_generation == generation
            && !self.revoked_connections.contains(&request.connection_id)
            && request
                .prepared
                .required_permissions
                .is_subset(&self.configured_scopes)
    }
    fn give_up_at(request: &UiRequest) -> Instant {
        request
            .deadline
            .checked_sub(Duration::from_millis(CONTROL_SCREENSHOT_GRACE_MS))
            .unwrap_or(request.deadline)
    }
    /// Deadline, authority and closed-target checks run even if that target cannot paint.
    pub(super) fn serve_awaiting_screenshot(
        &mut self,
        app: &mut ControlWindow,
        generation: u64,
        _ctx: &egui::Context,
        frame_started: Instant,
    ) -> usize {
        let mut served = 0;
        let mut waiting = std::mem::take(&mut self.captures.awaiting);
        while let Some(request) = waiting.pop_front() {
            let invalid = !self.request_authorized(&request, generation)
                || request.deadline <= Instant::now()
                || request
                    .window_id
                    .is_some_and(|window| app.pin_capture_window(Some(window)).is_err());
            let image = request.prepared.dispatch.needs_screenshot()
                && request
                    .window_id
                    .is_some_and(|window| self.captures.frames.has_image(window));
            let due = request.prepared.dispatch.needs_screenshot()
                && Instant::now() >= Self::give_up_at(&request);
            if !(invalid || image || due)
                || served >= CONTROL_UI_MAX_REQUESTS_PER_FRAME
                || (!invalid && !image && elapsed_us_since(frame_started) > CONTROL_UI_BUDGET_US)
            {
                self.captures.awaiting.push_back(request);
                continue;
            }
            let result = self.execute_on_ui(app, generation, &request);
            let _ = request.response.try_send(result);
            served += 1;
        }
        // Keep a remaining root capture armed even when this frame has no
        // projection budget. The matching paint refreshes its scene before rendering.
        let root = WireU64::new(0);
        if self.captures.awaiting.iter().any(|request| {
            request.window_id == Some(root)
                && request.prepared.dispatch.needs_screenshot()
                && self.request_authorized(request, generation)
        }) && !self.captures.frames.has_image(root)
            && self.captures.frames.rearm(root)
        {
            _ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Screenshot);
        }
        served
    }
    /// Admit only once. The target has already been pinned on the UI thread.
    pub(super) fn defer_for_screenshot(
        &mut self,
        request: &UiRequest,
        _ctx: &egui::Context,
    ) -> bool {
        request.window_id.is_some()
            && request.prepared.dispatch.needs_paint()
            && self.captures.awaiting.len()
                < quantick_control::limits::CONTROL_REQUEST_QUEUE_CAPACITY
            && (!request.prepared.dispatch.needs_screenshot()
                || self
                    .captures
                    .awaiting
                    .iter()
                    .filter(|pending| pending.prepared.dispatch.needs_screenshot())
                    .count()
                    < CONTROL_MAX_SCREENSHOT_WAITERS)
    }
    /// A single queue drain still owns admission. Viewport paints only complete
    /// admitted reads against the same count and service-time budget.
    pub(crate) fn finish_window_frame(&mut self, app: &mut ControlWindow, ctx: &egui::Context) {
        let window = WireU64::new(app.tab_reads().tabs().windows().presented().0);
        let generation = match &self.state {
            super::AccessState::Enabled(runtime) => runtime.grant_generation,
            _ => return,
        };
        let started = Instant::now();
        let mut waiting = std::mem::take(&mut self.captures.awaiting);
        while let Some(request) = waiting.pop_front() {
            let matching = request.window_id == Some(window);
            let image = self.captures.frames.has_image(window);
            let ready = !request.prepared.dispatch.needs_screenshot()
                || image
                || Instant::now() >= Self::give_up_at(&request);
            let budget = self.captures.served < CONTROL_UI_MAX_REQUESTS_PER_FRAME
                && (image
                    || self
                        .captures
                        .service_us
                        .saturating_add(elapsed_us_since(started))
                        <= CONTROL_UI_BUDGET_US);
            if !matching || !ready || !budget {
                self.captures.awaiting.push_back(request);
                continue;
            }
            let result = self.execute_on_ui(app, generation, &request);
            let _ = request.response.try_send(result);
            self.captures.served += 1;
            #[cfg(test)]
            self.observe_completion_for_test(&request.prepared.envelope.request_id);
        }
        self.captures.service_us = self
            .captures
            .service_us
            .saturating_add(elapsed_us_since(started));
        let needs_image = self.captures.awaiting.iter().any(|request| {
            request.window_id == Some(window)
                && request.prepared.dispatch.needs_screenshot()
                && self.request_authorized(request, generation)
        });
        if needs_image && !self.captures.frames.has_image(window) {
            self.arm_screenshot(app, ctx, window);
        }
        self.prune_frames();
    }
    fn arm_screenshot(&mut self, app: &ControlWindow, ctx: &egui::Context, window: WireU64) {
        let expected = crate::app::native_windows::viewport(
            quantick_workspace::arrangement::windows::WindowId(window.get()),
        );
        // An embedded fallback is not a native framebuffer for this window.
        if ctx.viewport_id() != expected {
            return;
        }
        let ticket = self.captures.frames.arm(
            window,
            PaintedScene {
                scene: super::super::scene::scene_snapshot(app),
                pixels_per_point: ctx.pixels_per_point(),
            },
        );
        if window.get() == 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        } else {
            self.captures.queue_rendered_capture(ctx, window, ticket);
        }
        ctx.request_repaint();
    }
    pub(super) fn prune_frames(&mut self) {
        let waiting = &self.captures.awaiting;
        let revoked = &self.revoked_connections;
        let scopes = &self.configured_scopes;
        let generation = self.grant_generation;
        #[cfg(any(feature = "control-harness", test))]
        let local = self.captures.local_window;
        self.captures.frames.retain(|window| {
            #[cfg(any(feature = "control-harness", test))]
            if local == Some(window) {
                return true;
            }
            waiting.iter().any(|request| {
                request.window_id == Some(window)
                    && request.grant_generation == generation
                    && !revoked.contains(&request.connection_id)
                    && request.prepared.required_permissions.is_subset(scopes)
            })
        });
    }

    pub(super) fn forget_evidence(&mut self) {
        self.evidence.clear();
        self.captures.screenshot = None;
        self.captures.frames.clear();
        #[cfg(any(feature = "control-harness", test))]
        {
            self.captures.local_window = None;
        }
        self.captures.awaiting.clear();
    }
}

#[cfg(test)]
impl ControlAccess {
    pub(crate) fn publish_screenshot_for_test(
        &mut self,
        app: &mut ControlWindow,
        mut raw: RawScreenshot,
    ) {
        let window = raw
            .window_id
            .unwrap_or_else(|| WireU64::new(app.tab_reads().tabs().windows().presented().0));
        let source = raw.source_window_id.unwrap_or(window);
        raw.window_id = Some(window);
        raw.source_window_id = Some(source);
        self.captures.frames.arm(
            window,
            PaintedScene {
                scene: super::super::scene::scene_snapshot(app),
                pixels_per_point: raw.pixels_per_point,
            },
        );
        self.accept_screenshot(app, window, source, raw);
    }

    pub(crate) fn awaiting_screenshot_for_test(&self) -> usize {
        self.captures
            .awaiting
            .iter()
            .filter(|request| request.prepared.dispatch.needs_screenshot())
            .count()
    }

    pub(crate) fn screenshot_armed_for_test(&self) -> bool {
        self.captures.frames.any_armed()
    }

    pub(crate) fn retained_evidence_for_test(&self) -> usize {
        self.evidence.retained()
    }
}

pub(super) struct RenderedFrame {
    window: WireU64,
    generation: u64,
    image: Arc<egui::ColorImage>,
    pixels_per_point: f32,
}
struct RenderCapture {
    window: WireU64,
    ticket: CaptureTicket,
    sender: crossbeam_channel::Sender<RenderedFrame>,
}
impl RenderCapture {
    fn publish(&self, size: [u32; 2], scale: f32, read: impl FnOnce() -> egui::ColorImage) {
        if !self.ticket.is_active()
            || self.sender.is_full()
            || size.contains(&0)
            || u64::from(size[0]).saturating_mul(u64::from(size[1])) > CONTROL_SCREENSHOT_MAX_PIXELS
        {
            return;
        }
        let image = Arc::new(read());
        if image.size != [size[0] as usize, size[1] as usize] || !self.ticket.is_active() {
            return;
        }
        let _ = self.sender.try_send(RenderedFrame {
            window: self.window,
            generation: self.ticket.generation(),
            image,
            pixels_per_point: scale,
        });
    }
}
impl CaptureService {
    fn queue_rendered_capture(&self, ctx: &egui::Context, window: WireU64, ticket: CaptureTicket) {
        let capture = RenderCapture {
            window,
            ticket,
            sender: self.rendered_tx.clone(),
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Debug,
            egui::Id::new("native_capture_final_paint"),
        ));
        // Appended after all target drawing; Debug is egui's last paint order.
        // Explicitly place this final layer above any debug layers already drawn.
        ctx.move_to_top(painter.layer_id());
        painter.add(egui::PaintCallback {
            rect: ctx.screen_rect(),
            callback: Arc::new(eframe::egui_glow::CallbackFn::new(move |info, painter| {
                capture.publish(info.screen_size_px, info.pixels_per_point, || {
                    painter.read_screen_rgba(info.screen_size_px)
                });
            })),
        });
    }
    pub(super) fn harvest_rendered(&mut self, app: &mut ControlWindow) {
        while let Ok(frame) = self.rendered_rx.try_recv() {
            let target = frame.window;
            let image = frame.image;
            let raw = RawScreenshot {
                window_id: Some(target),
                source_window_id: Some(target),
                width_px: u32::try_from(image.size[0]).unwrap_or(0),
                height_px: u32::try_from(image.size[1]).unwrap_or(0),
                pixels_per_point: frame.pixels_per_point,
                rgba: super::evidence::ScreenshotPixels::new(move || {
                    image
                        .pixels
                        .iter()
                        .flat_map(egui::Color32::to_srgba_unmultiplied)
                        .collect()
                }),
            };
            if self
                .frames
                .accept_generation(target, target, frame.generation, raw)
            {
                app.alerts().show_toast(SCREENSHOT_NOTICE.to_owned());
            }
        }
    }
}
