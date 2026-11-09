//! Saved chart values and explicit per-field ownership; absent legacy markers are explicit.
use quantick_appearance::Inheritance;
use quantick_chart::style::{
    CandleBodyMode, CandleStyle, CanvasStyle, ChartColorOwnership, ChartStyle, WickColorMode,
};
pub use quantick_chart::style::{
    MAX_BODY_HEIGHT, MAX_BODY_WIDTH_FRAC, MAX_CANDLE_GAP, MAX_CORNER_RADIUS, MAX_LINE_WIDTH,
    MIN_BODY_HEIGHT, MIN_BODY_WIDTH_FRAC, MIN_CANDLE_GAP, MIN_LINE_WIDTH,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StyleFile {
    #[serde(default)]
    ownership: OwnershipFile,
    candles: CandlesFile,
    canvas: CanvasFile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CandlesFile {
    outline_only: bool,
    bull_fill: [u8; 3],
    bear_fill: [u8; 3],
    bull_outline: [u8; 3],
    bear_outline: [u8; 3],
    fill_opacity: f32,
    outline_opacity: f32,
    outline_width: f32,
    show_wicks: bool,
    custom_wick_color: bool,
    wick: [u8; 3],
    wick_opacity: f32,
    wick_width: f32,
    body_width_frac: f32,
    gap_px: f32,
    corner_radius: f32,
    min_body_height: f32,
    forming_opacity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CanvasFile {
    background_enabled: bool,
    background: [u8; 3],
    grid_enabled: bool,
    /// Absent from styles saved before the grid split in two: those keep the
    /// one grid they had, the horizontal one, and open with no vertical grid.
    #[serde(default)]
    time_grid_enabled: bool,
    grid: [u8; 3],
    grid_opacity: f32,
}

impl StyleFile {
    pub fn valid(&self) -> bool {
        use quantick_chart::style::*;
        let c = &self.candles;
        let range =
            |value: f32, min: f32, max: f32| value.is_finite() && (min..=max).contains(&value);
        [
            c.fill_opacity,
            c.outline_opacity,
            c.wick_opacity,
            c.forming_opacity,
            self.canvas.grid_opacity,
        ]
        .into_iter()
        .all(|value| range(value, 0.0, 1.0))
            && range(c.outline_width, MIN_LINE_WIDTH, MAX_LINE_WIDTH)
            && range(c.wick_width, MIN_LINE_WIDTH, MAX_LINE_WIDTH)
            && range(c.body_width_frac, MIN_BODY_WIDTH_FRAC, MAX_BODY_WIDTH_FRAC)
            && range(c.gap_px, MIN_CANDLE_GAP, MAX_CANDLE_GAP)
            && range(c.corner_radius, 0.0, MAX_CORNER_RADIUS)
            && range(c.min_body_height, MIN_BODY_HEIGHT, MAX_BODY_HEIGHT)
    }
}

impl From<&ChartStyle> for StyleFile {
    fn from(style: &ChartStyle) -> Self {
        let c = &style.candles;
        let v = &style.canvas;
        Self {
            ownership: style.ownership.into(),
            candles: CandlesFile {
                outline_only: c.body_mode == CandleBodyMode::OutlineOnly,
                bull_fill: c.bull_fill,
                bear_fill: c.bear_fill,
                bull_outline: c.bull_outline,
                bear_outline: c.bear_outline,
                fill_opacity: c.fill_opacity,
                outline_opacity: c.outline_opacity,
                outline_width: c.outline_width,
                show_wicks: c.show_wicks,
                custom_wick_color: c.wick_color_mode == WickColorMode::Custom,
                wick: c.wick,
                wick_opacity: c.wick_opacity,
                wick_width: c.wick_width,
                body_width_frac: c.body_width_frac,
                gap_px: c.gap_px,
                corner_radius: c.corner_radius,
                min_body_height: c.min_body_height,
                forming_opacity: c.forming_opacity,
            },
            canvas: CanvasFile {
                background_enabled: v.background_enabled,
                background: v.background,
                grid_enabled: v.grid_enabled,
                time_grid_enabled: v.time_grid_enabled,
                grid: v.grid,
                grid_opacity: v.grid_opacity,
            },
        }
    }
}

impl From<StyleFile> for ChartStyle {
    fn from(file: StyleFile) -> Self {
        let c = file.candles;
        let v = file.canvas;
        Self {
            ownership: file.ownership.into(),
            candles: CandleStyle {
                body_mode: if c.outline_only {
                    CandleBodyMode::OutlineOnly
                } else {
                    CandleBodyMode::Filled
                },
                bull_fill: c.bull_fill,
                bear_fill: c.bear_fill,
                bull_outline: c.bull_outline,
                bear_outline: c.bear_outline,
                fill_opacity: c.fill_opacity,
                outline_opacity: c.outline_opacity,
                outline_width: c.outline_width,
                show_wicks: c.show_wicks,
                wick_color_mode: if c.custom_wick_color {
                    WickColorMode::Custom
                } else {
                    WickColorMode::MatchOutline
                },
                wick: c.wick,
                wick_opacity: c.wick_opacity,
                wick_width: c.wick_width,
                body_width_frac: c.body_width_frac,
                gap_px: c.gap_px,
                corner_radius: c.corner_radius,
                min_body_height: c.min_body_height,
                forming_opacity: c.forming_opacity,
            },
            canvas: CanvasStyle {
                background_enabled: v.background_enabled,
                background: v.background,
                grid_enabled: v.grid_enabled,
                time_grid_enabled: v.time_grid_enabled,
                grid: v.grid,
                grid_opacity: v.grid_opacity,
            },
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct OwnershipFile {
    fill_opacity: Inheritance,
    bull_fill: Inheritance,
    bear_fill: Inheritance,
    bull_outline: Inheritance,
    bear_outline: Inheritance,
    wick: Inheritance,
    background: Inheritance,
    grid: Inheritance,
    outline_width: Inheritance,
    outline_opacity: Inheritance,
    wick_opacity: Inheritance,
    time_grid: Inheritance,
}
impl From<ChartColorOwnership> for OwnershipFile {
    fn from(value: ChartColorOwnership) -> Self {
        Self {
            fill_opacity: value.fill_opacity,
            bull_fill: value.bull_fill,
            bear_fill: value.bear_fill,
            bull_outline: value.bull_outline,
            bear_outline: value.bear_outline,
            wick: value.wick,
            background: value.background,
            grid: value.grid,
            outline_width: value.outline_width,
            outline_opacity: value.outline_opacity,
            wick_opacity: value.wick_opacity,
            time_grid: value.time_grid,
        }
    }
}
impl From<OwnershipFile> for ChartColorOwnership {
    fn from(value: OwnershipFile) -> Self {
        Self {
            fill_opacity: value.fill_opacity,
            bull_fill: value.bull_fill,
            bear_fill: value.bear_fill,
            bull_outline: value.bull_outline,
            bear_outline: value.bear_outline,
            wick: value.wick,
            background: value.background,
            grid: value.grid,
            outline_width: value.outline_width,
            outline_opacity: value.outline_opacity,
            wick_opacity: value.wick_opacity,
            time_grid: value.time_grid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opacity_ownership_roundtrips_and_absent_field_preserves_legacy_numeric_value() {
        let light = quantick_appearance::ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        for ownership in [Inheritance::Theme, Inheritance::Explicit] {
            let mut style = ChartStyle::default();
            style.candles.wick_width = 3.0;
            style.candles.fill_opacity = 0.20;
            style.ownership.fill_opacity = ownership;
            let text = toml::to_string(&StyleFile::from(&style)).unwrap();
            let mut loaded: ChartStyle = toml::from_str::<StyleFile>(&text).unwrap().into();
            assert_eq!(loaded, style);
            loaded.apply_theme(&light);
            assert_eq!(
                loaded.candles.fill_opacity,
                if ownership == Inheritance::Theme {
                    1.0
                } else {
                    0.20
                }
            );
            assert_eq!(loaded.candles.wick_width, 3.0);
            let mut legacy: toml::Value = toml::from_str(&text).unwrap();
            legacy["ownership"]
                .as_table_mut()
                .unwrap()
                .remove("fill_opacity");
            let mut loaded: ChartStyle = legacy.try_into::<StyleFile>().unwrap().into();
            loaded.apply_theme(&light);
            assert_eq!(loaded.candles.fill_opacity, 0.20);
            assert_eq!(loaded.ownership.fill_opacity, Inheritance::Explicit);
        }
    }
    #[test]
    fn ownership_roundtrips_and_legacy_concrete_values_remain_explicit() {
        let mut style = ChartStyle::default();
        style.candles.outline_width = 3.0;
        // An edited width is the trader's, as the appearance window marks it.
        style.ownership.outline_width = Inheritance::Explicit;
        style.ownership.bull_fill = Inheritance::Explicit;
        let text = toml::to_string(&StyleFile::from(&style)).unwrap();
        let mut loaded: ChartStyle = toml::from_str::<StyleFile>(&text).unwrap().into();
        assert_eq!(loaded, style);
        loaded.apply_theme(
            &quantick_appearance::ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap(),
        );
        assert_eq!(loaded.candles.bull_fill, style.candles.bull_fill);
        assert_ne!(loaded.canvas, style.canvas);
        assert_eq!(loaded.candles.outline_width, 3.0);
        let mut legacy: toml::Value = toml::from_str(&text).unwrap();
        legacy.as_table_mut().unwrap().remove("ownership");
        let mut loaded: ChartStyle = legacy.try_into::<StyleFile>().unwrap().into();
        assert!(!loaded.apply_theme(
            &quantick_appearance::ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap()
        ));
    }
    /// A style saved before the grid split in two keeps its one grid as the
    /// horizontal one and opens with no vertical grid, under any theme.
    #[test]
    fn a_style_saved_before_the_time_grid_keeps_its_grid_and_opens_without_one() {
        let light = quantick_appearance::ResolvedTheme::parse(quantick_appearance::LIGHT).unwrap();
        for grid_enabled in [true, false] {
            let mut style = ChartStyle::default();
            style.canvas.grid_enabled = grid_enabled;
            let mut legacy: toml::Value =
                toml::from_str(&toml::to_string(&StyleFile::from(&style)).unwrap()).unwrap();
            legacy["canvas"]
                .as_table_mut()
                .unwrap()
                .remove("time_grid_enabled");
            for marker in [
                "outline_width",
                "outline_opacity",
                "wick_opacity",
                "time_grid",
            ] {
                legacy["ownership"].as_table_mut().unwrap().remove(marker);
            }
            let file: StyleFile = legacy.try_into().unwrap();
            assert!(file.valid());
            let mut loaded: ChartStyle = file.into();
            loaded.apply_theme(&light);
            assert_eq!(loaded.canvas.grid_enabled, grid_enabled);
            assert!(!loaded.canvas.time_grid_enabled);
            assert_eq!(loaded.ownership.time_grid, Inheritance::Explicit);
            assert_eq!(loaded.ownership.outline_width, Inheritance::Explicit);
            assert_eq!(loaded.candles.outline_width, style.candles.outline_width);
            assert_eq!(loaded.ownership.wick_opacity, Inheritance::Explicit);
            assert_eq!(loaded.candles.wick_opacity, style.candles.wick_opacity);
            assert_eq!(
                loaded.candles.outline_opacity,
                style.candles.outline_opacity
            );
        }
    }
    #[test]
    fn the_time_grid_and_its_owner_roundtrip() {
        let mut style = ChartStyle::default();
        style.canvas.time_grid_enabled = true;
        style.ownership.time_grid = Inheritance::Explicit;
        let text = toml::to_string(&StyleFile::from(&style)).unwrap();
        assert!(text.contains("time_grid_enabled = true"), "{text}");
        let loaded: ChartStyle = toml::from_str::<StyleFile>(&text).unwrap().into();
        assert_eq!(loaded, style);
    }
}
