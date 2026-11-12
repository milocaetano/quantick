use super::*;
use crate::drawing_chrome::DrawingChromeAsk;

fn ready() -> State {
    let mut quick = State::default();
    let owner = Owner {
        tab: 0,
        side: PaneSide::Time(2),
        pane: 0,
        revision: 0,
        layout: None,
    };
    update(
        &mut quick,
        Intent::Press {
            owner,
            position: [1.0, 1.0],
            value: ChartPoint::at_time(0.5, 100.0, Some(1)),
            eligibility: core::GestureEligibility {
                pointer_tool: true,
                unoccluded: true,
                area: core::GestureArea {
                    min: [0.0; 2],
                    max: [100.0; 2],
                },
            },
            band: DrawingBand::Price,
        },
    );
    update(
        &mut quick,
        Intent::Drag {
            owner,
            position: [40.0, 40.0],
            value: ChartPoint::at_time(4.5, 100.0, Some(1)),
            threshold_px: 4.0,
            level: false,
        },
    );
    update(&mut quick, Intent::Release(owner));
    quick
}

fn convert(state: &mut State, action: Action) -> Option<Box<PendingRangePlacement>> {
    update(state, Intent::Convert(action)).place_quick_range
}
fn finish(
    state: &mut State,
    conversion: PendingConversion,
    outcome: PlacementOutcome,
) -> ConversionReadback {
    *update(
        state,
        Intent::FinishConversion {
            conversion,
            outcome,
        },
    )
    .quick_range_readback
    .unwrap()
}

#[test]
fn merge_carries_one_continuation_with_its_original_side_and_request() {
    let mut quick = ready();
    let placement = convert(&mut quick, Action::Projection).unwrap();
    let request = placement.conversion.request().clone();
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
        finish(&mut quick, carried.conversion, PlacementOutcome::Placed).view,
        None
    );
}

#[test]
#[should_panic(expected = "one quick-range conversion producer")]
fn merge_rejects_a_second_producer_instead_of_dropping_its_continuation() {
    let mut first = ready();
    let mut second = ready();
    let mut ask = DrawingChromeAsk {
        place_quick_range: convert(&mut first, Action::Profile),
        ..DrawingChromeAsk::default()
    };
    ask.merge(DrawingChromeAsk {
        place_quick_range: convert(&mut second, Action::Retracement),
        ..DrawingChromeAsk::default()
    });
}

#[test]
fn dismiss_before_finishing_cannot_restore_a_range() {
    let mut quick = ready();
    let placement = convert(&mut quick, Action::Profile).unwrap();
    assert!(quick.present());
    update(&mut quick, Intent::Dismiss);
    assert!(!quick.present());
    assert_eq!(
        finish(
            &mut quick,
            placement.conversion,
            PlacementOutcome::ActionRefused
        ),
        ConversionReadback::default()
    );
}
