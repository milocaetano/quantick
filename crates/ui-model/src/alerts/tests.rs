use super::*;
use crate::Model as _;
use quantick_alerts::{AlertBook, SourceContext, sound};

fn context() -> Context {
    Context {
        market: MarketRef {
            source: SourceContext::Live {
                feed: "fixture".into(),
            },
            symbol: "TEST".into(),
            aggregation: "tick:100".into(),
        },
        price: Some(100.0),
        utc_ms: 1_800_000_000_000,
        display_offset_minutes: 0,
        drawings: vec![],
    }
}

fn opened_draft() -> State {
    let mut state = State::default();
    Model::update(&mut state, Intent::Apply(Box::new(Request {
        open: true,
        ..Request::default()
    })));
    Model::update(&mut state, Intent::Create {
        context: Box::new(context()),
        target: None,
    });
    state
}

fn edit(state: &mut State, change: impl FnOnce(&mut Draft)) {
    let mut draft = state.draft().cloned().expect("a draft is open");
    change(&mut draft);
    Model::update(state, Intent::Edit(Box::new(draft)));
}

#[test]
fn alert_editor_workflows() {
    let book = AlertBook::default();
    let mut state = opened_draft();
    edit(&mut state, |draft| {
        draft.spec.message = "Preserve me".into();
        draft.subview = Subview::Notifications;
        draft.spec.notification.sound = Some(sound::AlertSound::Beep.token().into());
        draft.subview = Subview::Conditions;
        draft.spec.frequency = Frequency::PerBarClose;
    });
    let draft = state.draft().unwrap();
    assert!(book.validate(&draft.spec).is_ok());
    assert_eq!(draft.spec.message, "Preserve me");
    Model::update(&mut state, Intent::SettleCommand(Err("Disk is full".into())));
    assert_eq!(state.draft().unwrap().spec.message, "Preserve me");
    assert_eq!(state.error(), Some("Disk is full"));
    Model::update(&mut state, Intent::SettleCommand(Ok(())));
    assert!(state.draft().is_none());
}

#[test]
fn escape_leaves_confirmation_then_draft_then_manager() {
    let mut state = opened_draft();
    edit(&mut state, |draft| {
        draft.revision = Some(3);
        draft.spec.id = AlertId(7);
        draft.confirming_delete = true;
    });
    Model::update(&mut state, Intent::Escape);
    assert!(!state.draft().unwrap().confirming_delete);
    Model::update(&mut state, Intent::Escape);
    assert!(state.draft().is_none());
    assert!(state.is_open(), "Escape returns to the manager before closing it");
    Model::update(&mut state, Intent::Escape);
    assert!(!state.is_open());
}

#[test]
fn delete_and_save_ask_the_host_and_wait() {
    let mut state = opened_draft();
    edit(&mut state, |draft| {
        draft.revision = Some(3);
        draft.spec.id = AlertId(7);
    });
    let effects = Model::update(&mut state, Intent::ConfirmDelete);
    assert_eq!(
        effects.request.unwrap().command,
        Some(AlertCommand::Delete {
            id: AlertId(7),
            expected_revision: 3
        })
    );
    assert!(state.draft().unwrap().waiting);
    Model::update(&mut state, Intent::SettleCommand(Err("busy".into())));
    assert!(!state.draft().unwrap().waiting);
    let effects = Model::update(&mut state, Intent::Submit);
    assert!(matches!(
        effects.request.unwrap().command,
        Some(AlertCommand::Edit {
            expected_revision: 3,
            ..
        })
    ));
}

#[test]
fn alert_target_resolution_keeps_operators_compatible() {
    for (kind, initial, expected) in [
        (TargetKind::Channel, Operator::Crossing, Operator::Entering),
        (
            TargetKind::Rectangle,
            Operator::Crossing,
            Operator::GreaterThan,
        ),
        (
            TargetKind::Rectangle,
            Operator::LessThan,
            Operator::LessThan,
        ),
    ] {
        let selector = DrawingSelector {
            tab_id: 1,
            pane_id: 2,
            drawing_id: 3,
        };
        let target = DurableDrawingRef {
            workspace: "w".into(),
            origin: "o".into(),
            pane: "p".into(),
            drawing: 3,
            incarnation: "i".into(),
        };
        let mut chart = context();
        chart.drawings.push(DrawingChoice {
            selector,
            target: None,
            kind,
            name: "Region fixture".into(),
        });
        let mut state = State::default();
        Model::update(&mut state, Intent::Create {
            context: Box::new(chart),
            target: None,
        });
        edit(&mut state, |draft| draft.spec.conditions[0].operator = initial);
        let effects = Model::update(&mut state, Intent::Resolve { index: 0, selector });
        assert_eq!(effects.request.unwrap().resolve, Some(selector));
        Model::update(&mut state, Intent::SettleTarget(Ok(target.clone())));
        let condition = &state.draft().unwrap().spec.conditions[0];
        assert_eq!(condition.target, TargetRef::Drawing(target));
        assert_eq!(condition.operator, expected);
    }
}

#[test]
fn host_owned_requests_are_refused_and_drawing_opens_wait_for_a_chart() {
    let mut state = State::default();
    Model::update(&mut state, Intent::Apply(Box::new(Request {
        locate: Some(DurableDrawingRef {
            workspace: "w".into(),
            origin: "o".into(),
            pane: "p".into(),
            drawing: 1,
            incarnation: "i".into(),
        }),
        ..Request::default()
    })));
    assert_eq!(state.error(), Some("Unavailable"));
    Model::update(&mut state, Intent::OpenDrawing(None));
    assert!(state.is_editing());
    Model::update(&mut state, Intent::CloseManager);
    assert!(state.is_open(), "an editor keeps the manager open");
    Model::update(&mut state, Intent::ConsumePending(None));
    assert_eq!(state.error(), Some("Open a chart before creating an alert."));
    Model::update(&mut state, Intent::OpenDrawing(None));
    Model::update(&mut state, Intent::ConsumePending(Some(Box::new(context()))));
    assert_eq!(state.draft().unwrap().spec.name, "TEST price alert");
}

#[test]
fn editing_a_saved_alert_restores_its_custom_expiry() {
    let mut spec = opened_draft().draft().unwrap().spec.clone();
    spec.expiration = Expiration::At {
        utc_ms: 1_800_000_000_000,
        display_offset_minutes: -180,
    };
    let mut state = State::default();
    Model::update(&mut state, Intent::EditExisting {
        spec: Box::new(spec),
        revision: 5,
        context: None,
        delete: true,
    });
    let draft = state.draft().unwrap();
    assert_eq!(
        (draft.revision, draft.expiration_choice),
        (Some(5), 4)
    );
    assert!(draft.confirming_delete && draft.focus_delete_cancel);
    assert_eq!(draft.context.display_offset_minutes, -180);
}
