//! Explicit palette ownership and immutable render snapshots.
use super::{
    HeatmapTheme,
    theme::{OrderflowRenderStyle, Palette, finite_unit, mix_rgb},
};
use quantick_appearance::orderflow::OrderflowAppearance;
use quantick_orderbook::BookSide;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "source", content = "value", rename_all = "snake_case")]
pub enum PaletteChoice {
    #[default]
    Inherited,
    Historical(HeatmapTheme),
    Snapshot(#[serde(deserialize_with = "read_snapshot")] OrderflowAppearance),
}
impl PaletteChoice {
    pub const SELECTABLE: [Self; 4] = [
        Self::Inherited,
        Self::Historical(HeatmapTheme::Bookmap),
        Self::Historical(HeatmapTheme::HighContrast),
        Self::Historical(HeatmapTheme::ColorBlind),
    ];
    pub fn label(self) -> &'static str {
        match self {
            PaletteChoice::Inherited => "Application appearance",
            PaletteChoice::Snapshot(_) => "Saved palette",
            PaletteChoice::Historical(HeatmapTheme::Bookmap) => "Bookmap",
            PaletteChoice::Historical(HeatmapTheme::HighContrast) => "High contrast",
            PaletteChoice::Historical(HeatmapTheme::ColorBlind) => "Color blind",
        }
    }

