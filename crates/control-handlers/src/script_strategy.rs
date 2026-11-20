//! `strategy.script.arm`, `.list` and `.disarm`: Pine scripts armed
//! alarm-only on a tab's charts, read and removed by id.
//!
//! The tab's strategies decide (`quantick_session::script_strategies`):
//! whether a script's titles name sides, what each instance is doing. What
//! is here is parsing, reading the source through the script library the
//! indicators use, the drawing check a region placement needs, the
//! footprint warmup, and the journal. The tab lends itself through
//! [`MarketTab::script_tab`](crate::market_tab::MarketTab::script_tab).

use quantick_chart::pane_series::PaneSeriesRead;
use quantick_control::error::ControlError;
use quantick_control::registry::RegistryError;
use quantick_control::wire::{ActorContext, WireU64};
use quantick_control_host::authority::STRATEGY_MODULE_ID;
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::script_strategy::{
    self as wire, ARMED_EVENT_KIND, ArmInput, ArmResult, CrossDirection, DISARMED_EVENT_KIND,
    DisarmInput, DisarmResult, LibraryListing, ListInput, ListResult, Placement, ScriptStrategy,
    ScriptStrategyState,
};
use quantick_control_schema::trade::ActionSide;
use quantick_drawings::{DrawingId, DrawingTool, Drawings};
use quantick_engine::{Bar, Side};
use quantick_session::script_strategies::{ScriptArm, ScriptPlacement, ScriptReading};
use quantick_session::strategies::TabStrategies;
use quantick_stores::script_library::{LoadedScript, ScriptLibrary};
use rust_decimal::Decimal;
use serde_json::Value;
use std::path::Path;

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
    };
    let reading = strategies
        .arm_script(request, |depth| footprint_warmup(&pane.series, depth))
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
        alarm_only: true,
    }
}

#[cfg(test)]
#[path = "script_strategy_tests.rs"]
mod tests;
