//! The `trade.account.*` family: which account orders go to, as named calls.
//!
//! The Trading panel's `Account` selector, said without a hand: read the
//! route and the accounts the terminal reports, go back to the simulator,
//! or ask for a broker route. The rules are `quantick_paper::route`'s, and the
//! two that matter most are enforced there, not here: no caller but the
//! trader's own hand arms a broker account, and REAL arms only by holding its
//! row. A refusal comes back with its route code in `details.route_code`.

use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use quantick_control::schema::generated_schema;
use quantick_control_host::authority::{ReadSpec, TRADE_MODULE_ID, read_descriptor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{Readback, journal};
use crate::trade::{TRADE_UNREACHABLE_TEST, descriptor};

pub const READ_CAPABILITY_ID: &str = "trade.account.read";

pub const SELECT_CAPABILITY_ID: &str = "trade.account.select";

pub const DISARM_CAPABILITY_ID: &str = "trade.account.disarm";

/// What a route change journals under, accepted or refused.
pub const ROUTE_EVENT_KIND: &str = "trade.account.routed";

/// Reads take nothing.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountReadInput {}

/// How to leave a broker route that still holds a position or orders.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LeaveChoice {
    /// Close the position and cancel the orders at the broker first.
    FlattenFirst,
    /// Leave them at the broker; their plates stay on the chart.
    LeaveAtBroker,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountSelectInput {
    /// `sim`, `demo` or `real`.
    pub route: String,
    /// Required to leave a broker route with exposure open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave: Option<LeaveChoice>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountDisarmInput {
    /// Required to leave a broker route with exposure open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leave: Option<LeaveChoice>,
}

/// One row of the account list, as the panel shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccountRowView {
    pub title: String,
    /// `in use`, `one click`, `hold`, or empty when the row is off.
    pub hint: String,
    /// `selected`, `enabled` or `off`.
    pub state: String,
    /// `click`, `hold` or `none`: what the panel's press does.
    pub gesture: String,
    pub details: Vec<String>,
}

/// The route and everything the panel says about it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AccountRouteResult {
    /// `sim`, `demo` or `real`.
    pub route: String,
    /// Where orders go, in the toolbar notice's words.
    pub orders_to: String,
    /// The Trading panel's title.
    pub title: String,
    /// Whether broker routes are offered at all (`[broker.metatrader]`).
    pub broker_enabled: bool,
    /// The chart's source cannot route: the selector stays on the simulator.
    pub locked: bool,
    pub accounts: Vec<AccountRowView>,
    /// Why the broker row is off, when it is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<String>,
    /// A leave with exposure waits for `leave`.
    pub leave_pending: bool,
}

pub fn read_route_descriptor() -> CapabilityDescriptor {
    read_descriptor::<AccountReadInput, AccountRouteResult>(&ReadSpec {
        id: READ_CAPABILITY_ID,
        module: TRADE_MODULE_ID,
        title: "Read where orders go",
        description: "Reads the account route - the simulator, or the broker account the MetaTrader terminal is logged into, demo or real - with the account list the Trading panel shows, each row's facts and why a row is off. Changes nothing.",
        permissions: &["observe", "observe.paper"],
        pagination: None,
    })
}

pub fn select_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        idempotency: IdempotencyPolicy::Optional,
        ..descriptor(
            SELECT_CAPABILITY_ID,
            "Choose where orders go",
            "Routes orders to the simulator, or asks for the broker's DEMO account. Only the trader's own hand arms a broker account: any other caller is refused (`route.agent_cannot_arm`), and a REAL account arms only by holding its row in the Trading panel (`route.real_needs_hold`). Leaving a broker route with a position or orders open needs `leave`.",
            generated_schema::<AccountSelectInput>(),
            false,
            "The call answers with the route now in force. Asking for the route already in force changes nothing, so a retried call is harmless.",
        )
    }
}

pub fn disarm_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        idempotency: IdempotencyPolicy::Optional,
        ..descriptor(
            DISARM_CAPABILITY_ID,
            "Go back to the simulator",
            "Disarms any broker route: orders go to the simulator from now on. Risk-reducing. With a position or orders open at the broker, `leave` says whether to flatten them first or leave them there, their plates staying on the chart.",
            generated_schema::<AccountDisarmInput>(),
            true,
            "Disarming an unarmed account changes nothing, so a retried call is harmless.",
        )
    }
}

/// How a client reconciles an interrupted route change.
pub const READBACKS: &[Readback] = &[
    journal(
        DISARM_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        ROUTE_EVENT_KIND,
        "payload.route",
        "an event after the pre-call cursor names `sim` as the route",
        &[TRADE_UNREACHABLE_TEST],
    ),
    journal(
        SELECT_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        ROUTE_EVENT_KIND,
        "payload.route",
        "an event after the pre-call cursor names the route asked for",
        &[TRADE_UNREACHABLE_TEST],
    ),
];
