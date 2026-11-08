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
    /// The candle contour width a fresh chart takes from its theme.
    pub outline_width: Inheritance,
    /// The contour's opacity, likewise.
    pub outline_opacity: Inheritance,
    /// The wick's opacity, likewise.
    pub wick_opacity: Inheritance,
    /// Whether the time grid is the theme's answer or the trader's.
    pub time_grid: Inheritance,
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
            outline_width: Inheritance::Theme,
            outline_opacity: Inheritance::Theme,
            wick_opacity: Inheritance::Theme,
            time_grid: Inheritance::Theme,
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
            time_grid: self.ownership.time_grid,
            ..ChartColorOwnership::default()
        };
    }
    /// The first theme-owned field this style changes, as a refusal naming the
    /// ownership marker that would make the value the trader's. `stored` is the
    /// style the pane holds and `shown` the style it paints: a value equal to
    /// either is an echo, not an edit. A theme-owned value is replaced by the
    /// theme on resolve, so accepting an edit would drop it silently.
    pub fn theme_owned_edit(&self, stored: &Self, shown: &Self) -> Option<&'static str> {
        macro_rules! owned {
            ($marker:ident, $($path:ident).+) => {
                if self.ownership.$marker == Inheritance::Theme
                    && self.$($path).+ != stored.$($path).+
                    && self.$($path).+ != shown.$($path).+
                {
                    return Some(concat!(
                        stringify!($($path).+),
                        " is owned by the theme and the new value would be ignored;                          set ownership.",
                        stringify!($marker),
                        " to \"explicit\" to make it yours"
                    ));
                }
            };
        }
        owned!(fill_opacity, candles.fill_opacity);
        owned!(bull_fill, candles.bull_fill);
        owned!(bear_fill, candles.bear_fill);
        owned!(bull_outline, candles.bull_outline);
        owned!(bear_outline, candles.bear_outline);
        owned!(wick, candles.wick);
        owned!(background, canvas.background);
        owned!(grid, canvas.grid);
        owned!(outline_width, candles.outline_width);
        owned!(outline_opacity, candles.outline_opacity);
        owned!(wick_opacity, candles.wick_opacity);
        owned!(time_grid, canvas.time_grid_enabled);
        None
    }
    /// Resolve inherited fields from a validated theme. RGB-only fields are
    /// guaranteed opaque by the appearance resolver, so no alpha is discarded.
    pub fn apply_theme(&mut self, theme: &ResolvedTheme) -> bool {
        let before = *self;
        self.candles.fill_opacity = self
            .ownership
            .fill_opacity
            .resolve(self.candles.fill_opacity, || theme.candle_fill_opacity);
        self.candles.outline_width = self
            .ownership
            .outline_width
            .resolve(self.candles.outline_width, || theme.candle_outline_width);
        self.candles.outline_opacity = self
            .ownership
            .outline_opacity
            .resolve(self.candles.outline_opacity, || {
                theme.candle_outline_opacity
            });
        self.candles.wick_opacity = self
            .ownership
            .wick_opacity
            .resolve(self.candles.wick_opacity, || theme.candle_wick_opacity);
        self.canvas.time_grid_enabled = self
            .ownership
            .time_grid
            .resolve(self.canvas.time_grid_enabled, || theme.chart_time_grid);
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
            .resolve(self.canvas.background, || rgb(Token::ChartCanvas));
        self.canvas.grid = self
            .ownership
            .grid
            .resolve(self.canvas.grid, || rgb(Token::ChartGrid));
        *self != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_grid_tracks_surfaces_and_preserves_authored_settings() {
        let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        let dark = ResolvedTheme::default();
        let mut style = ChartStyle::default();
        style.canvas.grid_opacity = 0.37;
        style.canvas.grid_enabled = false;
        for (surface, grid, background) in [
            (&light, [186, 187, 187], [255, 255, 255]),
            (light.bookmap(), [35, 41, 54], [37, 47, 55]),
            (&dark, [35, 41, 54], [19, 23, 34]),
        ] {
            style.apply_theme(surface);
            assert_eq!(style.canvas.grid, grid);
            assert_eq!(style.canvas.background, background);
            assert_eq!(style.canvas.grid_opacity, 0.37);
            assert!(!style.canvas.grid_enabled);
        }
        style.canvas.grid = [222, 222, 222];
        style.ownership.grid = Inheritance::Explicit;
        for surface in [&light, light.bookmap(), &dark] {
            style.apply_theme(surface);
            assert_eq!(style.canvas.grid, [222, 222, 222]);
        }
    }
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
    fn fresh_charts_take_the_theme_time_grid_and_contour_until_the_trader_sets_them() {
        let light = ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        let dark = ResolvedTheme::default();
        let mut fresh = ChartStyle::default();
        fresh.apply_theme(&light);
        assert!(fresh.canvas.time_grid_enabled);
        assert_eq!(fresh.candles.outline_width, 1.0);
        // Pure black, one pixel: TradingView's contour and wick.
        assert_eq!(fresh.candles.resolved(true, false).outline, [0, 0, 0, 255]);
        assert_eq!(
            fresh.candles.resolved(false, false).wick,
            Some([0, 0, 0, 255])
        );
        fresh.apply_theme(&dark);
        assert!(!fresh.canvas.time_grid_enabled);
        assert_eq!(fresh.candles.outline_width, 1.25);
        assert_eq!(
            fresh,
            ChartStyle::default(),
            "Dark resolves to the code default"
        );
        // The slate plot keeps Dark's contour; the time grid is Light's.
        fresh.apply_theme(light.bookmap());
        assert!(fresh.canvas.time_grid_enabled);
        assert_eq!(fresh.candles.outline_width, 1.25);
        // Switched by hand, the answer survives every theme.
        let mut chosen = ChartStyle::default();
        chosen.canvas.time_grid_enabled = false;
        chosen.ownership.time_grid = Inheritance::Explicit;
        chosen.candles.outline_width = 2.0;
        chosen.ownership.outline_width = Inheritance::Explicit;
        chosen.candles.wick_opacity = 0.5;
        chosen.ownership.wick_opacity = Inheritance::Explicit;
        for theme in [&light, &dark] {
            chosen.apply_theme(theme);
            assert!(!chosen.canvas.time_grid_enabled);
            assert_eq!(chosen.candles.outline_width, 2.0);
            assert_eq!(chosen.candles.wick_opacity, 0.5);
        }
        // A preset owns the contour, never the canvas's time grid.
        let mut preset = ChartStyle::default();
        preset.apply_preset(CandlePreset::Classic);
        assert_eq!(preset.ownership.outline_width, Inheritance::Explicit);
        assert_eq!(preset.ownership.time_grid, Inheritance::Theme);
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
        expected_flow.canvas.background = [0x25, 0x2F, 0x37];
        expected_flow.candles.wick = [0xB6, 0xC9, 0xD2];
        // The slate plot follows Light's time grid, so the switch that reads
        // it and the pane that paints it never disagree.
        expected_flow.canvas.time_grid_enabled = true;
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
