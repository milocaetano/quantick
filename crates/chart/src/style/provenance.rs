//! Theme inheritance is a statement of ownership, never a comparison of values.
use super::{CandlePreset, ChartStyle};
use quantick_appearance::{Inheritance, ResolvedTheme, Token};

/// Missing legacy markers mean explicit. Only fresh charts start inherited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChartColorOwnership {
    pub fill_opacity: Inheritance,
    pub bull_fill: Inheritance,
    pub bear_fill: Inheritance,
    pub bull_outline: Inheritance,
    pub bear_outline: Inheritance,
    pub wick: Inheritance,
    pub background: Inheritance,
    pub grid: Inheritance,
}
impl ChartColorOwnership {
    pub const fn inherited() -> Self {
        Self {
            fill_opacity: Inheritance::Theme,
            bull_fill: Inheritance::Theme,
            bear_fill: Inheritance::Theme,
            bull_outline: Inheritance::Theme,
            bear_outline: Inheritance::Theme,
            wick: Inheritance::Theme,
            background: Inheritance::Theme,
            grid: Inheritance::Theme,
        }
    }
}
impl ChartStyle {
    /// Capture the whole visible look for a user-named preset.
    pub fn snapshot(mut self) -> Self {
        self.ownership = ChartColorOwnership::default();
        self
    }
    /// A built-in preset owns candle colors and opacity, leaving canvas ownership intact.
    pub fn apply_preset(&mut self, preset: CandlePreset) {
        self.candles = preset.style();
        self.ownership = ChartColorOwnership {
            background: self.ownership.background,
            grid: self.ownership.grid,
            ..ChartColorOwnership::default()
        };
    }
    /// Resolve inherited fields from a validated theme. RGB-only fields are
    /// guaranteed opaque by the appearance resolver, so no alpha is discarded.
    pub fn apply_theme(&mut self, theme: &ResolvedTheme) -> bool {
        let before = *self;
        self.candles.fill_opacity = self
            .ownership
            .fill_opacity
            .resolve(self.candles.fill_opacity, || theme.candle_fill_opacity);
        let rgb = |token| {
            let [r, g, b, _] = theme.color(token).to_array();
            [r, g, b]
        };
        self.candles.bull_fill = self
            .ownership
            .bull_fill
            .resolve(self.candles.bull_fill, || rgb(Token::Buy));
        self.candles.bear_fill = self
            .ownership
            .bear_fill
            .resolve(self.candles.bear_fill, || rgb(Token::Sell));
        self.candles.bull_outline = self
            .ownership
            .bull_outline
            .resolve(self.candles.bull_outline, || rgb(Token::CandleBullOutline));
        self.candles.bear_outline = self
            .ownership
            .bear_outline
            .resolve(self.candles.bear_outline, || rgb(Token::CandleBearOutline));
        self.candles.wick = self
            .ownership
            .wick
            .resolve(self.candles.wick, || rgb(Token::CandleWick));
        self.canvas.background = self
            .ownership
            .background
            .resolve(self.canvas.background, || rgb(Token::Canvas));
        self.canvas.grid = self
            .ownership
            .grid
            .resolve(self.canvas.grid, || rgb(Token::Control));
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_opacity_changes_fresh_fills_but_preserves_authored_modes_and_presets() {
        let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        let dark = ResolvedTheme::default();
        let mut style = ChartStyle::default();
        style.candles.wick_width = 3.0;
        style.apply_theme(&light);
        assert_eq!(
            style.candles.resolved(true, false).fill,
            Some([76, 175, 80, 255])
        );
        assert_eq!(
            style.candles.resolved(false, false).fill,
            Some([0, 0, 0, 255])
        );
        assert_eq!(style.candles.wick_width, 3.0);
        style.apply_theme(&dark);
        assert_eq!(style.candles.fill_opacity, 0.20);
        style.candles.body_mode = super::super::CandleBodyMode::OutlineOnly;
        style.apply_theme(&light);
        assert_eq!(style.candles.resolved(true, false).fill, None);
        for preset in CandlePreset::ALL {
            style.apply_preset(preset);
            style.apply_theme(&light);
            assert_eq!(style.candles, preset.style());
            assert_eq!(style.ownership.fill_opacity, Inheritance::Explicit);
        }
    }
    #[test]
    fn width_only_edit_preserves_inheritance_and_equal_default_explicit_survives_switch() {
        let mut style = ChartStyle::default();
        let original = style.candles.bull_fill;
        style.candles.wick_width = 3.0;
        style.ownership.bull_fill = Inheritance::Explicit;
        let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        assert!(style.apply_theme(&light));
        assert_eq!(style.candles.bull_fill, original);
        assert_eq!(style.candles.wick_width, 3.0);
        assert_ne!(style.canvas, ChartStyle::default().canvas);
        assert_ne!(
            style.candles.bear_fill,
            ChartStyle::default().candles.bear_fill
        );
        assert!(!style.apply_theme(&light));
    }
    #[test]
    fn dark_is_exact_and_presets_capture_intended_fields() {
        let mut style = ChartStyle::default();
        assert!(!style.apply_theme(&ResolvedTheme::default()));
        style.apply_preset(CandlePreset::Classic);
        assert_eq!(style.ownership.bull_fill, Inheritance::Explicit);
        assert_eq!(style.ownership.background, Inheritance::Theme);
        let mut saved = style.snapshot();
        assert!(!saved.apply_theme(&ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap()));
    }
}

/// Cheap frame gate: re-resolve only after the theme or chart inputs change.
#[derive(Debug, Default)]
pub struct ChartThemeSync {
    last: Option<(u64, ChartStyle)>,
}
impl ChartThemeSync {
    pub fn sync(
        &mut self,
        style: &mut ChartStyle,
        appearance: &quantick_appearance::Appearance,
    ) -> bool {
        if self.last == Some((appearance.revision(), *style)) {
            return false;
        }
        let changed = style.apply_theme(appearance.resolved());
        self.last = Some((appearance.revision(), *style));
        changed
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    #[test]
    fn theme_and_override_changes_are_the_only_resolution_inputs() {
        let mut sync = ChartThemeSync::default();
        let mut style = ChartStyle::default();
        let mut appearance = quantick_appearance::Appearance::default();
        assert!(!sync.sync(&mut style, &appearance));
        assert!(!sync.sync(&mut style, &appearance));
        appearance.apply(quantick_appearance::LIGHT).unwrap();
        assert!(sync.sync(&mut style, &appearance));
        assert!(!sync.sync(&mut style, &appearance));
        style = ChartStyle::default();
        assert!(sync.sync(&mut style, &appearance));
    }
}

#[cfg(test)]
mod bookmap_tests {
    use super::*;

