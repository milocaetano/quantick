use super::*;
use crate::quick_range::{Anchor, GestureArea, GestureEligibility, Phase};

fn dragging(model: &mut QuickRangeModel, context: RangeContext) {
    let anchor = Anchor {
        bar: 1.5,
        price: 100.0,
        time_ms: Some(7),
    };
    model.update(
        Command::Press {
            position: [1.0, 1.0],
            anchor,
            eligibility: GestureEligibility {
                pointer_tool: true,
                unoccluded: true,
                area: GestureArea {
                    min: [0.0; 2],
                    max: [100.0; 2],
                },
            },
        },
        context,
    );
    model.update(
        Command::Drag {
            position: [30.0, 30.0],
            anchor: Anchor {
                bar: 200.5,
                time_ms: None,
                ..anchor
            },
            threshold_px: 4.0,
        },
        context,
    );
}

fn ready(model: &mut QuickRangeModel, context: RangeContext) {
    dragging(model, context);
    model.update(Command::Release, context);
}

fn input() -> ConversionInput {
    ConversionInput {
        action: Action::Profile,
        context: RangeContext::default(),
    }
}

#[test]
fn validated_stages_reject_missing_duplicate_forward_and_cyclic_dependencies() {
    assert!(valid(&STEPS));
    assert!(!valid(&STEPS[..3]));
    let mut duplicate = STEPS;
    duplicate[3] = duplicate[2];
    assert!(!valid(&duplicate));
    let mut swapped = STEPS;
    swapped.swap(0, 1);
    assert!(!valid(&swapped));
    let mut cycle = STEPS;
    cycle[0].after = Stage::Readback.bit();
    assert!(!valid(&cycle));
    let mut model = QuickRangeModel::default();
    ready(&mut model, input().context);
    let before = model.view();
    let result = valid(&swapped).then(|| Execution::new(input()).start(&mut model, &swapped));
    assert!(result.is_none());
    assert_eq!(model.view(), before, "rejected plans mutate nothing");
}

#[test]
fn each_action_yields_one_exact_request_and_reads_its_completed_state() {
    for action in Action::ALL {
        let mut model = QuickRangeModel::default();
        let input = ConversionInput { action, ..input() };
        ready(&mut model, input.context);
        let anchors = model.view().unwrap().anchors;
        let StartedConversion::Awaiting(pending) =
            QuickRangeConversionPlan::start(&mut model, input)
        else {
            panic!("ready action yields")
        };
        assert_eq!(pending.request().id, 1);
        assert_eq!(pending.request().context, input.context);
        assert_eq!(pending.request().action, action);
        assert_eq!(pending.request().anchors, anchors);
        assert_eq!(
            model.view().unwrap().phase,
            Phase::Pending(pending.request().id)
        );
        let readback = pending.finish(&mut model, input.context, PlacementOutcome::Placed);
        assert_eq!(readback, ConversionReadback::default());
        assert!(model.view().is_none());
    }
}

#[test]
fn refusal_reporting_and_retry_keep_the_same_model_authority() {
    let mut model = QuickRangeModel::default();
    ready(&mut model, input().context);
    for outcome in [
        PlacementOutcome::InputInvalid,
        PlacementOutcome::ActionRefused,
    ] {
        let StartedConversion::Awaiting(pending) =
            QuickRangeConversionPlan::start(&mut model, input())
        else {
            panic!("retry yields")
        };
        let result = pending.finish(&mut model, input().context, outcome);
        assert_eq!(result.view.unwrap().phase, Phase::Ready);
        assert_eq!(
            result.explain_refusal,
            outcome == PlacementOutcome::ActionRefused
        );
    }
}

#[test]
fn finish_uses_current_revision_and_cannot_consume_a_replacement() {
    let mut model = QuickRangeModel::default();
    ready(&mut model, input().context);
    let StartedConversion::Awaiting(pending) = QuickRangeConversionPlan::start(&mut model, input())
    else {
        panic!("yield")
    };
    let changed = RangeContext {
        revision: 1,
        ..input().context
    };
    let result = pending.finish(&mut model, changed, PlacementOutcome::Placed);
    assert_eq!(result.view.unwrap().phase, Phase::Stale);
    ready(&mut model, changed);
    let StartedConversion::Awaiting(old) = QuickRangeConversionPlan::start(
        &mut model,
        ConversionInput {
            context: changed,
            ..input()
        },
    ) else {
        panic!("yield")
    };
    ready(&mut model, changed);
    let replacement = model.view();
    assert_eq!(
        old.finish(&mut model, changed, PlacementOutcome::Placed)
            .view,
        replacement
    );
}

