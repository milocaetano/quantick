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
//! Every action here sits behind its own effect and its own permission, and
//! that permission is granted by **no shipped profile**. Not an oversight:
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

use std::collections::BTreeSet;

use quantick_control::{
    error::ControlError,
    id::{CapabilityId, CostClassId, EventKind, ModuleId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::ActorContext,
};
use quantick_engine::Side;
use quantick_sim::{Bracket, EntryKind, OrderId, OrderIntent, VenueEvent};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{app::QuantickApp, metrics};

use super::{
    actions::{ActionRegistry, NO_CONFIRMATION_ID, UI_BOUNDED_COST_ID},
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
};

pub(crate) const TRADE_MODULE_ID: &str = "trade";
pub(crate) const TRADE_EFFECT_ID: &str = "trade";
pub(crate) const TRADE_PERMISSION_ID: &str = "trade";

/// The journal kinds each action appends. An order that something other
/// than the trader's own hand asked for has to be distinguishable from one
/// they placed themselves — the same data-honesty rule that labels an
/// inferred aggressor side, applied to authorship.
pub(crate) const PLACE_EVENT_KIND: &str = "trade.order.placed";
pub(crate) const BRACKET_EVENT_KIND: &str = "trade.order.bracketed";
pub(crate) const CANCEL_EVENT_KIND: &str = "trade.order.cancelled";

pub(crate) const PLACE_CAPABILITY_ID: &str = "trade.order.place";
pub(crate) const BRACKET_CAPABILITY_ID: &str = "trade.order.bracket";
pub(crate) const CANCEL_CAPABILITY_ID: &str = "trade.order.cancel";
pub(crate) const CAPABILITY_VERSION: u32 = 1;

/// How an entry meets the market, stated rather than inferred.
///
/// The chart's aim infers the kind from where the pointer sits relative to
/// the last price. An action has no pointer, and an action whose meaning
/// depends on the market at the instant it lands is one nobody can replay —
/// so here the caller says which order they mean, and a kind that cannot
/// rest at the given price is refused with the venue's own words.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionEntryKind {
    /// Fill at the next print, whatever it is. Takes no price.
    Market,
    /// Rest at `price` until the market trades at or through it.
    Limit,
    /// Arm at `price` until the market trades at or through it.
    Stop,
}

impl ActionEntryKind {
    fn into_engine(self) -> EntryKind {
        match self {
            Self::Market => EntryKind::Market,
            Self::Limit => EntryKind::Limit,
            Self::Stop => EntryKind::Stop,
        }
    }
}

/// Which way the order goes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ActionSide {
    Buy,
    Sell,
}

impl ActionSide {
    fn into_engine(self) -> Side {
        match self {
            Self::Buy => Side::Buy,
            Self::Sell => Side::Sell,
        }
    }
}

/// Prices arrive as strings, not as JSON numbers.
///
/// A price is a decimal, and a decimal that has been through an IEEE double
/// is not the decimal that was sent — 0.1 + 0.2 is the classic, but on a
/// tick grid it shows up as an order resting one tick from where it was
/// asked for. The whole engine is `Decimal` for this reason; the wire
/// keeps it.
fn parse_price(field: &str, text: &str) -> Result<Decimal, ControlError> {
    text.trim()
        .parse::<Decimal>()
        .map_err(|error| ControlError::invalid_request(format!("{field}: {error}")))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PlaceInput {
    pub side: ActionSide,
    pub kind: ActionEntryKind,
    /// Size, as a decimal string.
    pub quantity: String,
    /// Limit price or stop trigger, as a decimal string. Required for
    /// `limit` and `stop`; refused for `market`, which has no price of its
    /// own by definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Protective stop to attach to the fill, as a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    /// Protective target to attach to the fill, as a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BracketInput {
    /// The working order to amend.
    pub order_id: u64,
    /// New protective stop, as a decimal string. Absent clears that leg —
    /// the amendment replaces both wholesale, exactly as the chart's drag
    /// does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    /// New protective target; see `stop_loss`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CancelInput {
    pub order_id: u64,
}

/// What every `trade.*` action answers with.
///
/// `accepted` is the one field a caller must read. A refusal is not an
/// error return here for the same reason it is not one in the venue port:
/// the venue's messages are written to teach, and a result that carried
/// only a status code would throw away the sentence that says what to do
/// instead.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct TradeResult {
    /// Whether the venue took the request.
    pub accepted: bool,
    /// The venue's own words when it did not. Absent when it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_because: Option<String>,
    /// The order this call is about, once the venue has named it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<u64>,
    /// Every order working after the call, so a caller never has to guess
    /// what the account looks like now.
    pub working_orders: Vec<WorkingOrderView>,
    /// The last price the venue had been shown when this call landed.
    ///
    /// Every refusal in this family is a statement about a price relative to
    /// the market, so a caller that got one needs the market to understand
    /// it — and a caller that got an acceptance needs it to know what its
    /// order is resting away from. Absent before the first print, which is
    /// also when nothing can be placed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark_price: Option<String>,
    /// A reminder in every result: these fills are simulated.
    pub simulated: bool,
}

/// One working order, as a caller reads it back.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct WorkingOrderView {
    pub order_id: u64,
    pub side: String,
    pub kind: String,
    /// Resting price, absent only for an order that has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    pub quantity: String,
    /// The protective stop riding this order, armed on its fill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_loss: Option<String>,
    /// The protective target riding this order; see `stop_loss`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub take_profit: Option<String>,
}