    pub fn legacy() -> Self {
        Self::Historical(HeatmapTheme::Bookmap)
    }
    pub fn historical(self) -> HeatmapTheme {
        match self {
            Self::Historical(theme) => theme,
            _ => HeatmapTheme::Bookmap,
        }
    }
    /// The choice in force over `inherited`: a palette bound to its surface
    /// (the lit flow pane's) outranks one picked for another ground.
    #[must_use]
    pub fn under(self, inherited: &OrderflowAppearance) -> Self {
        if inherited.surface_bound {
            Self::Inherited
        } else {
            self
        }
    }
    pub fn resolve(self, inherited: &OrderflowAppearance) -> OrderflowAppearance {
        match self.under(inherited) {
            Self::Inherited => *inherited,
            Self::Snapshot(palette) => palette,
            Self::Historical(theme) => {
                let p = Palette::for_theme(theme, |c| c);
                let mut result = *inherited;
                result.crown_backing = p.crown_backing;
                result.separator = p.separator;
                result.preview_grid = p.preview_grid;
                result.preview_path = p.preview_path;
                result.buy = p.buy;
                result.sell = p.sell;
                result.consumption = p.consumption;
                result.depth_only = p.depth_only;
                result.bubble_text = p.bubble_text;
                result.gap_fill = p.gap_fill;
                result.gap_boundary = p.gap_boundary;
                result.lane_now = p.lane_now;
                result.muted_text = p.muted_text;
                result.legend_text = p.legend_text;
                result.legend_background = p.legend_background;
                result.legend_border = p.legend_border;
                // Historical ramps retain their original knots in the renderer.
                result
            }
        }
    }
}
impl OrderflowRenderStyle {
    pub fn resolve_palette(&mut self, choice: PaletteChoice, inherited: &OrderflowAppearance) {
        let choice = choice.under(inherited);
        self.theme = choice.historical();
        self.palette = match choice {
            PaletteChoice::Historical(_) => None,
            _ => Some(choice.resolve(inherited)),
        };
    }
    pub fn colors<C>(&self, rgba: impl Fn([u8; 4]) -> C) -> Palette<C> {
        match self.palette {
            Some(p) => Palette {
                crown_backing: rgba(p.crown_backing),
                separator: rgba(p.separator),

                preview_grid: rgba(p.preview_grid),
                preview_path: rgba(p.preview_path),
                buy: rgba(p.buy),
                sell: rgba(p.sell),
                consumption: rgba(p.consumption),
                depth_only: rgba(p.depth_only),
                bubble_text: rgba(p.bubble_text),
                gap_fill: rgba(p.gap_fill),
                gap_boundary: rgba(p.gap_boundary),
                lane_now: rgba(p.lane_now),
                muted_text: rgba(p.muted_text),
                legend_text: rgba(p.legend_text),
                legend_background: rgba(p.legend_background),
                legend_border: rgba(p.legend_border),
                flat_rims: p.flat_rims.map(|rims| rims.map(&rgba)),
            },
            None => Palette::for_theme(self.theme, rgba),
        }
    }
    pub fn thermal_rgb(&self, intensity: f32) -> [u8; 3] {
        let Some(p) = self.palette else {
            return super::theme::thermal_rgb(self.theme, intensity);
        };
        let t = finite_unit(intensity);
        for i in 1..p.ramp.len() {
            let (from, to) = (p.ramp_stop(i - 1), p.ramp_stop(i));
            if t <= to {
                return mix_rgb(p.ramp[i - 1], p.ramp[i], (t - from) / (to - from));
            }
        }
        p.ramp[8]
    }
    /// Where on the ramp a cell of the look's `intensity` reads: the palette's
    /// own heat gamma over the look's, `(v^look)^palette`; unchanged without one.
    #[must_use]
    pub fn heat_position(&self, intensity: f32) -> f32 {
        let intensity = finite_unit(intensity);
        match self.palette.and_then(|p| p.heat_gamma()) {
            Some(gamma) => intensity.powf(gamma),
            None => intensity,
        }
    }
    /// How much of its ramp colour a cell of the look's `intensity` lays down,
    /// before the look's opacity: full from the palette's fade point up, linear
    /// below it. `None` where the palette authors none: opacity then follows
    /// intensity, as on a ramp that starts at the dark ground.
    #[must_use]
    pub fn heat_coverage(&self, intensity: f32) -> Option<f32> {
        let fade = self.palette.and_then(|p| p.heat_fade_below())?;
        Some((finite_unit(intensity) / fade).min(1.0))
    }
    pub fn resting_rgb(&self, side: BookSide, intensity: f32) -> [u8; 3] {
        let Some(p) = self.palette else {
            return super::theme::resting_rgb(self.theme, side, intensity);
        };
        let [r, g, b, _] = match side {
            BookSide::Bid => p.bid_tint,
            BookSide::Ask => p.ask_tint,
        };
        mix_rgb(
            self.thermal_rgb(intensity),
            [r, g, b],
            (1.0 - finite_unit(intensity)) * 0.045,
        )
    }
}
impl PaletteChoice {
    pub fn bubble_rgb(self, inherited: &OrderflowAppearance) -> super::theme::ThemeBubbleRgb {
        let p = self.resolve(inherited);
        let rgb = |[r, g, b, _]: [u8; 4]| [r, g, b];
        super::theme::ThemeBubbleRgb {
            buy: rgb(p.buy),
            sell: rgb(p.sell),
            front: rgb(p.consumption),
            text: rgb(p.bubble_text),
        }
    }
}

