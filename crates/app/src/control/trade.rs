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

use crate::app::{PaperPort, TabsMutPort, TabsPort};
pub(crate) use quantick_control_schema::trade::*;
// The version the tests invoke the trade actions at.
#[cfg(test)]
pub(crate) use quantick_control_host::authority::CAPABILITY_VERSION;

use quantick_control::{
    error::ControlError,
    id::{EventKind, ModuleId},
    registry::RegistryError,
    wire::ActorContext,
};

use quantick_engine::Side;

use quantick_sim::{Bracket, EntryKind, OrderId, OrderIntent, VenueEvent};

use rust_decimal::Decimal;

use serde_json::{Value, json};

use crate::{metrics, paper_trading::PaperTrading};

use super::{
    actions::ActionRegistry,
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
};

pub(crate) use quantick_control_host::authority::{TRADE_MODULE_ID, TRADE_PERMISSION_ID};

/// Turn the venue's answer into the result, whatever the call was.
fn answer<P: TabsPort + ?Sized>(app: &P, events: &[VenueEvent]) -> TradeResult {
    let rejected_because = events.iter().find_map(|event| match event {
        VenueEvent::Rejected(reason) => Some(reason.to_string()),
        _ => None,
    });
    let order_id = events.iter().find_map(|event| match event {
        VenueEvent::Placed(order) | VenueEvent::Updated(order) => Some(order.id.0),
        VenueEvent::Cancelled { order, .. } => Some(order.id.0),
        _ => None,
    });
    TradeResult {
        selected_strategy: app
            .tab_reads()
            .active_paper()
            .and_then(|paper| paper.account().selected_order_strategy())
            .map(|strategy| strategy.name.clone()),
        ruler_ticks: app
            .tab_reads()
            .active_paper()
            .map_or(0, crate::paper_trading::PaperTrading::ruler_ticks),
        accepted: rejected_because.is_none(),
        rejected_because,
        order_id,
        mark_price: app
            .tab_reads()
            .active_paper()
            .and_then(PaperTrading::mark_price)
            .map(|price| price.to_string()),
        working_orders: app
            .tab_reads()
            .active_paper()
            .map(PaperTrading::working_orders)
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
fn journal(
    access: &mut ControlAccess,
    actor: &ActorContext,
    kind: &str,
    result: &TradeResult,
    asked: Value,
) {
    let event_actor = EventActor {
        kind: actor.actor_kind,
        client_name: actor.client_name.clone(),
    };
    access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(TRADE_MODULE_ID).expect("static module ID is valid"),
            kind: EventKind::new(kind).expect("static event kind is valid"),
            actor: Some(event_actor),
            payload: json!({
                "asked": asked,
                "accepted": result.accepted,
                "rejected_because": result.rejected_because,
                "order_id": result.order_id,
                "simulated": true,
            }),
        },
        metrics::wall_clock_ms(),
    );
}

fn place_order<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
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
        let paper = app.tab_reads().active_paper().ok_or_else(no_chart_open)?;
        let reference = intent
            .price
            .or_else(|| paper.account().mark_price())
            .unwrap_or_default();
        paper.armed_bracket(intent.side, reference, intent.quantity)
    } else {
        named
    };
    let intent = intent.with_bracket(bracket);
    let paper = app
        .tabs_mut()
        .active_paper_mut()
        .ok_or_else(no_chart_open)?;
    // The risk per trade is a ceiling on the account, so it holds on this
    // path too. Asked of the same function the ticket asks, so an operator
    // reads the refusal the trader would have read - and gets it as an
    // error rather than as an empty answer it has to interpret.
    if let Some(refusal) = paper.account().risk_refusal_for(&intent) {
        return Err(ControlError::invalid_request(refusal));
    }
    let events = paper.account_mut().place_intent(intent);
    let result = answer(app, &events);
    journal(access, actor, PLACE_EVENT_KIND, &result, asked);
    to_value(result)
}

fn bracket_order<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
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
        .tabs_mut()
        .active_paper_mut()
        .ok_or_else(no_chart_open)?
        .account_mut()
        .set_order_bracket(OrderId(input.order_id), bracket);
    let result = answer(app, &events);
    journal(access, actor, BRACKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn cancel_order<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: CancelInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let events = app
        .tabs_mut()
        .active_paper_mut()
        .ok_or_else(no_chart_open)?
        .account_mut()
        .cancel_order(OrderId(input.order_id));
    let result = answer(app, &events);
    journal(access, actor, CANCEL_EVENT_KIND, &result, asked);
    to_value(result)
}

