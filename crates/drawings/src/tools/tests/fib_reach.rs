//! A fresh Fib extension projects from the end of its leg, over a reach the
//! trader can drag: the span, the grab strip at its end, the drag and the
//! saved reach, each read through the tool the host calls.

use super::*;

/// Ten pixels a bar, prices 0..100 over 500 pixels: `y = 500 - 5 * price`.
const PX_PER_BAR: f32 = 10.0;
fn chart() -> PxRect {
    PxRect::from_min_max(Px::ZERO, pos2(1000.0, 500.0))
}

/// A fall from 60 at x = 100 to 40 at x = 200: the leg is 100 px wide, and
/// the 100 % target is 20, at y = 400.
const LEG: [Px; 2] = [Px::new(100.0, 200.0), Px::new(200.0, 300.0)];
const TARGET_Y: f32 = 400.0;
const RADIUS: f32 = 4.0;

fn anchors() -> [ChartPoint; 2] {
    [ChartPoint::at(10.0, 60.0), ChartPoint::at(20.0, 40.0)]
}

fn scale() -> PriceScale {
    PriceScale::from_range(0.0, 100.0, 0.0, 500.0)
}

fn context<'a>(
    payload: &'a dyn DrawingPayload,
    anchors: &'a [ChartPoint],
    scale: &'a PriceScale,
) -> DrawContext<'a> {
    DrawContext {
        appearance: quantick_appearance::ResolvedTheme::bundled_dark(),
        payload,
        anchors,
        scale,
        px_per_bar: PX_PER_BAR,
        unit: ValueUnit::Price,
        primary_band: true,
        style: DrawingStyle::default(),
        selected: true,
        halo: false,
        content_editing: false,
        text: &crate::UNMEASURED,
    }
}

fn fib(payload: &dyn DrawingPayload) -> &FibPayload {
    payload
        .as_any()
        .downcast_ref::<FibPayload>()
        .expect("a Fib payload")
}

#[test]
fn a_fresh_projection_starts_at_the_end_of_its_leg_and_reaches_one_leg_further() {
    let extension = tool("fib-extension");
    let payload = extension.default_payload();
    assert_eq!(fib(payload.as_ref()).extend, FibExtend::Projection);
    let (anchors, scale) = (anchors(), scale());
    let ctxt = context(payload.as_ref(), &anchors, &scale);
    let on_target = |x| extension.hit_test(chart(), &LEG, pos2(x, TARGET_Y), RADIUS, &ctxt);
    assert!(!on_target(150.0), "no level line between the anchors");
    assert!(!on_target(60.0), "nor left of the leg");
    assert!(
        on_target(210.0) && on_target(295.0),
        "from the last anchor on"
    );
    assert!(!on_target(320.0), "for one leg's width and no further");
}

#[test]
fn the_end_of_the_reach_announces_a_sideways_resize() {
    let extension = tool("fib-extension");
    let payload = extension.default_payload();
    let (anchors, scale) = (anchors(), scale());
    let ctxt = context(payload.as_ref(), &anchors, &scale);
    let hint = extension.pointer_hint(chart(), &LEG, pos2(300.0, TARGET_Y), &ctxt);
    let Some(PointerHint::ResizeSide { handle }) = hint else {
        panic!("the reach's end must offer a sideways resize: {hint:?}");
    };
    assert_eq!(
        extension.handle_hint(handle),
        hint,
        "the drag keeps the cursor the hover showed"
    );
    assert_eq!(
        extension.pointer_hint(chart(), &LEG, pos2(250.0, TARGET_Y), &ctxt),
        None,
        "the middle of the levels moves the object, it does not resize it"
    );
    let retracement = tool("fib-retracement");
    let payload = retracement.default_payload();
    let ctxt = context(payload.as_ref(), &anchors, &scale);
    assert_eq!(
        retracement.pointer_hint(chart(), &LEG, pos2(200.0, 250.0), &ctxt),
        None,
        "a retracement has no reach"
    );
}

