//! `orderflow.bubbles.save_changes.set` through the fake window: the switch
//! is the bound asset's, and a pane with no asset or no view is refused
//! unedited.
use super::*;
use crate::test_support::{FakeBubbles, FakePane, FakeTab, FakeWindow};
use serde_json::json;

/// One tab (id 1) whose pane (id 2) has an order-flow view bound to `asset`.
fn window(asset: Option<&str>) -> FakeWindow {
    let mut pane = FakePane::with_trades(2, 50, []);
    pane.bubbles = Some(FakeBubbles {
        asset: asset.map(str::to_owned),
        ..FakeBubbles::default()
    });
    let mut tab = FakeTab::new(1, "BTCUSDT");
    tab.panes.push(pane);
    FakeWindow::new(vec![tab])
}

fn saving(window: &FakeWindow) -> bool {
    window.tabs[0].panes[0]
        .bubbles
        .as_ref()
        .is_some_and(|view| view.save_changes)
}

fn call(window: &mut FakeWindow, on: bool) -> Result<Value, ControlError> {
    let input = json!({"tab_id": "1", "pane_id": "2", "save_changes": on});
    set(window, &mut (), &FakeWindow::assistant(), &input)
}

#[test]
fn the_switch_is_the_bound_assets_and_reports_what_moved() {
    let mut flow = window(Some("binance:BTCUSDT"));
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
    assert!(!saving(&flow));
}

#[test]
fn a_pane_with_no_asset_or_no_view_is_refused_before_any_edit() {
    let mut unbound = window(None);
    let error = call(&mut unbound, true).unwrap_err();
    assert!(
        format!("{error:?}").contains("belong to no asset"),
        "{error:?}"
    );
    assert!(!saving(&unbound), "refused before any edit");
    let mut no_view = window(None);
    no_view.tabs[0].panes[0].bubbles = None;
    let error = call(&mut no_view, true).unwrap_err();
    assert!(
        format!("{error:?}").contains("has no order-flow view"),
        "{error:?}"
    );
}
