//! The `trade.*` action family: placing, bracketing and cancelling an
//! order as named calls rather than only as chart gestures.
//!
//! `CLAUDE.md`'s *operable without a hand* rule, applied to the one class of
//! capability that had no registry entry at all. Everything the chart's
//! order-entry gestures do now also exists as an action with an actor in its
//! signature, so a hotkey, a test, a control trace and — when the trader
//! decides so — an authorized operator all arrive at the same handler.
//!
//! # Why nothing can invoke these remotely yet
//!
//! Every action here sits behind its own effect and its own permission,
//! ceilinged at a `trader` profile that **nothing hands out**: the access
//! panel does not offer the scope and `configured_profile` never returns
//! that profile, so no connection can reach one. (The ceiling itself is not
//! the gate — a permission with no ceiling is not even representable.) Not
//! an oversight:
//! `annotate`'s own description promises it "never [affects] a position", so
//! a trade cannot borrow it, and inventing a profile that may trade is a
//! decision about a real account rather than a detail of this change. Until
//! such a profile exists the gateway refuses these before dispatch, exactly
//! as it refuses any capability outside a connection's ceiling — while the
//! in-process operator (a hotkey, the harness hooks, a deterministic test)
//! reaches them normally.
//!
//! The orders themselves are still simulated, and every surface still says
//! `SIM`. What is real here is the *shape*: when a broker implements
//! `quantick_trading::TradingVenue`, these actions reach it unchanged, and
//! the permission that guards them is already carved out.
//!
//! The family reaches the desk through [`TradePort`]: the active tab's paper
//! desk as [`TradeDesk`] — the account, the ruler and the armed bracket —
//! and the two persistence calls a ticket edit makes. What it decides from
//! those is all here.

pub use quantick_control_host::authority::CAPABILITY_VERSION;
pub use quantick_control_schema::trade::*;

use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_engine::Side;
use quantick_paper::{PaperAccount, format::PositionSummary, risk_sizing::RiskState};
use quantick_sim::{Bracket, EntryKind, Order, OrderId, OrderIntent, VenueEvent};
use rust_decimal::Decimal;
use serde_json::{Value, json};

use crate::JournalAccess;
use crate::dock::ActionDock;

pub use quantick_control_host::authority::{TRADE_MODULE_ID, TRADE_PERMISSION_ID};

/// The active tab's paper desk as a trade action reaches it: the account the
/// orders rest in, the ruler that shapes one order, and the bracket the
/// ticket has armed.
pub trait TradeDesk {
    /// The simulated account.
    fn account(&self) -> &PaperAccount;
    /// The simulated account, to place into.
    fn account_mut(&mut self) -> &mut PaperAccount;
    /// How many ticks the ruler is set to.
    fn ruler_ticks(&self) -> u32;
    /// Set the ruler to `ticks`.
    fn set_ruler_ticks(&mut self, ticks: u32);
    /// Clear the ruler: the next order starts bare.
    fn clear_ruler(&mut self);
    /// The last mark price, if the account has seen a print.
    fn mark_price(&self) -> Option<Decimal>;
    /// The resting orders.
    fn working_orders(&self) -> &[Order];
    /// The bracket the ticket would attach to an order of `quantity` on
    /// `side` around `reference`, exactly as a click attaches it.
    fn armed_bracket(&self, side: Side, reference: Decimal, quantity: Decimal) -> Bracket;
    /// The risk sizing's state and whether it blocks an entry, asked once.
    fn risk_report(&self) -> (RiskState, bool);
    /// The open position, as the desk's own surfaces summarise it.
    fn position_summary(&self) -> Option<PositionSummary>;
    /// Which closed-trade row the trader selected in the ledger.
    fn selected_trade_index(&self) -> Option<usize>;
}

/// The window as the trade family reaches it: the active tab's desk, and
/// the settings store a ticket edit persists into.
pub trait TradePort {
    /// The active tab's desk, `None` when no chart is open.
    fn active_desk(&self) -> Option<&dyn TradeDesk>;
    /// The active tab's desk, to act on.
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk>;
    /// Persist the named exit strategies and the ticket's selection.
    fn persist_order_strategies(&mut self);
    /// Persist the risk per trade, the declared capital and the instrument
    /// money.
    fn persist_risk_settings(&mut self);
}

