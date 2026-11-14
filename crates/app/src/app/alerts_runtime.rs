//! Native shell adapter for the workspace's headless alert owner.
use crate::metrics::wall_clock_ms;
use quantick_alerts::{
    AlertCommand, AlertError, AlertEvent, AlertView, CommandResult, DurableDrawingRef,
};
use quantick_control_schema::alerts::{DrawingSelector, TargetResolveError};

/// Only native clock state lives here. The workspace book/store stay headless.
pub(crate) struct Runtime {
    pub core: quantick_stores::alerts::AlertRuntime,
    pub clock_origin: std::time::Instant,
    pub(crate) cache: crate::tab::alerts::Cache,
}

impl Runtime {
    pub(super) fn new(path: std::path::PathBuf, utc_ms: i64) -> Self {
        Self {
            core: quantick_stores::alerts::AlertRuntime::open(path, utc_ms),
            clock_origin: std::time::Instant::now(),
            cache: crate::tab::alerts::Cache::default(),
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

/// Frame delivery docking; child implementation consumes the bounded events.
pub(super) fn deliver(
    runtime: &mut Runtime,
    audio: &mut super::replay_and_history::AlertState,
    toast: &mut crate::surfaces::ToastSurface,
    popup: &mut crate::surfaces::Adapter<
        quantick_ui_model::agent_popup::Model,
        crate::surfaces::agent_popup::View,
    >,
    strategy_cues: impl IntoIterator<Item = quantick_alerts::sound::Cue>,
) {
    use quantick_alerts::{
        DeliveryOutcome,
        sound::{AlertSound, Cue},
    };
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
    if let Some(message) = audio.play_pending(strategy_cues.into_iter().chain(alert_cues)) {
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
        self.apply_menu_command(super::menu_bar::MenuCommand::OpenAlerts);
        Ok(())
    }
    fn alert_resolve_target(
        &mut self,
        selector: DrawingSelector,
    ) -> Result<DurableDrawingRef, TargetResolveError> {
        use super::LayoutPort;
        self.layout_adapter().resolve_alert_target(selector)
    }
    fn alert_command(&mut self, command: AlertCommand) -> Result<CommandResult, AlertError> {
        self.runtime.alerts.core.command(command, wall_clock_ms())
    }
    fn alert_book(&self) -> &quantick_alerts::AlertBook {
        &self.runtime.alerts.core.book
    }
    fn alert_views(&self) -> Result<Vec<AlertView>, AlertError> {
        self.runtime.alerts.core.views()
    }
    fn alert_history(&self, after: u64, limit: usize) -> Result<Vec<AlertEvent>, AlertError> {
        self.runtime.alerts.core.history(after, limit)
    }
    fn alert_book_at(&mut self) -> (&mut quantick_alerts::AlertBook, i64) {
        (&mut self.runtime.alerts.core.book, wall_clock_ms())
    }
}

pub(crate) fn context(
    tab: &crate::tab::Tab,
    pane: &crate::pane::ChartPane,
    tab_id: u64,
    display_offset_minutes: i32,
) -> crate::surfaces::alerts::Context {
    use quantick_alerts::{MarketRef, SourceContext};
    use rust_decimal::prelude::ToPrimitive;
    crate::surfaces::alerts::Context {
        market: MarketRef {
            source: tab.link.replay.as_ref().map_or_else(
                || SourceContext::Live {
                    feed: tab.active.0.clone(),
                },
                |replay| SourceContext::Replay {
                    recording: replay.session.path.to_string_lossy().into_owned(),
                },
            ),
            symbol: tab.active.1.clone(),
            aggregation: pane.state.spec().to_config_string(),
        },
        price: pane
            .state
            .partial()
            .or_else(|| pane.state.bars().last())
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

pub(super) fn apply_alert_request(
    app: &mut super::QuantickApp,
    mut request: crate::surfaces::alerts::Request,
) {
    use super::LayoutPort;
    if request.open {
        let dock = &mut app.surfaces.dock;
        if !dock.visible() {
            dock.toggle_visible();
        }
        let _ = dock.update(quantick_ui_model::dock::Intent::Collapse);
    }
    if let Some(selector) = request.resolve.take() {
        let outcome = app
            .layout_adapter()
            .resolve_alert_target(selector)
            .map_err(|error| format!("{error:?}"));
        app.surfaces.alerts.settle_target(outcome);
    }
    if let Some(target) = request.locate.take() {
        let mut matches = Vec::new();
        let mut ambiguous = false;
        for (tab_index, (tab_id, tab)) in app.runtime.tabs.iter_with_ids().enumerate() {
            for (pane, _) in tab.panes() {
                let mut count = 0;
                for (index, drawing) in pane.drawings.items().iter().enumerate() {
                    if drawing.durable_identity.as_ref() == Some(&target) {
                        count += 1;
                        let known_mirror = pane.layout.drawings_key.as_ref().is_some_and(|key| {
                            let key = if drawing.all_timeframes() {
                                quantick_workspace::market_settings::DrawingKey::all_timeframes(
                                    key.market.clone(),
                                )
                            } else {
                                key.clone()
                            };
                            quantick_workspace::drawing_identity::belongs_to(&target, &key)
                        });
                        ambiguous |= !known_mirror;
                        let focused = tab_id == app.runtime.tabs.active_id()
                            && pane.id == tab.focused_pane().id;
                        matches.push((!focused, tab_index, pane.id, index));
                    }
                }
                ambiguous |= count > 1;
            }
        }
        matches.sort_by_key(|entry| entry.0);
        if let Some((_, tab_index, pane_id, index)) = matches.first().filter(|_| !ambiguous) {
            app.runtime.tabs.select(*tab_index);
            if let Some(tab) = app.runtime.tabs.get_mut(*tab_index) {
                tab.aim_drawing_at(*pane_id);
                if let Some(pane) = tab.panes_mut().find(|pane| pane.id == *pane_id) {
                    pane.drawings.select(Some(*index));
                }
            }
        } else {
            crate::surfaces::toast::note(
                &mut app.surfaces.toast,
                if matches.is_empty() {
                    "Alert target is missing from the open workspace"
                } else {
                    "Alert target identity is ambiguous"
                },
                std::time::Instant::now(),
            );
        }
    }
    let _ =
        (app.surfaces.alerts).update(quantick_ui_model::alerts::Intent::Apply(Box::new(request)));
}

/// After the dock: its bell opens the manager, another open dock tab closes
/// it, and the manager takes its space before chart layout.
pub(super) fn after_dock(app: &mut super::QuantickApp, ctx: &eframe::egui::Context, bell: bool) {
    if bell {
        app.apply_menu_command(super::menu_bar::MenuCommand::OpenAlerts);
    }
    if app.surfaces.dock.tab().is_some() {
        app.surfaces.alerts.close_manager();
    }
    draw_manager_panel(app, ctx);
}

/// The context bar's Create alert: open the editor on the resolved drawing.
pub(super) fn open_drawing_target(
    app: &mut super::QuantickApp,
    ask: &mut crate::surfaces::drawing_chrome::DrawingChromeAsk,
    now: std::time::Instant,
) {
    use super::LayoutPort;
    let Some(selector) = ask
        .readbacks
        .as_mut()
        .and_then(|back| back.alert_target.take())
    else {
        return;
    };
    match app.layout_adapter().resolve_alert_target(selector) {
        Ok(target) => app.surfaces.alerts.open_drawing(Some(target)),
        Err(error) => crate::surfaces::toast::note(
            &mut app.surfaces.toast,
            format!("Alert target unavailable: {error:?}"),
            now,
        ),
    }
}

/// Reserve manager space after the permanent right rail, before chart layout.
fn draw_manager_panel(app: &mut super::QuantickApp, ctx: &eframe::egui::Context) {
    if !app.surfaces.alerts.is_open() {
        return;
    }
    let tab = &app.runtime.tabs[app.runtime.tabs.active_index()];
    let context = context(
        tab,
        tab.pane(tab.focused_side()),
        app.runtime.tabs.active_id(),
        app.runtime.tz.minutes(),
    );
    let mut response = crate::surfaces::alerts::draw_manager_panel(
        &mut app.surfaces.alerts,
        ctx,
        &app.runtime.alerts.core.book,
        app.runtime.alerts.core.error.as_ref(),
        Some(&context),
    );
    if let Some(request) = response.alerts.as_mut()
        && let Some(command) = request.command.take()
    {
        let outcome = app
            .runtime
            .alerts
            .core
            .command(command, wall_clock_ms())
            .map(|_| ())
            .map_err(|error| format!("{error:?}"));
        app.surfaces.alerts.settle_command(outcome);
    }
}
