//! The P&L display call through a fake window: a read changes nothing, a
//! set reaches the account, persists once and is journaled, and the answer
//! names both figures' units and the point value's source.

use std::path::PathBuf;
use std::sync::Arc;

use quantick_paper::instruments::InstrumentCatalog;
use quantick_paper::pnl::PnlSettings;
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};

const CATALOG: &str = r#"
version = 1
[instruments."WIN*"]
currency = "BRL"
point_value = "0.20"
size_step = "1"
"#;

struct FakeDesk {
    account: Option<PaperAccount>,
    persisted: usize,
}

impl FakeDesk {
    fn on(symbol: &str) -> Self {
        let mut account = PaperAccount::with_trades_dir(PathBuf::from("no-such-trades-folder"));
        account.set_symbol(symbol);
        account.set_pnl_settings(PnlSettings {
            catalog: Arc::new(InstrumentCatalog::parse(CATALOG).expect("fixture catalog")),
            chosen: None,
        });
        Self {
            account: Some(account),
            persisted: 0,
        }
    }
}

impl PnlDisplayPort for FakeDesk {
    fn active_account(&mut self) -> Option<&mut PaperAccount> {
        self.account.as_mut()
    }
    fn persist_pnl_display(&mut self) {
        self.persisted += 1;
    }
}

fn call(desk: &mut FakeDesk, access: &mut FakeAccess, input: Value) -> Result<Value, ControlError> {
    pnl_display(desk, access, &FakeWindow::assistant(), &input)
}

#[test]
fn a_read_names_both_figures_and_changes_nothing() {
    let mut desk = FakeDesk::on("WINZ26");
    let mut access = FakeAccess::with_budget(0);
    let answer = call(&mut desk, &mut access, json!({})).expect("read");
    assert_eq!(
        answer["primary"], "money",
        "money leads where a point value is known"
    );
    assert_eq!(answer["chosen"], Value::Null);
    assert_eq!(answer["primary_unit"], "BRL");
    assert_eq!(answer["secondary_unit"], "pts per contract");
    assert_eq!(answer["point_value"], "0.2");
    assert_eq!(answer["money_source"], "config");
    assert_eq!(desk.persisted, 0, "a read persists nothing");
    assert!(access.events.is_empty(), "a read journals nothing");
}

#[test]
fn a_set_reaches_the_account_persists_once_and_is_journaled() {
    let mut desk = FakeDesk::on("WINZ26");
    let mut access = FakeAccess::with_budget(0);
    let answer = call(
        &mut desk,
        &mut access,
        json!({ "primary": "points_times_qty" }),
    )
    .expect("set");
    assert_eq!(answer["primary"], "points_times_qty");
    assert_eq!(answer["chosen"], "points_times_qty");
    assert_eq!(answer["primary_unit"], "pts times quantity");
    assert_eq!(answer["secondary_unit"], "BRL");
    assert_eq!(desk.persisted, 1);
    assert_eq!(access.events.len(), 1);
    assert_eq!(
        desk.account.as_ref().map(PaperAccount::pnl_primary),
        Some(PnlPrimary::PointsTimesQty)
    );
}

#[test]
fn an_unpriced_symbol_says_money_is_unset_rather_than_guessing() {
    let mut desk = FakeDesk::on("XYZ");
    let answer = call(&mut desk, &mut FakeAccess::with_budget(0), json!({})).expect("read");
    assert_eq!(answer["primary"], "points_per_contract");
    assert_eq!(
        answer["secondary_unit"],
        "money unset - declare a point value"
    );
    assert_eq!(answer["currency"], Value::Null);
}

#[test]
fn an_unknown_mode_and_a_missing_chart_are_refused() {
    let mut desk = FakeDesk::on("WINZ26");
    let refused = call(
        &mut desk,
        &mut FakeAccess::with_budget(0),
        json!({ "primary": "dollars" }),
    );
    assert!(refused.is_err(), "an unknown mode is invalid input");
    assert_eq!(desk.persisted, 0);
    let mut empty = FakeDesk {
        account: None,
        persisted: 0,
    };
    assert!(call(&mut empty, &mut FakeAccess::with_budget(0), json!({})).is_err());
}
