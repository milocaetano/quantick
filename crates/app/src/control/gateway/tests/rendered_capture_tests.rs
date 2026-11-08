//! The child-viewport capture mailbox: bounded, targeted and revocable
//! before any GPU readback.
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
