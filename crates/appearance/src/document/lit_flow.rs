//! The flow pane with its lights on: a second surface a theme may author for
//! the order-flow pane, picked by the bulb on the pane's band. It derives from
//! the root theme, not from the dark island, so drawings, plot inks and every
//! colour it leaves out are the root's paper defaults.
use crate::orderflow::OrderflowOverride;
use crate::{ColorValue, ResolvedTheme, ThemeDocument, ThemeError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LitFlowOverride {
    /// The plate under each candle, in logical pixels, painted in `casing`.
    pub candle_casing_px: Option<f32>,
    #[serde(default)]
    pub colors: BTreeMap<String, ColorValue>,
    /// The pane's own order-flow palette; what it leaves out is the root's.
    #[serde(default)]
    pub orderflow: OrderflowOverride,
}

/// The widest candle plate a theme may author, in logical pixels.
pub const CANDLE_CASING_MAX_PX: f32 = 4.0;

/// Which of a theme's surfaces a chart pane paints on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartSurface {
    /// The root theme: a time chart.
    #[default]
    Paper,
    /// The order-flow pane's dark island ([`ResolvedTheme::bookmap`]).
    Flow,
    /// The order-flow pane with its lights on, where the theme authors it.
    LitFlow,
}

impl ChartSurface {
    /// A pane's surface: `None` for a time chart, else whether its lights are on.
    #[must_use]
    pub const fn pane(flow_lights: Option<bool>) -> Self {
        match flow_lights {
            None => Self::Paper,
            Some(false) => Self::Flow,
            Some(true) => Self::LitFlow,
        }
    }

    /// Whether this is the order-flow pane's, lit or not.
    #[must_use]
    pub const fn is_flow(self) -> bool {
        !matches!(self, Self::Paper)
    }
}

impl LitFlowOverride {
    pub(crate) fn resolve(&self, parent: &ResolvedTheme) -> Result<ResolvedTheme, ThemeError> {
        if let Some(width) = self.candle_casing_px
            && !(width.is_finite() && (0.0..=CANDLE_CASING_MAX_PX).contains(&width))
        {
            return Err(ThemeError::new(
                "lit_flow.candle_casing_px",
                format!("must be finite and between 0 and {CANDLE_CASING_MAX_PX}"),
            ));
        }
        let mut lit = ThemeDocument {
            version: 1,
            name: parent.name.clone(),
            dark: parent.dark,
            candle_fill_opacity: None,
            candle_outline_width: None,
            candle_outline_opacity: None,
            candle_wick_opacity: None,
            chart_time_grid: None,
            chart_axes: None,
            colors: self.colors.clone(),
            plot_ink: BTreeMap::new(),
            typography: BTreeMap::new(),
            footprint_heat: Default::default(),
            orderflow: self.orderflow.clone(),
            bookmap: None,
            title_strip: None,
            lit_flow: None,
        }
        .resolve_with(Some(parent))
        .map_err(|error| ThemeError::new(format!("lit_flow.{}", error.field), error.message))?;
        // Paper's plot inks: the lit pane is paper, not the island.
        lit.plot_ink = parent.plot_ink.clone();
        lit.orderflow.surface_bound = true;
        lit.candle_casing_px = self.candle_casing_px.filter(|width| *width > 0.0);
        Ok(lit)
    }
}

impl ResolvedTheme {
    /// Whether the theme authors a lit flow pane: the bulb is offered only then.
    #[must_use]
    pub fn has_lit_flow(&self) -> bool {
        self.lit_flow.is_some()
    }

    /// The palette `surface` paints with. A theme without a lit flow pane
    /// paints the lit one as its island.
    #[must_use]
    pub fn surface(&self, surface: ChartSurface) -> &Self {
        match surface {
            ChartSurface::Paper => self,
            ChartSurface::Flow => self.bookmap(),
            ChartSurface::LitFlow => self.lit_flow.as_deref().unwrap_or_else(|| self.bookmap()),
        }
    }
}

#[cfg(test)]
#[path = "lit_flow_tests.rs"]
mod tests;
