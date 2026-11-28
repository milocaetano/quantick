//! `strategy.script.arm`, `.list` and `.disarm`: a Pine script from the
//! strategy library armed on one chart's bars, read and removed by id, as
//! named calls. `strategy.script.action.set` runs through the desk
//! (`crate::strategy_desk`) and parses here.
//!
//! A script fires when one of its `alertcondition`s holds on a closed bar
//! whose body meets the placement; the title's first word is the side. It
//! rings, or — once `strategy.script.action.set` says so — places a paper
//! order of a set quantity and protection. Instances live for the session
//! only.
//!
//! The tab's strategies decide (`quantick_session::script_strategies`):
//! whether a script's titles name sides, what each instance is doing. What
//! is here is parsing, reading the source through the script library the
//! indicators use, the drawing check a region placement needs, the
//! footprint warmup, and the journal. The tab lends itself through
//! [`MarketTab::script_tab`](crate::market_tab::MarketTab::script_tab).

use quantick_chart::pane_series::PaneSeriesRead;
use quantick_control::error::ControlError;
use quantick_control::registry::{
    CapabilityDescriptor, EffectPersistence, IdempotencyPolicy, RegistryError,
};
use quantick_control::schema::generated_schema;
use quantick_control::wire::WireU64;
use quantick_control_host::actions::ReplayTrace;
use quantick_control_host::authority::{
    ANNOTATE_PERMISSION_ID, SCRIPT_PERMISSION_ID, STRATEGY_MODULE_ID,
};
use quantick_control_host::journal::NewEvent;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use quantick_engine::{Bar, Side};
use quantick_session::script_strategies::{
    OrderQuantity, ScriptArm, ScriptPlacement, ScriptReading, SignalAction, SignalOrder,
};
use quantick_session::strategies::TabStrategies;
use quantick_stores::script_library::{LoadedScript, ScriptLibrary};
use quantick_trading::ActionSide;
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use crate::JournalAccess;
use crate::call::{Call, CallCx, CallSpec, dock_call, read_posture};
use crate::dock::ActionDock;
use crate::invalid;
use crate::market_tab::{LentTab, MarketTabHost};
use crate::tabs::{TabDirectory, tab_closed, tab_index};
use crate::wire::readback::{EVERY_FORBIDDEN_TEST, Readback, journal};

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
    /// What a firing does; absent rings. This call arms alarms only: an
    /// `order` is refused, since placing orders is the trade tier's — set it
    /// with `strategy.script.action.set`.
    #[serde(default)]
    pub action: Option<FireAction>,
}

/// What a firing does.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FireAction {
    Alarm,
    /// A paper market order.
    Order,
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
    /// True while `action` is `alarm`: it rings and places nothing.
    pub alarm_only: bool,
    pub action: FireAction,
    /// An order's size in the instrument's units, or `ticket` for the
    /// ticket's quantity when the bar closes. Absent for an alarm.
    pub quantity: Option<String>,
    /// An order's stop distance from the entry, in price; `0` places none.
    /// Absent for an alarm.
    pub stop: Option<String>,
    /// An order's target distance from the entry, in price; `0` places none.
    /// Absent for an alarm.
    pub gain: Option<String>,
}

/// The word [`ScriptStrategy::quantity`] uses for the ticket's quantity.
pub const TICKET_QUANTITY: &str = "ticket";

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
    /// Whether the folder exists and was read. The default folder absent is
    /// `false` with no `error`: there is simply no folder.
    pub found: bool,
    /// Why the folder could not be read; its scripts are then missing here.
    pub error: Option<String>,
    /// The scripts `strategy.script.arm` can name: the bundled ones, then
    /// the folder's `.pine` files, sorted.
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

/// One chart of a lent tab: the bars a script warms on, and the drawings a
/// region placement must name.
pub struct ScriptPane<'a> {
    pub id: u64,
    pub series: PaneSeriesRead<'a>,
    pub drawings: &'a Drawings<DrawingTool>,
}

