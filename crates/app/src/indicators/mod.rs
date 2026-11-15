//! UI-side indicator state: the columns the renderer reads.
//!
//! The UI's copy of the worker's truth — one [`IndicatorView`] per instance,
//! the [`IndicatorViews`] collection and the [`PaneSizing`] each pane asks
//! for — lives headless in [`quantick_chart::indicator_views`]. This module is
//! the app's address for it, plus the height arithmetic in `panes` that
//! carves the pane band out of a window rect.

pub(crate) mod library;
pub(crate) mod preset_file;
pub(crate) mod state_file;

// The facade re-addresses `quantick_chart::indicator_views` for app callers and
// keeps the egui `panes` split local.
mod panes;

#[cfg(test)]
pub(crate) use panes::split_panes;
pub(crate) use panes::{PaneSlot, split_panes_with_header};
pub(crate) use quantick_chart::indicator_views::{
    IndicatorView, IndicatorViews, MAX_PANES, MIN_PANE_HEIGHT_PX, PaneSizing,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod appearance_layout_tests {
    use super::*;
    #[test]
    fn enlarged_indicator_header_and_collapsed_strip_share_the_layout_budget() {
        for scale in [1.0, 2.0] {
            let theme = quantick_appearance::ResolvedTheme::parse(&format!(
                "version=1\nname='large'\n[typography.chart_label]\nsize={}\n",
                11.0 * scale
            ))
            .unwrap();
            let header = quantick_appearance::AxisMetrics::new(&theme.typography).indicator_header;
            let band = eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(400.0, 500.0),
            );
            let (_, slots) = split_panes_with_header(
                band,
                &[PaneSizing::Manual(150.0), PaneSizing::Collapsed],
                header,
            );
            let rect = crate::indicator_render::pane_header_rect(
                slots[0].rect,
                false,
                slots[0].header_height,
            );
            assert_eq!(rect.height(), 18.0 * scale);
            assert_eq!(slots[1].rect.height(), 20.0 * scale);
            assert!(
                rect.height()
                    >= theme
                        .typography
                        .get(quantick_appearance::FontRole::ChartLabel)
                        .size
                        + 6.0
            );
        }
    }
}
