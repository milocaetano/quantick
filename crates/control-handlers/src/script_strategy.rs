//! `strategy.script.arm`, `.list` and `.disarm`: Pine scripts armed
//! alarm-only on a tab's charts, read and removed by id.
//!
//! The tab's strategies decide (`quantick_session::script_strategies`): which
//! library file loads, whether its titles name sides, what each instance is
//! doing. What is here is parsing, the drawing check a region placement
//! needs, and the journal. The tab lends itself through
//! [`MarketTab::script_tab`](crate::market_tab::MarketTab::script_tab).

use quantick_chart::pane_series::PaneSeriesRead;
use quantick_control::error::ControlError;
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_host::authority::STRATEGY_MODULE_ID;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::script_strategy::{
    self as wire, ARMED_EVENT_KIND, ArmInput, ArmResult, CrossDirection, DISARMED_EVENT_KIND,
    DisarmInput, DisarmResult, LibraryListing, ListInput, ListResult, MAX_LIBRARY_NAMES, Placement,
    ScriptStrategy, ScriptStrategyState,
};
use quantick_control_schema::trade::ActionSide;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use quantick_engine::Side;
use quantick_session::script_strategies::{
    ScriptArm, ScriptPlace, ScriptReading, library_folder, library_scripts,
};
use quantick_session::strategies::TabStrategies;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::JournalAccess;
use crate::dock::ActionDock;
use crate::invalid;
use crate::market_tab::{LentTab, MarketTabHost};
use crate::tabs::{TabDirectory, tab_closed, tab_index};

/// One chart of a lent tab: the bars a script warms on, and the drawings a
/// region placement must name.
pub struct ScriptPane<'a> {
    pub id: u64,
    pub series: PaneSeriesRead<'a>,
    pub drawings: &'a Drawings<DrawingTool>,
}

/// One open tab, lent for one call: its strategies, its charts with the
/// flow chart first, and the library folder the launch named (`None`: the
/// default).
pub struct ScriptTab<'a> {
    pub strategies: &'a mut TabStrategies<DrawingId>,
    pub panes: Vec<ScriptPane<'a>>,
    pub library: Option<&'a str>,
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
    registry.register(wire::arm_descriptor(), arm::<H, A>)?;
    registry.register(wire::list_descriptor(), list::<H, A>)?;
    registry.register(wire::disarm_descriptor(), disarm::<H, A>)
}

fn arm<H: ScriptStrategyHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ArmInput = serde_json::from_value(input.clone()).map_err(invalid)?;
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
    if let ScriptPlace::Region(drawing) = place {
        let closed = pane.series.closed_slots();
        quantick_drawings::alert_geometry::check_arm(pane.drawings, drawing, closed)
            .map_err(|refusal| invalid(format!("drawing {}: {refusal}", drawing.0)))?;
    }
    let request = ScriptArm {
        pane_id: pane.id,
        library,
        script: &input.script,
        side: input.side.into_engine(),
        place,
    };
    let reading = strategies
        .arm_script(request, |depth| pane.series.warmup_bars(depth))
        .map_err(invalid)?;
    let result = ArmResult {
        tab_id: WireU64::new(tab_id),
        strategy: strategy(&reading),
    };
    let answer = serde_json::to_value(&result).map_err(invalid)?;
    access.record_event(NewEvent::by(
        STRATEGY_MODULE_ID,
        ARMED_EVENT_KIND,
        actor,
        answer.clone(),
    ));
    Ok(answer)
}

fn list<H: ScriptStrategyHost + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ListInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let index = tab_index(host, input.tab_id)?;
    let tab_id = host.tab_id_at(index);
    let tab = host.script_tab(index).ok_or_else(tab_closed)?;
    let result = ListResult {
        tab_id: WireU64::new(tab_id),
        library: listing(tab.library),
        armed: tab
            .strategies
            .script_readings()
            .iter()
            .map(strategy)
            .collect(),
    };
    serde_json::to_value(result).map_err(invalid)
}

fn disarm<H: ScriptStrategyHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: DisarmInput = serde_json::from_value(input.clone()).map_err(invalid)?;
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
    let answer = serde_json::to_value(&result).map_err(invalid)?;
    if result.disarmed {
        access.record_event(NewEvent::by(
            STRATEGY_MODULE_ID,
            DISARMED_EVENT_KIND,
            actor,
            answer.clone(),
        ));
    }
    Ok(answer)
}

/// The library as one call found it, bounded.
fn listing(folder: Option<&str>) -> LibraryListing {
    let (scripts, error) = match library_scripts(folder) {
        Ok(names) => (names, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let omitted = scripts.len().saturating_sub(MAX_LIBRARY_NAMES);
    LibraryListing {
        folder: library_folder(folder),
        scripts: scripts.into_iter().take(MAX_LIBRARY_NAMES).collect(),
        omitted: WireU64::new(u64::try_from(omitted).unwrap_or(u64::MAX)),
        error,
    }
}

fn price(field: &str, text: &str) -> Result<Decimal, ControlError> {
    text.trim()
        .parse()
        .map_err(|_| invalid(format!("`{field}` must be a decimal price, got `{text}`")))
}

fn place_of(placement: &Placement) -> Result<ScriptPlace<DrawingId>, ControlError> {
    Ok(match placement {
        Placement::Region { drawing_id } => ScriptPlace::Region(DrawingId(drawing_id.get())),
        Placement::Range { low, high } => ScriptPlace::Range {
            a: price("low", low)?,
            b: price("high", high)?,
        },
        Placement::Line { level, direction } => ScriptPlace::Line {
            level: price("level", level)?,
            rising: *direction == CrossDirection::Up,
        },
        Placement::Anywhere => ScriptPlace::Anywhere,
    })
}

fn placement(place: ScriptPlace<DrawingId>) -> Placement {
    match place {
        ScriptPlace::Region(drawing) => Placement::Region {
            drawing_id: WireU64::new(drawing.0),
        },
        ScriptPlace::Range { a, b } => Placement::Range {
            low: a.min(b).to_string(),
            high: a.max(b).to_string(),
        },
        ScriptPlace::Line { level, rising } => Placement::Line {
            level: level.to_string(),
            direction: if rising {
                CrossDirection::Up
            } else {
                CrossDirection::Down
            },
        },
        ScriptPlace::Anywhere => Placement::Anywhere,
    }
}

fn strategy(reading: &ScriptReading<DrawingId>) -> ScriptStrategy {
    ScriptStrategy {
        id: WireU64::new(reading.id),
        pane_id: WireU64::new(reading.pane_id),
        script: reading.script.clone(),
        side: match reading.side {
            Side::Buy => ActionSide::Buy,
            Side::Sell => ActionSide::Sell,
        },
        placement: placement(reading.place),
        state: if reading.watching {
            ScriptStrategyState::Watching
        } else {
            ScriptStrategyState::Disarmed
        },
        disarm_reason: reading.disarm_reason.clone(),
        status: reading.status.clone(),
        alarm_only: true,
    }
}

#[cfg(test)]
#[path = "script_strategy_tests.rs"]
mod tests;
