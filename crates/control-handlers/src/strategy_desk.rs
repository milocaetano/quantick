//! `strategy.desk.*`: every strategy the open tabs have armed — on a drawing
//! or as a script — grouped by asset (symbol and bar type), read in full and
//! paused, resumed, removed, armed again, duplicated or saved as named calls;
//! and `strategy.script.action.set`, the trade tier's desk command.
//!
//! An instance is named by [`InstanceId`]: a script by the id
//! `strategy.script.arm` answered, a drawn instance by its chart, its drawing
//! and the id the desk gave it. Ids are never reused within a tab's session,
//! so a removal never makes an id name another instance. Instances live for the
//! session; an instance call names its tab, since every tab numbers its own.
//!
//! The tab's strategies decide (`quantick_session::strategy_desk`): the
//! grouping, what each command changes, the detail and its history markers.
//! What is here is parsing, lending each tab with its symbol and its charts'
//! bar types, the chart facts the session asks for — closed bars, drawing
//! regions and names — the library write a save asks for, and the journal.

use quantick_chart::pane_series::PaneSeriesRead;
use quantick_chart::state::dec_from_f64;
use quantick_control::error::ControlError;
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy, RegistryError};
use quantick_control::schema::generated_schema;
use quantick_control::wire::WireU64;
use quantick_control_host::actions::ReplayTrace;
use quantick_control_host::authority::STRATEGY_MODULE_ID;
use quantick_control_host::authority::TRADE_PERMISSION_ID;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use quantick_engine::{Bar, Side};
use quantick_session::script_strategies::SignalAction;
use quantick_session::strategy_desk::{
    self as desk, Asset, DeskAction, DeskLink, DeskOutcome, DeskTab, InstanceRef,
    StrategyDeskCommand, StrategyDeskView, StrategyDetail,
};
use quantick_stores::script_library::ScriptLibrary;
use quantick_trading::ActionSide;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

use crate::JournalAccess;
use crate::call::{Call, CallCx, CallSpec, dock_call, read_posture};
use crate::dock::ActionDock;
use crate::invalid;
use crate::market_tab::MarketTabHost;
use crate::script_strategy::{
    DEFAULT_LIBRARY, FireAction, Placement, READ_PERMISSIONS, SCRIPT_NAME_MAX_BYTES, SCRIPT_TIER,
    ScriptPane, ScriptStrategyHost, ScriptTab, action_words, footprint_warmup, placement,
    script_action, signal_action,
};
use crate::session::SessionTab;
use crate::tabs::{tab_closed, tab_index};
use crate::wire::readback::{EVERY_FORBIDDEN_TEST, Readback, journal};
use command::DeskCommand;

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
/// The script call that runs the desk's set-action command: it shares the
/// desk's path and journal, and sits in the trade tier.
pub const ACTION_SET_CAPABILITY_ID: &str = "strategy.script.action.set";

/// Journaled by every desk command that went through; its payload is the
/// [`CommandResult`].
pub const CHANGED_EVENT_KIND: &str = "strategy.desk.changed";

/// The handler test that sends every desk command through a fake host.
pub const EVERY_DESK_ACTION_TEST: &str = "every_desk_action_has_a_call";

/// The handler test that sets a script's action and reads it back.
pub const ACTION_SET_TEST: &str = "script_action_set_reads_back_through_list";

pub type Command = StrategyDeskCommand<DrawingId>;
/// One command for the tab it names.
pub type Sent = (u64, Command);

/// The longest symbol or bar type an asset call may give.
pub const ASSET_TEXT_MAX_BYTES: usize = 64;

/// Which instance of a tab a call names.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstanceId {
    /// A script, by the id `strategy.script.arm` answered.
    Script { id: WireU64 },
    /// Instance `id` riding drawing `drawing_id` on chart `pane_id`, as
    /// `strategy.desk.list` reports it; never reused in the tab's session.
    Drawing {
        pane_id: WireU64,
        drawing_id: WireU64,
        id: WireU64,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListInput {}

/// One instance on one tab.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstanceInput {
    /// The tab the row came from: ids are numbered per tab.
    pub tab_id: WireU64,
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
    /// The tab the row came from: ids are numbered per tab.
    pub tab_id: WireU64,
    /// A script instance.
    pub instance: InstanceId,
    /// The new library file: letters, digits, `_` or `-`, then `.pine`.
    #[schemars(length(min = 1, max = SCRIPT_NAME_MAX_BYTES))]
    pub name: String,
}

