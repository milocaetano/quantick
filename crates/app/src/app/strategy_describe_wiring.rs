//! Capture the focused chart and apply the conversation owner's admitted effects.
use super::arrangement_host::ArrangementHost;
use crate::control::strategy_describe_contract::DescribeHost;
use crate::surfaces::Surfaces;
use quantick_control::error::ControlError;
use quantick_session::{
    script_draft::{self, DraftInput},
    script_strategies::ScriptPlacement,
};
use quantick_ui_model::strategy_describe::{
    self as model, Command, Context, Draft, Effects, HistoricalClose, Intent, MAX_BARS,
    MAX_CONTEXT_ITEMS, Phase, Status,
};

impl<T: super::control_host::TabsMutPort + ?Sized> DescribeHost for T {
    fn dispatch(&mut self, intent: Intent) -> Result<Status, ControlError> {
        let (surfaces, tabs) = self.describe_owners();
        dispatch(surfaces, tabs, intent)
    }
}

fn current(tabs: &ArrangementHost, context: &Context) -> bool {
    let tab = &tabs[tabs.active_index()];
    let pane = tab.focused_pane();
    context.matches_chart(
        tabs.owner(),
        tabs.active_id(),
        pane.id,
        (&tab.active.0, &tab.active.1),
        &pane.series.bar_config(),
    )
}
fn history(
    pane: &crate::pane::ChartPane,
    placement: ScriptPlacement<crate::drawings::DrawingId>,
    live_only: bool,
) -> Vec<HistoricalClose> {
    let series = pane.series_read();
    let first_slot = if live_only { series.seam_slot() } else { 0 };
    script_draft::read_history(&series, first_slot, MAX_BARS, |slot| {
        placement
            .drawing()
            .and_then(|id| crate::pane::strategies::drawing_region(&pane.drawings, id, slot))
    })
}
fn capture(tabs: &ArrangementHost) -> Context {
    let tab = &tabs[tabs.active_index()];
    let pane = tab.focused_pane();
    let placement = pane
        .drawings
        .selected_id()
        .filter(|id| {
            quantick_drawings::alert_geometry::check_arm(&pane.drawings, *id, pane.closed_slots())
                .is_ok()
        })
        .map_or(ScriptPlacement::Anywhere, ScriptPlacement::Region);
    let history = history(pane, placement, false);
    let with_poc = history.iter().filter(|s| s.close.poc.is_some()).count();
    Context {
        workspace: tabs.owner(),
        tab: tabs.active_id(),
        pane: pane.id,
        feed: tab.active.0.clone(),
        symbol: tab.active.1.clone(),
        bars: pane.series.bar_config(),
        indicators: pane
            .indicators
            .all()
            .iter()
            .take(MAX_CONTEXT_ITEMS)
            .map(|v| v.label.chars().take(128).collect())
            .collect(),
        drawings: pane
            .drawings
            .items()
            .iter()
            .take(MAX_CONTEXT_ITEMS)
            .map(|d| format!("{} #{}", d.tool.name(), d.id.0))
            .collect(),
        availability: vec![format!(
            "Footprint POC on {with_poc} of {} captured closes",
            history.len()
        )],
        placement,
        history,
    }
}

