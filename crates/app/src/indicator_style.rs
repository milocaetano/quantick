//! Per-plot style the trader set, layered over what the indicator declared.
//!
//! An indicator declares its plots — colour, width, shape — in its
//! [`IndicatorDescriptor`], and that declaration is the *author's* statement
//! about the series. What a trader changes in the settings dialog is not that
//! statement: it is a preference about their own chart, and it has to survive
//! the indicator being rebuilt (every input edit replaces the descriptor with
//! a fresh one) without ever being written back into the author's numbers.
//!
//! So the override lives here, beside the view, as a sparse layer: `None`
//! everywhere the trader said nothing, which is what makes rule 5 of this
//! change ("defaults preserve today's behaviour") true by construction rather
//! than by testing every field. It stays in `app` on purpose — the
//! `indicators` crate is headless and has no business knowing a dialog exists.
//!
//! [`IndicatorDescriptor`]: quantick_indicators::IndicatorDescriptor

use quantick_indicators::{PlotSpec, Rgba8};

/// Thinnest a plot may be dragged to and still be a line rather than a rumour.
pub(crate) const MIN_PLOT_WIDTH_PX: f32 = 0.5;
/// Thickest, past which a line stops reading as a series and starts reading as
/// a band — and hides the candles it is drawn over.
pub(crate) const MAX_PLOT_WIDTH_PX: f32 = 8.0;

/// What the trader changed about one plot. Every field `None` means "whatever
/// the indicator declared" — the state a plot nobody touched is always in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct PlotOverride {
    /// Draw this plot at all. Distinct from the legend's eye, which hides the
    /// whole indicator: this hides one series of several.
    pub visible: Option<bool>,
    /// Line/marker colour.
    pub color: Option<Rgba8>,
    /// Stroke width in pixels, clamped to [`MIN_PLOT_WIDTH_PX`]..=[`MAX_PLOT_WIDTH_PX`].
    pub width: Option<f32>,
}

impl PlotOverride {
    /// Nothing was changed about this plot.
    pub(crate) fn is_default(self) -> bool {
        self.visible.is_none() && self.color.is_none() && self.width.is_none()
    }
}

/// The style one plot actually draws with this frame: the declaration with the
/// trader's layer applied. Resolved rather than stored, so an indicator that
/// rebuilds with different declared colours still shows them everywhere the
/// trader did not override.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ResolvedPlot {
    pub visible: bool,
    pub color: Rgba8,
    pub width: f32,
}

/// One indicator's style layer, indexed by plot position.
///
/// Sparse and short (an indicator has a handful of plots), grown on demand:
/// an indicator nobody has styled carries an empty `Vec` and allocates
/// nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StyleOverride {
    plots: Vec<PlotOverride>,
}

impl StyleOverride {
    /// Build from a saved layer, in plot order.
    pub(crate) fn from_plots(plots: Vec<PlotOverride>) -> Self {
        let mut style = Self { plots };
        style.trim();
        style
    }

    /// What the trader set for plot `index` — the empty override when they set
    /// nothing, which is every plot until they open the dialog.
    pub(crate) fn get(&self, index: usize) -> PlotOverride {
        self.plots.get(index).copied().unwrap_or_default()
    }

    /// Record an override for plot `index`, growing the layer if needed.
    pub(crate) fn set(&mut self, index: usize, over: PlotOverride) {
        if index >= self.plots.len() {
            if over.is_default() {
                return;
            }
            self.plots.resize(index + 1, PlotOverride::default());
        }
        self.plots[index] = over;
        self.trim();
    }

    /// Hand every plot back to what the indicator declared.
    pub(crate) fn clear(&mut self) {
        self.plots.clear();
    }

    /// Nothing here overrides anything.
    pub(crate) fn is_default(&self) -> bool {
        self.plots.iter().all(|plot| plot.is_default())
    }

    /// The layer as stored, for persistence.
    pub(crate) fn plots(&self) -> &[PlotOverride] {
        &self.plots
    }

