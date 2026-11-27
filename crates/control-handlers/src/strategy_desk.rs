//! `strategy.desk.*`: the strategy desk over every open tab's drawn and
//! scripted instances, and `strategy.script.action.set`, the trade tier's
//! desk command.
//!
//! The tab's strategies decide (`quantick_session::strategy_desk`): the
//! grouping, what each command changes, the detail and its history markers.
//! What is here is parsing, lending each tab with its symbol and its charts'
//! bar types, the chart facts the session asks for — closed bars, drawing
//! regions and names — the library write a save asks for, and the journal.

use crate::wire::strategy_desk::{
    self as wire, ActionSetInput, AssetInput, CHANGED_EVENT_KIND, CommandResult, DeclaredInput,
    DetailResult, FireAction, InstanceId, InstanceInput, Link, ListInput, ListResult, SaveAsInput,
};
use quantick_chart::pane_series::PaneSeriesRead;
use quantick_chart::state::dec_from_f64;
use quantick_control::error::ControlError;
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy, RegistryError};
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_host::actions::ReplayTrace;
use quantick_control_host::authority::STRATEGY_MODULE_ID;
use quantick_control_host::authority::TRADE_PERMISSION_ID;
use quantick_control_host::journal::NewEvent;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use quantick_engine::{Bar, Side};
use quantick_session::script_strategies::SignalAction;
use quantick_session::strategy_desk::{
    Asset, DeskAction, DeskGroup, DeskLink, DeskOutcome, DeskRow, DeskState, DeskTab, InstanceRef,
    StrategyDeskCommand, StrategyDeskView, StrategyDetail,
};
use quantick_stores::script_library::ScriptLibrary;
use quantick_trading::ActionSide;
use rust_decimal::Decimal;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

use crate::JournalAccess;
use crate::call::{Call, CallCx, CallSpec, dock_call};
use crate::dock::ActionDock;
use crate::invalid;
use crate::market_tab::MarketTabHost;
use crate::script_strategy::{
    DEFAULT_LIBRARY, ScriptPane, ScriptStrategyHost, ScriptTab, action_words, footprint_warmup,
    placement, signal_action,
};
use crate::session::SessionTab;
use crate::tabs::{tab_closed, tab_index};

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
    registry.register(wire::list_descriptor(), list::<H, A>)?;
    registry.register(wire::detail_descriptor(), detail::<H, A>)?;
    let handlers: [quantick_control_host::actions::ActionHandler<H, A>; 8] = [
        pause::<H, A>,
        resume::<H, A>,
        remove::<H, A>,
        asset_pause::<H, A>,
        asset_resume::<H, A>,
        arm::<H, A>,
        save_as::<H, A>,
        duplicate::<H, A>,
    ];
    for (descriptor, handler) in wire::command_descriptors().into_iter().zip(handlers) {
        registry.register(descriptor, handler)?;
    }
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

fn list<H: StrategyDeskHost + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let ListInput {} = serde_json::from_value(input.clone()).map_err(invalid)?;
    let groups = groups(host).iter().map(group).collect();
    serde_json::to_value(ListResult { groups }).map_err(invalid)
}

/// Every open tab's instances grouped by asset: what `strategy.desk.list`
/// answers and the Strategies tab paints.
pub fn groups<H: StrategyDeskHost + ?Sized>(host: &mut H) -> Vec<DeskGroup<DrawingId>> {
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

fn detail<H: StrategyDeskHost + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: InstanceInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let detail = instance_detail(host, input.tab_id.get(), instance_ref(input.instance))?;
    let action = match input.instance {
        InstanceId::Script { id } => script_actions(host)
            .get(&(input.tab_id.get(), id.get()))
            .copied(),
        InstanceId::Drawing { .. } => None,
    };
    let (_, quantity, stop, gain) = action_words(action.unwrap_or_default());
    let result = DetailResult {
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
    };
    serde_json::to_value(result).map_err(invalid)
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

macro_rules! instance_command {
    ($name:ident, $variant:ident) => {
        fn $name<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
            host: &mut H,
            access: &mut A,
            actor: &ActorContext,
            input: &Value,
        ) -> Result<Value, ControlError> {
            let input: InstanceInput = serde_json::from_value(input.clone()).map_err(invalid)?;
            let command = StrategyDeskCommand::$variant(instance_ref(input.instance));
            let result = send(host, input.tab_id.get(), command, MAY_START_ORDERS)?;
            journaled(access, actor, &result)
        }
    };
}

instance_command!(pause, Pause);
instance_command!(resume, Resume);
instance_command!(remove, Remove);
instance_command!(arm, Arm);
instance_command!(duplicate, Duplicate);

