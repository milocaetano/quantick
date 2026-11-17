//! Where one drawing shows — every chart of its market showing its layout at
//! any timeframe, or only its own chart at the bars it is cut to, and whether
//! the charts of its market showing any other layout show it too — shared by
//! its context bar, its inspector and automation.

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
    /// Every chart of the drawing's market showing its layout, at any
    /// timeframe; with `all_windows`, every chart of its market.
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
    /// Whether every chart of its market shows it too, whatever layout that
    /// chart shows; only with `all_charts`. Absent leaves it as it stands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_windows: Option<bool>,
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
    /// Whether every chart of its market shows it after the call, whatever
    /// layout: asked for on a drawing on all timeframes, or one saved before
    /// drawings recorded their layout. Optional in the schema only so v1
    /// readers stay compatible.
    #[serde(default)]
    pub all_windows: bool,
    /// The strip layout it belongs to after the call, the `layout_id` the layout-tab
    /// calls (`layout.tab.switch`) name; absent for one saved before drawings recorded their layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_id: Option<WireU64>,
    /// Whether the call moved it; `false` when it already stood there.
    pub changed: bool,
}

/// Where a drawing stands after the call, as the result reads it back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScopeReadback {
    /// See [`DrawingScopeResult::pinned_bars`].
    pub pinned_bars: Option<String>,
    /// See [`DrawingScopeResult::all_windows`].
    pub all_windows: bool,
    /// See [`DrawingScopeResult::layout_id`].
    pub layout_id: Option<u64>,
}

impl DrawingScopeInput {
    /// What the call answers once the drawing stands as asked.
    #[must_use]
    pub fn result(&self, readback: ScopeReadback, changed: bool) -> DrawingScopeResult {
        DrawingScopeResult {
            drawing_id: self.drawing_id,
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            scope: self.scope,
            pinned_bars: readback.pinned_bars,
            all_windows: readback.all_windows,
            layout_id: readback.layout_id.map(WireU64::new),
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

/// The refusal for `all_windows: true` beside `this_chart`: a drawing kept to
/// the bars of its chart has no place on another chart, so the inspector
/// greys the switch and the call refuses it, with a reason to branch on.
#[must_use]
pub fn all_windows_needs_all_charts(tool_id: &str) -> ControlError {
    let mut error = ControlError::invalid_request(
        "all_windows needs scope all_charts: a drawing kept to its chart shows on its own chart only",
    );
    error.context.details = Some(serde_json::json!({
        "reason": "all_windows_needs_all_charts",
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
        "Sets one drawing's scope, named by the `tab_id`, `pane_id` and `drawing_id` analysis.drawings lists (a drawing id is unique only within its pane, and a pane lists only what its layout shows): `all_charts` shows it at any timeframe on every chart of its market (feed and asset) showing the named pane's layout, placed by market time and price; `this_chart`, a new drawing's default, keeps it to its chart, pinned to the bars that chart is cut to now. Optional `all_windows` also shows an `all_charts` drawing on that market's charts on any other layout, never on another market; absent leaves it as it stands. The inspector's \"Show on all timeframes\" and \"All windows\"; saved with the market's drawings and read back as `scope`, `all_windows` and `layout_id`. Refused with control.invalid_request: `all_charts` on a drawing with an anchor past the newest bar (`reason` `anchor_without_market_time`), `all_windows: true` beside `this_chart` (`reason` `all_windows_needs_all_charts`), and an id the named pane does not show.",
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
