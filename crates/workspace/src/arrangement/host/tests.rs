use super::*;
use crate::arrangement::RestoreEffect;

#[derive(Debug, PartialEq)]
struct Fake {
    symbol: &'static str,
    closed: bool,
}
impl TabRuntime for Fake {
    fn feed(&self) -> &str {
        "feed"
    }
    fn symbol(&self) -> &str {
        self.symbol
    }
    fn close(&mut self) {
        self.closed = true;
    }
}
fn fake(symbol: &'static str) -> Fake {
    Fake {
        symbol,
        closed: false,
    }
}
fn three() -> Arrangement<Fake> {
    let mut tabs = Arrangement::new(TabId(7), fake("A"));
    for symbol in ["B", "C"] {
        let plan = tabs.plan_open();
        tabs.append(plan, fake(symbol));
    }
    tabs
}
fn symbols(tabs: &Arrangement<Fake>) -> Vec<&'static str> {
    tabs.iter().map(|tab| tab.symbol).collect()
}

#[test]
fn opening_appends_in_physical_order_with_fresh_identities() {
    let tabs = three();
    assert_eq!(symbols(&tabs), ["A", "B", "C"]);
    let ids: Vec<u64> = tabs.iter_with_ids().map(|(id, _)| id).collect();
    assert_eq!(ids, [7, 8, 9]);
    assert_eq!(
        (tabs.id_at(2), tabs.position(8), tabs.position(42)),
        (9, Some(1), None)
    );
    assert_eq!(tabs.by_id(9).map(|tab| tab.symbol), Some("C"));
    assert_eq!(tabs[1].symbol, "B");
    assert_eq!(
        (tabs.len(), tabs.active_index(), tabs.active_id()),
        (3, 2, 9)
    );
}

#[test]
fn selection_and_cycle_wrap_while_a_single_tab_ignores_cycling() {
    let mut tabs = three();
    tabs.select(0);
    assert_eq!((tabs.active_index(), tabs.active_id()), (0, 7));
    tabs.cycle(-1);
    assert_eq!(tabs.active_index(), 2);
    tabs.cycle(1);
    assert_eq!(tabs.active_index(), 0);
    tabs.select(99);
    assert_eq!(tabs.active_index(), 0, "an out-of-range select is ignored");
    let mut single = Arrangement::new(TabId(0), fake("A"));
    single.cycle(1);
    assert_eq!(single.active_index(), 0);
}

#[test]
fn closing_removes_closes_and_hands_back_the_runtime() {
    let mut tabs = three();
    let plan = tabs.plan_close(1).unwrap();
    let closed = tabs.close_planned(plan).unwrap();
    assert_eq!(
        (closed.id, closed.runtime.symbol, closed.runtime.closed),
        (8, "B", true)
    );
    assert_eq!(symbols(&tabs), ["A", "C"]);
    assert!(tabs.iter().all(|tab| !tab.closed));
    assert_eq!(tabs.position(8), None);
}

#[test]
fn the_last_tab_cannot_be_closed_and_a_stale_plan_is_refused() {
    let single = Arrangement::new(TabId(0), fake("A"));
    assert_eq!(single.plan_close(0).err(), Some(ArrangementError::LastTab));
    let mut tabs = three();
    let stale = tabs.plan_close(0).unwrap();
    tabs.select(1);
    assert_eq!(
        tabs.validate_transition(&stale),
        Err(ArrangementError::StaleTransition)
    );
    assert_eq!(
        tabs.close_planned(stale).err(),
        Some(ArrangementError::StaleTransition)
    );
    assert_eq!(tabs.len(), 3);
}

#[test]
fn mutable_borrows_reach_runtimes_but_not_identity_or_order() {
    let mut tabs = three();
    tabs.runtime_mut(0).symbol = "X";
    tabs.get_mut(1).unwrap().symbol = "Y";
    tabs.by_id_mut(9).unwrap().symbol = "Z";
    for (_, tab) in tabs.iter_with_ids_mut() {
        tab.closed = true;
    }
    assert_eq!(symbols(&tabs), ["X", "Y", "Z"]);
    assert_eq!(
        tabs.iter_with_ids().map(|(id, _)| id).collect::<Vec<_>>(),
        [7, 8, 9]
    );
    assert!(tabs.get(3).is_none());
}

#[test]
fn named_restore_opens_the_saved_market_and_closes_the_stale_one() {
    let mut tabs = Arrangement::new(TabId(0), fake("old"));
    tabs.begin_restore(RestoreMode::Named, 1, Some(("feed", "new")), 0)
        .unwrap();
    let mut closed = Vec::new();
    loop {
        let step = tabs.restore_step();
        let effect = step.effect();
        if let Some(plan) = tabs.plan_restore(&step) {
            match plan.effect() {
                Effect::Append { .. } => tabs.append(plan, fake("new")),
                Effect::Remove { .. } => closed.push(tabs.close_planned(plan).unwrap()),
                Effect::Select => tabs.commit_selection(plan),
            }
        }
        tabs.advance_restore(step);
        if effect == RestoreEffect::Finish {
            break;
        }
    }
    assert_eq!(symbols(&tabs), ["new"]);
    assert_eq!(closed.len(), 1);
    assert!(closed[0].runtime.closed && closed[0].runtime.symbol == "old");
}

#[test]
fn fixture_helpers_rebuild_a_single_tab_owner() {
    let tabs = Arrangement::new(TabId(0), fake("A")).with_fixture_identity(41);
    assert_eq!(
        (tabs.active_id(), tabs.last().map(|tab| tab.symbol)),
        (41, Some("A"))
    );
    assert_eq!(tabs.into_single_runtime(), fake("A"));
}
