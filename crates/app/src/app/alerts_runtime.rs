//! Native shell adapter for the workspace's headless alert owner.
use crate::metrics::wall_clock_ms;
use quantick_alerts::sound::Cue;
use quantick_alerts::{AlertError, DurableDrawingRef};
use quantick_control_schema::alerts::{DrawingSelector, TargetResolveError};
use quantick_ui_model::alerts::requests;

/// Only native clock state lives here. The workspace book/store stay headless.
pub(crate) struct Runtime {
    pub core: quantick_stores::alerts::AlertRuntime,
    pub clock_origin: std::time::Instant,
    pub(crate) cache: quantick_stores::alerts::Cache,
}

impl Runtime {
    pub(super) fn new(path: std::path::PathBuf, utc_ms: i64) -> Self {
        Self {
            core: quantick_stores::alerts::AlertRuntime::open(path, utc_ms),
            clock_origin: std::time::Instant::now(),
            cache: quantick_stores::alerts::Cache::default(),
        }
    }

    /// Bound the source census so closed tabs cannot remain active forever.
    pub(super) fn begin_frame(&mut self) {
        self.cache.begin();
    }

    /// Reserved source-absence and idle-expiry tick, even when no prints arrive.
    pub(super) fn end_frame(&mut self) {
        self.cache.end(
            &mut self.core,
            wall_clock_ms(),
            self.clock_origin.elapsed().as_millis() as u64,
        );
    }
}

pub(crate) fn path() -> std::path::PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(quantick_stores::alerts::ALERTS_FILE);
    }
    #[cfg(feature = "scenario-harness")]
    if let Some(layouts) = crate::hooks::captured::var("QUANTICK_LAYOUTS") {
        return std::path::PathBuf::from(layouts)
            .with_file_name(quantick_stores::alerts::ALERTS_FILE);
    }
    crate::store_home::resolve(quantick_stores::alerts::ALERTS_FILE)
}

/// Frame delivery docking: the script alarms every tab labelled are recorded
/// and sound with the strategy alarms. Answers the script alarms
/// delivered this frame, for the journal.
pub(super) fn deliver(
    owners: &mut super::workspace_runtime::WorkspaceRuntime,
    audio: &mut super::replay_and_history::AlertState,
    toast: &mut crate::surfaces::ToastSurface,
    popup: &mut Popup,
) -> Vec<quantick_stores::script_alarms::ScriptAlarmEntry> {
    let (runtime, tabs) = (&mut owners.alerts, &mut owners.tabs);
    let raised = tabs.iter_mut().flat_map(|tab| tab.script_alarms.drain(..));
    let scripts = runtime.core.script_alarms.record(raised, wall_clock_ms());
    let script_cues = scripts.iter().map(|_| Cue::default());
    let strategy_cues = tabs.iter_mut().flat_map(|tab| tab.market.drain_alarms());
    let cues = strategy_cues.chain(script_cues);
    play(runtime, audio, toast, popup, cues);
    scripts
}

type Popup = crate::surfaces::Adapter<
    quantick_ui_model::agent_popup::Model,
    crate::surfaces::agent_popup::View,
>;