/// The folder the library scans when the launch named none, relative to the
/// working directory.
pub(crate) const DEFAULT_LIBRARY: &str = "strategies";

/// One open tab, lent for one call: its strategies, its charts with the
/// flow chart first, and the library folder the launch named (`None`: the
/// default).
pub struct ScriptTab<'a> {
    pub strategies: &'a mut TabStrategies<DrawingId>,
    pub panes: Vec<ScriptPane<'a>>,
    pub library: Option<&'a str>,
}

impl<'a> ScriptTab<'a> {
    /// Lend a tab from its strategies, the library folder the launch named,
    /// and each chart as `(id, series, drawings)`, the flow chart first.
    pub fn lend(
        strategies: &'a mut TabStrategies<DrawingId>,
        library: Option<&'a str>,
        panes: impl IntoIterator<Item = (u64, PaneSeriesRead<'a>, &'a Drawings<DrawingTool>)>,
    ) -> Self {
        let panes = (panes.into_iter())
            .map(|(id, series, drawings)| ScriptPane {
                id,
                series,
                drawings,
            })
            .collect();
        Self {
            strategies,
            panes,
            library,
        }
    }
}

/// The tabs as these calls reach them.
pub trait ScriptStrategyHost: TabDirectory {
    /// The tab at `index`, a position the directory gave.
    fn script_tab(&mut self, index: usize) -> Option<ScriptTab<'_>>;
}

/// A window reaches a tab's strategies through the tab it lends.
impl<T: MarketTabHost + ?Sized> ScriptStrategyHost for T {
    fn script_tab(&mut self, index: usize) -> Option<ScriptTab<'_>> {
        let LentTab { tab, .. } = self.market_tab_mut(index)?;
        tab.script_tab()
    }
}

/// # Errors
/// A capability is already docked.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: ScriptStrategyHost + ?Sized,
    A: JournalAccess,
{
    dock_call::<ArmInput, _, _, _>(registry)?;
    dock_call::<ListInput, _, _, _>(registry)?;
    dock_call::<DisarmInput, _, _, _>(registry)
}

/// The indicator-script tier's permissions: what `indicator.script.attach`
/// asks — code an operator puts on the chart, which never touches a
/// position.
pub(crate) const SCRIPT_TIER: &[&str] = &[ANNOTATE_PERMISSION_ID, SCRIPT_PERMISSION_ID];

/// What every `strategy.*` read asks.
pub(crate) const READ_PERMISSIONS: &[&str] =
    &["observe", "observe.indicators", "observe.user_text"];

/// An action in the indicator-script tier, gone with the session.
pub(crate) fn script_action(spec: &CallSpec, stale_input_safety: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        persistence: EffectPersistence::Transient,
        stale_input_safety: Some(stale_input_safety.to_owned()),
        // `dock_call` sets both schemas from the call's own types.
        ..crate::wire::script::script_descriptor(
            spec.id,
            spec.title,
            spec.description,
            Value::Null,
            Value::Null,
        )
    }
}

const ARMED_TWICE: &str = "Each arm adds one instance with a new id, so arming twice rings twice; no retry key can make that safe. Read strategy.script.list, or the strategy.script.armed events after a cursor taken before the call, before retrying one whose answer was lost.";

impl<H: ScriptStrategyHost + ?Sized> Call<H> for ArmInput {
    type Input = Self;
    type Answer = ArmResult;

    const SPEC: CallSpec = CallSpec {
        id: ARM_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "Arm a script strategy",
        description: "Loads a .pine file from the strategy library and arms it as an alarm on one chart's closed bars: an alertcondition that holds rings the alarm when the bar's body meets the placement (a drawn rectangle, a fixed range, a line crossed one way, or anywhere). The title's first word is the side (bull, buy or long; bear, sell or short; any case); a script with a title naming no side, a computed title, no alertcondition, no title naming the side asked for, or a compile error is refused with the reason, compile errors as `line N: <CODE> <message>`. Places no order: an `action` of `order` is refused, and strategy.script.action.set, in the trade tier, switches an armed instance to orders. A runtime error later disarms that instance alone; strategy.script.list shows the reason. Journaled as strategy.script.armed. Lives for the session.",
        permissions: SCRIPT_TIER,
        event: Some(ARMED_EVENT_KIND),
        replay: ReplayTrace::Recorded,
        posture: |spec| script_action(spec, ARMED_TWICE),
    };