#[test]
fn dragging_the_end_sets_the_reach_in_legs_and_leaves_the_anchors_alone() {
    let extension = tool("fib-extension");
    let payload = extension.default_payload();
    let (anchors, scale) = (anchors(), scale());
    let ctxt = context(payload.as_ref(), &anchors, &scale);
    let Some(PointerHint::ResizeSide { handle }) =
        extension.pointer_hint(chart(), &LEG, pos2(300.0, TARGET_Y), &ctxt)
    else {
        panic!("no reach handle");
    };
    assert!(
        extension
            .drag_handle(
                chart(),
                &LEG,
                handle,
                pos2(450.0, 10.0),
                &ctxt,
                Constrain::Free
            )
            .is_none(),
        "the reach handle moves no anchor"
    );
    let dragged = extension
        .drag_payload_handle(chart(), &LEG, handle, pos2(450.0, 10.0), &ctxt)
        .expect("the reach handle edits the payload");
    assert_eq!(fib(dragged.as_ref()).reach_legs, Some(2.5));

    let ctxt = context(dragged.as_ref(), &anchors, &scale);
    let on_target = |x| extension.hit_test(chart(), &LEG, pos2(x, TARGET_Y), RADIUS, &ctxt);
    assert!(
        on_target(440.0) && !on_target(470.0),
        "the levels follow the drag"
    );

    // Pulled back over the leg, the reach stops at nothing rather than
    // flipping, and its end stays clear of anchor B's handle to grab again.
    let collapsed = extension
        .drag_payload_handle(chart(), &LEG, handle, pos2(50.0, 10.0), &ctxt)
        .expect("still a reach drag");
    assert_eq!(fib(collapsed.as_ref()).reach_legs, Some(0.0));
    let ctxt = context(collapsed.as_ref(), &anchors, &scale);
    let end = (200..260)
        .map(|x| x as f32)
        .find(|&x| {
            extension
                .pointer_hint(chart(), &LEG, pos2(x, TARGET_Y), &ctxt)
                .is_some()
        })
        .expect("a collapsed reach keeps an end");
    assert!(
        end > LEG[1].x + crate::gesture::DRAWING_ANCHOR_RADIUS_PX,
        "{end}"
    );
    assert!(
        extension
            .drag_payload_handle(chart(), &LEG, 0, pos2(450.0, 10.0), &ctxt)
            .is_none(),
        "an anchor handle stays an anchor move"
    );
}

#[test]
fn the_reach_survives_a_save_and_an_older_save_has_none() {
    let mut dragged = FibPayload::new(FibKind::Extension);
    dragged.reach_legs = Some(2.5);
    let saved = dragged.export_preset().expect("a Fib exports");
    let mut loaded = tool("fib-extension").default_payload();
    assert!(loaded.import_preset(&saved));
    assert_eq!(fib(loaded.as_ref()).reach_legs, Some(2.5));

    let mut older = saved.clone();
    older.as_table_mut().expect("a table").remove("reach_legs");
    let mut loaded = tool("fib-extension").default_payload();
    assert!(loaded.import_preset(&older));
    assert_eq!(fib(loaded.as_ref()).reach_legs, None, "one leg");
}

#[test]
fn an_extension_saved_between_its_anchors_keeps_that_span() {
    let extension = tool("fib-extension");
    let mut payload = FibPayload::new(FibKind::Extension);
    payload.extend = FibExtend::Anchors;
    let (anchors, scale) = (anchors(), scale());
    let ctxt = context(&payload, &anchors, &scale);
    let on_target = |x| extension.hit_test(chart(), &LEG, pos2(x, TARGET_Y), RADIUS, &ctxt);
    assert!(on_target(150.0) && !on_target(250.0));
    assert_eq!(
        extension.pointer_hint(chart(), &LEG, pos2(200.0, TARGET_Y), &ctxt),
        None,
        "only a projection's reach is dragged"
    );
}

#[test]
fn a_leg_on_one_bar_still_draws_its_targets_but_offers_no_reach_to_drag() {
    let extension = tool("fib-extension");
    let payload = extension.default_payload();
    let (anchors, scale) = (anchors(), scale());
    let ctxt = context(payload.as_ref(), &anchors, &scale);
    let vertical = [Px::new(200.0, 200.0), Px::new(200.0, 300.0)];
    assert!(extension.hit_test(chart(), &vertical, pos2(210.0, TARGET_Y), RADIUS, &ctxt));
    assert!(
        (200..260).all(|x| extension
            .pointer_hint(chart(), &vertical, pos2(x as f32, TARGET_Y), &ctxt)
            .is_none()),
        "no width to measure a reach in, so no resize is offered"
    );
}

#[test]
fn only_an_extension_offers_a_projected_reach_on_the_levels_tab() {
    assert!(!FibExtend::choices(FibKind::Retracement).contains(&FibExtend::Projection));
    assert!(FibExtend::choices(FibKind::Extension).contains(&FibExtend::Projection));
}
