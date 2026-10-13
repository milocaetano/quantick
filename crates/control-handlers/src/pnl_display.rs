//! Which figure the paper P&L readouts lead with, as a named call.
//!
//! The ticket's mode switch and `trade.pnl_display.set` make the same
//! change through the same account method, and both persist through the
//! host's one fan-out, so a call leaves the durable trace a click does. The
//! answer names both figures' units and where the point value came from:
//! a caller reading "+80.00 BRL" must be able to tell a declared value
//! from the shipped catalog's.
//!
//! Rate class: a human or an agent changing a preference. Never per trade.

use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::{authority::TRADE_MODULE_ID, journal::NewEvent};
use quantick_control_schema::trade::{
    PnlDisplayMode, PnlDisplayResult, SetPnlDisplayInput, TICKET_EVENT_KIND, no_chart_open,
    pnl_display_descriptor,
};
use quantick_paper::account::PaperAccount;
use quantick_paper::pnl::PnlPrimary;
use serde_json::{Value, json};

use crate::dock::ActionDock;
use crate::notify::NotifyAccess;

/// The active chart's paper account, and the host's fan-out.
pub trait PnlDisplayPort {
    /// The account of the chart the trader is looking at; `None` with no
    /// chart open.
    fn active_account(&mut self) -> Option<&mut PaperAccount>;
    /// Persist the active account's settings and fan them out to every tab.
    fn persist_pnl_display(&mut self);
}

/// Dock the capability. A display preference belongs to no tape, so a
/// replay's control trace never re-runs it.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: PnlDisplayPort + ?Sized,
    A: NotifyAccess,
{
    registry.register_outside_replay(pnl_display_descriptor(), pnl_display)
}

fn pnl_display<H: PnlDisplayPort + ?Sized, A: NotifyAccess>(
    app: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: SetPnlDisplayInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let account = app.active_account().ok_or_else(no_chart_open)?;
    let Some(mode) = input.primary else {
        return to_value(&describe(account));
    };
    account.set_pnl_primary(from_mode(mode));
    app.persist_pnl_display();
    let result = describe(app.active_account().ok_or_else(no_chart_open)?);
    let payload = json!({ "asked": value, "pnl_display": to_value(&result)? });
    access.record_event(NewEvent::by(
        TRADE_MODULE_ID,
        TICKET_EVENT_KIND,
        actor,
        payload,
    ));
    to_value(&result)
}

/// The display in force on `account`, both figures named.
#[must_use]
pub fn describe(account: &PaperAccount) -> PnlDisplayResult {
    let symbol = account.symbol();
    let units = account.pnl_book().units(symbol);
    let suffix = units.unit_suffix();
    let money = units.money;
    let currency = money.map(|money| money.currency.code().to_owned());
    let money_unit = currency
        .clone()
        .unwrap_or_else(|| "money unset - declare a point value".to_owned());
    let per_contract = format!("{suffix} per contract");
    let primary = account.pnl_primary();
    let (primary_unit, secondary_unit) = match primary {
        PnlPrimary::Money => (money_unit, per_contract),
        PnlPrimary::PointsPerContract => (per_contract, money_unit),
        PnlPrimary::PointsTimesQty => (format!("{suffix} times quantity"), money_unit),
    };
    PnlDisplayResult {
        primary: to_mode(primary),
        chosen: account.pnl_settings().chosen.map(to_mode),
        symbol: symbol.to_owned(),
        primary_unit,
        secondary_unit,
        currency,
        point_value: money.map(|money| money.point_value.normalize().to_string()),
        money_source: money.map(|money| money.source.token().to_owned()),
    }
}

fn to_mode(primary: PnlPrimary) -> PnlDisplayMode {
    match primary {
        PnlPrimary::Money => PnlDisplayMode::Money,
        PnlPrimary::PointsPerContract => PnlDisplayMode::PointsPerContract,
        PnlPrimary::PointsTimesQty => PnlDisplayMode::PointsTimesQty,
    }
}

fn from_mode(mode: PnlDisplayMode) -> PnlPrimary {
    match mode {
        PnlDisplayMode::Money => PnlPrimary::Money,
        PnlDisplayMode::PointsPerContract => PnlPrimary::PointsPerContract,
        PnlDisplayMode::PointsTimesQty => PnlPrimary::PointsTimesQty,
    }
}

fn to_value(result: &PnlDisplayResult) -> Result<Value, ControlError> {
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

#[cfg(test)]
#[path = "pnl_display_tests.rs"]
mod pnl_display_tests;
