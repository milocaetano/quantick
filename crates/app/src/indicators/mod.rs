//! UI-side indicator state: the columns the renderer reads.
//!
//! The worker owns the truth (the [`IndicatorHost`]); this module owns the
//! UI's *copy* of it, kept in sync by applying delta events each frame — the
//! same shape as the `FeedEvent` pattern. No locks anywhere near the render
//! path: the renderer reads plain vectors this struct owns.
//!
//! [`IndicatorHost`]: quantick_indicators::IndicatorHost

use eframe::egui;
use quantick_indicators::{EvalError, IndicatorDescriptor, PreviewFrame};

use crate::indicator_worker::{IndicatorEvent, SlotId};

/// Fraction of the chart's height each indicator pane takes (plan §4.3:
/// fixed fraction v1, draggable dividers later).
pub(crate) const PANE_HEIGHT_FRAC: f32 = 0.20;
/// At most this many panes; further pane indicators wait until one is
/// removed (the honest alternative to shrinking panes into unreadability).
pub(crate) const MAX_PANES: usize = 3;

/// One indicator as the UI sees it.
pub(crate) struct IndicatorView {
    /// The UI-allocated slot this instance answers to.
    pub slot: SlotId,
    /// Descriptor as of the last rebuild (title, plots, overlay flag).
    pub descriptor: IndicatorDescriptor,
    /// Committed plot columns, one per descriptor plot, kept in lockstep
    /// with the worker via Rebuilt/Appended deltas.
    pub columns: Vec<Vec<f64>>,
    /// Latest forming-bar frame, if a bar is forming.
    pub preview: Option<PreviewFrame>,
    /// Error state (indicator disabled worker-side until rebuilt).
    pub error: Option<EvalError>,
    /// Eye toggle: hidden is render-side only — no recompute, state keeps
    /// flowing so unhiding is instant.
    pub hidden: bool,
}

impl IndicatorView {
    /// Rows committed so far (columns are always the same length).
    /// Exercised by the worker equivalence tests today; the M4 progress
    /// readout reads it live.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn rows(&self) -> usize {
        self.columns.first().map_or(0, Vec::len)
    }

    /// The label the UI shows for this indicator.
    pub(crate) fn label(&self) -> &str {
        self.descriptor
            .short_title
            .as_deref()
            .unwrap_or(&self.descriptor.title)
    }
}

/// Every indicator the UI knows about, in add order, plus the slot counter.
#[derive(Default)]
pub(crate) struct IndicatorViews {
    views: Vec<IndicatorView>,
    next_slot: u64,
}

impl IndicatorViews {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reserve a slot id for an add command about to be sent.
    pub(crate) fn allocate_slot(&mut self) -> SlotId {
        let slot = SlotId(self.next_slot);
        self.next_slot += 1;
        slot
    }

    /// Apply one worker delta. Events for slots the UI already removed are
    /// dropped silently — commands and events cross on the channel, and the
    /// remove always wins.
    pub(crate) fn apply(&mut self, event: IndicatorEvent) {
        match event {
            IndicatorEvent::Rebuilt {
                slot,
                descriptor,
                columns,
            } => {
                if let Some(view) = self.view_mut(slot) {
                    view.descriptor = descriptor;
                    view.columns = columns;
                    view.preview = None;
                    view.error = None;
                } else {
                    self.views.push(IndicatorView {
                        slot,
                        descriptor,
                        columns,
                        preview: None,
                        error: None,
                        hidden: false,
                    });
                }
            }
            IndicatorEvent::Appended { slot, row } => {
                if let Some(view) = self.view_mut(slot) {
                    for (column, value) in view.columns.iter_mut().zip(&row) {
                        column.push(*value);
                    }
                    // The old preview described the bar that just closed;
                    // drawing it one slot further right would be a lie. The
                    // next Preview event replaces it within the same batch.
                    view.preview = None;
                }
            }
            IndicatorEvent::Preview { slot, frame } => {
                if let Some(view) = self.view_mut(slot) {
                    view.preview = frame;
                }
            }
            IndicatorEvent::Error { slot, error } => {
                if let Some(view) = self.view_mut(slot) {
                    view.error = Some(error);
                    view.preview = None;
                }
            }
        }
    }

    fn view_mut(&mut self, slot: SlotId) -> Option<&mut IndicatorView> {
        self.views.iter_mut().find(|v| v.slot == slot)
    }

    /// Drop a slot UI-side (the worker gets the Remove command separately).
    pub(crate) fn remove(&mut self, slot: SlotId) {
        self.views.retain(|v| v.slot != slot);
    }

    /// Prepend `added` unknown rows to every column, keeping the views
    /// aligned with bars that just grew at the front.
    ///
    /// Older trades re-cut every bar, so the worker rebuilds from scratch —
    /// but that answer arrives a round-trip later, and until it does the
    /// renderer would draw every value `added` slots to the left of the
    /// candle it belongs to. `NaN` is the honest filler: it renders as a gap,
    /// which is exactly what "not computed yet" means here.
    pub(crate) fn shift_rows(&mut self, added: usize) {
        if added == 0 {
            return;
        }
        for view in &mut self.views {
            for column in &mut view.columns {
                column.splice(0..0, std::iter::repeat_n(f64::NAN, added));
            }
            // The forming bar moved with the candles; its frame is stale.
            view.preview = None;
        }
    }

