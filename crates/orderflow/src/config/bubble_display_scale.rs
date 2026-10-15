//! Independent presentation gains; neither value participates in grouping.
use serde::{Deserialize, Serialize};

pub const MIN_BUBBLE_DISPLAY_SCALE: f32 = 0.25;
pub const MAX_BUBBLE_DISPLAY_SCALE: f32 = 3.0;
pub const DEFAULT_BUBBLE_DISPLAY_SCALE: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BubbleScaleTarget {
    Tape,
    Candle,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BubbleDisplayScale {
    pub tape: f32,
    pub candle: f32,
}

impl Default for BubbleDisplayScale {
    fn default() -> Self {
        Self {
            tape: DEFAULT_BUBBLE_DISPLAY_SCALE,
            candle: DEFAULT_BUBBLE_DISPLAY_SCALE,
        }
    }
}

impl BubbleDisplayScale {
    #[must_use]
    pub fn sanitized(self) -> Self {
        Self {
            tape: sane_bubble_display_scale(self.tape),
            candle: sane_bubble_display_scale(self.candle),
        }
    }

    #[must_use]
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    #[must_use]
    pub fn get(self, target: BubbleScaleTarget) -> f32 {
        sane_bubble_display_scale(match target {
            BubbleScaleTarget::Tape => self.tape,
            BubbleScaleTarget::Candle => self.candle,
        })
    }

    pub fn set(&mut self, target: BubbleScaleTarget, scale: f32) {
        let value = sane_bubble_display_scale(scale);
        match target {
            BubbleScaleTarget::Tape => self.tape = value,
            BubbleScaleTarget::Candle => self.candle = value,
        }
    }
}

#[must_use]
pub fn sane_bubble_display_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(MIN_BUBBLE_DISPLAY_SCALE, MAX_BUBBLE_DISPLAY_SCALE)
    } else {
        DEFAULT_BUBBLE_DISPLAY_SCALE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BubbleStyle, HeatmapConfig};

    #[test]
    fn omitted_identity_preserves_legacy_presets_and_partial_pairs_default() {
        let mut style = BubbleStyle::default();
        let old = toml::to_string(&style).unwrap();
        assert!(!old.contains("display_scale"));
        assert_eq!(toml::from_str::<BubbleStyle>(&old).unwrap(), style);
        assert_eq!(
            toml::from_str::<BubbleDisplayScale>("tape = 0.5").unwrap(),
            BubbleDisplayScale {
                tape: 0.5,
                candle: 1.0
            }
        );
        style.display_scale.set(BubbleScaleTarget::Candle, 2.0);
        assert_eq!(
            toml::from_str::<BubbleStyle>(&toml::to_string(&style).unwrap()).unwrap(),
            style
        );
    }

    #[test]
    fn gains_are_bounded_independent_and_do_not_invalidate_factual_tape() {
        let base = HeatmapConfig::default();
        let mut changed = base.clone();
        changed.bubbles.display_scale = BubbleDisplayScale {
            tape: 0.0,
            candle: 99.0,
        };
        changed.bubbles.sanitize();
        assert_eq!(
            changed.bubbles.display_scale,
            BubbleDisplayScale {
                tape: 0.25,
                candle: 3.0
            }
        );
        assert!(base.same_tape_settings(&changed));
        assert!(base.same_tape_grouping(&changed));
        assert_eq!(base.volume_dots.tick_bubbles, None);
        assert_eq!(changed.volume_dots.tick_bubbles, None);
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(sane_bubble_display_scale(value), 1.0);
        }
        changed
            .bubbles
            .display_scale
            .set(BubbleScaleTarget::Tape, 0.5);
        assert_eq!(changed.bubbles.display_scale.candle, 3.0);
    }
}