    fn run<A: JournalAccess>(
        input: Self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<ArmResult, ControlError> {
        if input.action == Some(FireAction::Order) {
            return Err(invalid(ORDER_ARM));
        }
        let index = tab_index(host, input.tab_id)?;
        let tab_id = host.tab_id_at(index);
        let place = place_of(&input.placement)?;
        let ScriptTab {
            strategies,
            panes,
            library,
        } = host.script_tab(index).ok_or_else(tab_closed)?;
        let pane = match input.pane_id {
            None => panes.first(),
            Some(id) => panes.iter().find(|pane| pane.id == id.get()),
        }
        .ok_or_else(|| invalid("the tab has no such chart; omit pane_id for the flow chart"))?;
        if let ScriptPlacement::Region(drawing) = place {
            let closed = pane.series.closed_slots();
            quantick_drawings::alert_geometry::check_arm(pane.drawings, drawing, closed)
                .map_err(|refusal| invalid(format!("drawing {}: {refusal}", drawing.0)))?;
        }
        let (scripts, read) = library_of(library);
        let source = match scripts.load(&input.script) {
            LoadedScript::Read { text, .. } => text,
            LoadedScript::Unreadable { message, .. } => return Err(invalid(message)),
            LoadedScript::Missing { message } => {
                return Err(invalid(match read {
                    Err(folder) => format!("{message}; {folder}"),
                    Ok(_) => message,
                }));
            }
        };
        let request = ScriptArm {
            pane_id: pane.id,
            script: &input.script,
            source: &source,
            side: input.side.into_engine(),
            place,
            action: SignalAction::Alarm,
        };
        let reading = strategies
            .arm_script(request, |depth| footprint_warmup(&pane.series, depth))
            .map_err(invalid)?;
        Ok(ArmResult {
            tab_id: WireU64::new(tab_id),
            strategy: strategy(&reading),
        })
    }
}

/// Placing orders is the trade tier's; this call sits in the script tier.
const ORDER_ARM: &str = "strategy.script.arm arms alarms only: arm it, then switch it to orders with strategy.script.action.set";

impl<H: ScriptStrategyHost + ?Sized> Call<H> for ListInput {
    type Input = Self;
    type Answer = ListResult;

    const SPEC: CallSpec = CallSpec {
        id: LIST_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "List script strategies",
        description: "Reads the strategy library (the bundled scripts, then the folder's .pine files, sorted: what strategy.script.arm accepts) and every script strategy armed on the tab: its id, chart, script, side, placement, whether it is watching or disarmed and why, its badge line, and what a firing does — an alarm, or an order with its quantity, stop and gain. Changes nothing.",
        permissions: READ_PERMISSIONS,
        event: None,
        replay: ReplayTrace::Recorded,
        posture: read_posture,
    };

    fn run<A: JournalAccess>(
        input: Self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<ListResult, ControlError> {
        let index = tab_index(host, input.tab_id)?;
        let tab_id = host.tab_id_at(index);
        let tab = host.script_tab(index).ok_or_else(tab_closed)?;
        Ok(ListResult {
            tab_id: WireU64::new(tab_id),
            library: listing(tab.library),
            armed: tab
                .strategies
                .script_readings()
                .iter()
                .map(strategy)
                .collect(),
        })
    }
}

const DISARMED_ONCE: &str = "Disarming names one instance by id, and a second disarm of the same id reports it was not there.";

impl<H: ScriptStrategyHost + ?Sized> Call<H> for DisarmInput {
    type Input = Self;
    type Answer = DisarmResult;

