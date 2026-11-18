//! A script that never reached the worker still takes its slot.
use quantick_indicator_session::{
    IndicatorEvent as Event, IndicatorSource, SlotId, unloaded_script_events,
};

#[test]
fn an_unloaded_script_is_an_empty_titled_slot_then_its_error() {
    let [rebuilt, error] =
        unloaded_script_events(SlotId(7), "zigzag".into(), "zigzag.pine is gone".into());
    let Event::Rebuilt {
        slot,
        descriptor,
        columns,
        bar_paint,
        rows,
        inputs,
        stale,
    } = rebuilt
    else {
        panic!("the descriptor comes first, so the row has a title to show");
    };
    assert_eq!(slot, SlotId(7));
    assert_eq!(descriptor.title, "zigzag");
    assert!(descriptor.short_title.is_none() && !descriptor.overlay);
    assert!(descriptor.plots.is_empty() && descriptor.fills.is_empty());
    assert!(descriptor.inputs.is_empty() && inputs.is_empty());
    assert!(columns.is_empty() && bar_paint.is_empty() && rows == 0 && stale.is_none());
    let Event::Error { slot, error } = error else {
        panic!("the error follows the descriptor");
    };
    assert_eq!(slot, SlotId(7));
    assert_eq!(
        (error.bar_index, error.message.as_str()),
        (0, "zigzag.pine is gone")
    );
}

#[test]
fn an_unloaded_script_keeps_the_kind_its_source_would_carry() {
    let source = IndicatorSource::Script {
        name: "zigzag.pine".into(),
        text: String::new(),
    };
    assert_eq!(
        IndicatorSource::script_kind_id("zigzag.pine"),
        source.kind_id()
    );
}