    #[test]
    fn surface_resolution_preserves_explicit_styles_and_saved_snapshots() {
        let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        let dark = ResolvedTheme::default();
        let mut shared = ChartStyle::default();
        shared.apply_theme(&light);
        let mut flow = shared;
        flow.apply_theme(light.bookmap());
        let mut expected_flow = ChartStyle::default();
        expected_flow.canvas.background = [0x43, 0x4A, 0x52];
        expected_flow.candles.wick = [0xB6, 0xC9, 0xD2];
        assert_eq!(flow, expected_flow);
        for bullish in [false, true] {
            assert_eq!(
                flow.candles.resolved(bullish, false),
                ChartStyle::default().candles.resolved(bullish, false)
            );
        }
        flow.apply_theme(dark.bookmap());
        assert_eq!(flow, ChartStyle::default());
        assert_eq!(shared.candles.fill_opacity, 1.0);
        let saved = shared.snapshot();
        let mut explicit = saved;
        explicit.apply_theme(light.bookmap());
        assert_eq!(explicit, saved);
        for preset in CandlePreset::ALL {
            let mut configured = shared;
            configured.apply_preset(preset);
            configured.apply_theme(light.bookmap());
            assert_eq!(configured.candles, preset.style());
        }
        // Explicit fields equal to the current UI defaults are still explicit.
        shared.ownership.fill_opacity = Inheritance::Explicit;
        shared.ownership.bull_fill = Inheritance::Explicit;
        shared.candles.wick_width = 3.0;
        for surface in [light.bookmap(), &dark, light.bookmap()] {
            shared.apply_theme(surface);
            assert_eq!(shared.candles.fill_opacity, 1.0);
            assert_eq!(shared.candles.bull_fill, [76, 175, 80]);
            assert_eq!(shared.candles.wick_width, 3.0);
        }
    }
}
