use super::*;

struct Facts(Vec<(TabId, &'static str, &'static str)>);
impl Topology for Facts {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn tab(&self, index: usize) -> Option<TabFacts<'_>> {
        self.0.get(index).map(|(id, feed, symbol)| TabFacts {
            id: *id,
            feed,
            symbol,
        })
    }
}
fn boot() -> (ArrangementLifecycle, Facts) {
    (
        ArrangementLifecycle::new(TabId(0)),
        Facts(vec![(TabId(0), "feed", "A")]),
    )
}
fn open(model: &mut ArrangementLifecycle, facts: &mut Facts) {
    let plan = model.plan(Command::Open, facts).unwrap();
    let Effect::Append { id } = plan.effect() else {
        panic!("append")
    };
    facts.0.push((id, "feed", "A"));
    model.commit(plan, facts).unwrap();
}
#[test]
fn close_preserves_position_instead_of_selected_identity() {
    let (mut model, mut facts) = boot();
    open(&mut model, &mut facts);
    open(&mut model, &mut facts);
    let select = model.plan(Command::Select(1), &facts).unwrap();
    model.commit(select, &facts).unwrap();
    let close = model.plan(Command::Close(0), &facts).unwrap();
    facts.0.remove(0);
    model.commit(close, &facts).unwrap();
    assert_eq!((model.active_index(), model.active_id()), (1, TabId(2)));
}
#[test]
fn last_and_invalid_close_refuse_without_changing_selection() {
    let (model, facts) = boot();
    assert!(matches!(
        model.plan(Command::Close(0), &facts),
        Err(ArrangementError::LastTab)
    ));
    assert!(model.plan(Command::Close(4), &facts).is_err());
    assert_eq!(model.active_id(), TabId(0));
}
#[test]
fn duplicate_market_is_allowed_but_duplicate_identity_is_not() {
    let (mut model, mut facts) = boot();
    open(&mut model, &mut facts);
    assert_eq!(model.active_id(), TabId(1));
    facts.0[0].0 = TabId(1);
    assert!(matches!(
        model.plan(Command::Cycle(1), &facts),
        Err(ArrangementError::InvalidTopology)
    ));
}
#[test]
fn stale_plan_after_selection_cannot_commit() {
    let (mut model, mut facts) = boot();
    open(&mut model, &mut facts);
    let stale = model.plan(Command::Select(0), &facts).unwrap();
    let current = model.plan(Command::Select(1), &facts).unwrap();
    model.commit(current, &facts).unwrap();
    assert!(matches!(
        model.commit(stale, &facts),
        Err(ArrangementError::StaleTransition)
    ));
    assert_eq!(model.active_id(), TabId(1));
}
#[test]
fn unfinished_open_does_not_consume_an_identity() {
    let (model, facts) = boot();
    let refused = model.plan(Command::Open, &facts).unwrap();
    let retried = model.plan(Command::Open, &facts).unwrap();
    assert_eq!(refused.effect(), retried.effect());
}
#[test]
fn a_missing_physical_effect_cannot_commit_the_policy() {
    let (mut model, facts) = boot();
    let opening = model.plan(Command::Open, &facts).unwrap();
    assert!(model.commit(opening, &facts).is_err());
    assert_eq!(model.active_id(), TabId(0));
}
#[test]
fn cycles_wrap_and_clamped_restore_selection_keeps_literal_position() {
    let (mut model, mut facts) = boot();
    open(&mut model, &mut facts);
    let wrap = model.plan(Command::Cycle(1), &facts).unwrap();
    model.commit(wrap, &facts).unwrap();
    assert_eq!(model.active_index(), 0);
    let back = model.plan(Command::Cycle(-1), &facts).unwrap();
    model.commit(back, &facts).unwrap();
    assert_eq!(model.active_index(), 1);
    let clamp = model
        .plan(Command::SelectClamped(usize::MAX), &facts)
        .unwrap();
    model.commit(clamp, &facts).unwrap();
    assert_eq!(model.active_index(), 1);
}