fn read_snapshot<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<OrderflowAppearance, D::Error> {
    let palette = OrderflowAppearance::deserialize(deserializer)?;
    palette.validate().map_err(serde::de::Error::custom)?;
    Ok(palette)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configured_dark_preserves_every_historical_palette_byte_and_ramp_sample() {
        let inherited = &quantick_appearance::ResolvedTheme::bundled_dark().orderflow;
        let mut style = OrderflowRenderStyle::default();
        style.resolve_palette(PaletteChoice::Inherited, inherited);
        let p = style.colors(|c| c);
        let old = Palette::for_theme(HeatmapTheme::Bookmap, |c| c);
        assert_eq!(format!("{p:?}"), format!("{old:?}"));
        for i in 0..=1000 {
            let intensity = i as f32 / 1000.0;
            assert_eq!(
                style.thermal_rgb(intensity),
                super::super::theme::thermal_rgb(HeatmapTheme::Bookmap, intensity)
            );
            for side in [BookSide::Bid, BookSide::Ask] {
                assert_eq!(
                    style.resting_rgb(side, intensity),
                    super::super::theme::resting_rgb(HeatmapTheme::Bookmap, side, intensity)
                );
            }
        }
    }
    #[test]
    fn authored_palette_changes_fresh_defaults_while_historical_choices_keep_exact_output() {
        let authored = quantick_appearance::ResolvedTheme::parse(
            "version=1\nname='Authored palette'\n[orderflow]\nramp=['#000000','#111111','#222222','#333333','#444444','#555555','#666666','#777777','#888888']\n[orderflow.colors]\nbuy='#007752'\nseparator='#223344AA'",
        ).unwrap();
        let mut style = OrderflowRenderStyle::default();
        style.resolve_palette(PaletteChoice::Inherited, &authored.orderflow);
        assert_ne!(
            style.thermal_rgb(0.5),
            super::super::theme::thermal_rgb(HeatmapTheme::Bookmap, 0.5)
        );
        assert_eq!(style.colors(|c| c).buy, authored.orderflow.buy);
        for theme in [
            HeatmapTheme::Bookmap,
            HeatmapTheme::HighContrast,
            HeatmapTheme::ColorBlind,
        ] {
            style.resolve_palette(PaletteChoice::Historical(theme), &authored.orderflow);
            for i in 0..=100 {
                assert_eq!(
                    style.thermal_rgb(i as f32 / 100.0),
                    super::super::theme::thermal_rgb(theme, i as f32 / 100.0)
                );
            }
        }
    }
}

#[cfg(test)]
mod decoration_tests {
    use super::*;
    #[test]
    fn snapshot_missing_decoration_uses_legacy_bytes_without_discarding_saved_colors() {
        let authored = quantick_appearance::ResolvedTheme::parse(
            "version=1\nname='Authored decoration'\n[orderflow.colors]\nseparator='#223344AA'",
        )
        .unwrap();
        let mut saved = authored.orderflow;
        saved.buy = [12, 34, 56, 255];
        let source = toml::to_string(&PaletteChoice::Snapshot(saved)).unwrap();
        let legacy = source
            .lines()
            .filter(|line| !line.starts_with("crown_backing =") && !line.starts_with("separator ="))
            .collect::<Vec<_>>()
            .join("\n");
        let choice: PaletteChoice = toml::from_str(&legacy).unwrap();
        let restored = choice.resolve(&authored.orderflow);
        assert_eq!(restored.buy, [12, 34, 56, 255]);
        assert_eq!(restored.crown_backing, [0, 0, 0, 110]);
        assert_eq!(restored.separator, [0, 0, 0, 170]);
        let roundtrip: PaletteChoice = toml::from_str(&toml::to_string(&choice).unwrap()).unwrap();
        assert_eq!(roundtrip, choice);
        assert_eq!(
            PaletteChoice::Snapshot(saved)
                .resolve(&quantick_appearance::ResolvedTheme::default().orderflow),
            saved
        );
        assert_eq!(
            PaletteChoice::Historical(HeatmapTheme::Bookmap)
                .resolve(&authored.orderflow)
                .separator,
            [0, 0, 0, 170]
        );
        assert_ne!(
            PaletteChoice::Inherited
                .resolve(&authored.orderflow)
                .separator,
            [0, 0, 0, 170]
        );
    }
}

#[cfg(test)]
mod lit_tests {
    use super::*;
    use quantick_appearance::{ChartSurface, LIGHT, ResolvedTheme};

