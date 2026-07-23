//! User-configurable candle appearance.
//!
//! Colours are sRGB `[u8; 3]` triples (not egui `Color32`), so this module is
//! egui-free and the derivation logic — colour selection, width clamping — is
//! unit-tested in CI. The app converts to `Color32` at draw time and binds the
//! fields to egui colour pickers, which speak `[u8; 3]` directly.

/// How candles are drawn: colours, wicks and body width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleStyle {
    /// Body colour for up (close ≥ open) candles.
    pub bull: [u8; 3],
    /// Body colour for down candles.
    pub bear: [u8; 3],
    /// Wick colour, used when [`wick_matches_body`](CandleStyle::wick_matches_body) is off.
    pub wick: [u8; 3],
    /// When true, wicks take the candle's body colour (common in modern charts).
    pub wick_matches_body: bool,
    /// Whether to draw wicks at all.
    pub show_wicks: bool,
    /// Body width as a fraction of the bar slot, in `[0.1, 1.0]`.
    pub body_width_frac: f32,
    /// Chart background colour.
    pub background: [u8; 3],
}

impl Default for CandleStyle {
    /// Exocharts-like defaults: teal-green up, red down, wick matches body, on a
    /// dark background.
    fn default() -> Self {
        Self {
            bull: [38, 166, 154],
            bear: [239, 83, 80],
            wick: [150, 160, 175],
            wick_matches_body: true,
            show_wicks: true,
            body_width_frac: 0.7,
            background: [19, 23, 34],
        }
    }
}

impl CandleStyle {
    /// The body colour for an up (`true`) or down (`false`) candle.
    #[must_use]
    pub fn body_color(&self, up: bool) -> [u8; 3] {
        if up { self.bull } else { self.bear }
    }

    /// The wick colour for an up/down candle, honouring
    /// [`wick_matches_body`](CandleStyle::wick_matches_body).
    #[must_use]
    pub fn wick_color(&self, up: bool) -> [u8; 3] {
        if self.wick_matches_body {
            self.body_color(up)
        } else {
            self.wick
        }
    }

    /// The body width fraction, clamped to a sane `[0.1, 1.0]` so a candle is
    /// always visible and never wider than its slot.
    #[must_use]
    pub fn clamped_width_frac(&self) -> f32 {
        self.body_width_frac.clamp(0.1, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_exocharts_like() {
        let s = CandleStyle::default();
        assert_eq!(s.bull, [38, 166, 154]);
        assert_eq!(s.bear, [239, 83, 80]);
        assert!(s.wick_matches_body);
        assert!(s.show_wicks);
    }

    #[test]
    fn body_color_picks_by_direction() {
        let s = CandleStyle::default();
        assert_eq!(s.body_color(true), s.bull);
        assert_eq!(s.body_color(false), s.bear);
    }

    #[test]
    fn wick_matches_body_when_enabled() {
        let matched = CandleStyle {
            wick_matches_body: true,
            ..Default::default()
        };
        assert_eq!(matched.wick_color(true), matched.bull);
        assert_eq!(matched.wick_color(false), matched.bear);

        let fixed = CandleStyle {
            wick_matches_body: false,
            wick: [1, 2, 3],
            ..Default::default()
        };
        assert_eq!(fixed.wick_color(true), [1, 2, 3]);
        assert_eq!(fixed.wick_color(false), [1, 2, 3]);
    }

    #[test]
    fn width_is_clamped() {
        let over = CandleStyle {
            body_width_frac: 5.0,
            ..Default::default()
        };
        assert!((over.clamped_width_frac() - 1.0).abs() < f32::EPSILON);
        let under = CandleStyle {
            body_width_frac: 0.0,
            ..Default::default()
        };
        assert!((under.clamped_width_frac() - 0.1).abs() < f32::EPSILON);
        let mid = CandleStyle {
            body_width_frac: 0.5,
            ..Default::default()
        };
        assert!((mid.clamped_width_frac() - 0.5).abs() < f32::EPSILON);
    }
}
