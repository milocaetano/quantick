//! Immediate Glow child viewports do not service Screenshot commands in eframe
//! 0.29. Capture only a requested target's final paint, using the same renderer
//! readback as its root Screenshot path. No callbacks or GPU reads run idle.
use super::{ControlAccess, RawScreenshot};
use eframe::egui;
use quantick_control::{limits::CONTROL_SCREENSHOT_MAX_PIXELS, wire::WireU64};
use quantick_control_host::window_capture::CaptureTicket;
use std::sync::Arc;

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
impl ControlAccess {
    pub(super) fn queue_rendered_capture(
        &self,
        ctx: &egui::Context,
        window: WireU64,
        ticket: CaptureTicket,
    ) {
        let capture = RenderCapture {
            window,
            ticket,
            sender: self.captures.rendered_tx.clone(),
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
    pub(super) fn harvest_rendered(&mut self, app: &mut crate::app::ControlWindow) {
        while let Ok(frame) = self.captures.rendered_rx.try_recv() {
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
                .captures
                .frames
                .accept_generation(target, target, frame.generation, raw)
            {
                app.alerts().show_toast(super::SCREENSHOT_NOTICE.to_owned());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_mailbox_is_bounded_targeted_and_revocable_before_gpu_readback() {
        let mut frames = quantick_control_host::window_capture::WindowFrames::<(), ()>::default();
        let window = WireU64::new(9);
        let ticket = frames.arm(window, ());
        let (sender, receiver) = crossbeam_channel::bounded(1);
        let capture = RenderCapture {
            window,
            ticket,
            sender,
        };
        capture.publish([2, 3], 1.5, || {
            egui::ColorImage::new([2, 3], egui::Color32::RED)
        });
        capture.publish([2, 3], 1.5, || panic!("full mailbox must not read pixels"));
        let frame = receiver.try_recv().unwrap();
        assert_eq!(frame.window, window);
        assert_eq!(frame.pixels_per_point, 1.5);
        assert_eq!(frame.image.size, [2, 3]);
        frames.clear();
        capture.publish([2, 3], 1.5, || panic!("revoked paint must not read pixels"));
        assert!(receiver.is_empty());
        let capture = RenderCapture {
            window,
            ticket: frames.arm(window, ()),
            sender: capture.sender,
        };
        capture.publish([u32::MAX, u32::MAX], 1.0, || {
            panic!("oversized framebuffer must not allocate")
        });
        assert!(receiver.is_empty());
    }
}
