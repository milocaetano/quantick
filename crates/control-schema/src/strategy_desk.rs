//! `strategy.desk.*`: every strategy the open tabs have armed — on a drawing
//! or as a script — grouped by asset (symbol and bar type), read in full and
//! paused, resumed, removed, armed again, duplicated or saved as named calls.
//!
//! An instance is named by [`InstanceId`]: a script by the id
//! `strategy.script.arm` answered, a drawn instance by its chart, its drawing
//! and its place among that drawing's instances. Instances live for the
//! session; a call names its tab, absent for the one on screen.

use quantick_control::id::ModuleId;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
use quantick_control::schema::generated_schema;
use quantick_control::wire::WireU64;
use quantick_control_host::authority::{ReadSpec, STRATEGY_MODULE_ID, read_descriptor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_FORBIDDEN_TEST, Readback, journal};
use crate::script_strategy::{Placement, SCRIPT_NAME_MAX_BYTES};
use crate::trade::ActionSide;

pub const LIST_CAPABILITY_ID: &str = "strategy.desk.list";
pub const DETAIL_CAPABILITY_ID: &str = "strategy.desk.detail";
pub const PAUSE_CAPABILITY_ID: &str = "strategy.desk.pause";
pub const RESUME_CAPABILITY_ID: &str = "strategy.desk.resume";
pub const REMOVE_CAPABILITY_ID: &str = "strategy.desk.remove";
pub const ASSET_PAUSE_CAPABILITY_ID: &str = "strategy.desk.asset.pause";
pub const ASSET_RESUME_CAPABILITY_ID: &str = "strategy.desk.asset.resume";
pub const ARM_CAPABILITY_ID: &str = "strategy.desk.arm";
pub const SAVE_AS_CAPABILITY_ID: &str = "strategy.desk.save_as";
pub const DUPLICATE_CAPABILITY_ID: &str = "strategy.desk.duplicate";

/// Journaled by every desk command that went through; its payload is the
/// [`CommandResult`].
pub const CHANGED_EVENT_KIND: &str = "strategy.desk.changed";

/// The handler test that sends every desk command through a fake host.
pub const EVERY_DESK_ACTION_TEST: &str = "every_desk_action_has_a_call";

/// The longest symbol or bar type an asset call may give.
pub const ASSET_TEXT_MAX_BYTES: usize = 64;

/// Which instance of a tab a call names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstanceId {
    /// A script, by the id `strategy.script.arm` answered.
    Script { id: WireU64 },
    /// The `nth` instance (from zero, in arming order) riding drawing
    /// `drawing_id` on chart `pane_id`; removing an earlier one moves it down.
    Drawing {
        pane_id: WireU64,
        drawing_id: WireU64,
        nth: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListInput {}

/// One instance on one tab.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstanceInput {
    /// The tab; absent for the one on screen.
    #[serde(default)]
    pub tab_id: Option<WireU64>,
    pub instance: InstanceId,
}