    const SPEC: CallSpec = CallSpec {
        id: DISARM_CAPABILITY_ID,
        module: STRATEGY_MODULE_ID,
        title: "Disarm a script strategy",
        description: "Removes one armed script strategy by the id strategy.script.arm answered; it rings no more. An id no instance has answers disarmed: false. Journaled as strategy.script.disarmed.",
        permissions: SCRIPT_TIER,
        // Journaled by `run`, and only when one was removed.
        event: None,
        replay: ReplayTrace::Recorded,
        posture: |spec| script_action(spec, DISARMED_ONCE),
    };

    fn run<A: JournalAccess>(
        input: Self,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<DisarmResult, ControlError> {
        let index = tab_index(host, input.tab_id)?;
        let tab_id = host.tab_id_at(index);
        let tab = host.script_tab(index).ok_or_else(tab_closed)?;
        let removed = tab.strategies.disarm_script(input.id.get());
        let result = DisarmResult {
            tab_id: WireU64::new(tab_id),
            id: input.id,
            disarmed: removed.is_some(),
            strategy: removed.as_ref().map(strategy),
        };
        if result.disarmed {
            let answer = serde_json::to_value(&result).map_err(invalid)?;
            cx.access.record_event(NewEvent::by(
                STRATEGY_MODULE_ID,
                DISARMED_EVENT_KIND,
                cx.actor,
                answer,
            ));
        }
        Ok(result)
    }
}

/// The library over the folder the launch named, scanned for this call: a
/// script dropped in arms without a restart.
///
/// The folder is probed first, so how it read is labelled rather than lost:
/// `Ok(false)` for the default folder that does not exist (no folder, not a
/// fault), `Err` for one that cannot be read — the bundled scripts stand in
/// for both.
fn library_of(folder: Option<&str>) -> (ScriptLibrary, Result<bool, String>) {
    let path = Path::new(folder.unwrap_or(DEFAULT_LIBRARY));
    match std::fs::read_dir(path) {
        Ok(_) => (ScriptLibrary::scan_dir(path), Ok(true)),
        Err(error) if folder.is_none() && error.kind() == std::io::ErrorKind::NotFound => {
            (ScriptLibrary::bundled(path), Ok(false))
        }
        Err(error) => (
            ScriptLibrary::bundled(path),
            Err(format!(
                "the strategy library folder {} cannot be read: {error}",
                path.display()
            )),
        ),
    }
}

fn listing(folder: Option<&str>) -> LibraryListing {
    let (library, read) = library_of(folder);
    LibraryListing {
        folder: folder.unwrap_or(DEFAULT_LIBRARY).to_owned(),
        found: read == Ok(true),
        error: read.err(),
        scripts: (library.entries().iter())
            .map(|entry| entry.name.clone())
            .collect(),
    }
}

/// The pane's last `depth` closed live bars with their footprint POC, oldest
/// first: what the chart's own copy of a script read, so a strategy armed on
/// it judges the next brick against the same history.
pub(crate) fn footprint_warmup(
    series: &PaneSeriesRead<'_>,
    depth: usize,
) -> Vec<(Bar, Option<Decimal>)> {
    let live = series.state.bars().len();
    (live.saturating_sub(depth)..live)
        .map(|index| {
            let closed = series.footprint_bar(index);
            (closed.bar, closed.poc)
        })
        .collect()
}

/// The action `strategy.script.action.set` names, as the session takes it.
///
/// # Errors
/// An order with no quantity, a quantity that is neither a number nor
/// `ticket`, a distance that is not a decimal, or quantity, stop or gain
/// sent with an alarm. Sizes and signs are the session's to refuse.
pub fn signal_action(
    action: FireAction,
    quantity: Option<&str>,
    stop: Option<&str>,
    gain: Option<&str>,
) -> Result<SignalAction, ControlError> {
    if action == FireAction::Alarm {
        return match (quantity, stop, gain) {
            (None, None, None) => Ok(SignalAction::Alarm),
            _ => Err(invalid(
                "quantity, stop and gain size an order; an alarm takes none",
            )),
        };
    }
    let quantity = quantity.ok_or_else(|| {
        invalid(format!(
            "an order needs a quantity: a size or `{TICKET_QUANTITY}`"
        ))
    })?;
    let quantity = if quantity.trim().eq_ignore_ascii_case(TICKET_QUANTITY) {
        OrderQuantity::Ticket
    } else {
        OrderQuantity::Fixed(quantity.trim().parse().map_err(|_| {
            invalid(format!(
                "`quantity` must be a size or `{TICKET_QUANTITY}`, got `{quantity}`"
            ))
        })?)
    };
    let distance = |field, text: Option<&str>| text.map_or(Ok(Decimal::ZERO), |t| price(field, t));
    Ok(SignalAction::Order(SignalOrder {
        quantity,
        stop: distance("stop", stop)?,
        gain: distance("gain", gain)?,
    }))
}

/// `action` as the wire words it: what fires, then an order's quantity,
/// stop and gain.
pub(crate) fn action_words(
    action: SignalAction,
) -> (FireAction, Option<String>, Option<String>, Option<String>) {
    let SignalAction::Order(order) = action else {
        return (FireAction::Alarm, None, None, None);
    };
    let quantity = match order.quantity {
        OrderQuantity::Fixed(count) => count.normalize().to_string(),
        OrderQuantity::Ticket => TICKET_QUANTITY.to_owned(),
    };
    let word = |distance: Decimal| Some(distance.normalize().to_string());
    (
        FireAction::Order,
        Some(quantity),
        word(order.stop),
        word(order.gain),
    )
}

fn price(field: &str, text: &str) -> Result<Decimal, ControlError> {
    text.trim()
        .parse()
        .map_err(|_| invalid(format!("`{field}` must be a decimal price, got `{text}`")))
}

fn place_of(placement: &Placement) -> Result<ScriptPlacement<DrawingId>, ControlError> {
    Ok(match placement {
        Placement::Region { drawing_id } => ScriptPlacement::Region(DrawingId(drawing_id.get())),
        Placement::Range { low, high } => ScriptPlacement::Range {
            a: price("low", low)?,
            b: price("high", high)?,
        },
        Placement::Line { level, direction } => ScriptPlacement::Line {
            level: price("level", level)?,
            rising: *direction == CrossDirection::Up,
        },
        Placement::Anywhere => ScriptPlacement::Anywhere,
    })
}

pub(crate) fn placement(place: ScriptPlacement<DrawingId>) -> Placement {
    match place {
        ScriptPlacement::Region(drawing) => Placement::Region {
            drawing_id: WireU64::new(drawing.0),
        },
        ScriptPlacement::Range { a, b } => Placement::Range {
            low: a.min(b).to_string(),
            high: a.max(b).to_string(),
        },
        ScriptPlacement::Line { level, rising } => Placement::Line {
            level: level.to_string(),
            direction: if rising {
                CrossDirection::Up
            } else {
                CrossDirection::Down
            },
        },
        ScriptPlacement::Anywhere => Placement::Anywhere,
    }
}

fn strategy(reading: &ScriptReading<DrawingId>) -> ScriptStrategy {
    let (action, quantity, stop, gain) = action_words(reading.action);
    ScriptStrategy {
        id: WireU64::new(reading.id),
        pane_id: WireU64::new(reading.pane_id),
        script: reading.script.clone(),
        side: match reading.side {
            Side::Buy => ActionSide::Buy,
            Side::Sell => ActionSide::Sell,
        },
        placement: placement(reading.place),
        state: if reading.disarm_reason.is_none() {
            ScriptStrategyState::Watching
        } else {
            ScriptStrategyState::Disarmed
        },
        disarm_reason: reading.disarm_reason.clone(),
        status: reading.status.clone(),
        alarm_only: action == FireAction::Alarm,
        action,
        quantity,
        stop,
        gain,
    }
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

#[cfg(test)]
#[path = "script_strategy_tests.rs"]
mod tests;
