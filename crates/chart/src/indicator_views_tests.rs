//! The collection's contract, exercised through the module's address: the
//! leaves are private, so these name `IndicatorViews` exactly as a consumer
//! does.

use super::*;
use quantick_indicator_session::{IndicatorEvent, SlotId};
use quantick_indicators::{
    EvalError, IndicatorDescriptor, PlotId, PlotSpec, PlotStyle, PreviewFrame, Rgba8,
};

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

/// A `Rebuilt` carrying only the shape a test cares about: no paint, no
/// bound inputs, not stale, and the row count taken from the columns.
///
/// The row count is a field rather than a derivation in production
/// precisely because a paint-only indicator has no column to count — but
/// every test using this declares plots, so deriving it is exact for them.
fn rebuilt(
    slot: SlotId,
    descriptor: IndicatorDescriptor,
    columns: Vec<Vec<f64>>,
) -> IndicatorEvent {
    IndicatorEvent::Rebuilt {
        slot,
        descriptor,
        rows: columns.first().map_or(0, Vec::len),
        columns,
        bar_paint: Vec::new(),
        inputs: Vec::new(),
        stale: None,
    }
}

/// An `Appended` row that asks for no candle paint.
fn appended(slot: SlotId, row: Vec<f64>) -> IndicatorEvent {
    IndicatorEvent::Appended {
        slot,
        row,
        paint: None,
    }
}

#[test]
fn deltas_reconstruct_the_columns() {
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    views.apply(rebuilt(
        slot,
        descriptor(true, 2),
        vec![vec![1.0], vec![10.0]],
    ));
    views.apply(appended(slot, vec![2.0, 20.0]));
    let view = &views.all()[0];
    assert_eq!(view.rows, 2);
    assert_eq!(view.columns[0], vec![1.0, 2.0]);
    assert_eq!(view.columns[1], vec![10.0, 20.0]);
}

#[test]
fn appended_row_invalidates_the_preview() {
    let mut views = IndicatorViews::new();
    let slot = views.allocate_slot("test.indicator");
    views.apply(rebuilt(slot, descriptor(true, 1), vec![vec![]]));
    views.apply(IndicatorEvent::Preview {
        slot,
        frame: Some(PreviewFrame::new(vec![5.0])),
    });
    assert!(views.all()[0].preview.is_some());
    views.apply(appended(slot, vec![1.0]));
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
    views.apply(rebuilt(slot, descriptor(true, 1), vec![vec![1.0, 2.0]]));
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
    views.apply(rebuilt(slot, descriptor(true, 1), vec![vec![]]));
    views.remove(slot);
    views.apply(appended(slot, vec![1.0]));
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
    views.apply(rebuilt(slot, descriptor(true, 1), vec![vec![]]));
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
        views.apply(rebuilt(slot, descriptor(overlay, 1), vec![vec![]]));
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
    views.apply(rebuilt(first, descriptor(false, 1), vec![vec![1.0]]));
    let view = views.view_mut(first).expect("the pane is there");
    view.last_auto = Some((0.0, 10.0));
    view.scale.zoom(0.5, (0.0, 10.0));
    assert!(!views.all()[0].scale.is_auto());

    views.remove(first);
    let second = views.allocate_slot("test.indicator");
    views.apply(rebuilt(second, descriptor(false, 1), vec![vec![1.0]]));
    let fresh = &views.all()[0];
    assert!(fresh.scale.is_auto(), "a new pane fits its own values");
    assert!(fresh.last_auto.is_none(), "and has not been drawn yet");
}

/// The ask is floored for every sizing but a hand collapse, which asks for
/// exactly one labelled strip.
#[test]
fn a_pane_never_asks_for_less_than_the_readable_floor() {
    assert!((PaneSizing::Auto.desired(1000.0) - 1000.0 * PANE_HEIGHT_FRAC).abs() < 0.01);
    assert!((PaneSizing::Auto.desired(10.0) - MIN_PANE_HEIGHT_PX).abs() < 0.01);
    assert!((PaneSizing::Manual(300.0).desired(1000.0) - 300.0).abs() < 0.01);
    assert!((PaneSizing::Manual(5.0).desired(1000.0) - MIN_PANE_HEIGHT_PX).abs() < 0.01);
    assert!((PaneSizing::Collapsed.desired(1000.0) - COLLAPSED_PANE_HEIGHT_PX).abs() < 0.01);
}
