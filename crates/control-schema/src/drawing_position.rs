//! A long or short position's look and plan, shared by its floating bar
//! and automation: the zone colours, the zone opacity, the risk/reward, the
//! width, and the per-tool default its menus end with.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::annotation::ANNOTATE_MODULE_ID;
use quantick_control::id::PermissionId;
use quantick_control::registry::IdempotencyPolicy::Optional;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence};
use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_control_host::authority::COCKPIT_PERMISSION_ID;

pub const SET_CAPABILITY_ID: &str = "annotate.position.set";
pub const SET_EVENT_KIND: &str = "annotate.position.changed";
pub const DEFAULT_CAPABILITY_ID: &str = "annotate.position.default";
pub const DEFAULT_EVENT_KIND: &str = "annotate.position.default_changed";

/// Edit or inspect one position. Omit every optional field to inspect.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PositionSetInput {
    /// The tab holding the drawing, as `analysis.drawings` lists it.
    pub tab_id: WireU64,
    /// The pane holding the drawing; a drawing id is unique only within it.
    pub pane_id: WireU64,
    pub drawing_id: WireU64,
    /// Target zone colour, `#RRGGBB`.
    #[serde(default)]
    pub target_color: Option<String>,
    /// Stop zone colour, `#RRGGBB`.
    #[serde(default)]
    pub stop_color: Option<String>,
    /// Both zones' fill opacity, 0 through 100 percent.
    #[serde(default)]
    #[schemars(range(max = 100))]
    pub zone_opacity_pct: Option<u8>,
    /// Reward per unit of risk, 0.1 through 20, as a canonical decimal:
    /// `"2"` is risk 1 : reward 2. Only the target moves.
    #[serde(default)]
    pub ratio: Option<CanonicalDecimal>,
    /// Drawn width in bars, at least 14; the left edge holds. No price moves.
    #[serde(default)]
    pub width_bars: Option<CanonicalDecimal>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PositionSideDto {
    Long,
    Short,
}

/// A position's look: what its bar shows and a saved default carries.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct PositionLook {
    pub target_color: String,
    pub stop_color: String,
    pub zone_opacity_pct: u8,
    /// Reward per unit of risk.
    pub ratio: CanonicalDecimal,
    /// Whether the ratio is none of the menu's presets (1, 2, 3).
    pub custom: bool,
    /// The bar button's words, risk first: `R/R 1:2`, `Custom · R/R 1:1.3`.
    pub ratio_text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct PositionSetResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub drawing_id: WireU64,
    pub side: PositionSideDto,
    pub look: PositionLook,
    pub entry: CanonicalDecimal,
    pub stop: CanonicalDecimal,
    pub target: CanonicalDecimal,
    pub width_bars: CanonicalDecimal,
    /// Whether the call changed the drawing; `false` for an inspection or
    /// values already standing.
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DefaultAction {
    /// New positions of the drawing's side open with its look and ratio.
    Save,
    /// New positions of the drawing's side open with the factory look.
    Reset,
    /// Read what new positions of the drawing's side open with.
    Inspect,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PositionDefaultInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// The position whose tool, and for `save` whose look, the call uses.
    pub drawing_id: WireU64,
    pub action: DefaultAction,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct PositionDefaultResult {
    /// `long-position` or `short-position`: each keeps its own default.
    pub tool_id: String,
    /// What a new position of this tool opens with on this pane's ground.
    pub opening: PositionLook,
    /// Whether a saved default stands, rather than the factory look.
    pub saved: bool,
}

/// `annotate.position.set`.
pub fn set_descriptor() -> CapabilityDescriptor {
    descriptor::<PositionSetInput, PositionSetResult>(
        SET_CAPABILITY_ID,
        "Edit or inspect a long or short position",
        "Reads and optionally edits one long or short position drawing, named by the `tab_id`, `pane_id` and `drawing_id` analysis.drawings lists: target and stop zone colours (`#RRGGBB`), zone opacity (0-100 %), risk/reward as reward per unit of risk (0.1-20; only the target moves, entry and stop never do) and width in bars (at least 14; no price moves). The same edits as the position's floating bar. Omit every optional field to inspect. A locked position refuses ratio and width but takes colours. Invalid input changes nothing. One undo step; saved with the drawings. The result reads the look, legs and width back.",
        "Explicit values make retries harmless; the result reads the drawing back.",
    )
}

/// `annotate.position.default`.
pub fn default_descriptor() -> CapabilityDescriptor {
    descriptor::<PositionDefaultInput, PositionDefaultResult>(
        DEFAULT_CAPABILITY_ID,
        "Save, reset or read a position tool's default",
        "The `Save as default` and `Reset` rows that end a position's floating-bar menus. `save` makes new positions of the named drawing's side (long and short keep separate defaults) open with its colours, opacity and ratio, a custom ratio as its prices make it; `reset` returns them to the factory look (target #2EA064, stop #D63031, 10 %, risk 1 : reward 1); `inspect` changes nothing. Per chart ground, like every drawing default. Drawings already placed never change. The result reads back what a new position opens with.",
        "Save and reset name the state they want; repeating either is harmless.",
    )
}

fn descriptor<Input: JsonSchema, Output: JsonSchema>(
    id: &str,
    title: &str,
    description: &str,
    stale_input_safety: &str,
) -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<Input, Output>(
        id,
        ANNOTATE_MODULE_ID,
        title,
        description,
        stale_input_safety,
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.required_permissions =
        BTreeSet::from([PermissionId::new(COCKPIT_PERMISSION_ID).expect("static permission ID")]);
    descriptor
}

/// How a client reconciles an interrupted position call.
pub const READBACKS: &[Readback] = &[
    journal(
        SET_CAPABILITY_ID,
        Optional,
        SET_EVENT_KIND,
        "payload.position",
        "an event after the pre-call cursor names the drawing with the requested look",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        DEFAULT_CAPABILITY_ID,
        Optional,
        DEFAULT_EVENT_KIND,
        "payload.position_default",
        "an event after the pre-call cursor carries the tool's resulting opening look",
        &[EVERY_OPTIONAL_TEST],
    ),
];