    /// The lit flow pane's palette outranks any the pane picked for the island,
    /// reads its ramp on its own stops and rims its bubbles; the island keeps
    /// the trader's pick.
    #[test]
    fn the_lit_palette_outranks_a_picked_one_and_reads_its_own_stops() {
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        let lit = light.surface(ChartSurface::LitFlow).orderflow;
        let island = light.bookmap().orderflow;
        let picks = [
            PaletteChoice::Inherited,
            PaletteChoice::Snapshot(island),
            PaletteChoice::Historical(HeatmapTheme::HighContrast),
        ];
        for choice in picks {
            let mut style = OrderflowRenderStyle::default();
            style.resolve_palette(choice, &lit);
            assert_eq!(style.palette, Some(lit), "{choice:?}");
            assert_eq!(choice.resolve(&lit), lit);
            assert_eq!(choice.under(&island), choice);
        }
        let mut style = OrderflowRenderStyle::default();
        style.resolve_palette(PaletteChoice::Inherited, &lit);
        assert_eq!(style.thermal_rgb(0.0), [255, 255, 255]);
        assert_eq!(style.thermal_rgb(0.15), [0xF1, 0xF2, 0xF4]);
        assert_eq!(style.thermal_rgb(0.90), [0x8A, 0x2C, 0x7C]);
        assert_eq!(style.thermal_rgb(1.0), [0x3A, 0x0A, 0x33]);
        let rims = Some([[0x0A, 0x63, 0x44, 255], [0x8F, 0x1B, 0x27, 255]]);
        assert_eq!(style.colors(|c| c).flat_rims, rims);
        style.resolve_palette(PaletteChoice::Inherited, &island);
        assert_eq!(style.colors(|c| c).flat_rims, None);
    }

    /// On the lit pane weak liquidity reads further up the ramp and every cell
    /// from the fade point up paints its grey opaque, so the book does not
    /// wash out to the white ground. Paper, the island and Dark read as before.
    #[test]
    fn the_lit_palette_lifts_weak_liquidity_and_paints_it_opaque() {
        use super::super::theme::heat_fill_parts;
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        let mut lit = OrderflowRenderStyle::default();
        let lit_palette = light.surface(ChartSurface::LitFlow).orderflow;
        lit.resolve_palette(PaletteChoice::Inherited, &lit_palette);
        assert!(
            lit.heat_position(0.05) >= 0.12,
            "{}",
            lit.heat_position(0.05)
        );
        assert_eq!(lit.heat_position(0.5), 0.5_f32.powf(0.7));
        // A half-strength cell at the look's full opacity lays its ramp grey
        // down whole, not half of it over white.
        let (rgb, alpha) = heat_fill_parts(&lit, BookSide::Bid, 0.5, 0.5).unwrap();
        assert_eq!(alpha, 1.0);
        let band = super::super::theme::quantize_heat(0.5_f32.powf(0.7));
        assert_eq!(rgb, lit.resting_rgb(BookSide::Bid, band));
        // The band and the side's faint tint are all that part it from the ramp.
        let ramp = lit.thermal_rgb(0.5_f32.powf(0.7));
        for (got, want) in rgb.into_iter().zip(ramp) {
            assert!(got.abs_diff(want) <= 6, "{rgb:?} against {ramp:?}");
        }
        assert!(rgb.into_iter().max().unwrap() < 200, "{rgb:?}");
        // Below the fade point a cell fades in; a carried one keeps its dimming.
        let (_, faint) = heat_fill_parts(&lit, BookSide::Ask, 0.05, 0.05).unwrap();
        assert!((faint - 0.5).abs() < 1e-5, "{faint}");
        let (_, carried) = heat_fill_parts(&lit, BookSide::Ask, 0.5, 0.25).unwrap();
        assert!((carried - 0.5).abs() < 1e-5, "{carried}");

        let dark = ResolvedTheme::parse(quantick_appearance::DARK).unwrap();
        for plain in [&light, light.bookmap(), &dark, dark.bookmap()] {
            let mut style = OrderflowRenderStyle::default();
            style.resolve_palette(PaletteChoice::Inherited, &plain.orderflow);
            for i in 0..=1000 {
                let v = i as f32 / 1000.0;
                assert_eq!(style.heat_position(v).to_bits(), v.to_bits());
                assert_eq!(style.heat_coverage(v), None);
            }
            assert_eq!(heat_fill_parts(&style, BookSide::Bid, 0.05, 0.05), None);
        }
    }
}
