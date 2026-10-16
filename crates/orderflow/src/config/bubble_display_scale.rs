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

    /// The gains a pane paints. The candle gain belongs to a tick chart:
    /// both of its setters (the pane's "Tick bubble size" slider and
    /// `orderflow.bubbles.size.set`) refuse it anywhere else, so a pane on
    /// any other bar spec paints its candle prints at the default gain even
    /// when the look carries one a tick chart set.
    #[must_use]
    pub fn painted(self, tick_chart: bool) -> Self {
        Self {
            candle: if tick_chart {
                self.candle
            } else {
                DEFAULT_BUBBLE_DISPLAY_SCALE
            },
            ..self
        }
    }

    pub fn set(&mut self, target: BubbleScaleTarget, scale: f32) {
        let value = sane_bubble_display_scale(scale);
        match target {
            BubbleScaleTarget::Tape => self.tape = value,
            BubbleScaleTarget::Candle => self.candle = value,
        }
    }

    /// Set `target` to `scale` only when [`admitted_bubble_display_scale`]
    /// admits it; a refused gain leaves the pair as it was. Answers whether
    /// it was admitted. Every setter a person or a caller drives goes here,
    /// never through the clamping [`Self::set`].
    pub fn try_set(&mut self, target: BubbleScaleTarget, scale: f32) -> bool {
        let admitted = admitted_bubble_display_scale(f64::from(scale));
        if let Some(value) = admitted {
            self.set(target, value);
        }
        admitted.is_some()
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

/// The gain `scale` as the display keeps it, or `None` when it is not
/// finite or falls outside [`MIN_BUBBLE_DISPLAY_SCALE`] through
/// [`MAX_BUBBLE_DISPLAY_SCALE`]. Checked in `f64`, so a requested value just
/// outside the range never rounds into it.
#[must_use]
pub fn admitted_bubble_display_scale(scale: f64) -> Option<f32> {
    let range = f64::from(MIN_BUBBLE_DISPLAY_SCALE)..=f64::from(MAX_BUBBLE_DISPLAY_SCALE);
    (scale.is_finite() && range.contains(&scale)).then_some(scale as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BubbleStyle, HeatmapConfig};

    /// The slider and `orderflow.bubbles.size.set` both set through
    /// `try_set`: a refused gain changes nothing, an admitted one moves only
    /// its own target.
    #[test]
    fn try_set_admits_only_finite_in_range_gains_and_moves_one_target() {
        for refused in [
            f64::NAN,
            f64::INFINITY,
            0.0,
            -1.0,
            0.249_999_999_9,
            3.000_000_1,
        ] {
            assert_eq!(admitted_bubble_display_scale(refused), None, "{refused}");
        }
        for admitted in [0.25, 1.0, 3.0] {
            assert_eq!(
                admitted_bubble_display_scale(admitted),
                Some(admitted as f32)
            );
        }
        let mut scale = BubbleDisplayScale::default();
        for target in [BubbleScaleTarget::Tape, BubbleScaleTarget::Candle] {
            for refused in [f32::NAN, f32::INFINITY, 0.0, 4.0] {
                assert!(!scale.try_set(target, refused));
                assert_eq!(scale, BubbleDisplayScale::default());
            }
        }
        assert!(scale.try_set(BubbleScaleTarget::Tape, 2.0));
        assert!(scale.try_set(BubbleScaleTarget::Candle, 0.5));
        assert_eq!(
            scale,
            BubbleDisplayScale {
                tape: 2.0,
                candle: 0.5
            }
        );
    }

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

    #[test]
    fn the_candle_gain_paints_only_on_a_tick_chart() {
        let set = BubbleDisplayScale {
            tape: 2.0,
            candle: 0.5,
        };
        assert_eq!(set.painted(true), set);
        assert_eq!(
            set.painted(false),
            BubbleDisplayScale {
                tape: 2.0,
                candle: 1.0
            }
        );
    }
}
