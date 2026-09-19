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

use quantick_control_host::authority::{
    CAPABILITY_VERSION, NO_CONFIRMATION_ID, TRADE_EFFECT_ID, TRADE_MODULE_ID, TRADE_PERMISSION_ID,
    UI_BOUNDED_COST_ID,
};

use std::collections::BTreeSet;

use quantick_control_host::admission::known_error;

use quantick_control::{
    error::{ControlError, codes},
    id::{CapabilityId, CostClassId, ModuleId, RiskFlagId},
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RevisionPolicy,
    },
    schema::generated_schema,
};

use quantick_engine::Side;

use quantick_sim::EntryKind;

use rust_decimal::Decimal;

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use serde_json::Value;

/// The journal kinds each action appends. An order that something other
/// than the trader's own hand asked for has to be distinguishable from one
/// they placed themselves — the same data-honesty rule that labels an
/// inferred aggressor side, applied to authorship.
pub const PLACE_EVENT_KIND: &str = "trade.order.placed";

pub const BRACKET_EVENT_KIND: &str = "trade.order.bracketed";

pub const CANCEL_EVENT_KIND: &str = "trade.order.cancelled";

pub const PLACE_CAPABILITY_ID: &str = "trade.order.place";

pub const BRACKET_CAPABILITY_ID: &str = "trade.order.bracket";

/// What the two shaping calls journal under. They place nothing, so an
/// order-placed event would put a phantom order in the trail the constants
/// above exist to keep honest.
pub const TICKET_EVENT_KIND: &str = "trade.ticket.changed";

pub const SELECT_STRATEGY_CAPABILITY_ID: &str = "trade.strategy.select";

pub const SET_RULER_CAPABILITY_ID: &str = "trade.ruler.set";

pub const SET_RISK_CAPABILITY_ID: &str = "trade.risk.set";

pub const SET_INSTRUMENT_MONEY_CAPABILITY_ID: &str = "trade.instrument.set_money";

pub const CANCEL_CAPABILITY_ID: &str = "trade.order.cancel";

/// How an entry meets the market, stated rather than inferred.
///
/// The chart's aim infers the kind from where the pointer sits relative to
/// the last price. An action has no pointer, and an action whose meaning
/// depends on the market at the instant it lands is one nobody can replay —
/// so here the caller says which order they mean, and a kind that cannot
/// rest at the given price is refused with the venue's own words.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ActionEntryKind {
    /// Fill at the next print, whatever it is. Takes no price.
    Market,
    /// Rest at `price` until the market trades at or through it.
    Limit,
    /// Arm at `price` until the market trades at or through it.
    Stop,
}