/// One asset across every open tab, as `strategy.desk.list` groups it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AssetInput {
    #[schemars(length(min = 1, max = ASSET_TEXT_MAX_BYTES))]
    pub symbol: String,
    #[schemars(length(min = 1, max = ASSET_TEXT_MAX_BYTES))]
    pub bar_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveAsInput {
    #[serde(default)]
    pub tab_id: Option<WireU64>,
    /// A script instance.
    pub instance: InstanceId,
    /// The new library file: letters, digits, `_` or `-`, then `.pine`.
    #[schemars(length(min = 1, max = SCRIPT_NAME_MAX_BYTES))]
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Link {
    /// It rides the drawing; `name` is the drawing's, when it has one.
    Drawing {
        drawing_id: WireU64,
        name: Option<String>,
    },
    /// A script on a fixed range, a line or anywhere on the chart.
    Chart,
}

/// What a firing does.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FireAction {
    Alarm,
    /// A paper order: drawn strategies only, never a script.
    Order,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeskState {
    Watching,
    /// Rings and places nothing, still reading closes.
    Paused,
    /// Stopped until `strategy.desk.arm`; `ended_reason` says why.
    Ended,
}

/// One instance as the desk lists it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeskRow {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub instance: InstanceId,
    /// The script's file name, or the drawn instance's preset.
    pub name: String,
    pub state: DeskState,
    pub ended_reason: Option<String>,
    pub link: Link,
    pub action: FireAction,
    /// The instance's badge line.
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeskGroup {
    pub symbol: String,
    /// The chart's bar type as its toolbar summarises it (`tick(4)`).
    pub bar_type: String,
    /// True when none of its instances watches and at least one is paused.
    pub paused: bool,
    pub count: u32,
    pub rows: Vec<DeskRow>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListResult {
    /// In symbol, then bar type order; each tab's rows in chart then arming
    /// order.
    pub groups: Vec<DeskGroup>,
}

/// One declared input with the value the instance runs on.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DeclaredInput {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DetailResult {
    pub strategy: DeskRow,
    /// The script's declared title, or the drawn preset's setup in words.
    pub description: String,
    pub side: ActionSide,
    /// Where it may fire; a drawn instance always rides its region.
    pub placement: Placement,
    /// Read-only: an armed instance runs on these.
    pub inputs: Vec<DeclaredInput>,
    /// Closes it fired on during the UTC day of the tab's latest print.
    pub fires_today: u32,
    /// The script's source; none for a drawn instance.
    pub code: Option<String>,
    /// The chart's closed slots (from zero, the oldest loaded bar) a fresh
    /// copy armed on the oldest fires on, through the live kernel.
    pub history_slots: Vec<u64>,
}

/// What a desk command did.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommandResult {
    /// The call that ran.
    pub capability: String,
    /// The tab, absent for an asset call that spans every tab.
    pub tab_id: Option<WireU64>,
    /// How many instances changed: zero when each already stood as asked.
    pub changed: u32,
    /// The copy a duplicate armed, paused.
    pub created: Option<InstanceId>,
    /// The library file a save wrote.
    pub saved_as: Option<String>,
}

/// An action in the indicator-script tier, owned by the `strategy` module:
/// the instances it reaches are the session's.
fn action<Input: JsonSchema>(
    id: &str,
    title: &str,
    description: &str,
    stale_input_safety: &str,
) -> CapabilityDescriptor {
    let mut descriptor = crate::script::script_descriptor(
        id,
        title,
        description,
        generated_schema::<Input>(),
        generated_schema::<CommandResult>(),
    );
    descriptor.module = ModuleId::new(STRATEGY_MODULE_ID).expect("static module ID is valid");
    descriptor.persistence = EffectPersistence::Transient;
    descriptor.stale_input_safety = Some(stale_input_safety.to_owned());
    descriptor
}

const BY_INSTANCE: &str = "The instance is named by id; a drawn instance's place shifts when an earlier sibling is removed, so read strategy.desk.list again before a retry, or the strategy.desk.changed events after a cursor taken before the call.";

/// Every desk command's descriptor, in the order the handlers register them.
#[must_use]
pub fn command_descriptors() -> Vec<CapabilityDescriptor> {
    vec![
        action::<InstanceInput>(
            PAUSE_CAPABILITY_ID,
            "Pause a strategy",
            "Pauses one watching strategy: it rings and places nothing, while its script or ruler keeps reading closes, so a resume judges the next close as a never-paused one would. A resting order or an open position keeps its course. Pausing a paused one changes nothing; an ended one is refused. Journaled as strategy.desk.changed.",
            BY_INSTANCE,
        ),
        action::<InstanceInput>(
            RESUME_CAPABILITY_ID,
            "Resume a strategy",
            "Resumes one paused strategy from the next close. Resuming a watching one changes nothing; an ended one is refused. Journaled as strategy.desk.changed.",
            BY_INSTANCE,
        ),
        action::<InstanceInput>(
            REMOVE_CAPABILITY_ID,
            "Remove a strategy",
            "Removes one strategy; a drawn one's resting entry is cancelled, an open position stays the trader's, and its drawing keeps its other strategies. Journaled as strategy.desk.changed.",
            "A second remove of the same id names another instance or none. Read strategy.desk.list, or the strategy.desk.changed events after a cursor taken before the call, before retrying.",
        ),
        action::<AssetInput>(
            ASSET_PAUSE_CAPABILITY_ID,
            "Pause an asset's strategies",
            "Pauses every watching strategy on charts of the symbol and bar type, on every open tab; the other assets' are untouched. Answers how many changed. Journaled as strategy.desk.changed.",
            "Pausing again changes nothing; the answer counts what changed.",
        ),
        action::<AssetInput>(
            ASSET_RESUME_CAPABILITY_ID,
            "Resume an asset's strategies",
            "Resumes every paused strategy on charts of the symbol and bar type, on every open tab. Journaled as strategy.desk.changed.",
            "Resuming again changes nothing; the answer counts what changed.",
        ),
        action::<InstanceInput>(
            ARM_CAPABILITY_ID,
            "Arm an ended strategy",
            "Arms one ended strategy again, warmed on the chart's latest closes and unpaused; a script is recompiled from the source it was armed with. A watching one is refused. Journaled as strategy.desk.changed.",
            BY_INSTANCE,
        ),
        action::<SaveAsInput>(
            SAVE_AS_CAPABILITY_ID,
            "Save a script strategy as",
            "Writes the source a script strategy was armed with to the strategy library as a new .pine file; the name must be free. The instance is unchanged. A drawn strategy has no script and is refused. Journaled as strategy.desk.changed.",
            "A second save under the same name is refused because the file exists.",
        ),
        action::<InstanceInput>(
            DUPLICATE_CAPABILITY_ID,
            "Duplicate a strategy",
            "Arms a copy beside one strategy — the same script or preset, side and placement on the same chart — paused, and answers its id. Journaled as strategy.desk.changed.",
            "Each duplicate arms another copy; read strategy.desk.list, or the strategy.desk.changed events after a cursor taken before the call, before retrying.",
        ),
    ]
}

