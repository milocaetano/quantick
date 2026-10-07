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

// The leaves are private: the re-exports below are this module's address, so
// naming `IndicatorView` costs this file and not the collection's event loop.
mod panes;

pub(crate) use panes::{PaneSlot, split_panes};
pub(crate) use quantick_chart::indicator_views::{
    IndicatorView, IndicatorViews, MAX_PANES, MIN_PANE_HEIGHT_PX, PaneSizing,
};

#[cfg(test)]
mod tests {
    // The pane split, exercised through the module's address. The
    // collection's own tests moved with it into `quantick_chart`.
    use super::*;
    use eframe::egui;

    use crate::indicators::panes::MIN_CHART_HEIGHT_PX;
    use quantick_chart::indicator_views::COLLAPSED_PANE_HEIGHT_PX;

    fn band(height: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, height))
    }

    fn auto(n: usize) -> Vec<PaneSizing> {
        vec![PaneSizing::Auto; n]
    }

    #[test]
    fn pane_split_is_stacked_and_bounded() {
        let chart = band(1000.0);
        let (shrunk, panes) = split_panes(chart, &auto(2));
        assert_eq!(shrunk.height(), 600.0, "two panes take 2 x 20%");
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].rect.top(), 600.0);
        assert_eq!(panes[1].rect.top(), 800.0);
        assert_eq!(panes[1].rect.bottom(), 1000.0);
        assert!(panes.iter().all(|pane| !pane.collapsed), "there is room");

        let (unchanged, none) = split_panes(chart, &auto(0));
        assert_eq!(unchanged, chart);
        assert!(none.is_empty());

        let (_, capped) = split_panes(chart, &auto(9));
        assert_eq!(capped.len(), MAX_PANES);
    }

    /// The headline of this change: a band too short for three readable panes
    /// gives room to the ones it can and collapses the rest, instead of
    /// handing all three a sliver. Room goes top-down, so the pane the user
    /// added first is the last to lose its curve.
    #[test]
    fn a_short_band_collapses_the_panes_it_cannot_draw_legibly() {
        let (chart, panes) = split_panes(band(300.0), &auto(3));

        assert_eq!(panes.len(), 3, "every pane is still present");
        assert!(!panes[0].collapsed, "the first keeps its curve");
        assert!(
            panes[1].collapsed && panes[2].collapsed,
            "300 px cannot hold two readable panes and the candles' own floor"
        );
        for pane in &panes {
            assert!(
                pane.rect.height() >= COLLAPSED_PANE_HEIGHT_PX,
                "no pane is ever thinner than its own name"
            );
            assert!(
                pane.collapsed || pane.rect.height() >= MIN_PANE_HEIGHT_PX,
                "an expanded pane always clears the readable floor: {pane:?}"
            );
        }
        assert!(
            chart.height() >= MIN_CHART_HEIGHT_PX,
            "and the candles keep their floor: {}",
            chart.height()
        );
    }

    /// The floor holds all the way down. At the smallest window the app
    /// allows, three panes are three labelled strips and the candles are still
    /// candles — the state the old split turned into three unreadable bands.
    #[test]
    fn the_floors_hold_at_every_band_height() {
        for height in [0.0_f32, 60.0, 120.0, 200.0, 300.0, 560.0, 1000.0] {
            let (chart, panes) = split_panes(band(height), &auto(MAX_PANES));
            let total: f32 = panes.iter().map(|pane| pane.rect.height()).sum();
            assert!(
                (chart.height() + total - height.max(0.0)).abs() < 0.01 || chart.height() == 0.0,
                "height {height}: the band and the chart must tile the space"
            );
            for pane in &panes {
                assert!(
                    pane.collapsed || pane.rect.height() >= MIN_PANE_HEIGHT_PX,
                    "height {height}: an expanded pane below the floor: {pane:?}"
                );
            }
            assert!(
                panes
                    .windows(2)
                    .all(|pair| { (pair[0].rect.bottom() - pair[1].rect.top()).abs() < 0.01 }),
                "height {height}: panes must not overlap or leave a seam"
            );
        }
    }

    /// A pane the user collapsed by hand stays collapsed however much room
    /// appears — the automatic rule decides what *cannot* be shown, never what
    /// the user decided not to show.
    #[test]
    fn a_hand_collapsed_pane_stays_collapsed_in_a_tall_band() {
        let (_, panes) = split_panes(band(1000.0), &[PaneSizing::Collapsed, PaneSizing::Auto]);
        assert!(panes[0].collapsed, "the user's choice survives the room");
        assert!(!panes[1].collapsed);
    }

    /// A dragged height is honoured, and still cannot go below the floor: the
    /// divider stops rather than producing a pane nobody can read.
    #[test]
    fn a_dragged_height_is_honoured_but_never_below_the_floor() {
        let (_, tall) = split_panes(band(1000.0), &[PaneSizing::Manual(300.0)]);
        assert!((tall[0].rect.height() - 300.0).abs() < 0.01);

        let (_, squeezed) = split_panes(band(1000.0), &[PaneSizing::Manual(5.0)]);
        assert!(
            (squeezed[0].rect.height() - MIN_PANE_HEIGHT_PX).abs() < 0.01,
            "the drag stops at the floor: {:?}",
            squeezed[0]
        );
    }
}