fn save_as<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: SaveAsInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let command = StrategyDeskCommand::SaveAs {
        instance: instance_ref(input.instance),
        name: input.name,
    };
    let result = send(host, input.tab_id.get(), command, MAY_START_ORDERS)?;
    journaled(access, actor, &result)
}

macro_rules! asset_command {
    ($name:ident, $variant:ident) => {
        fn $name<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
            host: &mut H,
            access: &mut A,
            actor: &ActorContext,
            input: &Value,
        ) -> Result<Value, ControlError> {
            let AssetInput { symbol, bar_type } =
                serde_json::from_value(input.clone()).map_err(invalid)?;
            let command = StrategyDeskCommand::$variant(Asset { symbol, bar_type });
            let result = send(host, 0, command, MAY_START_ORDERS)?;
            journaled(access, actor, &result)
        }
    };
}

asset_command!(asset_pause, PauseAsset);
asset_command!(asset_resume, ResumeAsset);

/// `strategy.script.action.set`: the trade tier's, because it can make a
/// script place paper orders — the desk's own calls never start orders.
/// Setting the same action twice leaves the same action.
impl<H: StrategyDeskHost + ?Sized> Call<H> for ActionSetInput {
    type Answer = CommandResult;

    const SPEC: CallSpec = CallSpec {
        id: wire::ACTION_SET_CAPABILITY_ID,
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
        self,
        host: &mut H,
        _cx: &mut CallCx<'_, A>,
    ) -> Result<CommandResult, ControlError> {
        let action = signal_action(
            self.action,
            self.quantity.as_deref(),
            self.stop.as_deref(),
            self.gain.as_deref(),
        )?;
        let tab_id = host.tab_id_at(tab_index(host, self.tab_id)?);
        let id = InstanceRef::Script(self.id.get());
        let command = StrategyDeskCommand::SetAction { id, action };
        send(host, tab_id, command, TRADE_TIER_MAY_START_ORDERS)
    }
}

/// `strategy.script.action.set` sits in the trade tier: the trader's own.
const TRADE_TIER_MAY_START_ORDERS: bool = true;

/// The call that carries `command`.
const fn capability(command: &StrategyDeskCommand<DrawingId>) -> &'static str {
    match command {
        StrategyDeskCommand::Pause(_) => wire::PAUSE_CAPABILITY_ID,
        StrategyDeskCommand::Resume(_) => wire::RESUME_CAPABILITY_ID,
        StrategyDeskCommand::Remove(_) => wire::REMOVE_CAPABILITY_ID,
        StrategyDeskCommand::PauseAsset(_) => wire::ASSET_PAUSE_CAPABILITY_ID,
        StrategyDeskCommand::ResumeAsset(_) => wire::ASSET_RESUME_CAPABILITY_ID,
        StrategyDeskCommand::Arm(_) => wire::ARM_CAPABILITY_ID,
        StrategyDeskCommand::SaveAs { .. } => wire::SAVE_AS_CAPABILITY_ID,
        StrategyDeskCommand::Duplicate(_) => wire::DUPLICATE_CAPABILITY_ID,
        StrategyDeskCommand::SetAction { .. } => wire::ACTION_SET_CAPABILITY_ID,
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
    command: StrategyDeskCommand<DrawingId>,
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
    command: &StrategyDeskCommand<DrawingId>,
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

fn journaled<A: JournalAccess>(
    access: &mut A,
    actor: &ActorContext,
    result: &CommandResult,
) -> Result<Value, ControlError> {
    let answer = serde_json::to_value(result).map_err(invalid)?;
    access.record_event(NewEvent::by(
        STRATEGY_MODULE_ID,
        CHANGED_EVENT_KIND,
        actor,
        answer.clone(),
    ));
    Ok(answer)
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

fn group(group: &DeskGroup<DrawingId>) -> wire::DeskGroup {
    wire::DeskGroup {
        symbol: group.asset.symbol.clone(),
        bar_type: group.asset.bar_type.clone(),
        paused: group.paused,
        count: count(group.rows.len()),
        rows: group.rows.iter().map(row).collect(),
    }
}

fn row(row: &DeskRow<DrawingId>) -> wire::DeskRow {
    let (state, ended_reason) = match &row.state {
        DeskState::Watching => (wire::DeskState::Watching, None),
        DeskState::Paused => (wire::DeskState::Paused, None),
        DeskState::Ended { reason } => (wire::DeskState::Ended, Some(reason.clone())),
    };
    wire::DeskRow {
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

#[cfg(test)]
#[path = "strategy_desk_tests.rs"]
mod tests;