/// Child implementation: consumes the bounded events and plays `cues` (the
/// strategy and script alarms) beside the alerts' own.
pub(super) fn play(
    runtime: &mut Runtime,
    audio: &mut super::replay_and_history::AlertState,
    toast: &mut crate::surfaces::ToastSurface,
    popup: &mut Popup,
    cues: impl IntoIterator<Item = Cue>,
) {
    use quantick_alerts::{DeliveryOutcome, sound::AlertSound};
    // Persist the firing decision before attempting an irreversible notification.
    let persisted = runtime.core.events.is_empty() || runtime.core.flush().is_ok();
    let events: Vec<_> = runtime.core.events.drain().collect();
    let alert_cues = events.iter().filter(|_| persisted).filter_map(|event| {
        event
            .spec
            .notification
            .sound
            .as_deref()
            .and_then(AlertSound::from_token)
            .map(|sound| Cue::new(sound, event.spec.notification.sound_seconds))
    });
    if let Some(message) = audio.play_pending(cues.into_iter().chain(alert_cues)) {
        crate::surfaces::toast::note(toast, message, std::time::Instant::now());
    }
    if events.is_empty() && runtime.core.error.is_none() {
        return;
    }
    // The existing sink reports one aggregate result. A failed batch cannot
    // identify which cue failed, so every sound-bearing event is conservative.
    let sound_failed = audio.alert_failure.is_some();
    for event in events {
        let mut outcome = DeliveryOutcome::Failed;
        if persisted {
            let notification = &event.spec.notification;
            outcome = DeliveryOutcome::Delivered;
            if notification.toast {
                crate::surfaces::toast::note(toast, event.spec.notice(), std::time::Instant::now());
            }
            if notification.popup {
                let _ = popup.update(quantick_ui_model::agent_popup::Intent::Show(
                    crate::control::AgentPopup {
                        title: event.spec.name.clone(),
                        message: event.spec.message.clone(),
                        author: "Local alert".to_owned(),
                    },
                ));
            }
            if notification.sound.is_some() && sound_failed {
                outcome = DeliveryOutcome::Failed;
            }
        }
        let _ = runtime.core.book.record_delivery(event.sequence, outcome);
    }
    if let Err(error) = runtime.core.flush() {
        let message = format!("Alert storage: {error:?}");
        crate::surfaces::toast::note(toast, message, std::time::Instant::now());
    }
}

impl quantick_control_handlers::alerts::AlertDefinitionsPort for super::QuantickApp {
    fn alert_open(&mut self) -> Result<(), AlertError> {
        let open = crate::surfaces::alerts::Request::opening();
        apply_alert_request(self.indicator_wiring(), open);
        Ok(())
    }
    fn alert_resolve_target(
        &mut self,
        selector: DrawingSelector,
    ) -> Result<DurableDrawingRef, TargetResolveError> {
        self.layout_adapter().resolve_alert_target(selector)
    }
    fn alert_runtime(&mut self) -> &mut quantick_stores::alerts::AlertRuntime {
        &mut self.runtime.alerts.core
    }
}

pub(crate) fn context(
    tab: &crate::tab::Tab,
    pane: &crate::pane::ChartPane,
    tab_id: u64,
    display_offset_minutes: i32,
) -> crate::surfaces::alerts::Context {
    use quantick_alerts::MarketRef;
    use rust_decimal::prelude::ToPrimitive;
    crate::surfaces::alerts::Context {
        market: MarketRef {
            source: tab.alert_source(),
            symbol: tab.active.1.clone(),
            aggregation: pane.series.bar_config(),
        },
        price: pane
            .series
            .state
            .partial()
            .or_else(|| pane.series.state.bars().last())
            .and_then(|bar| bar.close.to_f64())
            .filter(|price| price.is_finite()),
        utc_ms: wall_clock_ms(),
        display_offset_minutes,
        drawings: pane
            .drawings
            .items()
            .iter()
            .enumerate()
            .filter_map(|(index, drawing)| {
                let kind = drawing.alert_target_kind()?;
                Some(crate::surfaces::alerts::DrawingChoice {
                    selector: DrawingSelector {
                        tab_id,
                        pane_id: pane.id,
                        drawing_id: drawing.id.0,
                    },
                    target: drawing.durable_identity.clone(),
                    kind,
                    name: drawing.display_label(index),
                })
            })
            .collect(),
    }
}

/// The window's side of an alert request: the layout ports a target resolves
/// through, the dock the manager opens on, and the alert owner.
pub(super) struct RequestAdapter<'a> {
    pub(super) layout: super::layout_wiring::LayoutAdapter<'a>,
    pub(super) dock: &'a mut crate::surfaces::dock::DockSurface,
    pub(super) alerts: &'a mut quantick_stores::alerts::AlertRuntime,
}

