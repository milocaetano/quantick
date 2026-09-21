//! UI-side indicator state: the columns the renderer reads.
//!
//! The worker owns the truth (the [`IndicatorHost`]); this module owns the
//! UI's *copy* of it, kept in sync by applying delta events each frame — the
//! same shape as the `FeedEvent` pattern. No locks anywhere near the render
//! path: the renderer reads plain vectors this struct owns.
//!
//! [`IndicatorHost`]: quantick_indicators::IndicatorHost

pub(crate) mod library;
pub(crate) mod preset_file;
pub(crate) mod state_file;

// One instance in `view`, the collection and its delta handling in `views`,
// the height arithmetic in `panes`. The leaves are private: the re-exports
// below are this module's address, so naming `IndicatorView` costs this file
// and not the collection's event loop.
mod panes;
mod view;
mod views;

pub(crate) use panes::{MIN_PANE_HEIGHT_PX, PaneSizing, PaneSlot, split_panes};
pub(crate) use view::{IndicatorView, MAX_PANES, PANE_HEIGHT_FRAC};
pub(crate) use views::IndicatorViews;

#[cfg(test)]
mod tests {
    // The module's own contract, exercised through the address above: the
    // leaves are private, so these name `IndicatorViews` and `split_panes`
    // exactly as the rest of the crate does.
    use super::*;
    use eframe::egui;
    use quantick_indicators::{
        EvalError, IndicatorDescriptor, PlotId, PlotSpec, PlotStyle, PreviewFrame, Rgba8,
    };

    use crate::indicator_worker::{IndicatorEvent, SlotId};
    use crate::indicators::panes::{COLLAPSED_PANE_HEIGHT_PX, MIN_CHART_HEIGHT_PX};

    fn descriptor(overlay: bool, plots: usize) -> IndicatorDescriptor {
        IndicatorDescriptor {
            title: "test".to_owned(),
            short_title: None,
            overlay,
            plots: (0..plots)
                .map(|i| PlotSpec {
                    id: PlotId::new(i),
                    title: format!("p{i}"),
                    style: PlotStyle::Line,
                    base_color: Rgba8::opaque(255, 255, 255),
                    width: 1.0,
                    offset: 0,
                    marker: None,
                })
                .collect(),
            inputs: Vec::new(),
            fills: Vec::new(),
        }
    }

