use super::*;

fn ready() -> QuickRange {
    let mut quick = QuickRange {
        side: Some(PaneSide::Time(2)),
        ..QuickRange::default()
    };
    let context = core::RangeContext::default();
    let anchor = core::Anchor {
        bar: 0.5,
        price: 100.0,
        time_ms: Some(1),
    };
    quick.model.update(
        Command::Press {
            position: [1.0, 1.0],
            anchor,
            eligibility: core::GestureEligibility {
                pointer_tool: true,
                unoccluded: true,
                area: core::GestureArea {
                    min: [0.0; 2],
                    max: [100.0; 2],
                },
            },
        },
        context,
    );
    quick.model.update(
        Command::Drag {
            position: [40.0, 40.0],
            anchor: core::Anchor { bar: 4.5, ..anchor },
            threshold_px: 4.0,
        },
        context,
    );
    quick.model.update(Command::Release, context);
    quick
}

#[test]
fn merge_carries_one_continuation_with_its_original_side_and_request() {
    let mut quick = ready();
    let placement = quick.convert(Action::Projection).unwrap();
    let request = *placement.conversion.request();
    let mut ask = DrawingChromeAsk::default();
    ask.merge(DrawingChromeAsk {
        place_quick_range: Some(placement),
        ..DrawingChromeAsk::default()
    });
    ask.merge(DrawingChromeAsk {
        show_all: true,
        ..DrawingChromeAsk::default()
    });
    assert!(ask.show_all);
    let carried = ask.place_quick_range.take().unwrap();
    assert_eq!(carried.side, PaneSide::Time(2));
    assert_eq!(*carried.conversion.request(), request);
    assert_eq!(
        quick
            .finish_conversion(carried.conversion, PlacementOutcome::Placed)
            .view,
        None
    );
}

#[test]
#[should_panic(expected = "one quick-range conversion producer")]
fn merge_rejects_a_second_producer_instead_of_dropping_its_continuation() {
    let mut first = ready();
    let mut second = ready();
    let mut ask = DrawingChromeAsk {
        place_quick_range: first.convert(Action::Profile),
        ..DrawingChromeAsk::default()
    };
    ask.merge(DrawingChromeAsk {
        place_quick_range: second.convert(Action::Retracement),
        ..DrawingChromeAsk::default()
    });
}

#[test]
fn dismiss_before_finishing_cannot_restore_a_range() {
    let mut quick = ready();
    let placement = quick.convert(Action::Profile).unwrap();
    assert!(quick.dismiss());
    assert_eq!(
        quick.finish_conversion(placement.conversion, PlacementOutcome::ActionRefused),
        ConversionReadback::default()
    );
}