/// Turn the venue's answer into the result, whatever the call was.
fn answer<P: TradePort + ?Sized>(app: &P, events: &[VenueEvent]) -> TradeResult {
    let rejected_because = events.iter().find_map(|event| match event {
        VenueEvent::Rejected(reason) => Some(reason.to_string()),
        _ => None,
    });
    let order_id = events.iter().find_map(|event| match event {
        VenueEvent::Placed(order) | VenueEvent::Updated(order) => Some(order.id.0),
        VenueEvent::Cancelled { order, .. } => Some(order.id.0),
        _ => None,
    });
    let desk = app.active_desk();
    TradeResult {
        selected_strategy: desk
            .and_then(|desk| desk.account().selected_order_strategy())
            .map(|strategy| strategy.name.clone()),
        ruler_ticks: desk.map_or(0, TradeDesk::ruler_ticks),
        accepted: rejected_because.is_none(),
        rejected_because,
        order_id,
        mark_price: desk
            .and_then(TradeDesk::mark_price)
            .map(|price| price.to_string()),
        working_orders: desk
            .map(TradeDesk::working_orders)
            .unwrap_or_default()
            .iter()
            .map(|order| WorkingOrderView {
                order_id: order.id.0,
                side: match order.side {
                    Side::Buy => "buy".to_owned(),
                    Side::Sell => "sell".to_owned(),
                },
                kind: order.kind.as_str().to_owned(),
                price: order.price.map(|price| price.to_string()),
                quantity: order.quantity.to_string(),
                stop_loss: order.bracket.stop_loss().map(|level| level.to_string()),
                take_profit: order.bracket.take_profit().map(|level| level.to_string()),
            })
            .collect(),
        simulated: true,
    }
}

/// Append one journal event naming who asked and what the venue answered.
///
/// Every `trade.*` action records, accepted or refused, and the actor rides
/// in the event rather than beside it. An order placed by an operator that
/// looked exactly like one the trader placed would be the authorship half of
/// the honesty contract quietly dropped — and a refusal is worth recording
/// too, since "the agent tried to buy here and was told no" is precisely the
/// line a trader reviewing a session wants to find.
fn journal<A: JournalAccess>(
    access: &mut A,
    actor: &ActorContext,
    kind: &str,
    result: &TradeResult,
    asked: Value,
) {
    let payload = json!({
        "asked": asked,
        "accepted": result.accepted,
        "rejected_because": result.rejected_because,
        "order_id": result.order_id,
        "simulated": true,
    });
    access.record_event(NewEvent::by(TRADE_MODULE_ID, kind, actor, payload));
}

fn place_order<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: PlaceInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let quantity = parse_price("quantity", &input.quantity)?;
    let side = input.side.into_engine();
    let price = input
        .price
        .as_deref()
        .map(|text| parse_price("price", text))
        .transpose()?;
    let intent = match (input.kind.into_engine(), price) {
        (EntryKind::Market, None) => OrderIntent::market(side, quantity),
        (EntryKind::Limit, Some(price)) => OrderIntent::limit(side, quantity, price),
        (EntryKind::Stop, Some(trigger)) => OrderIntent::stop(side, quantity, trigger),
        (EntryKind::Market, Some(_)) => {
            return Err(ControlError::invalid_request(
                "a market order has no price of its own - drop `price`, or ask for a limit or a stop"
                    .to_owned(),
            ));
        }
        (EntryKind::Limit | EntryKind::Stop, None) => {
            return Err(ControlError::invalid_request(
                "a limit or stop needs the price it rests at".to_owned(),
            ));
        }
    };
    let named = Bracket::whole(
        input
            .stop_loss
            .as_deref()
            .map(|text| parse_price("stop_loss", text))
            .transpose()?,
        input
            .take_profit
            .as_deref()
            .map(|text| parse_price("take_profit", text))
            .transpose()?,
    );
    // Levels the caller named win; otherwise the call takes what the
    // ticket is set to, exactly as a click does. A named call that
    // ignored the armed ladder while the result it answers with reports
    // that ladder would be the two-surfaces bug this rule exists to
    // prevent.
    let bracket = if named.is_empty() {
        let desk = app.active_desk().ok_or_else(no_chart_open)?;
        let reference = intent
            .price
            .or_else(|| desk.account().mark_price())
            .unwrap_or_default();
        desk.armed_bracket(intent.side, reference, intent.quantity)
    } else {
        named
    };
    let intent = intent.with_bracket(bracket);
    let desk = app.active_desk_mut().ok_or_else(no_chart_open)?;
    // The risk per trade is a ceiling on the account, so it holds on this
    // path too. Asked of the same function the ticket asks, so an operator
    // reads the refusal the trader would have read - and gets it as an
    // error rather than as an empty answer it has to interpret.
    if let Some(refusal) = desk.account().risk_refusal_for(&intent) {
        return Err(ControlError::invalid_request(refusal));
    }
    let events = desk.account_mut().place_intent(intent);
    // The ruler shaped this one order, as it does a click's; the next starts bare.
    desk.clear_ruler();
    let result = answer(app, &events);
    journal(access, actor, PLACE_EVENT_KIND, &result, asked);
    to_value(result)
}

