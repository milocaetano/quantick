//! `strategy.script.arm`, `.list` and `.disarm`: a Pine script from the
//! strategy library armed alarm-only on one chart's bars, as named calls.
//!
//! A script rings when one of its `alertcondition`s holds on a closed bar
//! whose body meets the placement; the title's first word is the side. It
//! places nothing, ever. Instances live for the session only.

use quantick_control::id::ModuleId;
use quantick_control::registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy};
use quantick_control::schema::generated_schema;
use quantick_control::wire::WireU64;
use quantick_control_host::authority::{ReadSpec, STRATEGY_MODULE_ID, read_descriptor};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::readback::{EVERY_FORBIDDEN_TEST, Readback, journal};
use crate::trade::ActionSide;

pub const ARM_CAPABILITY_ID: &str = "strategy.script.arm";
pub const LIST_CAPABILITY_ID: &str = "strategy.script.list";
pub const DISARM_CAPABILITY_ID: &str = "strategy.script.disarm";

/// Journaled by every arm that armed; its payload is the [`ArmResult`].
pub const ARMED_EVENT_KIND: &str = "strategy.script.armed";
/// Journaled by every disarm that removed one; its payload is the
/// [`DisarmResult`].
pub const DISARMED_EVENT_KIND: &str = "strategy.script.disarmed";

/// The handler test that arms, lists and disarms through a fake host.
pub const ARM_LIST_DISARM_TEST: &str = "arm_list_and_disarm_a_script_strategy";

/// The longest script file name a call may give.
pub const SCRIPT_NAME_MAX_BYTES: usize = 128;