/// What one armed script does when it fires, from its next close.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionSetInput {
    /// The tab; absent for the one on screen.
    #[serde(default)]
    pub tab_id: Option<WireU64>,
    /// The id `strategy.script.arm` answered.
    pub id: WireU64,
    pub action: FireAction,
    /// An order's size above zero, in the instrument's units (contracts on
    /// a future), or `ticket` for the ticket's quantity when the bar closes. Required for an order, refused
    /// for an alarm.
    #[serde(default)]
    pub quantity: Option<String>,
    /// An order's stop distance from the entry, in price; absent or `0`
    /// places none, never below zero. Refused for an alarm.
    #[serde(default)]
    pub stop: Option<String>,
    /// An order's target distance from the entry, in price; absent or `0`
    /// places none, never below zero. Refused for an alarm.
    #[serde(default)]
    pub gain: Option<String>,
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
    /// A script order's size, stop and gain, as `strategy.script.list` words
    /// them; absent for an alarm and for a drawn instance, whose preset
    /// sizes and protects its orders.
    pub quantity: Option<String>,
    pub stop: Option<String>,
    pub gain: Option<String>,
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

/// The tabs as the desk reaches them: each lent as for `strategy.script.*`,
/// beside how many there are and the symbol each shows.
pub trait StrategyDeskHost: ScriptStrategyHost {
    fn desk_tab_count(&self) -> usize;
    /// The symbol of the tab's active source: the recording's while one
    /// plays, never the live feed parked beneath it.
    fn desk_symbol(&self, index: usize) -> Option<String>;
}

/// A window's tabs answer through their market; the symbol is the one the
/// session scopes read, which follows a replay.
impl<T> StrategyDeskHost for T
where
    T: MarketTabHost + ?Sized,
    T::Tab: SessionTab,
{
    fn desk_tab_count(&self) -> usize {
        self.tab_count()
    }
    fn desk_symbol(&self, index: usize) -> Option<String> {
        self.open_tab(index)
            .map(|tab| SessionTab::symbol(tab).to_owned())
    }
}

/// # Errors
/// A capability is already docked.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: StrategyDeskHost + ?Sized,
    A: JournalAccess,
{
    dock_call::<ListInput, _, _, _>(registry)?;
    dock_call::<Detail, _, _, _>(registry)?;
    dock_call::<Pause, _, _, _>(registry)?;
    dock_call::<Resume, _, _, _>(registry)?;
    dock_call::<Remove, _, _, _>(registry)?;
    dock_call::<AssetPause, _, _, _>(registry)?;
    dock_call::<AssetResume, _, _, _>(registry)?;
    dock_call::<Arm, _, _, _>(registry)?;
    dock_call::<SaveAsInput, _, _, _>(registry)?;
    dock_call::<Duplicate, _, _, _>(registry)?;
    dock_call::<ActionSetInput, _, _, _>(registry)
}

/// Each chart of a lent tab with its bar type.
fn bar_types(panes: &[ScriptPane<'_>]) -> Vec<(u64, String)> {
    let summary = |pane: &ScriptPane<'_>| pane.series.spec.spec().summary();
    panes.iter().map(|pane| (pane.id, summary(pane))).collect()
}

fn pane<'p, 'a>(panes: &'p [ScriptPane<'a>], id: u64) -> Option<&'p ScriptPane<'a>> {
    panes.iter().find(|pane| pane.id == id)
}

