use super::*;
#[derive(Default)]
struct Saved {
    value: u32,
    receipt: bool,
}
enum SaveIntent {
    Save(u32),
    Receipt(u32),
}
#[derive(Debug, PartialEq)]
struct SaveEffects {
    local: Option<u32>,
    event: &'static str,
}
struct SaveModel;
impl Model for SaveModel {
    type State = Saved;
    type Intent = SaveIntent;
    type Effects = SaveEffects;
    fn update(state: &mut Saved, intent: SaveIntent) -> SaveEffects {
        match intent {
            SaveIntent::Save(value) => {
                state.value = value;
                SaveEffects {
                    local: Some(value),
                    event: "requested",
                }
            }
            SaveIntent::Receipt(value) => {
                state.value = value;
                state.receipt = true;
                SaveEffects {
                    local: None,
                    event: "settled",
                }
            }
        }
    }
}
#[test]
fn local_receipt_settles_before_return_and_reborrow_keeps_one_sink() {
    let mut adapter = Adapter::<SaveModel, (), _>::new((), |effect| effect);
    let mut delivered = vec![];
    let mut executed = vec![];
    adapter.with_updates(
        |effect| delivered.push(effect),
        |_, session| {
            session.with_local_effects(
                &mut |mut effect: SaveEffects| {
                    let receipt = effect.local.take().map(|value| {
                        executed.push(value);
                        SaveIntent::Receipt(value + 1)
                    });
                    (effect, receipt)
                },
                |session| {
                    session.reborrow().update(SaveIntent::Save(3));
                    assert!(session.state().receipt);
                    assert_eq!(
                        session.state().value,
                        4,
                        "receipt visible before next widget"
                    );
                },
            );
            session.update(SaveIntent::Save(9));
        },
    );
    assert_eq!(executed, vec![3]);
    assert_eq!(
        delivered,
        vec![
            SaveEffects {
                local: None,
                event: "requested"
            },
            SaveEffects {
                local: None,
                event: "settled"
            },
            SaveEffects {
                local: Some(9),
                event: "requested"
            }
        ]
    );
}
#[test]
fn local_receipts_are_isolated_to_the_selected_adapter_scope() {
    let mut adapter = Adapter::<SaveModel, (), _>::new((), |effect| effect);
    for (scope, value) in [(10, 4), (20, 8)] {
        adapter.with_scoped_updates(
            scope,
            |_| {},
            |_, session| {
                session.with_local_effects(
                    &mut |mut effect: SaveEffects| {
                        let receipt = effect.local.take().map(SaveIntent::Receipt);
                        (effect, receipt)
                    },
                    |session| session.update(SaveIntent::Save(value)),
                );
            },
        );
    }
    assert_eq!(adapter.scoped(10).0.value, 4);
    assert_eq!(adapter.scoped(20).0.value, 8);
    assert_eq!(adapter.model_state.value, 0);
}
struct FailedModel;
enum FailedIntent {
    Request,
    Refused(String),
}
impl Model for FailedModel {
    type State = Option<String>;
    type Intent = FailedIntent;
    type Effects = bool;
    fn update(state: &mut Self::State, intent: FailedIntent) -> bool {
        match intent {
            FailedIntent::Request => true,
            FailedIntent::Refused(reason) => {
                *state = Some(reason);
                false
            }
        }
    }
}
#[test]
fn a_different_model_settles_failure_without_recursive_local_execution() {
    let mut adapter = Adapter::<FailedModel, (), _>::new((), |effect| effect);
    let mut calls = 0;
    let mut delivered = vec![];
    adapter.with_updates(
        |effect| delivered.push(effect),
        |_, session| {
            session.with_local_effects(
                &mut |request| {
                    assert!(request);
                    calls += 1;
                    (false, Some(FailedIntent::Refused("IO refused".into())))
                },
                |session| {
                    session.update(FailedIntent::Request);
                    assert_eq!(session.state().as_deref(), Some("IO refused"));
                },
            );
        },
    );
    assert_eq!(calls, 1);
    assert_eq!(delivered, vec![false, false]);
}
#[test]
#[should_panic(expected = "local effect handlers cannot be nested")]
fn nested_local_handlers_are_rejected_instead_of_silently_replacing_the_owner() {
    let mut adapter = Adapter::<FailedModel, (), _>::new((), |effect| effect);
    adapter.with_updates(
        |_| {},
        |_, session| {
            session.with_local_effects(&mut |effect| (effect, None), |session| {
                session.with_local_effects(&mut |effect| (effect, None), |_| {});
            });
        },
    );
}
#[test]
fn declared_passes_share_state_and_local_receipts_use_the_converter() {
    let mut adapter =
        Adapter::<crate::first::Model, bool, (i64, u64)>::new(false, |(v, t)| (v, t + 1000))
            .in_slots(&[Slot::PinnedInspector, Slot::DrawingChrome]);
    let mut effects = vec![];
    let mut pass = |adapter: &mut Adapter<_, bool, _>, slot, delta| {
        adapter.with_pass(
            slot,
            |effect| effects.push(effect),
            |view: &mut bool, session: &mut UpdateSession<'_, crate::first::Model>| {
                let seen = (*view, *session.state());
                *view = true;
                session.update((delta, 17));
                seen
            },
        )
    };
    assert_eq!(
        pass(&mut adapter, Slot::PinnedInspector, 3),
        Some((false, 0))
    );
    assert_eq!(pass(&mut adapter, Slot::Toolbar, 99), None, "undeclared");
    assert_eq!(pass(&mut adapter, Slot::DrawingChrome, 7), Some((true, 3)));
    adapter.with_updates(
        |effect| effects.push(effect),
        |_, session| {
            session.with_local_effects(&mut |effect| (effect, Some((1, 99))), |session| {
                session.update((2, 20))
            })
        },
    );
    assert_eq!(adapter.model_state, 13);
    assert_eq!(
        effects,
        [(3, 1017), (10, 1017), (12, 1020), (13, 1099)],
        "each pass and receipt folds once, through the converter, in order"
    );
}