fn restore_trace(
    model: &mut ArrangementLifecycle,
    facts: &mut Facts,
    refuse_open: bool,
) -> Vec<RestoreEffect> {
    let mut trace = Vec::new();
    loop {
        let step = model.restore_step(facts).unwrap();
        let effect = step.effect();
        trace.push(effect);
        let opening = matches!(effect, RestoreEffect::OpenSaved { adopt: None, .. });
        if !(opening && refuse_open)
            && let Some(plan) = model.plan_restore(&step, facts).unwrap()
        {
            match plan.effect() {
                Effect::Append { id } => facts.0.push((id, "feed", "A")),
                Effect::Remove { index, .. } => {
                    facts.0.remove(index);
                }
                Effect::Select => {}
            }
            model.commit(plan, facts).unwrap();
        }
        model.advance_restore(step, facts).unwrap();
        if effect == RestoreEffect::Finish {
            return trace;
        }
    }
}
#[test]
fn startup_restore_literal_sequence_adopts_only_boot_market() {
    let (mut model, mut facts) = boot();
    model
        .begin_restore(
            RestoreMode::StartupOrImport,
            1,
            Some(("feed", "A")),
            99,
            &facts,
        )
        .unwrap();
    assert_eq!(
        restore_trace(&mut model, &mut facts, false),
        vec![
            RestoreEffect::AdoptSettings,
            RestoreEffect::RestoreChrome,
            RestoreEffect::OpenSaved {
                index: 0,
                adopt: Some(TabId(0))
            },
            RestoreEffect::ArrangeSaved {
                index: 0,
                target: TabId(0)
            },
            RestoreEffect::SelectSaved { index: 99 },
            RestoreEffect::RefreshLabel,
            RestoreEffect::Finish,
        ]
    );
}
#[test]
fn named_restore_opens_before_closes_and_restores_chrome_afterwards() {
    let (mut model, mut facts) = boot();
    model
        .begin_restore(RestoreMode::Named, 1, Some(("feed", "A")), 0, &facts)
        .unwrap();
    assert_eq!(
        restore_trace(&mut model, &mut facts, false),
        vec![
            RestoreEffect::OpenSaved {
                index: 0,
                adopt: None
            },
            RestoreEffect::ArrangeSaved {
                index: 0,
                target: TabId(1)
            },
            RestoreEffect::CloseStale { index: Some(0) },
            RestoreEffect::RestoreChrome,
            RestoreEffect::SelectSaved { index: 0 },
            RestoreEffect::RefreshLabel,
            RestoreEffect::Finish,
        ]
    );
    assert_eq!(facts.0[0].0, TabId(1));
}
#[test]
fn refused_open_still_arranges_last_then_preserves_last_close_refusal() {
    let (mut model, mut facts) = boot();
    open(&mut model, &mut facts);
    model
        .begin_restore(
            RestoreMode::StartupOrImport,
            1,
            Some(("missing", "A")),
            99,
            &facts,
        )
        .unwrap();
    assert_eq!(
        restore_trace(&mut model, &mut facts, true),
        vec![
            RestoreEffect::AdoptSettings,
            RestoreEffect::RestoreChrome,
            RestoreEffect::OpenSaved {
                index: 0,
                adopt: None
            },
            RestoreEffect::ArrangeSaved {
                index: 0,
                target: TabId(1)
            },
            RestoreEffect::CloseStale { index: Some(0) },
            RestoreEffect::CloseStale { index: Some(0) },
            RestoreEffect::SelectSaved { index: 99 },
            RestoreEffect::RefreshLabel,
            RestoreEffect::Finish,
        ]
    );
    assert_eq!(facts.0[0].0, TabId(1));
}
#[test]
fn empty_startup_still_emits_settings_and_chrome_without_selection() {
    let (mut model, mut facts) = boot();
    model
        .begin_restore(RestoreMode::StartupOrImport, 0, None, 99, &facts)
        .unwrap();
    assert_eq!(
        restore_trace(&mut model, &mut facts, false),
        vec![
            RestoreEffect::AdoptSettings,
            RestoreEffect::RestoreChrome,
            RestoreEffect::Finish
        ]
    );
}
#[test]
fn reentrant_selection_invalidates_an_old_restore_step() {
    let (mut model, facts) = boot();
    model
        .begin_restore(
            RestoreMode::StartupOrImport,
            1,
            Some(("feed", "A")),
            0,
            &facts,
        )
        .unwrap();
    let stale = model.restore_step(&facts).unwrap();
    let selected = model.plan(Command::Select(0), &facts).unwrap();
    model.commit(selected, &facts).unwrap();
    assert!(matches!(
        model.advance_restore(stale, &facts),
        Err(ArrangementError::StaleTransition)
    ));
}
#[test]
fn replacement_restore_invalidates_the_previous_token() {
    let (mut model, facts) = boot();
    model
        .begin_restore(RestoreMode::Named, 1, Some(("feed", "A")), 0, &facts)
        .unwrap();
    let stale = model.restore_step(&facts).unwrap();
    model
        .begin_restore(RestoreMode::Named, 1, Some(("feed", "A")), 0, &facts)
        .unwrap();
    assert!(model.plan_restore(&stale, &facts).is_err());
}