fn bracket_order<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: BracketInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let bracket = Bracket::whole(
        input
            .stop_loss
            .as_deref()
            .map(|text| parse_price("stop_loss", text))
            .transpose()?,
        input
            .take_profit
            .as_deref()
            .map(|text| parse_price("take_profit", text))
            .transpose()?,
    );
    let events = app
        .active_desk_mut()
        .ok_or_else(no_chart_open)?
        .account_mut()
        .set_order_bracket(OrderId(input.order_id), bracket);
    let result = answer(app, &events);
    journal(access, actor, BRACKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn cancel_order<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: CancelInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let events = app
        .active_desk_mut()
        .ok_or_else(no_chart_open)?
        .account_mut()
        .cancel_order(OrderId(input.order_id));
    let result = answer(app, &events);
    journal(access, actor, CANCEL_EVENT_KIND, &result, asked);
    to_value(result)
}

fn select_strategy<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SelectStrategyInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let desk = app.active_desk_mut().ok_or_else(no_chart_open)?;
    let strategies = desk.account().order_strategies().to_vec();
    if let Some(name) = input.name.as_deref()
        && !strategies.iter().any(|strategy| strategy.name == name)
    {
        return Err(ControlError::invalid_request(format!(
            "no exit strategy is named `{name}`"
        )));
    }
    desk.account_mut()
        .set_order_strategies(strategies, input.name.as_deref());
    app.persist_order_strategies();
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_ruler<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SetRulerInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    app.active_desk_mut()
        .ok_or_else(no_chart_open)?
        .set_ruler_ticks(input.ticks);
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_risk<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SetRiskInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let edit = crate::ticket_money::risk_edit(&input)?;
    app.active_desk_mut()
        .ok_or_else(no_chart_open)?
        .account_mut()
        .apply_risk_edit(edit);
    app.persist_risk_settings();
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_instrument_money<P: TradePort + ?Sized, A: JournalAccess>(
    app: &mut P,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SetInstrumentMoneyInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let desk = app.active_desk_mut().ok_or_else(no_chart_open)?;
    crate::ticket_money::declare_money(desk.account_mut(), &input)?;
    app.persist_risk_settings();
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

/// Dock the family. One call from the window's action registry, nothing
/// else opens.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: TradePort + ?Sized,
    A: JournalAccess,
{
    registry.register(select_strategy_descriptor(), select_strategy::<H, A>)?;
    registry.register(set_ruler_descriptor(), set_ruler::<H, A>)?;
    registry.register(set_risk_descriptor(), set_risk::<H, A>)?;
    registry.register(
        set_instrument_money_descriptor(),
        set_instrument_money::<H, A>,
    )?;
    registry.register(place_descriptor(), place_order::<H, A>)?;
    registry.register(bracket_descriptor(), bracket_order::<H, A>)?;
    registry.register(cancel_descriptor(), cancel_order::<H, A>)?;
    Ok(())
}

#[cfg(test)]
#[path = "trade_tests.rs"]
mod trade_tests;