fn select_strategy<P: TabsPort + TabsMutPort + PaperPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SelectStrategyInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let paper = app
        .tabs_mut()
        .active_paper_mut()
        .ok_or_else(no_chart_open)?;
    let strategies = paper.account().order_strategies().to_vec();
    if let Some(name) = input.name.as_deref()
        && !strategies.iter().any(|strategy| strategy.name == name)
    {
        return Err(ControlError::invalid_request(format!(
            "no exit strategy is named `{name}`"
        )));
    }
    paper
        .account_mut()
        .set_order_strategies(strategies, input.name.as_deref());
    app.paper_settings()
        .persist(crate::app::paper_wiring::PaperSettingsChange::OrderStrategies);
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_ruler<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SetRulerInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    app.tabs_mut()
        .active_paper_mut()
        .ok_or_else(no_chart_open)?
        .set_ruler_ticks(input.ticks);
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_risk<P: TabsPort + TabsMutPort + PaperPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SetRiskInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let basis = crate::risk_sizing::RiskBasis::from_token(&input.basis).ok_or_else(|| {
        ControlError::invalid_request("basis must be `off`, `amount` or `percent`")
    })?;
    let decimal = |field: &str, text: &Option<String>| match text {
        None => Ok(None),
        Some(text) => Decimal::from_str_exact(text.trim())
            .map(Some)
            .map_err(|_| ControlError::invalid_request(format!("{field} must be a decimal"))),
    };
    let amount = decimal("amount", &input.amount)?;
    let percent = decimal("percent", &input.percent)?;
    let capital = decimal("capital", &input.capital)?;
    // The UI and the launch hook both refuse a non-positive risk; a third
    // entry point that accepts one would persist it and fan it to every tab,
    // leaving the ticket saying "set a risk per trade above zero" about a
    // number the trader never typed - and surviving a restart.
    for (field, value) in [("amount", amount), ("percent", percent)] {
        if value.is_some_and(|value| value <= Decimal::ZERO) {
            return Err(ControlError::invalid_request(format!(
                "{field} must be above zero"
            )));
        }
    }
    // A capital with no currency has nothing to be keyed by, and a currency
    // with no capital declares nothing. Both or neither.
    let currency = match (&capital, &input.currency) {
        (Some(_), Some(code)) => Some(
            quantick_sim::Currency::new(code)
                .ok_or_else(|| ControlError::invalid_request("currency must not be blank"))?,
        ),
        (None, None) => None,
        _ => {
            return Err(ControlError::invalid_request(
                "capital and currency go together - nothing here converts between currencies",
            ));
        }
    };
    let paper = app
        .tabs_mut()
        .active_paper_mut()
        .ok_or_else(no_chart_open)?;
    // The currency an amount set through this call is denominated in: the one
    // the call named, or the chart's own instrument. Read before the mutation
    // so it describes the instrument the caller was looking at.
    let instrument_currency = paper
        .account()
        .instrument_money()
        .get(paper.account().symbol())
        .map(|money| money.currency.clone());
    let mut risk = paper.account().risk_settings().clone();
    risk.basis = basis;
    if let Some(amount) = amount {
        risk.amount = amount;
        // Stamped with the currency it was entered in, never left to adopt
        // whichever instrument a tab happens to be on later.
        risk.amount_currency = currency.clone().or(instrument_currency);
    }
    if let Some(percent) = percent {
        risk.percent = percent;
    }
    if let Some(lock) = input.lock {
        risk.lock = lock;
    }
    paper.account_mut().set_risk_settings(risk);
    if let (Some(amount), Some(currency)) = (capital, currency) {
        let mut declared = paper.account().capital().clone();
        if amount > Decimal::ZERO {
            declared.insert(currency.code().to_owned(), amount);
        } else {
            declared.remove(currency.code());
        }
        paper.account_mut().set_capital(declared);
    }
    app.paper_settings()
        .persist(crate::app::paper_wiring::PaperSettingsChange::RiskSettings);
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn set_instrument_money<P: TabsPort + TabsMutPort + PaperPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: SetInstrumentMoneyInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let paper = app
        .tabs_mut()
        .active_paper_mut()
        .ok_or_else(no_chart_open)?;
    let symbol = match input.symbol.as_deref().map(str::trim) {
        Some(symbol) if !symbol.is_empty() => symbol.to_owned(),
        _ => paper.account().symbol().to_owned(),
    };
    if symbol.is_empty() {
        return Err(ControlError::invalid_request(
            "no chart symbol to declare money for - name one",
        ));
    }
    let mut book = paper.account().instrument_money().clone();
    let declared = match (
        input.point_value.as_deref(),
        input.size_step.as_deref(),
        input.currency.as_deref(),
    ) {
        (Some(point_value), Some(size_step), Some(currency)) => {
            let parse = |field: &str, text: &str| {
                Decimal::from_str_exact(text.trim()).map_err(|_| {
                    ControlError::invalid_request(format!("{field} must be a decimal"))
                })
            };
            let point_value = parse("point_value", point_value)?;
            let size_step = parse("size_step", size_step)?;
            if point_value <= Decimal::ZERO || size_step <= Decimal::ZERO {
                return Err(ControlError::invalid_request(
                    "point_value and size_step must both be above zero",
                ));
            }
            let currency = quantick_sim::Currency::new(currency)
                .ok_or_else(|| ControlError::invalid_request("currency must not be blank"))?;
            let existing = book.get(&symbol);
            Some(quantick_sim::InstrumentMoney {
                point_value,
                size_step,
                // Whatever minimum is already declared is the trader's, the
                // same as the maximum beside it.
                min_size: existing.map_or(size_step, |money| money.min_size),
                max_size: existing.and_then(|money| money.max_size),
                currency,
                source: quantick_sim::MoneySource::Declared,
            })
        }
        // Half a declaration is no declaration: clearing is the honest
        // answer, and it is what returns the ticket to saying so.
        _ => None,
    };
    match declared {
        Some(money) => {
            book.insert(symbol, money);
        }
        None => {
            book.remove(&symbol);
        }
    }
    paper.account_mut().set_instrument_money(book);
    app.paper_settings()
        .persist(crate::app::paper_wiring::PaperSettingsChange::RiskSettings);
    let result = answer(app, &[]);
    journal(access, actor, TICKET_EVENT_KIND, &result, asked);
    to_value(result)
}

