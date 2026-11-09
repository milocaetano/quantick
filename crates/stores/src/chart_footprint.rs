//! One chart's footprint setup against the window's.
//!
//! The window keeps the last setup a trader settled on, and a chart follows
//! it until it is configured on its own. A tab's workspace holds a footprint
//! style, so a tab keeps that style while every other knob still follows the
//! window: an edit on another tab reaches this one's knobs, never its style.

use crate::footprint_config::{FootprintConfig, FootprintStyle};

/// What a chart draws its footprint with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChartFootprint {
    /// The setup drawn with, while it is not the window's own.
    config: Option<FootprintConfig>,
    /// Only the style is the chart's: `config` is the window's on that style.
    style_only: bool,
}

impl ChartFootprint {
    /// The setup this chart draws with.
    #[must_use]
    pub fn resolve<'a>(&'a self, window: &'a FootprintConfig) -> &'a FootprintConfig {
        self.config.as_ref().unwrap_or(window)
    }

    /// Whether the chart was configured on its own, knobs and all.
    #[must_use]
    pub fn is_own(&self) -> bool {
        self.config.is_some() && !self.style_only
    }

    /// The chart's own setup from here on; `None` follows the window again.
    pub fn set_own(&mut self, config: Option<FootprintConfig>) {
        self.config = config;
        self.style_only = false;
    }

    /// Hold `style`. A chart configured on its own keeps its other knobs; one
    /// following the window keeps following it on every knob but the style.
    pub fn pin_style(&mut self, style: FootprintStyle, window: &FootprintConfig) {
        match self.config.as_mut() {
            Some(own) if !self.style_only => own.style = style,
            _ => {
                self.config = Some(FootprintConfig {
                    style,
                    ..window.clone()
                });
                self.style_only = true;
            }
        }
    }

    /// Hold the style it draws with now. No pixel changes.
    pub fn pin_current_style(&mut self, window: &FootprintConfig) {
        self.pin_style(self.resolve(window).style, window);
    }

    /// The window's setup moved: a chart holding only its style takes every
    /// other knob.
    pub fn follow(&mut self, window: &FootprintConfig) {
        if let (true, Some(pinned)) = (self.style_only, self.config.as_ref()) {
            let style = pinned.style;
            self.config = Some(FootprintConfig {
                style,
                ..window.clone()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(style: FootprintStyle, stacked: usize) -> FootprintConfig {
        FootprintConfig {
            style,
            stacked_count: stacked,
            ..FootprintConfig::default()
        }
    }

    #[test]
    fn a_pinned_style_stays_while_every_other_knob_follows_the_window() {
        let first = window(FootprintStyle::Split, 3);
        let mut chart = ChartFootprint::default();
        assert_eq!(chart.resolve(&first), &first, "a fresh chart follows");
        chart.pin_current_style(&first);
        assert_eq!(chart.resolve(&first), &first, "pinning changes no pixel");
        assert!(!chart.is_own());

        let edited = window(FootprintStyle::Ladder, 5);
        chart.follow(&edited);
        assert_eq!(chart.resolve(&edited).style, FootprintStyle::Split);
        assert_eq!(chart.resolve(&edited).stacked_count, 5);
    }

    #[test]
    fn a_chart_of_its_own_keeps_its_knobs_and_takes_a_style() {
        let first = window(FootprintStyle::Split, 3);
        let mut chart = ChartFootprint::default();
        chart.set_own(Some(window(FootprintStyle::Split, 2)));
        chart.pin_style(FootprintStyle::Ladder, &first);
        chart.follow(&window(FootprintStyle::Split, 9));
        assert!(chart.is_own());
        assert_eq!(chart.resolve(&first).style, FootprintStyle::Ladder);
        assert_eq!(chart.resolve(&first).stacked_count, 2);
        chart.set_own(None);
        assert_eq!(chart.resolve(&first), &first);
    }
}
