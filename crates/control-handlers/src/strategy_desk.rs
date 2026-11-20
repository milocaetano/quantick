//! `strategy.desk.*`: the strategy desk over every open tab's drawn and
//! scripted instances.
//!
//! The tab's strategies decide (`quantick_session::strategy_desk`): the
//! grouping, what each command changes, the detail and its history markers.
//! What is here is parsing, lending each tab with its symbol and its charts'
//! bar types, the chart facts the session asks for — closed bars, drawing
//! regions and names — the library write a save asks for, and the journal.

use quantick_chart::pane_series::PaneSeriesRead;
use quantick_chart::state::dec_from_f64;
use quantick_control::error::ControlError;
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_host::authority::STRATEGY_MODULE_ID;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::strategy_desk::{
    self as wire, AssetInput, CHANGED_EVENT_KIND, CommandResult, DeclaredInput, DetailResult,
    FireAction, InstanceId, InstanceInput, Link, ListInput, ListResult, SaveAsInput,
};
use quantick_control_schema::trade::ActionSide;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use quantick_engine::{Bar, Side};
use quantick_session::strategy_desk::{
    Asset, DeskAction, DeskGroup, DeskLink, DeskOutcome, DeskRow, DeskState, DeskTab, InstanceRef,
    StrategyDeskCommand, StrategyDeskView,
};
use quantick_stores::script_library::ScriptLibrary;
use rust_decimal::Decimal;
use serde_json::Value;
use std::path::Path;

use crate::JournalAccess;
use crate::dock::ActionDock;
use crate::invalid;
use crate::market_tab::MarketTabHost;
use crate::script_strategy::{
    DEFAULT_LIBRARY, ScriptPane, ScriptStrategyHost, ScriptTab, footprint_warmup, placement,
};
use crate::tabs::{tab_closed, tab_index};

/// The tabs as the desk reaches them: each lent as for `strategy.script.*`,
/// beside how many there are and the symbol each shows.
pub trait StrategyDeskHost: ScriptStrategyHost {
    fn desk_tab_count(&self) -> usize;
    /// The symbol the tab at `index` shows.
    fn desk_symbol(&self, index: usize) -> Option<String>;
}

/// A window's tabs answer through their market.
impl<T: MarketTabHost + ?Sized> StrategyDeskHost for T {
    fn desk_tab_count(&self) -> usize {
        self.tab_count()
    }
    fn desk_symbol(&self, index: usize) -> Option<String> {
        self.open_tab(index)
            .map(|tab| crate::market_tab::MarketTab::market(tab).symbol)
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
    Ok(())
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
    let groups = view.groups().iter().map(group).collect();
    serde_json::to_value(ListResult { groups }).map_err(invalid)
}

fn detail<H: StrategyDeskHost + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: InstanceInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let index = tab_index(host, input.tab_id)?;
    let tab_id = host.tab_id_at(index);
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
    let detail = strategies
        .detail(
            &tab,
            instance_ref(input.instance),
            |pane_id, drawing| drawing_name(&panes, pane_id, drawing),
            |pane_id| pane(&panes, pane_id).map_or_else(Vec::new, |p| closed_bars(&p.series)),
            |pane_id, drawing, slot| region(pane(&panes, pane_id)?.drawings, drawing, slot),
        )
        .ok_or_else(|| invalid(NO_SUCH))?;
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
        code: detail.code,
        history_slots: (detail.history.into_iter())
            .map(|slot| u64::try_from(slot).unwrap_or(u64::MAX))
            .collect(),
    };
    serde_json::to_value(result).map_err(invalid)
}

/// These calls sit in the indicator-script tier, below the trade tier, so
/// none of them lets a drawn strategy start placing paper orders again.
const MAY_START_ORDERS: bool = false;

const NO_SUCH: &str = "no armed strategy on this tab has that id; read strategy.desk.list again";

macro_rules! instance_command {
    ($name:ident, $capability:expr, $variant:ident) => {
        fn $name<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
            host: &mut H,
            access: &mut A,
            actor: &ActorContext,
            input: &Value,
        ) -> Result<Value, ControlError> {
            let input: InstanceInput = serde_json::from_value(input.clone()).map_err(invalid)?;
            let id = instance_ref(input.instance);
            let command = StrategyDeskCommand::$variant(id);
            on_tab(host, access, actor, $capability, input.tab_id, command)
        }
    };
}

instance_command!(pause, wire::PAUSE_CAPABILITY_ID, Pause);
instance_command!(resume, wire::RESUME_CAPABILITY_ID, Resume);
instance_command!(remove, wire::REMOVE_CAPABILITY_ID, Remove);
instance_command!(arm, wire::ARM_CAPABILITY_ID, Arm);
instance_command!(duplicate, wire::DUPLICATE_CAPABILITY_ID, Duplicate);

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
    let capability = wire::SAVE_AS_CAPABILITY_ID;
    on_tab(host, access, actor, capability, input.tab_id, command)
}

fn asset_pause<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let capability = wire::ASSET_PAUSE_CAPABILITY_ID;
    on_asset(
        host,
        access,
        actor,
        capability,
        input,
        StrategyDeskCommand::PauseAsset,
    )
}

fn asset_resume<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let capability = wire::ASSET_RESUME_CAPABILITY_ID;
    on_asset(
        host,
        access,
        actor,
        capability,
        input,
        StrategyDeskCommand::ResumeAsset,
    )
}

/// Send `command` to one tab's instances, write what a save hands back, and
/// journal the answer.
fn on_tab<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    capability: &str,
    tab_id: Option<WireU64>,
    command: StrategyDeskCommand<DrawingId>,
) -> Result<Value, ControlError> {
    let index = tab_index(host, tab_id)?;
    let tab_id = host.tab_id_at(index);
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
    let outcome = strategies
        .execute_desk(&tab, command, MAY_START_ORDERS, warmup)
        .map_err(invalid)?;
    let mut result = CommandResult {
        capability: capability.to_owned(),
        tab_id: Some(WireU64::new(tab_id)),
        changed: 0,
        created: None,
        saved_as: None,
    };
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
    journaled(access, actor, &result)
}

/// Send an asset command to every open tab's instances on that asset.
fn on_asset<H: StrategyDeskHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    capability: &str,
    input: &Value,
    command: fn(Asset) -> StrategyDeskCommand<DrawingId>,
) -> Result<Value, ControlError> {
    let AssetInput { symbol, bar_type } = serde_json::from_value(input.clone()).map_err(invalid)?;
    let asset = Asset { symbol, bar_type };
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
        let outcome =
            strategies.execute_desk(&tab, command(asset.clone()), MAY_START_ORDERS, |_, _| {
                Vec::new()
            });
        if let Ok(DeskOutcome::Changed(count)) = outcome {
            changed += count;
        }
    }
    let result = CommandResult {
        capability: capability.to_owned(),
        tab_id: None,
        changed: count(changed),
        created: None,
        saved_as: None,
    };
    journaled(access, actor, &result)
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