/// Register the family. One call from `standard_actions`, nothing else opens.
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(select_strategy_descriptor(), select_strategy)?;
    registry.register(set_ruler_descriptor(), set_ruler)?;
    registry.register(set_risk_descriptor(), set_risk)?;
    registry.register(set_instrument_money_descriptor(), set_instrument_money)?;
    registry.register(place_descriptor(), place_order)?;
    registry.register(bracket_descriptor(), bracket_order)?;
    registry.register(cancel_descriptor(), cancel_order)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_control_host::authority::TRADE_EFFECT_ID;
    use serde_json::json;

    #[test]
    fn the_trade_actions_register_and_validate_their_schemas() {
        let registry = super::super::actions::standard_actions().unwrap();
        for id in [
            PLACE_CAPABILITY_ID,
            BRACKET_CAPABILITY_ID,
            CANCEL_CAPABILITY_ID,
        ] {
            let action = registry
                .lookup(id, CAPABILITY_VERSION)
                .unwrap_or_else(|| panic!("{id} is registered"));
            assert_eq!(action.descriptor.effect.as_str(), TRADE_EFFECT_ID);
            assert!(!action.descriptor.read_only, "{id} changes the account");
            assert!(
                action
                    .descriptor
                    .required_permissions
                    .iter()
                    .any(|permission| permission.as_str() == TRADE_PERMISSION_ID),
                "{id} sits behind the trade permission and nothing softer"
            );
        }

        let place = registry.lookup(PLACE_CAPABILITY_ID, 1).unwrap();
        place
            .input
            .validate(&json!({
                "side": "buy",
                "kind": "limit",
                "quantity": "2",
                "price": "95.5",
                "stop_loss": "90"
            }))
            .unwrap();
        assert!(
            place
                .input
                .validate(&json!({ "side": "buy", "kind": "limit" }))
                .is_err(),
            "quantity is not optional"
        );
        assert!(
            place
                .input
                .validate(&json!({
                    "side": "buy",
                    "kind": "limit",
                    "quantity": "1",
                    "price": 95.5
                }))
                .is_err(),
            "a price is a decimal string, never a JSON double"
        );
        assert!(
            place
                .input
                .validate(&json!({ "side": "sideways", "kind": "limit", "quantity": "1" }))
                .is_err()
        );
    }

    /// The one thing that must stay true while no profile may trade: the
    /// permission is real, and it is not the annotate tier's.
    #[test]
    fn trading_never_borrows_the_annotate_permission() {
        let registry = super::super::actions::standard_actions().unwrap();
        let cancel = registry.lookup(CANCEL_CAPABILITY_ID, 1).unwrap();
        assert!(
            !cancel.descriptor.destructive,
            "a working order is an instruction, not the trader's work"
        );
        assert!(
            cancel.descriptor.risk_reducing,
            "and taking one off the book can only ever reduce exposure"
        );
        assert!(
            !cancel
                .descriptor
                .required_permissions
                .iter()
                .any(|permission| permission.as_str().starts_with("annotate")),
            "annotate promises it never affects a position; a trade cannot borrow it"
        );
    }
}