pub(super) fn dispatch(
    surfaces: &mut Surfaces,
    tabs: &mut ArrangementHost,
    intent: Intent,
) -> Result<Status, ControlError> {
    let surface = &mut surfaces.strategy_describe;
    if !surface.view.worker.available() {
        if matches!(intent, Intent::Command(Command::Read)) {
            return Ok(surface.model_state.status.clone());
        }
        return Err(quantick_control_schema::strategy_describe::unavailable_error());
    }
    let state = &mut surface.model_state;
    if state.status.phase == Phase::Unavailable {
        state.status.phase = Phase::Closed;
    }
    if state
        .context()
        .is_some_and(|context| !current(tabs, context))
    {
        surface.view.worker.invalidate();
        model::update(state, Intent::TargetChanged);
    }
    if let Intent::Command(command) = &intent {
        state
            .admit(command)
            .map_err(ControlError::invalid_request)?;
        if matches!(command, Command::Open | Command::Cancel { .. }) {
            surface.view.worker.invalidate();
        }
    }
    let intent = if matches!(intent, Intent::Command(Command::Open)) {
        surface.view.open();
        Intent::Opened(capture(tabs).into())
    } else {
        intent
    };
    let response = surfaces.update(crate::surfaces::HostIntent::StrategyDescribe(Box::new(
        intent,
    )));
    let mut effect = response
        .model_effects
        .into_iter()
        .find_map(|effect| match effect {
            quantick_ui_model::HostEffect::StrategyDescribe(effect) => Some(effect),
            _ => None,
        })
        .unwrap_or_default();
    let surface = &mut surfaces.strategy_describe;
    let state = &mut surface.model_state;
    loop {
        let intent = match effect {
            Effects::Request(request) => {
                let revision = request.revision;
                match surface.view.worker.submit(request) {
                    Ok(()) => break,
                    Err(message) => Intent::Replied {
                        revision,
                        reply: Err(message),
                    },
                }
            }
            Effects::Preview {
                revision,
                context,
                source,
                summary,
            } => {
                let result = script_draft::preview(DraftInput {
                    id: "preview.pine",
                    source: &source,
                    placement: context.placement,
                    history: &context.history,
                })
                .map(|markers| Draft {
                    source,
                    summary,
                    markers,
                });
                Intent::Previewed { revision, result }
            }
            Effects::Save { revision, id } => {
                let context = state.context().expect("save requires pinned context");
                let source = &state
                    .status
                    .draft
                    .as_ref()
                    .expect("save requires preview")
                    .source;
                let tab = tabs.runtime_mut(tabs.active_index());
                let result = (|| {
                    let pane = tab.focused_pane();
                    if let Some(id) = context.placement.drawing() {
                        quantick_drawings::alert_geometry::check_arm(
                            &pane.drawings,
                            id,
                            pane.closed_slots(),
                        )
                        .map_err(|err| err.to_string())?;
                    }
                    let warm = history(pane, context.placement, true);
                    let prepared = script_draft::prepare(DraftInput {
                        id: &id,
                        source,
                        placement: context.placement,
                        history: &warm,
                    })
                    .map_err(|errors| {
                        errors.first().map_or_else(
                            || "Warmup failed".into(),
                            |d| format!("Warmup failed: {} {}", d.code, d.message),
                        )
                    })?;
                    let paths = crate::launch::operator_paths();
                    let folder = surface.view.library.as_deref().unwrap_or_else(|| {
                        std::path::Path::new(
                            paths.strategies_dir.as_deref().unwrap_or("strategies"),
                        )
                    });
                    quantick_stores::script_library::ScriptLibrary::open(folder)
                        .create_source(&id, source)?;
                    script_draft::commit(&mut tab.market.strategies, context.pane, prepared);
                    Ok(id)
                })();
                Intent::Saved { revision, result }
            }
            Effects::None | Effects::Dispatch(_) => break,
        };
        effect = model::update(state, intent);
    }
    Ok(state.status.clone())
}

pub(super) fn frame(surfaces: &mut Surfaces, tabs: &mut ArrangementHost, effect: Option<Effects>) {
    let surface = &mut surfaces.strategy_describe;
    if !surface.view.worker.available() {
        return;
    }
    if surface
        .model_state
        .context()
        .is_some_and(|context| !current(tabs, context))
    {
        surface.view.worker.invalidate();
        model::update(&mut surface.model_state, Intent::TargetChanged);
    }
    let command = surface.view.command.take().or(match effect {
        Some(Effects::Dispatch(c)) => Some(c),
        _ => None,
    });
    if let Some(command) = command {
        let _ = dispatch(surfaces, tabs, Intent::Command(command));
    }
    if let Some((revision, reply)) = surfaces.strategy_describe.view.worker.poll() {
        let _ = dispatch(surfaces, tabs, Intent::Replied { revision, reply });
    }
}