impl ActionEntryKind {
    /// The simulator's entry kind.
    #[must_use]
    pub fn into_engine(self) -> EntryKind {
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
pub enum ActionSide {
    Buy,
    Sell,
}

impl ActionSide {
    /// The engine's side.
    #[must_use]
    pub fn into_engine(self) -> Side {
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
pub fn parse_price(field: &str, text: &str) -> Result<Decimal, ControlError> {
    text.trim()
        .parse::<Decimal>()
        .map_err(|error| ControlError::invalid_request(format!("{field}: {error}")))
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelInput {
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

/// The refusal every action gives when there is no chart to trade on.
///
/// A window with no tab is not a state this application reaches, and the
/// point of answering rather than indexing is that the control plane must
/// not be the thing that discovers otherwise by taking a live session down.
pub fn no_chart_open() -> ControlError {
    known_error(
        codes::CAPABILITY_UNAVAILABLE,
        "this window has no chart open",
        true,
    )
}

pub fn to_value(result: TradeResult) -> Result<Value, ControlError> {
    serde_json::to_value(result)
        .map_err(|error| ControlError::invalid_request(format!("trade result: {error}")))
}

/// The shared shape of every descriptor here — one place for the risk
/// posture, so a fourth action cannot quietly ship a gentler one.
pub fn descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    risk_reducing: bool,
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
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(stale_input_safety.to_owned()),
        dry_run_supported: false,
        // An order dies with the session, like every simulated position.
        persistence: EffectPersistence::Transient,
        reversible: true,
        // Cancelling is not `destructive` in this registry's sense, and the
        // first attempt at it here said so wrongly. Destructive means the
        // trader's *work* is gone — a drawing, a layout, an annotation —
        // and the registry answers that by demanding an expected revision,
        // which this tier's envelopes forbid outright
        // (`ObserverContract::prepare`), so the guard could never have run.
        // A working order is an instruction, re-issuable in one call, and
        // removing one only ever reduces exposure: that is exactly what
        // `risk_reducing` is for, and it is the honest flag.
        destructive: false,
        risk_reducing,
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

pub fn place_descriptor() -> CapabilityDescriptor {
    descriptor(
        PLACE_CAPABILITY_ID,
        "Place a simulated order",
        "Places one market, limit or stop order on the charted symbol, optionally with the protective stop and target that arm when it fills. The kind is stated, never inferred from where the market happens to be - a kind that cannot rest at the given price is refused with the reason, in the venue's own words.",
        generated_schema::<PlaceInput>(),
        false,
        "Every price is judged against the market at the moment the order lands, not at the moment it was written: a limit that would fill at once, or a stop that would trigger at once, is refused with the reason. A caller working from a stale chart can therefore place an order at a level that has gone stale, but never one the venue would not have accepted from a caller reading the live tape.",
    )
}

pub fn bracket_descriptor() -> CapabilityDescriptor {
    descriptor(
        BRACKET_CAPABILITY_ID,
        "Set a working order's protective prices",
        "Replaces the stop and target riding a working order, which arm the moment it fills. Both legs are replaced wholesale: an absent leg is a cleared leg, exactly as dragging one off the chart clears it. The levels are judged against the order's own resting price, not the market.",
        generated_schema::<BracketInput>(),
        false,
        "The order is named by id, and ids are never reused. A stale caller amends the order it meant or nothing at all - an id that has since filled or been cancelled is reported as unknown rather than resolved to whatever is working now.",
    )
}

pub fn cancel_descriptor() -> CapabilityDescriptor {
    descriptor(
        CANCEL_CAPABILITY_ID,
        "Cancel a working order",
        "Removes one working order without trading. Risk-reducing: it can only ever take exposure off, never add any. Re-placing the same order afterwards is a new order at the back of the queue.",
        generated_schema::<CancelInput>(),
        true,
        "The order is named by id, and ids are never reused, so a stale cancel removes the order it meant or reports that there is no such order. It can never cancel a different one that happens to be working now.",
    )
}

/// Which named exit ladder the ticket arms next.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectStrategyInput {
    /// The strategy's name, exactly as `observe.session.paper` reports it.
    /// Omitted or null selects none, which is the bare order the trader
    /// brackets by hand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// How far the ruler walks the projected bracket from an aimed entry.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRulerInput {
    /// Distance in ticks, the same on both sides. Zero puts the ruler away
    /// and leaves the next order bare.
    pub ticks: u32,
}

/// What one trade may lose, and whether an entry over it is refused.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRiskInput {
    /// `off` leaves the size to whoever types it; `amount` reads `amount`;
    /// `percent` reads `percent` against the capital declared for the
    /// instrument's own currency.
    pub basis: String,
    /// The fixed amount one trade may lose, as a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    /// The share of declared capital one trade may lose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<String>,
    /// Declare the capital for `currency`. Both or neither: a capital with
    /// no currency has nothing to be keyed by, and nothing here converts
    /// between currencies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capital: Option<String>,
    /// The currency `capital` is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Whether an entry over the risk per trade is refused. Left out, the
    /// lock stays as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock: Option<bool>,
}

/// What one point of an instrument is worth, and its smallest tradable size.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetInstrumentMoneyInput {
    /// The symbol to declare for. Left out, the chart's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// What one point of price is worth per unit held, as a decimal string.
    /// Given with either of the other two missing, the declaration is
    /// cleared instead — which returns the instrument to "nothing here knows
    /// what a point is worth", the same contract `set_ruler_step` uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_value: Option<String>,
    /// The smallest tradable increment of quantity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_step: Option<String>,
    /// The currency the point value is in. Never converted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

/// A shaping call's descriptor: `descriptor`'s shape with the three fields
/// that are simply not true of it corrected.
///
/// Placing an order is transient, forbidden to retry and answers with the
/// order it made. Choosing a ladder or moving the ruler is none of those: it
/// rewrites the paper-state sidecar and fans out to every tab, so it is
/// durable; setting the same value twice leaves the same value, so it is
/// idempotent; and it places nothing.
pub fn shaping_descriptor(inner: CapabilityDescriptor) -> CapabilityDescriptor {
    CapabilityDescriptor {
        persistence: EffectPersistence::Durable,
        idempotency: IdempotencyPolicy::Optional,
        ..inner
    }
}

pub fn select_strategy_descriptor() -> CapabilityDescriptor {
    shaping_descriptor(descriptor(
        SELECT_STRATEGY_CAPABILITY_ID,
        "Choose the ticket's exit ladder",
        "Sets which named exit strategy the next order rests with, or none for a bare order. Changes no order that already exists and never touches an open position; the same call the ticket's own selector makes.",
        generated_schema::<SelectStrategyInput>(),
        false,
        "The call names the strategy it selected and answers with the one that is now set, so a caller working from a stale read can see that it chose something else. It changes nothing that is already working.",
    ))
}

pub fn set_ruler_descriptor() -> CapabilityDescriptor {
    shaping_descriptor(descriptor(
        SET_RULER_CAPABILITY_ID,
        "Set the aim's ruler distance",
        "Walks the projected stop and target out from an aimed entry, the same distance on both sides, in ticks of the instrument. Zero puts the ruler away. Shapes what the next order would carry rather than changing one that exists - the read a trader takes before committing.",
        generated_schema::<SetRulerInput>(),
        false,
        "The call answers with where the ruler actually landed, clamped to what the wheel itself can reach, so a caller that asked for more can see what it got. It changes nothing that is already working.",
    ))
}

pub fn set_risk_descriptor() -> CapabilityDescriptor {
    shaping_descriptor(descriptor(
        SET_RISK_CAPABILITY_ID,
        "Set the risk per trade",
        "Says what one trade may lose - a fixed amount, or a share of the capital declared for the instrument's currency - and whether an entry whose stop risks more than that is refused. Shapes the size the next order would carry; changes no order that exists and never touches an open position.",
        generated_schema::<SetRiskInput>(),
        false,
        "The call answers with the risk that is now set and with what it makes of the entry the ticket is holding, including the reason when it can name no size, so a caller sees the same sentence the trader is reading. It changes nothing that is already working.",
    ))
}

pub fn set_instrument_money_descriptor() -> CapabilityDescriptor {
    shaping_descriptor(descriptor(
        SET_INSTRUMENT_MONEY_CAPABILITY_ID,
        "Declare what an instrument's point is worth",
        "Records what one point of price is worth per unit held, the smallest tradable size, and the currency - the three facts no feed reports and nothing here derives, without which risk sizing has nothing to size against. Leaving any of them out clears the declaration rather than half-setting it.",
        generated_schema::<SetInstrumentMoneyInput>(),
        false,
        "The call answers with what the ticket now makes of the entry it is holding, so a caller can see the instrument turn from unknown into sized. It changes nothing that is already working, and it never converts between currencies.",
    ))
}
