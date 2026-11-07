//! Chart-surface scalars a theme may set beside its colours: the candle
//! contour width and time grid a fresh chart takes, and the axis reservations.
use crate::{ResolvedTheme, ThemeError};
use serde::{Deserialize, Serialize};

/// Thinnest and thickest candle contour a theme may ask for, in logical
/// pixels: the chart style's own line-width bounds.
pub const CANDLE_OUTLINE_WIDTH_RANGE: std::ops::RangeInclusive<f32> = 0.5..=4.0;
/// Narrowest and widest axis reservation a theme may ask for, in logical
/// pixels. Below the floor no label fits; above the ceiling the axis eats the plot.
pub const AXIS_RESERVATION_RANGE: std::ops::RangeInclusive<f32> = 16.0..=240.0;

/// Fixed axis reservations. Absent, each is derived from the axis font, as
/// Dark always has been.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChartAxes {
    /// Width of the price gutter, in logical pixels, not counting the plot's
    /// right margin.
    pub price_gutter: Option<f32>,
    /// Height of the time strip, in logical pixels, its top rule included.
    pub time_strip: Option<f32>,
    /// Price label size, in logical pixels, over the axis font's own.
    pub price_label_size: Option<f32>,
    /// Time label size, in logical pixels, over the axis font's own.
    pub time_label_size: Option<f32>,
    /// Least room between price gridlines, in logical pixels: the price step
    /// becomes the smallest round one that keeps it. Absent, the axis asks
    /// for a fixed count of lines, as Dark always has.
    pub price_line_spacing: Option<f32>,
}

impl ChartAxes {
    pub(crate) fn resolve(
        authored: Option<Self>,
        fallback: Option<&ResolvedTheme>,
    ) -> Result<Self, ThemeError> {
        let inherited = fallback.map(|theme| theme.chart_axes).unwrap_or_default();
        let authored = authored.unwrap_or_default();
        let axes = Self {
            price_gutter: authored.price_gutter.or(inherited.price_gutter),
            time_strip: authored.time_strip.or(inherited.time_strip),
            price_label_size: authored.price_label_size.or(inherited.price_label_size),
            time_label_size: authored.time_label_size.or(inherited.time_label_size),
            price_line_spacing: authored.price_line_spacing.or(inherited.price_line_spacing),
        };
        if let Some(value) = axes.price_line_spacing
            && (!value.is_finite() || !(12.0..=240.0).contains(&value))
        {
            return Err(ThemeError::new(
                "chart_axes.price_line_spacing",
                "must be finite and between 12 and 240 logical pixels",
            ));
        }
        for (field, value) in [
            ("chart_axes.price_label_size", axes.price_label_size),
            ("chart_axes.time_label_size", axes.time_label_size),
        ] {
            if let Some(value) = value
                && (!value.is_finite() || !(6.0..=72.0).contains(&value))
            {
                return Err(ThemeError::new(
                    field,
                    "must be finite and between 6 and 72 logical pixels",
                ));
            }
        }
        for (field, value) in [
            ("chart_axes.price_gutter", axes.price_gutter),
            ("chart_axes.time_strip", axes.time_strip),
        ] {
            if let Some(value) = value
                && (!value.is_finite() || !AXIS_RESERVATION_RANGE.contains(&value))
            {
                return Err(ThemeError::new(
                    field,
                    "must be finite and between 16 and 240 logical pixels",
                ));
            }
        }
        Ok(axes)
    }
}

impl ResolvedTheme {
    /// The chart's axis reservations: the theme's fixed ones, else the font's.
    #[must_use]
    pub fn axis_metrics(&self) -> crate::AxisMetrics {
        crate::AxisMetrics::new(&self.typography).with_axes(self.chart_axes)
    }
}

pub(crate) fn outline_width(
    authored: Option<f32>,
    fallback: Option<&ResolvedTheme>,
) -> Result<f32, ThemeError> {
    let field = "candle_outline_width";
    let width = authored
        .or_else(|| fallback.map(|theme| theme.candle_outline_width))
        .ok_or_else(|| ThemeError::new(field, "required in a complete theme"))?;
    if !width.is_finite() || !CANDLE_OUTLINE_WIDTH_RANGE.contains(&width) {
        return Err(ThemeError::new(
            field,
            "must be finite and between 0.5 and 4 logical pixels",
        ));
    }
    Ok(width)
}

pub(crate) fn time_grid(
    authored: Option<bool>,
    fallback: Option<&ResolvedTheme>,
) -> Result<bool, ThemeError> {
    authored
        .or_else(|| fallback.map(|theme| theme.chart_time_grid))
        .ok_or_else(|| ThemeError::new("chart_time_grid", "required in a complete theme"))
}

#[cfg(test)]
mod tests {
    use crate::{DARK, LIGHT, ResolvedTheme};

    #[test]
    fn dark_derives_its_axes_and_light_fixes_them() {
        let dark = ResolvedTheme::parse(DARK).unwrap();
        assert_eq!(dark.chart_axes, super::ChartAxes::default());
        assert_eq!(dark.candle_outline_width, 1.25);
        assert!(!dark.chart_time_grid);
        let light = ResolvedTheme::parse(LIGHT).unwrap();
        assert_eq!(light.chart_axes.price_gutter, Some(76.0));
        assert_eq!(light.chart_axes.time_strip, Some(30.0));
        assert_eq!(light.candle_outline_width, 1.0);
        assert!(light.chart_time_grid);
        // The slate plot keeps Dark's contour, and the time grid follows Light.
        assert_eq!(light.bookmap().candle_outline_width, 1.25);
        assert!(light.bookmap().chart_time_grid);
    }

    #[test]
    fn an_older_document_inherits_and_out_of_range_values_are_refused() {
        let older = ResolvedTheme::parse("version=1\nname='Older'\ndark=false\n").unwrap();
        assert_eq!(older.candle_outline_width, 1.25);
        assert!(!older.chart_time_grid);
        assert_eq!(older.chart_axes, super::ChartAxes::default());
        let half = ResolvedTheme::parse("version=1\nname='Half'\n[chart_axes]\ntime_strip=30.0\n")
            .unwrap();
        assert_eq!(half.chart_axes.time_strip, Some(30.0));
        assert_eq!(half.chart_axes.price_gutter, None);
        for (source, field) in [
            ("candle_outline_width=nan", "candle_outline_width"),
            ("candle_outline_width=0.4", "candle_outline_width"),
            ("candle_outline_width=4.5", "candle_outline_width"),
            ("[chart_axes]\nprice_gutter=8.0", "chart_axes.price_gutter"),
            ("[chart_axes]\ntime_strip=inf", "chart_axes.time_strip"),
            (
                "[chart_axes]\nprice_label_size=5.0",
                "chart_axes.price_label_size",
            ),
            (
                "[chart_axes]\nprice_line_spacing=4.0",
                "chart_axes.price_line_spacing",
            ),
        ] {
            let error =
                ResolvedTheme::parse(&format!("version=1\nname='Bad'\n{source}")).unwrap_err();
            assert_eq!(error.field, field, "{source}");
        }
        assert!(ResolvedTheme::parse("version=1\nname='Bad'\n[chart_axes]\nwidth=8.0").is_err());
    }
}
