//! The independent presentation gains on the asset appearance owner:
//! `OrderflowView::bubble_display_scale` and its setter.

use super::*;
use quantick_orderflow::BubbleScaleTarget;

#[test]
fn independent_size_controls_preserve_legacy_tick_policy_and_tape_settings() {
    let mut view = OrderflowView::new("WINV26");
    view.edit_config(|config| {
        config.show_aggressions = true;
        config.volume_dots.enabled = true;
        config.volume_dots.tick_bubbles = None;
        config.live_lane.native_tape = true;
        config.live_lane.tape_only = false;
        config.live_lane.enabled = true;
        config.live_lane.show_aggressions = true;
    });
    let before = view.cached_config().clone();
    for target in [BubbleScaleTarget::Tape, BubbleScaleTarget::Candle] {
        assert_eq!(view.bubble_display_scale(target), 1.0);
        assert!(!view.set_bubble_display_scale(target, 1.0));
        assert!(!view.set_bubble_display_scale(target, f32::NAN));
        assert!(!view.set_bubble_display_scale(target, 0.0));
    }
    assert_eq!(*view.cached_config(), before, "refusals edit nothing");
    assert!(view.set_bubble_display_scale(BubbleScaleTarget::Tape, 2.0));
    assert_eq!(view.bubble_display_scale(BubbleScaleTarget::Candle), 1.0);
    assert!(view.set_bubble_display_scale(BubbleScaleTarget::Candle, 0.5));
    assert_eq!(view.bubble_display_scale(BubbleScaleTarget::Tape), 2.0);
    assert!(view.cached_config().same_tape_settings(&before));
    assert!(view.cached_config().volume_dots.tick_bubbles.is_none());
    let mut restored = view.cached_config().clone();
    restored.bubbles.display_scale = before.bubbles.display_scale;
    assert_eq!(restored, before);
}