/// Turn the venue's answer into the result, whatever the call was.
fn answer(app: &QuantickApp, events: &[VenueEvent]) -> TradeResult {
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
        accepted: rejected_because.is_none(),
        rejected_because,
        order_id,
        mark_price: app
            .control_tab_at(app.control_active_tab_index())
            .and_then(|tab| tab.paper.mark_price())
            .map(|price| price.to_string()),
        working_orders: app
            .control_tab_at(app.control_active_tab_index())
            .map(|tab| tab.paper.working_orders())
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
                stop_loss: order.bracket.stop_loss.map(|level| level.to_string()),
                take_profit: order.bracket.take_profit.map(|level| level.to_string()),
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

fn to_value(result: TradeResult) -> Result<Value, ControlError> {
    serde_json::to_value(result)
        .map_err(|error| ControlError::invalid_request(format!("trade result: {error}")))
}

fn place_order(
    app: &mut QuantickApp,
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
    let bracket = Bracket {
        stop_loss: input
            .stop_loss
            .as_deref()
            .map(|text| parse_price("stop_loss", text))
            .transpose()?,
        take_profit: input
            .take_profit
            .as_deref()
            .map(|text| parse_price("take_profit", text))
            .transpose()?,
    };
    let events = app
        .control_active_paper_mut()
        .place_intent(intent.with_bracket(bracket));
    let result = answer(app, &events);
    journal(access, actor, PLACE_EVENT_KIND, &result, asked);
    to_value(result)
}

fn bracket_order(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: BracketInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let bracket = Bracket {
        stop_loss: input
            .stop_loss
            .as_deref()
            .map(|text| parse_price("stop_loss", text))
            .transpose()?,
        take_profit: input
            .take_profit
            .as_deref()
            .map(|text| parse_price("take_profit", text))
            .transpose()?,
    };
    let events = app
        .control_active_paper_mut()
        .set_order_bracket(OrderId(input.order_id), bracket);
    let result = answer(app, &events);
    journal(access, actor, BRACKET_EVENT_KIND, &result, asked);
    to_value(result)
}

fn cancel_order(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked = input.clone();
    let input: CancelInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let events = app
        .control_active_paper_mut()
        .cancel_order(OrderId(input.order_id));
    let result = answer(app, &events);
    journal(access, actor, CANCEL_EVENT_KIND, &result, asked);
    to_value(result)
}

/// The shared shape of every descriptor here — one place for the risk
/// posture, so a fourth action cannot quietly ship a gentler one.
fn descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    destructive: bool,
    stale_input_safety: &str,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(TRADE_MODULE_ID).expect("static module ID is valid"),
        input_schema,
        output_schema: generated_schema::<TradeResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(TRADE_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        // Placing the same order twice places two orders. There is no key
        // that could make that safe to retry, so the policy says so rather
        // than offering one that does not hold.
        idempotency: IdempotencyPolicy::Forbidden,
        // A cancel removes the trader's working order, so the registry
        // asks the caller to say which state it believed it was acting on.
        // Placing and bracketing add to or amend by id and need no such
        // proof - see `stale_input_safety`.
        revision_policy: if destructive {
            RevisionPolicy::Required
        } else {
            RevisionPolicy::OptionalForAdditive
        },
        // Only an additive action may argue that stale input is safe;
        // a destructive one is asked for an expected revision instead.
        stale_input_safety: (!destructive).then(|| stale_input_safety.to_owned()),
        dry_run_supported: false,
        // An order dies with the session, like every simulated position.
        persistence: EffectPersistence::Transient,
        reversible: !destructive,
        destructive,
        risk_reducing: false,
        required_permissions: [TRADE_PERMISSION_ID]
            .into_iter()
            .map(|id| quantick_control::id::PermissionId::new(id).expect("static permission"))
            .collect(),
        preconditions: Vec::new(),
        confirmation_class: quantick_control::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}

fn place_descriptor() -> CapabilityDescriptor {
    descriptor(
        PLACE_CAPABILITY_ID,
        "Place a simulated order",
        "Places one market, limit or stop order on the charted symbol, optionally with the protective stop and target that arm when it fills. The kind is stated, never inferred from where the market happens to be - a kind that cannot rest at the given price is refused with the reason, in the venue's own words.",
        generated_schema::<PlaceInput>(),
        false,
        "Every price is judged against the market at the moment the order lands, not at the moment it was written: a limit that would fill at once, or a stop that would trigger at once, is refused with the reason. A caller working from a stale chart can therefore place an order at a level that has gone stale, but never one the venue would not have accepted from a caller reading the live tape.",
    )
}

fn bracket_descriptor() -> CapabilityDescriptor {
    descriptor(
        BRACKET_CAPABILITY_ID,
        "Set a working order's protective prices",
        "Replaces the stop and target riding a working order, which arm the moment it fills. Both legs are replaced wholesale: an absent leg is a cleared leg, exactly as dragging one off the chart clears it. The levels are judged against the order's own resting price, not the market.",
        generated_schema::<BracketInput>(),
        false,
        "The order is named by id, and ids are never reused. A stale caller amends the order it meant or nothing at all - an id that has since filled or been cancelled is reported as unknown rather than resolved to whatever is working now.",
    )
}

fn cancel_descriptor() -> CapabilityDescriptor {
    descriptor(
        CANCEL_CAPABILITY_ID,
        "Cancel a working order",
        "Removes one working order without trading. Destructive in the registry's sense: the order is gone, and re-placing it is a new order at the back of the queue.",
        generated_schema::<CancelInput>(),
        true,
        // Unused: a destructive action is asked for an expected revision
        // rather than for an argument that staleness is harmless.
        "",
    )
}

/// Register the family. One call from `standard_actions`, nothing else opens.
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(place_descriptor(), place_order)?;
    registry.register(bracket_descriptor(), bracket_order)?;
    registry.register(cancel_descriptor(), cancel_order)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
            cancel.descriptor.destructive,
            "cancelling removes working state and says so"
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
