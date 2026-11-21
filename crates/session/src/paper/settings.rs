//! The ticket settings every tab shares, and the sidecar record of them.
//!
//! A ticket setting is app-wide: a trader who arms cmd trading, sets a risk
//! ceiling or builds a ladder in one tab means it in all of them, and the
//! choice survives a restart in `paper-state.toml`. What is shared, how one
//! tab's copy reaches every other tab and which sidecar fields record it is
//! decided here; the host reads and writes the file and walks its tabs.

use std::borrow::BorrowMut;
use std::collections::BTreeMap;

use quantick_paper::PaperAccount;
use quantick_paper::desk::CmdTradingSettings;
use quantick_paper::order_strategies::OrderStrategy;
use quantick_paper::pnl::PnlSettings;
use quantick_paper::risk_sizing::{
    Capital, InstrumentBook, RiskSettings, capital_from_records, records_from_book,
    records_from_capital, settings_from_sidecar,
};
use quantick_paper::state::PaperState;
use rust_decimal::Decimal;

use super::TabPaper;

/// One change the ticket, the dock or a capability made to a setting that
/// every tab shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedChange {
    /// The cmd-trading modifiers and entry kind.
    CmdTrading,
    /// The risk per trade, the declared capital, the instrument money and
    /// the figure P&L readouts lead with.
    RiskSettings,
    /// The named exit strategies, the ticket's selection and the wheel's
    /// per-instrument step.
    OrderStrategies,
}

/// One shared setting as one tab holds it: what every tab is told, and what
/// the sidecar records.
#[derive(Debug, Clone)]
pub enum SharedSettings {
    /// The cmd-trading settings.
    CmdTrading(CmdTradingSettings),
    /// The risk per trade and the money it is measured in.
    Risk {
        risk: RiskSettings,
        capital: Capital,
        book: InstrumentBook,
        pnl: PnlSettings,
    },
    /// The named ladders, the ticket's pick and the wheel's steps, which
    /// ride with them: both are configured once and meant everywhere.
    Strategies {
        strategies: Vec<OrderStrategy>,
        selected: Option<String>,
        steps: BTreeMap<String, Decimal>,
    },
}

impl SharedSettings {
    /// The setting `change` names, as `paper` holds it.
    #[must_use]
    pub fn of<A: BorrowMut<PaperAccount>>(paper: &TabPaper<A>, change: SharedChange) -> Self {
        let account: &PaperAccount = paper.account.borrow();
        match change {
            SharedChange::CmdTrading => Self::CmdTrading(paper.cmd_trading()),
            SharedChange::RiskSettings => Self::Risk {
                risk: account.risk_settings().clone(),
                capital: account.capital().clone(),
                book: account.instrument_money().clone(),
                pnl: account.pnl_settings().clone(),
            },
            SharedChange::OrderStrategies => Self::Strategies {
                strategies: account.order_strategies().to_vec(),
                selected: (account.selected_order_strategy()).map(|pick| pick.name.clone()),
                steps: (paper.ruler_steps().iter())
                    .map(|(symbol, step)| (symbol.clone(), step.normalize()))
                    .collect(),
            },
        }
    }

    /// Tell `paper` this setting.
    pub fn apply<A: BorrowMut<PaperAccount>>(&self, paper: &mut TabPaper<A>) {
        match self {
            Self::CmdTrading(settings) => paper.set_cmd_trading(*settings),
            Self::Risk {
                risk,
                capital,
                book,
                pnl,
            } => {
                let account: &mut PaperAccount = paper.account.borrow_mut();
                account.set_risk_settings(risk.clone());
                account.set_capital(capital.clone());
                account.set_instrument_money(book.clone());
                account.set_pnl_settings(pnl.clone());
            }
            Self::Strategies {
                strategies,
                selected,
                steps,
            } => {
                let account: &mut PaperAccount = paper.account.borrow_mut();
                account.set_order_strategies(strategies.clone(), selected.as_deref());
                paper.set_ruler_steps(steps.clone());
            }
        }
    }

    /// Write this setting into the sidecar's fields, leaving the rest alone.
    pub fn record(&self, state: &mut PaperState) {
        match self {
            Self::CmdTrading(settings) => {
                state.cmd_trading_enabled = Some(settings.enabled);
                state.cmd_buy_modifier = Some(settings.buy.as_str().to_owned());
                state.cmd_entry_kind = Some(settings.kind.as_str().to_owned());
                state.cmd_sell_modifier = Some(settings.sell.as_str().to_owned());
            }
            Self::Risk {
                risk,
                capital,
                book,
                pnl,
            } => {
                state.risk_per_trade_basis = Some(risk.basis.token().to_owned());
                state.risk_per_trade_amount = Some(risk.amount.normalize().to_string());
                state.risk_per_trade_percent = Some(risk.percent.normalize().to_string());
                state.risk_per_trade_lock = Some(risk.lock);
                state.paper_capital = records_from_capital(capital);
                state.instrument_money = records_from_book(book);
                state.pnl_primary = pnl.chosen.map(|primary| primary.token().to_owned());
            }
            Self::Strategies {
                strategies,
                selected,
                steps,
            } => {
                state.order_strategies = Some(strategies.clone());
                state.selected_order_strategy.clone_from(selected);
                state.ruler_steps = (steps.iter())
                    .map(|(symbol, step)| (symbol.clone(), step.to_string()))
                    .collect();
            }
        }
    }
}

impl<A: BorrowMut<PaperAccount>> TabPaper<A> {
    /// Open with the ticket settings the sidecar remembers: cmd trading, the
    /// named ladders and the one the ticket was set to, the wheel's steps,
    /// and the risk per trade with its capital unless a launch hook set the
    /// risk for this run (an explicit one-run request outranks the store).
    ///
    /// A selection naming a strategy the file no longer carries selects
    /// nothing; a step this build cannot parse is dropped, so the instrument
    /// follows its derived default rather than a silently broken zero.
    pub fn restore_settings(&mut self, state: &PaperState) {
        self.set_cmd_trading(CmdTradingSettings::from_state(state));
        let account: &mut PaperAccount = self.account.borrow_mut();
        account.set_order_strategies(
            state.order_strategies.clone().unwrap_or_default(),
            state.selected_order_strategy.as_deref(),
        );
        self.set_ruler_steps(
            (state.ruler_steps.iter())
                .filter_map(|(symbol, step)| Some((symbol.clone(), step.parse().ok()?)))
                .collect(),
        );
        let account: &mut PaperAccount = self.account.borrow_mut();
        if !account.risk_from_hook() {
            account.set_risk_settings(settings_from_sidecar(
                state.risk_per_trade_basis.as_deref(),
                state.risk_per_trade_amount.as_deref(),
                state.risk_per_trade_currency.as_deref(),
                state.risk_per_trade_percent.as_deref(),
                state.risk_per_trade_lock,
            ));
            account.set_capital(capital_from_records(&state.paper_capital));
        }
    }
}