/// The name drawing `drawing` on chart `pane_id` carries.
fn drawing_name(panes: &[ScriptPane<'_>], pane_id: u64, drawing: DrawingId) -> Option<String> {
    let drawings = pane(panes, pane_id)?.drawings;
    drawings
        .items()
        .get(drawings.index_of(drawing)?)?
        .name
        .clone()
}

/// Every closed slot of the chart, slot zero first, with its footprint POC
/// where the tape kept one: the venue prefix has none.
fn closed_bars(series: &PaneSeriesRead<'_>) -> Vec<(Bar, Option<Decimal>)> {
    let bars = series.closed_footprint_bars().into_iter();
    bars.map(|closed| (closed.bar, closed.poc)).collect()
}

fn region(
    drawings: &Drawings<DrawingTool>,
    drawing: DrawingId,
    slot: usize,
) -> Option<(Decimal, Decimal, bool)> {
    let (a, b, active) = quantick_drawings::alert_geometry::region_at(drawings, drawing, slot)?;
    Some((dec_from_f64(a), dec_from_f64(b), active))
}

impl<H: StrategyDeskHost + ?Sized> Call<H> for ListInput {
    type Input = Self;
    type Answer = ListResult;

    const SPEC: CallSpec = CallSpec {
        id: LIST_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "List the strategy desk",
        description: "Reads every strategy the open tabs have armed, drawn and scripted, grouped by symbol and bar type: per group whether it is paused and how many it holds, per strategy its tab, chart, id, name, state (watching, paused or ended and why), what it rides (a drawing or the chart), whether it rings an alarm or places an order, and its badge line. Changes nothing.",
        permissions: READ_PERMISSIONS,
        event: None,
        replay: ReplayTrace::Recorded,
        posture: read_posture,
    };

    fn run<A: JournalAccess>(
        _input: Self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<ListResult, ControlError> {
        let groups = groups(host).iter().map(group).collect();
        Ok(ListResult { groups })
    }
}

/// Every open tab's instances grouped by asset: what `strategy.desk.list`
/// answers and the Strategies tab paints.
pub fn groups<H: StrategyDeskHost + ?Sized>(host: &mut H) -> Vec<desk::DeskGroup<DrawingId>> {
    let mut view = StrategyDeskView::default();
    for index in 0..host.desk_tab_count() {
        let tab_id = host.tab_id_at(index);
        let Some(symbol) = host.desk_symbol(index) else {
            continue;
        };
        let Some(ScriptTab {
            strategies, panes, ..
        }) = host.script_tab(index)
        else {
            continue;
        };
        let bar_types = bar_types(&panes);
        let tab = DeskTab {
            tab_id,
            symbol: &symbol,
            panes: &bar_types,
        };
        view.read(&tab, strategies, |pane_id, drawing| {
            drawing_name(&panes, pane_id, drawing)
        });
    }
    view.groups()
}

/// What each open tab's scripts do when they fire, by tab and script id:
/// the order settings a desk row leaves out.
pub fn script_actions<H: StrategyDeskHost + ?Sized>(
    host: &mut H,
) -> BTreeMap<(u64, u64), SignalAction> {
    let mut actions = BTreeMap::new();
    for index in 0..host.desk_tab_count() {
        let tab_id = host.tab_id_at(index);
        let Some(tab) = host.script_tab(index) else {
            continue;
        };
        for reading in tab.strategies.script_readings() {
            actions.insert((tab_id, reading.id), reading.action);
        }
    }
    actions
}

struct Detail;

impl<H: StrategyDeskHost + ?Sized> Call<H> for Detail {
    type Input = InstanceInput;
    type Answer = DetailResult;

    const SPEC: CallSpec = CallSpec {
        id: DETAIL_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "Read one strategy in full",
        description: "Reads one strategy: its desk row, description, side, placement, declared inputs with their values, closes it fired on today (UTC day of the tab's latest print), its script source, and the chart's loaded slots a fresh copy fires on through the live kernel. Changes nothing.",
        permissions: READ_PERMISSIONS,
        event: None,
        replay: ReplayTrace::Recorded,
        posture: read_posture,
    };

    fn run<A: JournalAccess>(
        input: InstanceInput,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<DetailResult, ControlError> {
        let detail = instance_detail(host, input.tab_id.get(), instance_ref(input.instance))?;
        let action = match input.instance {
            InstanceId::Script { id } => script_actions(host)
                .get(&(input.tab_id.get(), id.get()))
                .copied(),
            InstanceId::Drawing { .. } => None,
        };
        let (_, quantity, stop, gain) = action_words(action.unwrap_or_default());
        Ok(DetailResult {
            strategy: row(&detail.row),
            description: detail.description,
            side: match detail.side {
                Side::Buy => ActionSide::Buy,
                Side::Sell => ActionSide::Sell,
            },
            placement: placement(detail.placement),
            inputs: (detail.inputs.into_iter())
                .map(|(name, value)| DeclaredInput { name, value })
                .collect(),
            fires_today: detail.fires_today,
            quantity,
            stop,
            gain,
            code: detail.code,
            history_slots: (detail.history.into_iter())
                .map(|slot| u64::try_from(slot).unwrap_or(u64::MAX))
                .collect(),
        })
    }
}

/// Instance `id` on tab `tab_id` in full, history markers included: what
/// `strategy.desk.detail` answers and the strategy window paints.
///
/// # Errors
/// No open tab has `tab_id`, or no instance on it has `id`.
pub fn instance_detail<H: StrategyDeskHost + ?Sized>(
    host: &mut H,
    tab_id: u64,
    id: InstanceRef<DrawingId>,
) -> Result<StrategyDetail<DrawingId>, ControlError> {
    let index = tab_index(host, Some(WireU64::new(tab_id)))?;
    let symbol = host.desk_symbol(index).ok_or_else(tab_closed)?;
    let ScriptTab {
        strategies, panes, ..
    } = host.script_tab(index).ok_or_else(tab_closed)?;
    let bar_types = bar_types(&panes);
    let tab = DeskTab {
        tab_id,
        symbol: &symbol,
        panes: &bar_types,
    };
    strategies
        .detail(
            &tab,
            id,
            |pane_id, drawing| drawing_name(&panes, pane_id, drawing),
            |pane_id| pane(&panes, pane_id).map_or_else(Vec::new, |p| closed_bars(&p.series)),
            |pane_id, drawing, slot| region(pane(&panes, pane_id)?.drawings, drawing, slot),
        )
        .ok_or_else(|| invalid(NO_SUCH))
}

/// These calls sit in the indicator-script tier, below the trade tier, so
/// none of them lets a drawn strategy start placing paper orders again.
const MAY_START_ORDERS: bool = false;

const NO_SUCH: &str = "no armed strategy on this tab has that id; read strategy.desk.list again";

const BY_INSTANCE: &str = "The instance is named by an id never reused in the tab's session, so a repeat reaches the same instance or none; read strategy.desk.list, or the strategy.desk.changed events after a cursor taken before the call, before retrying.";

/// What the desk's own commands share: the indicator-script tier, owned by
/// the `strategy` module and journaled as [`CHANGED_EVENT_KIND`]. Each
/// command states its id, title and description, and its posture when a
/// repeat is not [`BY_INSTANCE`]'s.
const COMMAND: CallSpec = CallSpec {
    id: "",
    module: STRATEGY_MODULE_ID,
    title: "",
    description: "",
    permissions: SCRIPT_TIER,
    event: Some(CHANGED_EVENT_KIND),
    replay: ReplayTrace::Recorded,
    posture: |spec| script_action(spec, BY_INSTANCE),
};

/// `pub` only because the blanket [`Call`] impl names it; the private module
/// keeps it the family's.
mod command {
    use super::{CallSpec, JsonSchema, Sent};
    use serde::de::DeserializeOwned;

    /// One of the desk's own commands: its spec and what its input sends.
    pub trait DeskCommand {
        type Input: DeserializeOwned + JsonSchema;
        const SPEC: CallSpec;
        fn command(input: Self::Input) -> Sent;
    }
}

/// Every desk command runs the one path the Strategies tab shares, and none
/// may start a drawn instance's paper orders.
impl<H: StrategyDeskHost + ?Sized, C: DeskCommand> Call<H> for C {
    type Input = C::Input;
    type Answer = CommandResult;

    const SPEC: CallSpec = C::SPEC;

    fn run<A: JournalAccess>(
        input: C::Input,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<CommandResult, ControlError> {
        let (tab_id, command) = C::command(input);
        send(host, tab_id, command, MAY_START_ORDERS)
    }
}

/// `command` for the instance `input` names, on its tab.
fn of_instance(input: InstanceInput, command: fn(InstanceRef<DrawingId>) -> Command) -> Sent {
    (input.tab_id.get(), command(instance_ref(input.instance)))
}

/// `command` for the asset `input` names, which reaches every tab.
fn of_asset(AssetInput { symbol, bar_type }: AssetInput, command: fn(Asset) -> Command) -> Sent {
    (0, command(Asset { symbol, bar_type }))
}

struct Pause;

impl DeskCommand for Pause {
    type Input = InstanceInput;
    const SPEC: CallSpec = CallSpec {
        id: PAUSE_CAPABILITY_ID,
        title: "Pause a strategy",
        description: "Pauses one watching strategy: it rings and places nothing, while its script or ruler keeps reading closes, so a resume judges the next close as a never-paused one would. A resting order or an open position keeps its course. Pausing a paused one changes nothing; an ended one is refused. Journaled as strategy.desk.changed.",
        ..COMMAND
    };
    fn command(input: InstanceInput) -> Sent {
        of_instance(input, StrategyDeskCommand::Pause)
    }
}

struct Resume;

impl DeskCommand for Resume {
    type Input = InstanceInput;
    const SPEC: CallSpec = CallSpec {
        id: RESUME_CAPABILITY_ID,
        title: "Resume a strategy",
        description: "Resumes one paused strategy from the next close. Resuming a watching one changes nothing; an ended one is refused, and so is a drawn strategy that places paper orders: only the trader restarts those. Journaled as strategy.desk.changed.",
        ..COMMAND
    };
    fn command(input: InstanceInput) -> Sent {
        of_instance(input, StrategyDeskCommand::Resume)
    }
}

const REMOVED_ONCE: &str = "A second remove of the same id finds no instance and is refused. Read strategy.desk.list, or the strategy.desk.changed events after a cursor taken before the call, before retrying.";

struct Remove;

impl DeskCommand for Remove {
    type Input = InstanceInput;
    const SPEC: CallSpec = CallSpec {
        id: REMOVE_CAPABILITY_ID,
        title: "Remove a strategy",
        description: "Removes one strategy; a drawn one's resting entry is cancelled, an open position stays the trader's, and its drawing keeps its other strategies. Journaled as strategy.desk.changed.",
        posture: |spec| script_action(spec, REMOVED_ONCE),
        ..COMMAND
    };
    fn command(input: InstanceInput) -> Sent {
        of_instance(input, StrategyDeskCommand::Remove)
    }
}

const PAUSED_AGAIN: &str = "Pausing again changes nothing; the answer counts what changed.";

struct AssetPause;

impl DeskCommand for AssetPause {
    type Input = AssetInput;
    const SPEC: CallSpec = CallSpec {
        id: ASSET_PAUSE_CAPABILITY_ID,
        title: "Pause an asset's strategies",
        description: "Pauses every watching strategy on charts of the symbol and bar type, on every open tab; the other assets' are untouched. Answers how many changed. Journaled as strategy.desk.changed.",
        posture: |spec| script_action(spec, PAUSED_AGAIN),
        ..COMMAND
    };
    fn command(input: AssetInput) -> Sent {
        of_asset(input, StrategyDeskCommand::PauseAsset)
    }
}

const RESUMED_AGAIN: &str = "Resuming again changes nothing; the answer counts what changed.";

struct AssetResume;

impl DeskCommand for AssetResume {
    type Input = AssetInput;
    const SPEC: CallSpec = CallSpec {
        id: ASSET_RESUME_CAPABILITY_ID,
        title: "Resume an asset's strategies",
        description: "Resumes every paused strategy on charts of the symbol and bar type, on every open tab, except drawn strategies that place paper orders, which stay paused for the trader to restart; the answer counts what changed. Journaled as strategy.desk.changed.",
        posture: |spec| script_action(spec, RESUMED_AGAIN),
        ..COMMAND
    };
    fn command(input: AssetInput) -> Sent {
        of_asset(input, StrategyDeskCommand::ResumeAsset)
    }
}

struct Arm;

impl DeskCommand for Arm {
    type Input = InstanceInput;
    const SPEC: CallSpec = CallSpec {
        id: ARM_CAPABILITY_ID,
        title: "Arm an ended strategy",
        description: "Arms one ended strategy again, warmed on the chart's latest closes and unpaused; a script is recompiled from the source it was armed with. A watching one, one whose drawing no longer passes the arm check, and a drawn one that places paper orders are refused. Journaled as strategy.desk.changed.",
        ..COMMAND
    };
    fn command(input: InstanceInput) -> Sent {
        of_instance(input, StrategyDeskCommand::Arm)
    }
}

const SAVED_ONCE: &str = "A second save under the same name is refused because the file exists.";

impl DeskCommand for SaveAsInput {
    type Input = Self;
    const SPEC: CallSpec = CallSpec {
        id: SAVE_AS_CAPABILITY_ID,
        title: "Save a script strategy as",
        description: "Writes the source a script strategy was armed with to the strategy library as a new .pine file; the name must be free. The instance is unchanged. A drawn strategy has no script and is refused. Journaled as strategy.desk.changed.",
        posture: |spec| script_action(spec, SAVED_ONCE),
        ..COMMAND
    };
    fn command(input: Self) -> Sent {
        let instance = instance_ref(input.instance);
        let name = input.name;
        (input.tab_id.get(), StrategyDeskCommand::SaveAs { instance, name })
    }
}

const EACH_DUPLICATE_ARMS: &str = "Each duplicate arms another copy; read strategy.desk.list, or the strategy.desk.changed events after a cursor taken before the call, before retrying.";

struct Duplicate;

impl DeskCommand for Duplicate {
    type Input = InstanceInput;
    const SPEC: CallSpec = CallSpec {
        id: DUPLICATE_CAPABILITY_ID,
        title: "Duplicate a strategy",
        description: "Arms a copy beside one strategy — the same script or preset, side and placement on the same chart — paused, and answers its id. A drawing that no longer passes the arm check, and a drawn strategy that places paper orders, are refused. Journaled as strategy.desk.changed.",
        posture: |spec| script_action(spec, EACH_DUPLICATE_ARMS),
        ..COMMAND
    };
    fn command(input: InstanceInput) -> Sent {
        of_instance(input, StrategyDeskCommand::Duplicate)
    }
}

/// `strategy.script.action.set`: the trade tier's, because it can make a
/// script place paper orders — the desk's own calls never start orders.
/// Setting the same action twice leaves the same action.
impl<H: StrategyDeskHost + ?Sized> Call<H> for ActionSetInput {
    type Input = Self;
    type Answer = CommandResult;

    const SPEC: CallSpec = CallSpec {
        id: ACTION_SET_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "Set what a script strategy does when it fires",
        description: "Sets what one armed script strategy does from its next close: `alarm` rings and places nothing; `order` places a paper market order of `quantity` (a size above zero in the instrument's units, or `ticket` for the ticket's quantity when the bar closes) with a stop `stop` and a target `gain` away from the entry, in price (absent or 0: no leg). A resting order or an open position keeps its course. A drawn strategy takes its action from its preset and is refused, as are an order with no quantity, a quantity of zero or less, a distance below zero, and quantity, stop or gain sent with `alarm`. strategy.script.list and strategy.desk.detail read it back. Journaled as strategy.desk.changed.",
        permissions: &[TRADE_PERMISSION_ID],
        event: Some(CHANGED_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| CapabilityDescriptor {
            idempotency: IdempotencyPolicy::Optional,
            ..crate::trade::descriptor(
                spec.id,
                spec.title,
                spec.description,
                false,
                "The instance is named by an id never reused in the tab's session, and the call states the whole action, so a repeat leaves the instance as the first call did; an id that has since been removed is refused.",
            )
        },
    };

    fn run<A: JournalAccess>(
        input: Self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<CommandResult, ControlError> {
        let action = signal_action(
            input.action,
            input.quantity.as_deref(),
            input.stop.as_deref(),
            input.gain.as_deref(),
        )?;
        let tab_id = host.tab_id_at(tab_index(host, input.tab_id)?);
        let id = InstanceRef::Script(input.id.get());
        let command = StrategyDeskCommand::SetAction { id, action };
        send(host, tab_id, command, TRADE_TIER_MAY_START_ORDERS)
    }
}

/// `strategy.script.action.set` sits in the trade tier: the trader's own.
const TRADE_TIER_MAY_START_ORDERS: bool = true;

/// The call that carries `command`.
const fn capability(command: &Command) -> &'static str {
    match command {
        StrategyDeskCommand::Pause(_) => PAUSE_CAPABILITY_ID,
        StrategyDeskCommand::Resume(_) => RESUME_CAPABILITY_ID,
        StrategyDeskCommand::Remove(_) => REMOVE_CAPABILITY_ID,
        StrategyDeskCommand::PauseAsset(_) => ASSET_PAUSE_CAPABILITY_ID,
        StrategyDeskCommand::ResumeAsset(_) => ASSET_RESUME_CAPABILITY_ID,
        StrategyDeskCommand::Arm(_) => ARM_CAPABILITY_ID,
        StrategyDeskCommand::SaveAs { .. } => SAVE_AS_CAPABILITY_ID,
        StrategyDeskCommand::Duplicate(_) => DUPLICATE_CAPABILITY_ID,
        StrategyDeskCommand::SetAction { .. } => ACTION_SET_CAPABILITY_ID,
    }
}

/// Send `command` to the instances it names, and write what a save hands
/// back to the library: the one path the `strategy.desk.*` calls and the
/// Strategies tab share. An instance command reaches tab `tab_id`; an asset
/// command reaches every open tab showing its symbol and ignores `tab_id`.
/// Only a caller that `may_start_orders` — the trader — resumes, arms or
/// duplicates a drawn instance that places paper orders.
///
/// # Errors
/// No open tab has `tab_id`, or the session refused the command.
pub fn send<H: StrategyDeskHost + ?Sized>(
    host: &mut H,
    tab_id: u64,
    command: Command,
    may_start_orders: bool,
) -> Result<CommandResult, ControlError> {
    let mut result = CommandResult {
        capability: capability(&command).to_owned(),
        tab_id: None,
        changed: 0,
        created: None,
        saved_as: None,
    };
    if let StrategyDeskCommand::PauseAsset(asset) | StrategyDeskCommand::ResumeAsset(asset) =
        &command
    {
        result.changed = count(on_asset(host, asset, &command, may_start_orders));
        return Ok(result);
    }
    let index = tab_index(host, Some(WireU64::new(tab_id)))?;
    let symbol = host.desk_symbol(index).ok_or_else(tab_closed)?;
    let ScriptTab {
        strategies,
        panes,
        library,
    } = host.script_tab(index).ok_or_else(tab_closed)?;
    let bar_types = bar_types(&panes);
    let tab = DeskTab {
        tab_id,
        symbol: &symbol,
        panes: &bar_types,
    };
    let warmup = |pane_id: u64, depth: usize| {
        pane(&panes, pane_id).map_or_else(Vec::new, |p| footprint_warmup(&p.series, depth))
    };
    let check = |pane_id: u64, drawing: DrawingId| {
        let held = pane(&panes, pane_id).ok_or("the chart closed while the call ran")?;
        let closed = held.series.closed_slots();
        quantick_drawings::alert_geometry::check_arm(held.drawings, drawing, closed)
            .map_err(|refusal| format!("drawing {}: {refusal}", drawing.0))
    };
    let outcome = strategies
        .execute_desk(&tab, command, may_start_orders, check, warmup)
        .map_err(invalid)?;
    result.tab_id = Some(WireU64::new(tab_id));
    match outcome {
        DeskOutcome::Changed(changed) => result.changed = count(changed),
        DeskOutcome::Duplicated(copy) => {
            result.changed = 1;
            result.created = Some(instance_id(copy));
        }
        DeskOutcome::Save { name, source } => {
            let folder = Path::new(library.unwrap_or(DEFAULT_LIBRARY));
            let saved = ScriptLibrary::open(folder).save_as(&name, &source);
            result.saved_as = Some(saved.map_err(invalid)?.id);
            result.changed = 1;
        }
    }
    Ok(result)
}

/// Send an asset command to every open tab's instances on `asset`; how many
/// changed.
fn on_asset<H: StrategyDeskHost + ?Sized>(
    host: &mut H,
    asset: &Asset,
    command: &Command,
    may_start_orders: bool,
) -> usize {
    let mut changed = 0;
    for index in 0..host.desk_tab_count() {
        let tab_id = host.tab_id_at(index);
        if host.desk_symbol(index).as_deref() != Some(asset.symbol.as_str()) {
            continue;
        }
        let Some(ScriptTab {
            strategies, panes, ..
        }) = host.script_tab(index)
        else {
            continue;
        };
        let bar_types = bar_types(&panes);
        let tab = DeskTab {
            tab_id,
            symbol: &asset.symbol,
            panes: &bar_types,
        };
        let outcome = strategies.execute_desk(
            &tab,
            command.clone(),
            may_start_orders,
            |_, _| Ok(()),
            |_, _| Vec::new(),
        );
        if let Ok(DeskOutcome::Changed(count)) = outcome {
            changed += count;
        }
    }
    changed
}

fn count(changed: usize) -> u32 {
    u32::try_from(changed).unwrap_or(u32::MAX)
}

fn instance_ref(id: InstanceId) -> InstanceRef<DrawingId> {
    match id {
        InstanceId::Script { id } => InstanceRef::Script(id.get()),
        InstanceId::Drawing {
            pane_id,
            drawing_id,
            id,
        } => InstanceRef::Drawing {
            pane_id: pane_id.get(),
            drawing: DrawingId(drawing_id.get()),
            id: id.get(),
        },
    }
}

fn instance_id(id: InstanceRef<DrawingId>) -> InstanceId {
    match id {
        InstanceRef::Script(id) => InstanceId::Script {
            id: WireU64::new(id),
        },
        InstanceRef::Drawing {
            pane_id,
            drawing,
            id,
        } => InstanceId::Drawing {
            pane_id: WireU64::new(pane_id),
            drawing_id: WireU64::new(drawing.0),
            id: WireU64::new(id),
        },
    }
}

fn group(group: &desk::DeskGroup<DrawingId>) -> DeskGroup {
    DeskGroup {
        symbol: group.asset.symbol.clone(),
        bar_type: group.asset.bar_type.clone(),
        paused: group.paused,
        count: count(group.rows.len()),
        rows: group.rows.iter().map(row).collect(),
    }
}

fn row(row: &desk::DeskRow<DrawingId>) -> DeskRow {
    let (state, ended_reason) = match &row.state {
        desk::DeskState::Watching => (DeskState::Watching, None),
        desk::DeskState::Paused => (DeskState::Paused, None),
        desk::DeskState::Ended { reason } => (DeskState::Ended, Some(reason.clone())),
    };
    DeskRow {
        tab_id: WireU64::new(row.tab_id),
        pane_id: WireU64::new(row.pane_id),
        instance: instance_id(row.id),
        name: row.name.clone(),
        state,
        ended_reason,
        link: match &row.link {
            DeskLink::Drawing { drawing, name } => Link::Drawing {
                drawing_id: WireU64::new(drawing.0),
                name: name.clone(),
            },
            DeskLink::Chart => Link::Chart,
        },
        action: match row.action {
            DeskAction::Alarm => FireAction::Alarm,
            DeskAction::Order => FireAction::Order,
        },
        status: row.status.clone(),
    }
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
    journal(
        ACTION_SET_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        CHANGED_EVENT_KIND,
        "payload.capability",
        "an event after the pre-call cursor names the call; strategy.script.list shows the action, quantity, stop and gain",
        &[
            ACTION_SET_TEST,
            crate::wire::readback::TRADE_UNREACHABLE_TEST,
        ],
    ),
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
            "strategy-script-action-set-input-v1.schema.json",
            generated_schema::<ActionSetInput>(),
        ),
        (
            "strategy-desk-command-result-v1.schema.json",
            generated_schema::<CommandResult>(),
        ),
    ]
}

#[cfg(test)]
#[path = "strategy_desk_tests.rs"]
mod tests;