#[must_use]
pub fn list_descriptor() -> CapabilityDescriptor {
    read_descriptor::<ListInput, ListResult>(&ReadSpec {
        id: LIST_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "List the strategy desk",
        description: "Reads every strategy the open tabs have armed, drawn and scripted, grouped by symbol and bar type: per group whether it is paused and how many it holds, per strategy its tab, chart, id, name, state (watching, paused or ended and why), what it rides (a drawing or the chart), whether it rings an alarm or places an order, and its badge line. Changes nothing.",
        permissions: &["observe", "observe.indicators", "observe.user_text"],
        pagination: None,
    })
}

#[must_use]
pub fn detail_descriptor() -> CapabilityDescriptor {
    read_descriptor::<InstanceInput, DetailResult>(&ReadSpec {
        id: DETAIL_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "Read one strategy in full",
        description: "Reads one strategy: its desk row, description, side, placement, declared inputs with their values, closes it fired on today (UTC day of the tab's latest print), its script source, and the chart's loaded slots a fresh copy fires on through the live kernel. Changes nothing.",
        permissions: &["observe", "observe.indicators", "observe.user_text"],
        pagination: None,
    })
}

/// How a client reconciles a desk command whose answer it lost.
pub const READBACKS: &[Readback] = &[
    changed(PAUSE_CAPABILITY_ID),
    changed(RESUME_CAPABILITY_ID),
    changed(REMOVE_CAPABILITY_ID),
    changed(ASSET_PAUSE_CAPABILITY_ID),
    changed(ASSET_RESUME_CAPABILITY_ID),
    changed(ARM_CAPABILITY_ID),
    changed(SAVE_AS_CAPABILITY_ID),
    changed(DUPLICATE_CAPABILITY_ID),
];

const fn changed(capability: &'static str) -> Readback {
    journal(
        capability,
        IdempotencyPolicy::Forbidden,
        CHANGED_EVENT_KIND,
        "payload.capability",
        "an event after the pre-call cursor names the call",
        &[EVERY_DESK_ACTION_TEST, EVERY_FORBIDDEN_TEST],
    )
}

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "strategy-desk-list-input-v1.schema.json",
            generated_schema::<ListInput>(),
        ),
        (
            "strategy-desk-list-result-v1.schema.json",
            generated_schema::<ListResult>(),
        ),
        (
            "strategy-desk-instance-input-v1.schema.json",
            generated_schema::<InstanceInput>(),
        ),
        (
            "strategy-desk-detail-result-v1.schema.json",
            generated_schema::<DetailResult>(),
        ),
        (
            "strategy-desk-asset-input-v1.schema.json",
            generated_schema::<AssetInput>(),
        ),
        (
            "strategy-desk-save-as-input-v1.schema.json",
            generated_schema::<SaveAsInput>(),
        ),
        (
            "strategy-desk-command-result-v1.schema.json",
            generated_schema::<CommandResult>(),
        ),
    ]
}