/// Where a script may ring. Prices are decimal strings.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Placement {
    /// The rectangle drawing `drawing_id` on the pane marks, while its time
    /// span covers the bar.
    Region { drawing_id: WireU64 },
    /// A fixed band; both edges inside, either order.
    Range { low: String, high: String },
    /// A horizontal line the bar's body must cross in `direction`.
    Line {
        level: String,
        direction: CrossDirection,
    },
    /// Any closed bar.
    Anywhere,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CrossDirection {
    Up,
    Down,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArmInput {
    /// The tab; absent for the one on screen.
    #[serde(default)]
    pub tab_id: Option<WireU64>,
    /// The chart whose closed bars the script judges; absent for the flow
    /// chart.
    #[serde(default)]
    pub pane_id: Option<WireU64>,
    /// A `.pine` file name in the strategy library, as `strategy.script.list`
    /// reports it.
    #[schemars(length(min = 1, max = SCRIPT_NAME_MAX_BYTES))]
    pub script: String,
    /// The side the instance hunts: alerts whose title names the other side
    /// do not ring it.
    pub side: ActionSide,
    pub placement: Placement,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListInput {
    #[serde(default)]
    pub tab_id: Option<WireU64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DisarmInput {
    #[serde(default)]
    pub tab_id: Option<WireU64>,
    /// The id `strategy.script.arm` answered.
    pub id: WireU64,
}

/// One armed script.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScriptStrategy {
    /// Stable for the tab's session; never reused.
    pub id: WireU64,
    pub pane_id: WireU64,
    pub script: String,
    pub side: ActionSide,
    pub placement: Placement,
    pub state: ScriptStrategyState,
    /// Why it stopped: `line N: <CODE> <message>` when its script failed at
    /// runtime, otherwise the disarm reason in the badge's words.
    pub disarm_reason: Option<String>,
    /// What the last closed bar did, as the instance's badge line says it.
    pub status: String,
    /// Always true: a script strategy never places an order.
    pub alarm_only: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStrategyState {
    Watching,
    Disarmed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ArmResult {
    pub tab_id: WireU64,
    pub strategy: ScriptStrategy,
}

/// The strategy library as the call found it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LibraryListing {
    /// The folder the launch named (`QUANTICK_STRATEGIES_DIR`), else
    /// `strategies` beside the working directory.
    pub folder: String,
    /// The scripts `strategy.script.arm` can name: the bundled ones, then
    /// the folder's `.pine` files, sorted. An unreadable folder lists the
    /// bundled ones only.
    pub scripts: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListResult {
    pub tab_id: WireU64,
    pub library: LibraryListing,
    /// Every armed script on the tab, charts in id order, each chart's in
    /// arming order.
    pub armed: Vec<ScriptStrategy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DisarmResult {
    pub tab_id: WireU64,
    pub id: WireU64,
    /// False when no armed script had the id.
    pub disarmed: bool,
    /// The instance as it stood when removed.
    pub strategy: Option<ScriptStrategy>,
}

/// An action in the indicator-script tier: what `indicator.script.attach`
/// asks — code an operator puts on the chart, which never touches a
/// position — owned by the `strategy` module and gone with the session.
fn action<Input: JsonSchema, Output: JsonSchema>(
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
        generated_schema::<Output>(),
    );
    descriptor.module = ModuleId::new(STRATEGY_MODULE_ID).expect("static module ID is valid");
    descriptor.persistence = EffectPersistence::Transient;
    descriptor.stale_input_safety = Some(stale_input_safety.to_owned());
    descriptor
}

#[must_use]
pub fn arm_descriptor() -> CapabilityDescriptor {
    action::<ArmInput, ArmResult>(
        ARM_CAPABILITY_ID,
        "Arm a script strategy",
        "Loads a .pine file from the strategy library and arms it alarm-only on one chart's closed bars: an alertcondition that holds rings the alarm when the bar's body meets the placement (a drawn rectangle, a fixed range, a line crossed one way, or anywhere). The title's first word is the side (bull, buy or long; bear, sell or short; any case); a script with a title naming no side, a computed title, no alertcondition, no title naming the side asked for, or a compile error is refused with the reason, compile errors as `line N: <CODE> <message>`. Never places an order. A runtime error later disarms that instance alone; strategy.script.list shows the reason. Journaled as strategy.script.armed. Lives for the session.",
        "Each arm adds one instance with a new id, so arming twice rings twice; no retry key can make that safe. Read strategy.script.list, or the strategy.script.armed events after a cursor taken before the call, before retrying one whose answer was lost.",
    )
}

#[must_use]
pub fn disarm_descriptor() -> CapabilityDescriptor {
    action::<DisarmInput, DisarmResult>(
        DISARM_CAPABILITY_ID,
        "Disarm a script strategy",
        "Removes one armed script strategy by the id strategy.script.arm answered; it rings no more. An id no instance has answers disarmed: false. Journaled as strategy.script.disarmed.",
        "Disarming names one instance by id, and a second disarm of the same id reports it was not there.",
    )
}

/// How a client reconciles an arm or a disarm whose answer it lost.
pub const READBACKS: &[Readback] = &[
    journal(
        ARM_CAPABILITY_ID,
        IdempotencyPolicy::Forbidden,
        ARMED_EVENT_KIND,
        "payload.strategy.id",
        "an event after the pre-call cursor names the armed instance, its script and placement as the call asked",
        &[ARM_LIST_DISARM_TEST, EVERY_FORBIDDEN_TEST],
    ),
    journal(
        DISARM_CAPABILITY_ID,
        IdempotencyPolicy::Forbidden,
        DISARMED_EVENT_KIND,
        "payload.id",
        "an event after the pre-call cursor carries the id the call named",
        &[ARM_LIST_DISARM_TEST, EVERY_FORBIDDEN_TEST],
    ),
];

#[must_use]
pub fn list_descriptor() -> CapabilityDescriptor {
    read_descriptor::<ListInput, ListResult>(&ReadSpec {
        id: LIST_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "List script strategies",
        description: "Reads the strategy library (the bundled scripts, then the folder's .pine files, sorted: what strategy.script.arm accepts) and every script strategy armed on the tab: its id, chart, script, side, placement, whether it is watching or disarmed and why, and its badge line. Changes nothing.",
        permissions: &["observe", "observe.indicators", "observe.user_text"],
        pagination: None,
    })
}

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "strategy-script-arm-input-v1.schema.json",
            generated_schema::<ArmInput>(),
        ),
        (
            "strategy-script-arm-result-v1.schema.json",
            generated_schema::<ArmResult>(),
        ),
        (
            "strategy-script-list-input-v1.schema.json",
            generated_schema::<ListInput>(),
        ),
        (
            "strategy-script-list-result-v1.schema.json",
            generated_schema::<ListResult>(),
        ),
        (
            "strategy-script-disarm-input-v1.schema.json",
            generated_schema::<DisarmInput>(),
        ),
        (
            "strategy-script-disarm-result-v1.schema.json",
            generated_schema::<DisarmResult>(),
        ),
    ]
}
