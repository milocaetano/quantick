//! The `trade.*` calls' own words: each call's input is a [`TradeCall`] that
//! states one [`TradeCommand`], and every call answers with a
//! [`TradeResult`]. These are the wire: the control plane decodes the input,
//! runs the command through [`super::TabPaper::execute`] like any other
//! surface, and sends back the result as this module shapes it. Nothing
//! between the two restates them.

use quantick_engine::Side;
use quantick_paper::PaperAccount;
use quantick_sim::{Bracket, OrderId, OrderIntent, VenueEvent};
use quantick_trading::{ActionEntryKind, ActionSide, EntryKind};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::TradeCommand;

/// The journal kinds each call appends. An order that something other
/// than the trader's own hand asked for has to be distinguishable from one
/// they placed themselves — the same data-honesty rule that labels an
/// inferred aggressor side, applied to authorship.
pub const PLACE_EVENT_KIND: &str = "trade.order.placed";

pub const BRACKET_EVENT_KIND: &str = "trade.order.bracketed";

pub const CANCEL_EVENT_KIND: &str = "trade.order.cancelled";

/// The two one-shot position calls' journal kinds.
pub const FLATTEN_EVENT_KIND: &str = "trade.position.flattened";

pub const REVERSE_EVENT_KIND: &str = "trade.position.reversed";

/// One named call's input: what it says, as the command a click would send.
pub trait TradeCall: DeserializeOwned + JsonSchema {
    /// What the call journals under.
    const EVENT_KIND: &'static str;

    /// The command the input states.
    ///
    /// # Errors
    ///
    /// Why the input states no order, in words a caller can correct.
    fn command(self) -> Result<TradeCommand, String>;
}

/// Prices arrive as strings, not as JSON numbers.
///
/// A price is a decimal, and a decimal that has been through an IEEE double
/// is not the decimal that was sent — 0.1 + 0.2 is the classic, but on a
/// tick grid it shows up as an order resting one tick from where it was
/// asked for. The whole engine is `Decimal` for this reason; the wire
/// keeps it.
fn decimal(field: &str, text: &str) -> Result<Decimal, String> {
    text.trim()
        .parse::<Decimal>()
        .map_err(|error| format!("{field}: {error}"))
}

fn optional(field: &str, text: Option<&str>) -> Result<Option<Decimal>, String> {
    text.map(|text| decimal(field, text)).transpose()
}

/// Both legs, replaced wholesale: an absent leg is a cleared one.
fn bracket(stop_loss: Option<&str>, take_profit: Option<&str>) -> Result<Bracket, String> {
    Ok(Bracket::whole(
        optional("stop_loss", stop_loss)?,
        optional("take_profit", take_profit)?,
    ))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceInput {
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

impl TradeCall for PlaceInput {
    const EVENT_KIND: &'static str = PLACE_EVENT_KIND;

    /// The armed bracket, the risk per trade and the ruler stay the tab's
    /// decisions, made where a click makes them; this only states the order.
    fn command(self) -> Result<TradeCommand, String> {
        let quantity = decimal("quantity", &self.quantity)?;
        let side = self.side.into_engine();
        let price = optional("price", self.price.as_deref())?;
        let intent = match (self.kind.into_engine(), price) {
            (EntryKind::Market, None) => OrderIntent::market(side, quantity),
            (EntryKind::Limit, Some(price)) => OrderIntent::limit(side, quantity, price),
            (EntryKind::Stop, Some(trigger)) => OrderIntent::stop(side, quantity, trigger),
            (EntryKind::Market, Some(_)) => {
                return Err(
                    "a market order has no price of its own - drop `price`, or ask for a limit or a stop"
                        .to_owned(),
                );
            }
            (EntryKind::Limit | EntryKind::Stop, None) => {
                return Err("a limit or stop needs the price it rests at".to_owned());
            }
        };
        let named = bracket(self.stop_loss.as_deref(), self.take_profit.as_deref())?;
        Ok(TradeCommand::Place {
            intent: Box::new(intent.with_bracket(named)),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BracketInput {
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

impl TradeCall for BracketInput {
    const EVENT_KIND: &'static str = BRACKET_EVENT_KIND;

    fn command(self) -> Result<TradeCommand, String> {
        Ok(TradeCommand::Amend {
            id: OrderId(self.order_id),
            bracket: Box::new(bracket(
                self.stop_loss.as_deref(),
                self.take_profit.as_deref(),
            )?),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelInput {
    pub order_id: u64,
}

impl TradeCall for CancelInput {
    const EVENT_KIND: &'static str = CANCEL_EVENT_KIND;

    fn command(self) -> Result<TradeCommand, String> {
        Ok(TradeCommand::Cancel {
            id: OrderId(self.order_id),
        })
    }
}

/// The one-shot position calls take nothing: the position is the account's.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PositionInput")]
pub struct FlattenInput {}

impl TradeCall for FlattenInput {
    const EVENT_KIND: &'static str = FLATTEN_EVENT_KIND;

    fn command(self) -> Result<TradeCommand, String> {
        Ok(TradeCommand::Flatten)
    }
}

/// The one-shot position calls take nothing: the position is the account's.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PositionInput")]
pub struct ReverseInput {}

impl TradeCall for ReverseInput {
    const EVENT_KIND: &'static str = REVERSE_EVENT_KIND;

    /// Refused while flat, by the tab, because a reversal of nothing is a
    /// caller working from a stale read.
    fn command(self) -> Result<TradeCommand, String> {
        Ok(TradeCommand::Reverse)
    }
}

/// What every `trade.*` action answers with.
///
/// `accepted` is the one field a caller must read. A refusal is not an
/// error return here for the same reason it is not one in the venue port:
/// the venue's messages are written to teach, and a result that carried
/// only a status code would throw away the sentence that says what to do
/// instead.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TradeResult {
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
    /// The named exit ladder the ticket is set to, if any. Every call in
    /// this family reports it, because it is what the *next* order will
    /// carry and a caller that placed one needs to know which ladder it
    /// just armed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_strategy: Option<String>,
    /// How far the aim's ruler stands from an entry, in ticks; zero when it
    /// is not in use.
    pub ruler_ticks: u32,
    /// A reminder in every result: these fills are simulated.
    pub simulated: bool,
}

/// One working order, as a caller reads it back.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorkingOrderView {
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

impl TradeResult {
    /// What `account` looks like after the venue answered `events`, with the
    /// ruler at `ruler_ticks`: the call's answer, whatever the call was.
    #[must_use]
    pub fn of(account: &PaperAccount, ruler_ticks: u32, events: &[VenueEvent]) -> Self {
        let rejected_because = events.iter().find_map(|event| match event {
            VenueEvent::Rejected(reason) => Some(reason.to_string()),
            _ => None,
        });
        let order_id = events.iter().find_map(|event| match event {
            VenueEvent::Placed(order) | VenueEvent::Updated(order) => Some(order.id.0),
            VenueEvent::Cancelled { order, .. } => Some(order.id.0),
            _ => None,
        });
        Self {
            accepted: rejected_because.is_none(),
            rejected_because,
            order_id,
            working_orders: account
                .working_orders()
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
            mark_price: account.mark_price().map(|price| price.to_string()),
            selected_strategy: account
                .selected_order_strategy()
                .map(|strategy| strategy.name.clone()),
            ruler_ticks,
            simulated: true,
        }
    }
}

#[cfg(test)]
#[path = "paper_wire_tests.rs"]
mod tests;