    /// Flip the render-side eye toggle.
    pub(crate) fn toggle_hidden(&mut self, slot: SlotId) {
        if let Some(view) = self.view_mut(slot) {
            view.hidden = !view.hidden;
        }
    }

    /// All views, in add order (for the manager UI).
    pub(crate) fn all(&self) -> &[IndicatorView] {
        &self.views
    }

    /// Overlay indicators that should draw on the price chart right now.
    pub(crate) fn visible_overlays(&self) -> impl Iterator<Item = &IndicatorView> {
        self.views
            .iter()
            .filter(|v| v.descriptor.overlay && !v.hidden && v.error.is_none())
    }

    /// Pane indicators that get a pane right now, capped at [`MAX_PANES`].
    pub(crate) fn visible_panes(&self) -> impl Iterator<Item = &IndicatorView> {
        self.views
            .iter()
            .filter(|v| !v.descriptor.overlay && !v.hidden && v.error.is_none())
            .take(MAX_PANES)
    }
}

/// Carve the pane band off the bottom of the chart rect: each pane takes
/// [`PANE_HEIGHT_FRAC`] of the *original* height, the chart keeps the rest.
/// Pure, so the split is unit-testable without a display.
pub(crate) fn split_panes(chart: egui::Rect, pane_count: usize) -> (egui::Rect, Vec<egui::Rect>) {
    let count = pane_count.min(MAX_PANES);
    if count == 0 {
        return (chart, Vec::new());
    }
    let pane_height = chart.height() * PANE_HEIGHT_FRAC;
    let chart_bottom = chart.bottom() - pane_height * count as f32;
    let shrunk = egui::Rect::from_min_max(chart.min, egui::pos2(chart.right(), chart_bottom));
    let panes = (0..count)
        .map(|i| {
            let top = chart_bottom + pane_height * i as f32;
            egui::Rect::from_min_max(
                egui::pos2(chart.left(), top),
                egui::pos2(chart.right(), top + pane_height),
            )
        })
        .collect();
    (shrunk, panes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_indicators::{PlotId, PlotSpec, PlotStyle, Rgba8};

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
                })
                .collect(),
            inputs: Vec::new(),
        }
    }

    #[test]
    fn deltas_reconstruct_the_columns() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot();
        views.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: descriptor(true, 2),
            columns: vec![vec![1.0], vec![10.0]],
        });
        views.apply(IndicatorEvent::Appended {
            slot,
            row: vec![2.0, 20.0],
        });
        let view = &views.all()[0];
        assert_eq!(view.rows(), 2);
        assert_eq!(view.columns[0], vec![1.0, 2.0]);
        assert_eq!(view.columns[1], vec![10.0, 20.0]);
    }

    #[test]
    fn appended_row_invalidates_the_preview() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot();
        views.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: descriptor(true, 1),
            columns: vec![vec![]],
        });
        views.apply(IndicatorEvent::Preview {
            slot,
            frame: Some(PreviewFrame { values: vec![5.0] }),
        });
        assert!(views.all()[0].preview.is_some());
        views.apply(IndicatorEvent::Appended {
            slot,
            row: vec![1.0],
        });
        assert!(
            views.all()[0].preview.is_none(),
            "a frame describing the closed bar must not draw one slot further right"
        );
    }

    #[test]
    fn events_for_removed_slots_are_dropped() {
        let mut views = IndicatorViews::new();
        let slot = views.allocate_slot();
        views.apply(IndicatorEvent::Rebuilt {
            slot,
            descriptor: descriptor(true, 1),
            columns: vec![vec![]],
        });
        views.remove(slot);
        views.apply(IndicatorEvent::Appended {
            slot,
            row: vec![1.0],
        });
        assert!(views.all().is_empty(), "the remove always wins the race");
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
            let slot = views.allocate_slot();
            views.apply(IndicatorEvent::Rebuilt {
                slot,
                descriptor: descriptor(overlay, 1),
                columns: vec![vec![]],
            });
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

    #[test]
    fn pane_split_is_stacked_and_bounded() {
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let (shrunk, panes) = split_panes(chart, 2);
        assert_eq!(shrunk.height(), 60.0, "two panes take 2 x 20%");
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].top(), 60.0);
        assert_eq!(panes[1].top(), 80.0);
        assert_eq!(panes[1].bottom(), 100.0);

        let (unchanged, none) = split_panes(chart, 0);
        assert_eq!(unchanged, chart);
        assert!(none.is_empty());

        let (_, capped) = split_panes(chart, 9);
        assert_eq!(capped.len(), MAX_PANES);
    }
}