    /// The style plot `index` draws with, given what its spec declared.
    pub(crate) fn resolve(&self, index: usize, spec: &PlotSpec) -> ResolvedPlot {
        let over = self.get(index);
        ResolvedPlot {
            visible: over.visible.unwrap_or(true),
            color: over.color.unwrap_or(spec.base_color),
            width: over.width.map_or(spec.width, |px| {
                px.clamp(MIN_PLOT_WIDTH_PX, MAX_PLOT_WIDTH_PX)
            }),
        }
    }

    /// Drop trailing empty entries so a layer reset by hand serialises as
    /// absent rather than as a row of nulls.
    fn trim(&mut self) {
        while self.plots.last().is_some_and(|plot| plot.is_default()) {
            self.plots.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_indicators::{PlotId, PlotStyle};

    fn spec() -> PlotSpec {
        PlotSpec {
            id: PlotId::new(0),
            title: "p0".to_owned(),
            style: PlotStyle::Line,
            base_color: Rgba8::opaque(0x26, 0xA6, 0x9A),
            width: 1.5,
            offset: 0,
            marker: None,
        }
    }

    /// Criterion 5 of this change, as a test: an untouched layer resolves to
    /// exactly what the indicator declared, field for field. Everything the
    /// renderer draws goes through `resolve`, so this is what makes "a chart
    /// nobody styled looks like it did before" a fact rather than a hope.
    #[test]
    fn an_untouched_layer_draws_exactly_what_the_indicator_declared() {
        let style = StyleOverride::default();
        let resolved = style.resolve(0, &spec());
        assert!(resolved.visible);
        assert_eq!(resolved.color, spec().base_color);
        assert!((resolved.width - spec().width).abs() < f32::EPSILON);
        assert!(style.is_default());
        // And a plot far past anything ever set resolves the same way.
        assert_eq!(style.resolve(7, &spec()), resolved);
    }

    #[test]
    fn set_fields_win_and_the_rest_still_fall_through() {
        let mut style = StyleOverride::default();
        style.set(
            1,
            PlotOverride {
                color: Some(Rgba8::opaque(1, 2, 3)),
                ..PlotOverride::default()
            },
        );
        let resolved = style.resolve(1, &spec());
        assert_eq!(resolved.color, Rgba8::opaque(1, 2, 3), "the override wins");
        assert!(
            (resolved.width - spec().width).abs() < f32::EPSILON,
            "an untouched field still falls through to the declaration"
        );
        assert!(resolved.visible);
        assert!(!style.is_default());

        // Plot 0 was never named and is untouched, even though 1 grew the Vec.
        assert!(style.get(0).is_default());
    }

    /// A width is a stroke the renderer hands to egui: a hand-edited state file
    /// asking for zero (an invisible line the trader cannot find) or for two
    /// hundred (a band over the whole chart) is clamped, not obeyed.
    #[test]
    fn widths_are_clamped_to_something_drawable() {
        let mut style = StyleOverride::default();
        for (asked, expected) in [(0.0, MIN_PLOT_WIDTH_PX), (900.0, MAX_PLOT_WIDTH_PX)] {
            style.set(
                0,
                PlotOverride {
                    width: Some(asked),
                    ..PlotOverride::default()
                },
            );
            let resolved = style.resolve(0, &spec());
            assert!(
                (resolved.width - expected).abs() < f32::EPSILON,
                "{asked} px should clamp to {expected}"
            );
        }
    }

    #[test]
    fn clearing_and_resetting_return_the_layer_to_absent() {
        let mut style = StyleOverride::default();
        style.set(
            2,
            PlotOverride {
                visible: Some(false),
                ..PlotOverride::default()
            },
        );
        assert_eq!(style.plots().len(), 3, "the layer grew to reach plot 2");

        style.set(2, PlotOverride::default());
        assert!(
            style.plots().is_empty(),
            "an override taken back leaves no trailing nulls to persist"
        );

        style.set(
            0,
            PlotOverride {
                width: Some(3.0),
                ..PlotOverride::default()
            },
        );
        style.clear();
        assert!(style.is_default() && style.plots().is_empty());
    }
}
