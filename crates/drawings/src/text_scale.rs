//! Persisted reference for chart-relative note sizes.
use serde::{Deserialize, Serialize};

const MIN_RENDERED_TEXT_PX: f32 = 0.5;
const MAX_RENDERED_TEXT_PX: f32 = 512.0;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct TextZoom {
    px_per_bar: f32,
    size_px: f32,
}

impl TextZoom {
    pub(crate) fn valid(self) -> bool {
        self.px_per_bar.is_finite()
            && self.px_per_bar > 0.0
            && self.size_px.is_finite()
            && self.size_px > 0.0
    }

    pub(crate) fn bind(&mut self, size_px: f32, px_per_bar: f32) -> bool {
        if !px_per_bar.is_finite() || px_per_bar <= 0.0 {
            return false;
        }
        if self.valid() && self.size_px == size_px {
            return false;
        }
        *self = Self {
            px_per_bar,
            size_px,
        };
        true
    }

    pub(crate) fn rendered_size(self, size_px: f32, px_per_bar: f32) -> f32 {
        let ratio = if self.valid()
            && self.size_px == size_px
            && px_per_bar.is_finite()
            && px_per_bar > 0.0
        {
            px_per_bar / self.px_per_bar
        } else {
            1.0
        };
        (size_px * ratio).clamp(MIN_RENDERED_TEXT_PX, MAX_RENDERED_TEXT_PX)
    }
}
