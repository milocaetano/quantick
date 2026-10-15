//! Independent presentation controls use the existing asset appearance owner.
use super::OrderflowView;
use quantick_orderflow::{BubbleScaleTarget, MAX_BUBBLE_DISPLAY_SCALE, MIN_BUBBLE_DISPLAY_SCALE};

impl OrderflowView {
    pub(crate) fn bubble_display_scale(&self, target: BubbleScaleTarget) -> Option<f32> {
        let scale = self.config.bubbles.display_scale;
        match target {
            BubbleScaleTarget::Tape => self.config.lane_aggressions_drawn().then_some(scale.tape),
            BubbleScaleTarget::Candle => {
                (self.config.show_aggressions && !self.config.tape_only()).then_some(scale.candle)
            }
        }
    }

    /// Reject invalid input before it can mark the asset dirty or reach a worker.
    pub(crate) fn set_bubble_display_scale(
        &mut self,
        target: BubbleScaleTarget,
        value: f32,
    ) -> bool {
        if !value.is_finite()
            || !(MIN_BUBBLE_DISPLAY_SCALE..=MAX_BUBBLE_DISPLAY_SCALE).contains(&value)
            || self.bubble_display_scale(target).is_none()
        {
            return false;
        }
        self.edit_config(|config| match target {
            BubbleScaleTarget::Tape => config.bubbles.display_scale.tape = value,
            BubbleScaleTarget::Candle => config.bubbles.display_scale.candle = value,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(view.bubble_display_scale(target), Some(1.0));
            assert!(!view.set_bubble_display_scale(target, 1.0));
            assert!(!view.set_bubble_display_scale(target, f32::NAN));
            assert!(!view.set_bubble_display_scale(target, 0.0));
        }
        assert!(view.set_bubble_display_scale(BubbleScaleTarget::Tape, 2.0));
        assert_eq!(
            view.bubble_display_scale(BubbleScaleTarget::Candle),
            Some(1.0)
        );
        assert!(view.set_bubble_display_scale(BubbleScaleTarget::Candle, 0.5));
        assert_eq!(
            view.bubble_display_scale(BubbleScaleTarget::Tape),
            Some(2.0)
        );
        assert!(view.cached_config().same_tape_settings(&before));
        assert!(view.cached_config().volume_dots.tick_bubbles.is_none());
        let mut restored = view.cached_config().clone();
        restored.bubbles.display_scale = before.bubbles.display_scale;
        assert_eq!(restored, before);
    }
}
