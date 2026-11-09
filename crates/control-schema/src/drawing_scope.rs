//! Where one drawing shows — every chart of its market at any timeframe, or
//! only its own chart at the bars it is cut to — shared by its context bar,
//! its inspector and automation.

use std::collections::BTreeSet;

use schemars::JsonSchema;

use crate::analysis::DRAWINGS_SCOPE_ID;
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};
use quantick_control::annotation::ANNOTATE_MODULE_ID;
use quantick_control::error::ControlError;
use quantick_control::id::PermissionId;
use quantick_control::registry::IdempotencyPolicy::Optional;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence};
use quantick_control::wire::WireU64;
use quantick_control_host::authority::COCKPIT_PERMISSION_ID;
use serde::{Deserialize, Serialize};

pub const DRAWING_SCOPE_CAPABILITY_ID: &str = "annotate.drawing_scope.set";

pub const DRAWING_SCOPE_EVENT_KIND: &str = "annotate.drawing_scope.changed";

/// A drawing's scope as `analysis.drawings` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScopeName {
    /// Every chart of the drawing's market, at any timeframe.
    AllCharts,
    /// Only the chart it is on, at the bars that chart is cut to.
    ThisChart,
}

impl ScopeName {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AllCharts => "all_charts",
            Self::ThisChart => "this_chart",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DrawingScopeInput {
    /// The tab holding the drawing, as `analysis.drawings` lists it.
    pub tab_id: WireU64,
    /// The pane holding the drawing on that tab, as `analysis.drawings`
    /// lists it. A drawing id is unique only within its pane.
    pub pane_id: WireU64,
    /// The drawing, by the `drawing_id` `analysis.drawings` lists.
    pub drawing_id: WireU64,
    /// Where it should show.
    pub scope: ScopeName,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct DrawingScopeResult {
    pub drawing_id: WireU64,
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// The drawing's scope as it stands after the call.
    pub scope: ScopeName,
    /// The bar rule (`time:15m`) a drawing kept to its chart shows at; absent
    /// for one shown on all timeframes, and for one saved before drawings
    /// recorded their bars, which shows at every bar rule of its chart.
    pub pinned_bars: Option<String>,
    /// Whether the call moved it; `false` when it already stood there.
    pub changed: bool,
}

impl DrawingScopeInput {
    /// What the call answers once the drawing stands as asked.
    #[must_use]
    pub fn result(&self, pinned_bars: Option<String>, changed: bool) -> DrawingScopeResult {
        DrawingScopeResult {
            drawing_id: self.drawing_id,
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            scope: self.scope,
            pinned_bars,
            changed,
        }
    }

    /// The refusal for an id the named pane does not hold.
    #[must_use]
    pub fn unknown_drawing(&self) -> ControlError {
        ControlError::invalid_request(format!(
            "pane {} on tab {} holds no drawing with id {}",
            self.pane_id.get(),
            self.tab_id.get(),
            self.drawing_id.get()
        ))
    }
}

/// The journal payload of a change the call made.
#[must_use]
pub fn event_payload(result: &DrawingScopeResult) -> serde_json::Value {
    serde_json::json!({ "drawing_scope": result })
}

/// The refusal for `all_charts` on a drawing with an anchor that names no
/// market instant: the context bar offers it no such switch, and the call
/// refuses what the bar would not offer, with a reason a client can branch on.
#[must_use]
pub fn no_market_time(tool_id: &str) -> ControlError {
    let mut error = ControlError::invalid_request(
        "this drawing has an anchor with no market time, so no other chart can place it",
    );
    error.context.details = Some(serde_json::json!({
        "reason": "anchor_without_market_time",
        "tool_id": tool_id,
    }));
    error
}

/// `annotate.drawing_scope.set`: where one drawing shows, the switch its
/// context bar and inspector flip.
#[must_use]
pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<DrawingScopeInput, DrawingScopeResult>(
        DRAWING_SCOPE_CAPABILITY_ID,
        ANNOTATE_MODULE_ID,
        "Set where one drawing shows",
        "Sets one drawing's scope, named by the `tab_id`, `pane_id` and `drawing_id` analysis.drawings lists (a drawing id is unique only within its pane): `all_charts` shows it on every chart of its market (feed and asset) at any timeframe, placed by market time and price; `this_chart` keeps it to the chart it is on, pinned to the bars that chart is cut to now, so switching that chart to other bars hides it and switching back restores it. The same switch as \"Show on all timeframes\" on the drawing's context bar and inspector; it is saved with the market's drawings, and analysis.drawings reads it back as the drawing's `scope`. `all_charts` on a drawing with an anchor past the newest bar (no market time) is refused with control.invalid_request and `reason` `anchor_without_market_time`; an id no pane on that tab holds is refused too.",
        "The call names the scope it wants rather than flipping it; repeating it is harmless and the result reads the scope back.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.required_permissions =
        BTreeSet::from([PermissionId::new(COCKPIT_PERMISSION_ID).expect("static permission ID")]);
    descriptor
}

/// How a client reconciles an interrupted call that sets a drawing's scope.
pub const READBACKS: &[Readback] = &[snapshot(
    DRAWING_SCOPE_CAPABILITY_ID,
    Optional,
    DRAWINGS_SCOPE_ID,
    "tabs[].panes[].drawings[].scope",
    "the row under the named `tab_id` and `pane_id` whose `drawing_id` was named carries the requested scope",
    &[EVERY_OPTIONAL_TEST],
)];
