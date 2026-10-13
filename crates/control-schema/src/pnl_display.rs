//! Which figure the paper P&L readouts lead with: a display preference over
//! the trader's own journal, beside `session.journal.set`'s ledger view.
//!
//! It changes no order, position or stored trade, so it sits in the cockpit
//! tier a trader can grant, not in the `trade` tier no grant reaches, and it
//! stays out of a replay's control trace like the journal acts beside it.

use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
use quantick_control::schema::generated_schema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};

pub const PNL_DISPLAY_CAPABILITY_ID: &str = "session.pnl_display.set";

/// What a set that moved the choice journals.
pub const PNL_DISPLAY_EVENT_KIND: &str = "session.pnl_display.changed";

/// Which figure a paper P&L readout leads with; the other always follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PnlDisplayMode {
    /// Money in the instrument's own currency.
    Money,
    /// The move one contract made, with the quantity beside it.
    PointsPerContract,
    /// The move times the quantity — the stored figure.
    PointsTimesQty,
}

/// Read the P&L display, or set which figure leads it.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPnlDisplayInput {
    /// The figure to lead with. Left out, the call only reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<PnlDisplayMode>,
}

/// What the paper P&L readouts now mean: both figures and their units.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct PnlDisplayResult {
    /// The figure readouts of the chart's symbol lead with.
    pub primary: PnlDisplayMode,
    /// The trader's choice; null defers to the catalog's default (money
    /// where a point value is known, else points per contract).
    pub chosen: Option<PnlDisplayMode>,
    /// The chart symbol the units below were resolved for.
    pub symbol: String,
    /// The primary figure's unit, e.g. `BRL`, `pts per contract`.
    pub primary_unit: String,
    /// The figure that always follows it, and its unit.
    pub secondary_unit: String,
    /// The money's currency; null when no point value is known.
    pub currency: Option<String>,
    /// What one point is worth per unit held, as a decimal string.
    pub point_value: Option<String>,
    /// Where the point value came from: `declared` or `config`.
    pub money_source: Option<String>,
    /// Whether this call moved the choice; `false` for a read or a repeat.
    pub changed: bool,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::recovery::descriptor(
        PNL_DISPLAY_CAPABILITY_ID,
        "Read or set the P&L display",
        "Says which figure every paper P&L readout leads with - money in the instrument's currency, points per contract with the quantity, or points times quantity - and answers with both figures' units, the point value and where it came from. Left without `primary` it only reads. The other figure always stays beside the primary, and nothing is ever converted between currencies. A `primary` outside the three modes is refused with control.invalid_request and changes nothing. A set that moves the choice journals `session.pnl_display.changed`; repeating it changes nothing and answers `changed: false`.",
        false,
        generated_schema::<SetPnlDisplayInput>(),
    );
    descriptor.module = quantick_control::id::ModuleId::new(crate::session::MODULE_ID)
        .expect("static module ID is valid");
    descriptor.output_schema = generated_schema::<PnlDisplayResult>();
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.stale_input_safety = Some(
        "The call names the mode it wants rather than cycling it; repeating it leaves the same display and answers it. It changes no order, position or stored trade."
            .to_owned(),
    );
    descriptor
}

/// How a client reconciles an interrupted set.
pub const READBACKS: &[Readback] = &[journal(
    PNL_DISPLAY_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    PNL_DISPLAY_EVENT_KIND,
    "payload.pnl_display.primary",
    "an event after the pre-call cursor carries the requested mode as `primary`",
    &[EVERY_OPTIONAL_TEST],
)];