    #[test]
    fn deltas_reconstruct_the_columns() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor(true, 2),
            vec![vec![1.0], vec![10.0]],
        ));
        views.apply(crate::indicator_worker::event_fixture::appended(
            slot,
            vec![2.0, 20.0],
        ));
        let view = &views.all()[0];
        assert_eq!(view.rows, 2);
        assert_eq!(view.columns[0], vec![1.0, 2.0]);
        assert_eq!(view.columns[1], vec![10.0, 20.0]);
    }

    #[test]
    fn appended_row_invalidates_the_preview() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![]],
        ));
        views.apply(IndicatorEvent::Preview {
            slot,
            frame: Some(PreviewFrame::new(vec![5.0])),
        });
        assert!(views.all()[0].preview.is_some());
        views.apply(crate::indicator_worker::event_fixture::appended(
            slot,
            vec![1.0],
        ));
        assert!(
            views.all()[0].preview.is_none(),
            "a frame describing the closed bar must not draw one slot further right"
        );
    }

    const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
    const BLUE: Rgba8 = Rgba8::opaque(0, 0, 255);

    /// A paint-only indicator: no plots at all, which is exactly the shape
    /// `force_bar.pine` has and the shape a row count derived from `columns`
    /// would get wrong.
    fn paint_only(views: &mut IndicatorViews, rows: usize) -> SlotId {
        let slot = views.allocate_slot("script.force_bar");
        views.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: descriptor(true, 0),
            columns: Vec::new(),
            bar_paint: Vec::new(),
            rows,
            inputs: Vec::new(),
            stale: None,
        });
        slot
    }

    #[test]
    fn a_late_first_paint_lands_on_the_bar_that_asked_for_it() {
        // The trap: an indicator with no plots and no paint yet has nothing
        // to count rows from, and a colour arriving at bar 7 would land on
        // bar 0 — the wrong candle, silently.
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 7);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });

        assert_eq!(views.bar_paint(7), Some(RED), "the bar that asked");
        assert_eq!(views.bar_paint(0), None, "and no other");
        assert_eq!(views.all()[0].rows, 8);
    }

    /// Past a certain zoom the chart folds several bars into one candle. The
    /// paint has to survive that fold, or the marks disappear at exactly the
    /// zoom a trader reaches for to see the whole session.
    #[test]
    fn a_folded_slot_wears_the_newest_paint_it_contains() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        for paint in [Some(RED), None, Some(BLUE), None] {
            views.apply(IndicatorEvent::Appended {
                slot,
                row: Vec::new(),
                paint,
            });
        }

        // A slot folding bars 0..4: RED at 0, BLUE at 2, and BLUE is newer.
        assert_eq!(views.slot_paint(0..4, false), Some(BLUE));
        // A slot folding only 0..2 never sees the blue one.
        assert_eq!(views.slot_paint(0..2, false), Some(RED));
        // One that folds only unpainted bars stays unpainted.
        assert_eq!(views.slot_paint(3..4, false), None);
    }

    #[test]
    fn the_forming_bar_outranks_every_committed_bar_in_its_slot() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Preview {
            slot,
            frame: Some(PreviewFrame {
                paint: Some(BLUE),
                ..PreviewFrame::new(Vec::new())
            }),
        });

        assert_eq!(
            views.slot_paint(0..4, true),
            Some(BLUE),
            "the forming bar is the newest end of the fold"
        );
        assert_eq!(
            views.slot_paint(0..4, false),
            Some(RED),
            "a slot the forming bar does not belong to ignores it"
        );
    }

    #[test]
    fn a_hidden_indicator_paints_nothing() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Preview {
            slot,
            frame: Some(PreviewFrame {
                paint: Some(RED),
                ..PreviewFrame::new(Vec::new())
            }),
        });
        assert_eq!(views.bar_paint(0), Some(RED));
        assert_eq!(views.forming_paint(), Some(RED));

        views.toggle_hidden(slot);
        assert_eq!(
            views.bar_paint(0),
            None,
            "the eye is the trader saying not now; colours left on the \
             candles would be unexplainable from the screen"
        );
        assert_eq!(views.forming_paint(), None);
        assert!(!views.paints_any());
    }

    #[test]
    fn the_last_view_to_ask_paints_the_bar() {
        let mut views = IndicatorViews::new();
        let lower = paint_only(&mut views, 0);
        let upper = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot: lower,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Appended {
            slot: upper,
            row: Vec::new(),
            paint: Some(BLUE),
        });
        assert_eq!(views.bar_paint(0), Some(BLUE));

        // Bar 1: only the lower one asks, so it shows through.
        views.apply(IndicatorEvent::Appended {
            slot: lower,
            row: Vec::new(),
            paint: Some(RED),
        });
        views.apply(IndicatorEvent::Appended {
            slot: upper,
            row: Vec::new(),
            paint: None,
        });
        assert_eq!(views.bar_paint(1), Some(RED));
    }

    #[test]
    fn prepended_history_carries_the_paint_with_it() {
        let mut views = IndicatorViews::new();
        let slot = paint_only(&mut views, 0);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: Vec::new(),
            paint: Some(RED),
        });
        assert_eq!(views.bar_paint(0), Some(RED));

        views.shift_rows(3);
        assert_eq!(
            views.bar_paint(3),
            Some(RED),
            "older bars arrived in front; the colour moved with its bar"
        );
        assert_eq!(views.bar_paint(0), None);
    }

    #[test]
    fn an_ordinary_chart_never_looks_up_paint() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("native.ema");
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![1.0, 2.0]],
        ));
        assert!(
            !views.paints_any(),
            "no indicator paints, so the renderer skips the channel entirely"
        );
        assert_eq!(views.bar_paint(0), None);
    }

    #[test]
    fn events_for_removed_slots_are_dropped() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![]],
        ));
        views.remove(slot);
        views.apply(crate::indicator_worker::event_fixture::appended(
            slot,
            vec![1.0],
        ));
        assert!(views.all().is_empty(), "the remove always wins the race");
    }

    /// A slot removed before its first build answered: the answer must not
    /// resurrect it. The layout switch is the production path that races
    /// this way — the add's `Rebuilt` was still on the channel.
    #[test]
    fn a_first_build_for_a_removed_slot_is_dropped_too() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot("test.indicator");
        views.remove(slot);
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            slot,
            descriptor(true, 1),
            vec![vec![]],
        ));
        assert!(
            views.all().is_empty(),
            "no ghost view for a slot the remove took"
        );
    }

    #[test]
    fn overlay_and_pane_filters_respect_state() {
        let mut views = IndicatorViews::new();
        for (overlay, hidden, errored) in [
            (true, false, false),  // drawn overlay
            (true, true, false),   // hidden overlay
            (false, false, false), // drawn pane
            (false, false, true),  // errored pane
        ] {
            let slot = views.allocate_slot("test.indicator");
            views.apply(crate::indicator_worker::event_fixture::rebuilt(
                slot,
                descriptor(overlay, 1),
                vec![vec![]],
            ));
            if hidden {
                views.toggle_hidden(slot);
            }
            if errored {
                views.apply(IndicatorEvent::Error {
                    slot,
                    error: EvalError {
                        bar_index: 0,
                        message: "boom".to_owned(),
                    },
                });
            }
        }
        assert_eq!(views.visible_overlays().count(), 1);
        assert_eq!(views.visible_panes().count(), 1);
    }

    /// A pane's scale belongs to the indicator, not to the slot's position on
    /// screen: removing one and adding another must not hand the newcomer a
    /// range someone set for a different series.
    #[test]
    fn a_removed_pane_takes_its_scale_with_it() {
        let mut views = IndicatorViews::new();
        let first = views.allocate_slot("test.indicator");
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            first,
            descriptor(false, 1),
            vec![vec![1.0]],
        ));
        let view = views.view_mut(first).expect("the pane is there");
        view.last_auto = Some((0.0, 10.0));
        view.scale.zoom(0.5, (0.0, 10.0));
        assert!(!views.all()[0].scale.is_auto());

        views.remove(first);
        let second = views.allocate_slot("test.indicator");
        views.apply(crate::indicator_worker::event_fixture::rebuilt(
            second,
            descriptor(false, 1),
            vec![vec![1.0]],
        ));
        let fresh = &views.all()[0];
        assert!(fresh.scale.is_auto(), "a new pane fits its own values");
        assert!(fresh.last_auto.is_none(), "and has not been drawn yet");
    }

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
