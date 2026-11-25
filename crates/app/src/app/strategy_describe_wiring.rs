//! The focused chart the strategy conversation reads and saves onto; the
//! conversation's decisions live in `quantick_ui_model::strategy_describe::conversation`.
use super::arrangement_host::ArrangementHost;
use crate::control::strategy_describe_contract::DescribeHost;
use crate::drawings::DrawingId;
use crate::surfaces::Surfaces;
use quantick_control::error::ControlError;
use quantick_session::{
    script_draft, script_strategies::ScriptPlacement, strategies::TabStrategies,
};
use quantick_ui_model::strategy_describe::{
    Context, Effects, HistoricalClose, Intent, MAX_BARS, MAX_CONTEXT_ITEMS, Status,
    conversation::{self, Chart, Provider as _},
};

impl<T: super::control_host::TabsMutPort + ?Sized> DescribeHost for T {
    fn dispatch(&mut self, intent: Intent) -> Result<Status, ControlError> {
        let (surfaces, tabs) = self.describe_owners();
        dispatch(surfaces, tabs, intent)
    }
}

/// The focused chart port, borrowed for one dispatch.
struct Focused<'a> {
    tabs: &'a mut ArrangementHost,
    library: Option<&'a std::path::Path>,
    opened: Option<eframe::egui::ViewportId>,
}

fn focused_tab(tabs: &ArrangementHost) -> u64 {
    let windows = tabs.windows();
    windows
        .window(windows.focused())
        .expect("focused market window")
        .selected()
        .0
}

fn history(
    pane: &crate::pane::ChartPane,
    placement: ScriptPlacement<DrawingId>,
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

impl Focused<'_> {
    fn pinned(&self, context: &Context) -> &crate::pane::ChartPane {
        self.tabs
            .by_id(context.tab)
            .expect("validated pinned tab")
            .focused_pane()
    }
}

impl Chart for Focused<'_> {
    fn capture(&mut self) -> Context {
        let tabs = &*self.tabs;
        let id = focused_tab(tabs);
        let tab = tabs.by_id(id).expect("focused tab");
        let pane = tab.focused_pane();
        let placement = pane
            .drawings
            .selected_id()
            .filter(|id| {
                quantick_drawings::alert_geometry::check_arm(
                    &pane.drawings,
                    *id,
                    pane.closed_slots(),
                )
                .is_ok()
            })
            .map_or(ScriptPlacement::Anywhere, ScriptPlacement::Region);
        let history = history(pane, placement, false);
        let with_poc = history.iter().filter(|s| s.close.poc.is_some()).count();
        Context {
            workspace: tabs.owner(),
            tab: id,
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
    fn shows(&self, context: &Context) -> bool {
        let id = focused_tab(self.tabs);
        let tab = self.tabs.by_id(id).expect("focused tab");
        let pane = tab.focused_pane();
        context.matches_chart(
            self.tabs.owner(),
            id,
            pane.id,
            (&tab.active.0, &tab.active.1),
            &pane.series.bar_config(),
        )
    }
    fn opened(&mut self) {
        let focused = self.tabs.windows().focused();
        self.opened = Some(super::native_windows::viewport(focused));
    }
    fn check_region(&self, context: &Context, drawing: DrawingId) -> Result<(), String> {
        let pane = self.pinned(context);
        quantick_drawings::alert_geometry::check_arm(&pane.drawings, drawing, pane.closed_slots())
            .map_err(|err| err.to_string())
    }
    fn warm_history(&self, context: &Context) -> Vec<HistoricalClose> {
        history(self.pinned(context), context.placement, true)
    }
    fn store(&mut self, id: &str, source: &str) -> Result<(), String> {
        let paths = crate::launch::operator_paths();
        let folder = self.library.unwrap_or_else(|| {
            std::path::Path::new(paths.strategies_dir.as_deref().unwrap_or("strategies"))
        });
        quantick_stores::script_library::ScriptLibrary::open(folder)
            .create_source(id, source)
            .map(drop)
    }
    fn strategies(&mut self, context: &Context) -> &mut TabStrategies<DrawingId> {
        let tab = self.tabs.by_id_mut(context.tab);
        &mut tab.expect("validated pinned tab").market.strategies
    }
}

pub(super) fn dispatch(
    surfaces: &mut Surfaces,
    tabs: &mut ArrangementHost,
    intent: Intent,
) -> Result<Status, ControlError> {
    let surface = &mut surfaces.strategy_describe;
    let library = surface.view.library.as_deref();
    let mut chart = Focused {
        tabs,
        library,
        opened: None,
    };
    let worker = &mut surface.view.worker;
    let status = conversation::dispatch(&mut surface.model_state, worker, &mut chart, intent);
    if let Some(viewport) = chart.opened {
        surface.view.open(viewport);
    }
    status
}

pub(super) fn frame(surfaces: &mut Surfaces, tabs: &mut ArrangementHost, effect: Option<Effects>) {
    let surface = &mut surfaces.strategy_describe;
    if !surface.view.worker.available() || tabs.windows().presented() != tabs.windows().focused() {
        return;
    }
    let command = surface.view.command.take();
    let library = surface.view.library.as_deref();
    let mut chart = Focused {
        tabs,
        library,
        opened: None,
    };
    let worker = &mut surface.view.worker;
    let state = &mut surface.model_state;
    let result = conversation::frame(state, worker, &mut chart, command, effect);
    if let Some(viewport) = chart.opened {
        surface.view.open(viewport);
    }
    if let Err(error) = result {
        let now = std::time::Instant::now();
        crate::surfaces::toast::note(&mut surfaces.toast, error.message, now);
    }
}
