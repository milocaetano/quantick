use crate::quick_range::*;

fn context() -> RangeContext {
    RangeContext {
        owner: Owner {
            tab: 7,
            pane: 3,
            layout: Some(1),
        },
        revision: 2,
    }
}

fn anchor(bar: f32) -> Anchor {
    Anchor {
        bar,
        price: 100.0 + f64::from(bar),
        time_ms: Some(1000),
    }
}

fn press(model: &mut QuickRangeModel, context: RangeContext) {
    model.update(
        Command::Press {
            position: [10.0, 10.0],
            anchor: anchor(1.5),
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
}

fn selected(model: &mut QuickRangeModel, context: RangeContext) {
    press(model, context);
    model.update(
        Command::Drag {
            position: [30.0, 10.0],
            anchor: anchor(4.5),
            threshold_px: 4.0,
        },
        context,
    );
    model.update(Command::Release, context);
}

#[test]
fn a_click_never_becomes_a_temporary_range() {
    let mut model = QuickRangeModel::default();
    let ctx = context();
    press(&mut model, ctx);
    model.update(
        Command::Drag {
            position: [14.0, 10.0],
            anchor: anchor(2.5),
            threshold_px: 4.0,
        },
        ctx,
    );
    assert!(
        model.view().is_none(),
        "the exact threshold is still a click"
    );
    model.update(Command::Release, ctx);
    assert!(!model.present());
}

#[test]
fn a_drag_settles_one_range_and_a_second_press_replaces_it() {
    let mut model = QuickRangeModel::default();
    let ctx = context();
    selected(&mut model, ctx);
    assert_eq!(model.view().unwrap().anchors, [anchor(1.5), anchor(4.5)]);
    assert_eq!(model.view().unwrap().phase, Phase::Ready);
    press(
        &mut model,
        RangeContext {
            owner: Owner {
                tab: 8,
                ..ctx.owner
            },
            ..ctx
        },
    );
    assert!(model.view().is_none());
}

#[test]
fn leaving_the_owning_tab_drops_even_an_unreleased_range() {
    let mut model = QuickRangeModel::default();
    let ctx = context();
    press(&mut model, ctx);
    model.observe(Event::ActiveTab(8), ctx);
    assert!(!model.present());
    selected(&mut model, ctx);
    model.observe(Event::PaneRemoved(ctx.owner.pane), ctx);
    assert!(!model.present());
}

#[test]
fn foreign_pane_input_does_not_complete_a_gesture() {
    let mut model = QuickRangeModel::default();
    let ctx = context();
    press(&mut model, ctx);
    model.update(
        Command::Drag {
            position: [50.0, 10.0],
            anchor: anchor(10.5),
            threshold_px: 4.0,
        },
        RangeContext {
            owner: Owner {
                pane: 99,
                ..ctx.owner
            },
            ..ctx
        },
    );
    assert!(model.view().is_none());
    assert!(model.present());
}

#[test]
fn reconciliation_precedes_conversion_and_paint_projection() {
    let mut model = QuickRangeModel::default();
    let ctx = context();
    selected(&mut model, ctx);
    let rewritten = RangeContext { revision: 3, ..ctx };
    let effect = model.update(Command::Convert(Action::Profile), rewritten);
    assert!(effect.effect.is_none());
    assert_eq!(model.view().unwrap().phase, Phase::Stale);
    assert!(!model.view().unwrap().paintable());
    assert_eq!(
        model.view().unwrap().unavailable_reason(),
        Some("range_series_changed")
    );
}

#[test]
fn changed_layout_and_persistent_selection_release_temporary_chrome() {
    let mut model = QuickRangeModel::default();
    let ctx = context();
    selected(&mut model, ctx);
    model.observe(
        Event::Reconcile,
        RangeContext {
            owner: Owner {
                layout: Some(2),
                ..ctx.owner
            },
            ..ctx
        },
    );
    assert!(!model.present());
    selected(&mut model, ctx);
    model.observe(
        Event::Selection(Some(Selection {
            pane: 99,
            drawing: 12,
        })),
        ctx,
    );
    assert!(!model.present());
}

#[test]
fn all_three_actions_preserve_future_coordinates() {
    for action in Action::ALL {
        let mut model = QuickRangeModel::default();
        let ctx = context();
        press(&mut model, ctx);
        let future = Anchor {
            time_ms: None,
            ..anchor(400.5)
        };
        model.update(
            Command::Drag {
                position: [30.0, 10.0],
                anchor: future,
                threshold_px: 4.0,
            },
            ctx,
        );
        model.update(Command::Release, ctx);
        let Some(Effect::Place(request)) = model.update(Command::Convert(action), ctx).effect
        else {
            panic!("valid future coordinates are actionable");
        };
        assert_eq!(request.action, action);
        assert_eq!(request.anchors[1], future);
    }
}

#[test]
fn refusal_retains_range_and_late_completion_cannot_clear_its_replacement() {
    let mut model = QuickRangeModel::default();
    let ctx = context();
    selected(&mut model, ctx);
    let Some(Effect::Place(first)) = model.update(Command::Convert(Action::Profile), ctx).effect
    else {
        panic!("ready request");
    };
    let result = model.observe(Event::Refused(first.id), ctx);
    assert_eq!(result.effect, Some(Effect::ExplainRefusal));
    assert_eq!(model.view().unwrap().phase, Phase::Ready);
    let Some(Effect::Place(second)) = model
        .update(Command::Convert(Action::Projection), ctx)
        .effect
    else {
        panic!("retry request");
    };
    assert_ne!(first.id, second.id);
    selected(&mut model, ctx);
    model.observe(Event::Completed(second.id), ctx);
    assert_eq!(model.view().unwrap().phase, Phase::Ready);
}

#[test]
fn history_geometry_excludes_tape_but_accepts_empty_future_chart_space() {
    let area = GestureArea {
        min: [0.0, 0.0],
        max: [90.0, 100.0],
    };
    assert!(!area.accepts([95.0, 50.0]));
    assert!(area.accepts([85.0, 50.0]));
    assert_eq!(area.clamp([110.0, -5.0]), [90.0, 0.0]);
    let mut model = QuickRangeModel::default();
    model.update(
        Command::Press {
            position: [95.0, 50.0],
            anchor: anchor(300.5),
            eligibility: GestureEligibility {
                pointer_tool: true,
                unoccluded: true,
                area,
            },
        },
        context(),
    );
    assert!(!model.present());
}

#[test]
fn a_headless_executor_consumes_the_same_effect_and_completion_event() {
    #[derive(Default)]
    struct Recorder {
        placed: Vec<PlaceRequest>,
    }
    impl Recorder {
        fn execute(&mut self, effect: Effect) -> Event {
            let Effect::Place(request) = effect else {
                panic!("placement expected")
            };
            self.placed.push(request);
            Event::Completed(request.id)
        }
    }
    let mut model = QuickRangeModel::default();
    let mut consumer = Recorder::default();
    let ctx = context();
    selected(&mut model, ctx);
    let effect = model
        .update(Command::Convert(Action::Retracement), ctx)
        .effect
        .unwrap();
    model.observe(consumer.execute(effect), ctx);
    assert!(!model.present());
    assert_eq!(consumer.placed.len(), 1);
}