impl quantick_control_handlers::alerts::AlertDefinitionsPort for RequestAdapter<'_> {
    fn alert_resolve_target(
        &mut self,
        selector: DrawingSelector,
    ) -> Result<DurableDrawingRef, TargetResolveError> {
        self.layout.resolve_alert_target(selector)
    }
    fn alert_runtime(&mut self) -> &mut quantick_stores::alerts::AlertRuntime {
        self.alerts
    }
}

impl requests::RequestHost for RequestAdapter<'_> {
    fn reveal_dock(&mut self) {
        self.dock.reveal_collapsed();
    }
    fn shown_panes(&self) -> Vec<requests::ShownPane> {
        let tabs = &*self.layout.tabs;
        let active = tabs.active_id();
        let mut panes = Vec::new();
        for (tab_index, (tab_id, tab)) in tabs.iter_with_ids().enumerate() {
            let focused = (tab_id == active).then(|| tab.focused_pane().id);
            panes.extend(tab.panes().map(|(pane, _)| requests::ShownPane {
                tab_index,
                pane_id: pane.id,
                key: pane.layout.drawings_key.clone(),
                focused: focused == Some(pane.id),
                drawings: requests::ShownDrawing::of_shown(&pane.drawings),
            }));
        }
        panes
    }
    fn select_copy(&mut self, copy: requests::DrawingCopy) {
        let tabs = &mut *self.layout.tabs;
        tabs.select(copy.tab_index);
        if let Some(tab) = tabs.get_mut(copy.tab_index) {
            tab.aim_drawing_at(copy.pane_id);
            if let Some(pane) = tab.panes_mut().find(|pane| pane.id == copy.pane_id) {
                pane.drawings.select(Some(copy.index));
            }
        }
    }
    fn note(&mut self, message: String) {
        crate::surfaces::toast::note(self.layout.toast, message, std::time::Instant::now());
    }
}

pub(super) fn apply_alert_request(
    wiring: super::indicator_wiring::IndicatorWiring<'_>,
    request: crate::surfaces::alerts::Request,
) {
    let (mut host, model) = wiring.into_alerts();
    requests::apply(model, &mut host, request);
}

/// After the dock: another open dock tab closes the manager, and the manager
/// reserves its space after the permanent right rail, before chart layout.
pub(super) fn after_dock(
    runtime: &mut super::workspace_runtime::WorkspaceRuntime,
    surfaces: &mut crate::surfaces::Surfaces,
    ctx: &eframe::egui::Context,
) {
    if surfaces.dock.tab().is_some() {
        surfaces.alerts.close_manager();
    }
    if !surfaces.alerts.is_open() {
        return;
    }
    let tab = &runtime.tabs[runtime.tabs.active_index()];
    let context = context(
        tab,
        tab.pane(tab.focused_side()),
        runtime.tabs.active_id(),
        runtime.tz.minutes(),
    );
    let response = crate::surfaces::alerts::draw_manager_panel(
        &mut surfaces.alerts,
        ctx,
        &runtime.alerts.core.book,
        runtime.alerts.core.error.as_ref(),
        Some(&context),
    );
    let late = quantick_ui_model::late_effects::LateEffects::fold(response.model_effects);
    if let Some(command) = late.alerts.and_then(|mut request| request.command.take()) {
        let model = &mut surfaces.alerts.model_state;
        requests::settle_command(model, &mut runtime.alerts.core, command, wall_clock_ms());
    }
}

/// The context bar's Create alert: open the editor on the resolved drawing.
/// The wiring is borrowed only when the ask carries a target.
pub(super) fn open_drawing_target<'a>(
    ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
    wiring: impl FnOnce() -> super::indicator_wiring::IndicatorWiring<'a>,
) {
    let Some(selector) = ask
        .readbacks
        .as_mut()
        .and_then(|back| back.alert_target.take())
    else {
        return;
    };
    let (mut host, model) = wiring().into_alerts();
    requests::open_drawing(model, &mut host, selector);
}
