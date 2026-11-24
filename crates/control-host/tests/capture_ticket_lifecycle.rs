use quantick_control::wire::WireU64;
use quantick_control_host::window_capture::WindowFrames;

#[test]
fn a_foreign_generation_cannot_consume_another_windows_capture() {
    let root = WireU64::new(1);
    let child = WireU64::new(2);
    let mut frames = WindowFrames::default();
    let root_ticket = frames.arm(root, "root");
    let child_ticket = frames.arm(child, "child");
    assert!(!frames.accept_generation(root, root, child_ticket.generation(), "foreign"));
    assert!(root_ticket.is_active());
    assert!(child_ticket.is_active());
    assert!(frames.armed(root));
    assert!(!frames.has_image(root));
    assert!(frames.accept_generation(root, root, root_ticket.generation(), "root pixels"));
    assert_eq!(frames.take_image(root), Some("root pixels"));
    assert!(frames.armed(child));
}

#[test]
fn a_wrong_source_does_not_cancel_the_correct_generations_ticket() {
    let window = WireU64::new(1);
    let mut frames = WindowFrames::default();
    let ticket = frames.arm(window, "scene");
    assert!(!frames.accept_generation(window, WireU64::new(2), ticket.generation(), "wrong"));
    assert!(ticket.is_active());
    assert!(frames.armed(window));
    assert!(frames.accept_generation(window, window, ticket.generation(), "correct"));
    assert!(!ticket.is_active());
    assert_eq!(frames.take_image(window), Some("correct"));
}

#[test]
fn duplicate_callback_cannot_replace_the_accepted_image() {
    let window = WireU64::new(1);
    let mut frames = WindowFrames::default();
    let ticket = frames.arm(window, "scene");
    let callback_copy = ticket.clone();
    assert!(frames.accept_generation(window, window, ticket.generation(), "first"));
    assert!(!callback_copy.is_active());
    assert!(!frames.accept_generation(window, window, callback_copy.generation(), "second"));
    assert!(!frames.accept(window, window, "unversioned duplicate"));
    assert_eq!(frames.take_image(window), Some("first"));
    assert_eq!(frames.take_image(window), None);
}

#[test]
fn rearming_for_transport_retry_keeps_the_pending_scene_and_generation() {
    let window = WireU64::new(1);
    let mut frames = WindowFrames::default();
    let ticket = frames.arm(window, "requested scene");
    assert!(frames.rearm(window));
    assert!(ticket.is_active());
    assert_eq!(frames.scene(window), Some(&"requested scene"));
    assert!(frames.accept_generation(window, window, ticket.generation(), "retried pixels"));
    assert_eq!(frames.take_image(window), Some("retried pixels"));
    assert!(!frames.any_armed());
}

#[test]
fn a_new_capture_after_consumption_gets_a_new_scene_and_generation() {
    let window = WireU64::new(1);
    let mut frames = WindowFrames::default();
    let first = frames.arm(window, "before");
    frames.accept_generation(window, window, first.generation(), "old pixels");
    assert_eq!(frames.take_image(window), Some("old pixels"));
    let next = frames.arm(window, "after");
    assert!(next.generation() > first.generation());
    assert_eq!(frames.scene(window), Some(&"after"));
    assert!(!first.is_active());
    assert!(!frames.accept_generation(window, window, first.generation(), "late pixels"));
    assert!(frames.accept_generation(window, window, next.generation(), "new pixels"));
    assert_eq!(frames.take_image(window), Some("new pixels"));
}

#[test]
fn retaining_one_window_cancels_only_removed_callbacks() {
    let root = WireU64::new(1);
    let child = WireU64::new(2);
    let mut frames = WindowFrames::default();
    let root_ticket = frames.arm(root, "root");
    let child_ticket = frames.arm(child, "child");
    frames.retain(|window| window == child);
    assert!(!root_ticket.is_active());
    assert!(child_ticket.is_active());
    assert_eq!(frames.scene(root), None);
    assert!(!frames.rearm(root));
    assert!(frames.any_armed());
    assert!(!frames.accept_generation(root, root, root_ticket.generation(), "late root"));
    assert!(frames.accept_generation(child, child, child_ticket.generation(), "child pixels"));
    assert_eq!(frames.take_image(child), Some("child pixels"));
}

#[test]
fn clearing_cancels_pending_callbacks_and_discards_already_delivered_images() {
    let root = WireU64::new(1);
    let child = WireU64::new(2);
    let mut frames = WindowFrames::default();
    let pending = frames.arm(root, "root");
    let delivered = frames.arm(child, "child");
    assert!(frames.accept_generation(child, child, delivered.generation(), "child pixels"));
    frames.clear();
    assert!(!pending.is_active());
    assert!(!delivered.is_active());
    assert!(!frames.any_armed());
    assert_eq!(frames.take_image(child), None);
    assert_eq!(frames.scene(root), None);
    assert_eq!(frames.scene(child), None);
    let reopened = frames.arm(root, "reopened");
    assert!(reopened.generation() > delivered.generation());
    assert!(!frames.accept_generation(root, root, pending.generation(), "late root"));
    assert!(frames.accept_generation(root, root, reopened.generation(), "fresh root"));
}

#[test]
fn unknown_window_operations_do_not_allocate_or_arm_a_capture() {
    let window = WireU64::new(42);
    let mut frames = WindowFrames::<&str, &str>::default();
    assert!(!frames.rearm(window));
    assert!(!frames.accept(window, window, "unsolicited"));
    assert!(!frames.accept_generation(window, window, 1, "unsolicited"));
    assert!(!frames.any_armed());
    assert_eq!(frames.take_image(window), None);
    assert_eq!(frames.scene(window), None);
    let first = frames.arm(window, "requested");
    assert_eq!(first.generation(), 1);
    assert!(frames.accept_generation(window, window, first.generation(), "solicited"));
    assert_eq!(frames.take_image(window), Some("solicited"));
}
