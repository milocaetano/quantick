//! `orderflow.bubbles.save_changes.set` through a fake pane: the switch is
//! the bound asset's, and a pane with no asset or no view is refused unedited.
use super::*;
use crate::bubble_fake::FakePane;
use crate::bubble_preset::TickSpherePane;
use serde_json::json;

fn call(pane: &mut FakePane, on: bool) -> Result<Value, ControlError> {
    let actor = crate::test_support::FakeWindow::assistant();
    let input = json!({"tab_id": "1", "pane_id": "2", "save_changes": on});
    set(pane, &mut (), &actor, &input)
}

fn pane() -> FakePane {
    FakePane::new(TickSpherePane::new(false, false))
}

#[test]
fn the_switch_is_the_bound_assets_and_reports_what_moved() {
    let mut flow = pane();
    flow.look_mut().asset = Some("binance:BTCUSDT".to_owned());
    let on = call(&mut flow, true).unwrap();
    assert_eq!(on["asset"], "binance:BTCUSDT");
    assert_eq!(on["save_changes"], true);
    assert_eq!(on["changed"], true);
    assert_eq!(on["screen"], "stored");
    assert_eq!(
        (on["tab_id"].clone(), on["pane_id"].clone()),
        (json!("1"), json!("2"))
    );
    let again = call(&mut flow, true).unwrap();
    assert_eq!(
        (again["changed"].clone(), again["screen"].clone()),
        (json!(false), json!("kept"))
    );
    let off = call(&mut flow, false).unwrap();
    assert_eq!(
        (off["changed"].clone(), off["screen"].clone()),
        (json!(true), json!("kept"))
    );
    assert!(!flow.look().save_changes);
}

#[test]
fn a_pane_with_no_asset_or_no_view_is_refused_before_any_edit() {
    let mut unbound = pane();
    let error = call(&mut unbound, true).unwrap_err();
    assert!(
        format!("{error:?}").contains("belong to no asset"),
        "{error:?}"
    );
    assert!(!unbound.look().save_changes, "refused before any edit");
    let mut no_view = FakePane {
        view: None,
        ..pane()
    };
    let error = call(&mut no_view, true).unwrap_err();
    assert!(
        format!("{error:?}").contains("has no order-flow view"),
        "{error:?}"
    );
}
