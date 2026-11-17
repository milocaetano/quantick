//! The ticket settings every tab shares: restored from the sidecar, read off
//! one tab, told to another and recorded back.

use std::path::PathBuf;

use quantick_paper::PaperAccount;
use quantick_paper::desk::{CmdEntryKind, CmdModifier};
use quantick_paper::risk_sizing::RiskBasis;
use quantick_paper::state::PaperState;
use rust_decimal::Decimal;

use crate::paper::{SharedChange, SharedSettings, TabPaper};

fn tab() -> TabPaper {
    TabPaper::new(PaperAccount::with_trades_dir(PathBuf::new()))
}

fn stored() -> PaperState {
    PaperState {
        cmd_trading_enabled: Some(false),
        cmd_buy_modifier: Some(CmdModifier::Ctrl.as_str().to_owned()),
        cmd_entry_kind: Some(CmdEntryKind::Auto.as_str().to_owned()),
        risk_per_trade_basis: Some(RiskBasis::Amount.token().to_owned()),
        risk_per_trade_amount: Some("250".to_owned()),
        selected_order_strategy: Some("gone".to_owned()),
        ruler_steps: [("WIN", "5"), ("BAD", "five")]
            .map(|(symbol, step)| (symbol.to_owned(), step.to_owned()))
            .into(),
        ..PaperState::default()
    }
}

#[test]
fn a_restored_tab_records_what_the_sidecar_said() {
    let mut paper = tab();
    paper.restore_settings(&stored());
    assert!(!paper.cmd_trading().enabled);
    assert_eq!(paper.cmd_trading().buy, CmdModifier::Ctrl);
    assert_eq!(paper.account().risk_settings().basis, RiskBasis::Amount);
    assert_eq!(paper.account().selected_order_strategy(), None);
    let steps: Vec<_> = paper.ruler_steps().iter().collect();
    assert_eq!(steps, [(&"WIN".to_owned(), &Decimal::from(5))]);

    let mut written = PaperState::default();
    for change in [
        SharedChange::CmdTrading,
        SharedChange::RiskSettings,
        SharedChange::OrderStrategies,
    ] {
        SharedSettings::of(&paper, change).record(&mut written);
    }
    assert_eq!(written.cmd_trading_enabled, Some(false));
    assert_eq!(written.cmd_buy_modifier.as_deref(), Some("ctrl"));
    assert_eq!(written.risk_per_trade_basis, stored().risk_per_trade_basis);
    assert_eq!(written.risk_per_trade_amount.as_deref(), Some("250"));
    assert_eq!(written.selected_order_strategy, None);
    assert_eq!(
        written.ruler_steps.get("WIN").map(String::as_str),
        Some("5")
    );
    assert!(!written.ruler_steps.contains_key("BAD"));
}

#[test]
fn one_tab_s_setting_reaches_another_and_only_that_setting() {
    let mut source = tab();
    source.restore_settings(&stored());
    let mut other = tab();
    SharedSettings::of(&source, SharedChange::CmdTrading).apply(&mut other);
    assert_eq!(other.cmd_trading(), source.cmd_trading());
    assert_eq!(other.account().risk_settings().basis, RiskBasis::Off);
    assert!(other.ruler_steps().is_empty());

    SharedSettings::of(&source, SharedChange::RiskSettings).apply(&mut other);
    assert_eq!(
        other.account().risk_settings(),
        source.account().risk_settings()
    );
    SharedSettings::of(&source, SharedChange::OrderStrategies).apply(&mut other);
    assert_eq!(other.ruler_steps(), source.ruler_steps());
}