#[test]
fn idle_and_dragging_models_emit_no_effect() {
    let mut model = QuickRangeModel::default();
    assert_eq!(
        QuickRangeConversionPlan::start(&mut model, input()),
        StartedConversion::Settled(ConversionReadback::default())
    );
    dragging(&mut model, input().context);
    let before = model.view();
    assert!(matches!(
        QuickRangeConversionPlan::start(&mut model, input()),
        StartedConversion::Settled(ConversionReadback { view, .. }) if view == before
    ));
    model.update(Command::Dismiss, input().context);
    assert!(matches!(
        QuickRangeConversionPlan::start(&mut model, input()),
        StartedConversion::Settled(_)
    ));
}

// These are deliberately unchecked private orders. They dispatch the exact
// production stage bodies, with an effect counter standing in for the caller.
fn mutated(steps: &[Step]) -> (ConversionReadback, Option<RangeView>, usize) {
    let mut model = QuickRangeModel::default();
    ready(&mut model, input().context);
    let (readback, effects) = match Execution::new(input()).start(&mut model, steps) {
        StartedConversion::Settled(readback) => (readback, 0),
        StartedConversion::Awaiting(pending) => (
            pending.finish_with(&mut model, input().context, PlacementOutcome::Placed, steps),
            1,
        ),
    };
    (readback, model.view(), effects)
}

#[test]
fn readback_before_completion_exposes_the_unsettled_state() {
    assert_eq!(mutated(&STEPS), (ConversionReadback::default(), None, 1));
    let mut order = STEPS;
    order.swap(2, 3);
    let (readback, final_view, effects) = mutated(&order);
    assert!(matches!(readback.view.unwrap().phase, Phase::Pending(_)));
    assert!(final_view.is_none());
    assert_eq!(effects, 1);
}

#[test]
fn result_before_update_cannot_complete_the_later_request() {
    let order = [STEPS[2], STEPS[0], STEPS[1], STEPS[3]];
    let mut model = QuickRangeModel::default();
    ready(&mut model, input().context);
    let expected = PlaceRequest {
        id: 1,
        action: input().action,
        context: input().context,
        anchors: model.view().unwrap().anchors,
    };
    // Seed a real successful completion for the ID the following Update
    // will issue, so DeliverResult calls observe rather than skipping it.
    let mut execution = Execution::new(input());
    execution.request = Some(expected);
    execution.outcome = Some(PlacementOutcome::Placed);
    let StartedConversion::Awaiting(pending) = execution.start(&mut model, &order) else {
        panic!("later update yields its request")
    };
    assert_eq!(*pending.request(), expected);
    assert_eq!(model.view().unwrap().phase, Phase::Pending(expected.id));
    let readback = pending.finish_with(
        &mut model,
        input().context,
        PlacementOutcome::Placed,
        &order,
    );
    assert_eq!(readback.view.unwrap().phase, Phase::Pending(expected.id));
    assert_eq!(readback.view, model.view());
}

#[test]
#[should_panic(expected = "readback cannot precede suspension")]
fn unsupported_pre_yield_readback_cannot_be_silently_discarded() {
    let order = [STEPS[0], STEPS[3], STEPS[1], STEPS[2]];
    let mut model = QuickRangeModel::default();
    ready(&mut model, input().context);
    let _ = Execution::new(input()).start(&mut model, &order);
}

#[test]
fn suspended_storage_is_bounded_by_the_request_and_cursor() {
    use std::mem::{align_of, size_of};
    let alignment = align_of::<PlaceRequest>().max(align_of::<usize>());
    let bound = (size_of::<PlaceRequest>() + size_of::<usize>()).next_multiple_of(alignment);
    assert!(size_of::<PendingConversion>() <= bound);
}

#[test]
fn yielding_before_update_does_not_deliver_a_drawing_effect() {
    let order = [STEPS[1], STEPS[0], STEPS[2], STEPS[3]];
    let (readback, final_view, effects) = mutated(&order);
    assert_eq!(effects, 0);
    assert!(matches!(readback.view.unwrap().phase, Phase::Pending(_)));
    assert_eq!(readback.view, final_view);
}
