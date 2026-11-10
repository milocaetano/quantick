//! Native shell adapter for the workspace's headless alert owner.

/// Only native clock state lives here. The workspace book/store stay headless.
pub(crate) struct Runtime {
    pub core: quantick_stores::alerts::AlertRuntime,
    pub clock_origin: std::time::Instant,
}

impl Runtime {
    pub(super) fn new(path: std::path::PathBuf, utc_ms: i64) -> Self {
        Self {
            core: quantick_stores::alerts::AlertRuntime::open(path, utc_ms),
            clock_origin: std::time::Instant::now(),
        }
    }

    /// Bound the source census so closed tabs cannot remain active forever.
    pub(super) fn begin_frame(&mut self) {}

    /// Reserved source-absence and idle-expiry tick, even when no prints arrive.
    pub(super) fn end_frame(&mut self) {}
}

pub(crate) fn path() -> std::path::PathBuf {
    crate::store_home::resolve(quantick_stores::alerts::ALERTS_FILE)
}

/// Frame delivery docking; child implementation consumes the bounded events.
pub(super) fn deliver(
    _runtime: &mut Runtime,
    _audio: &mut super::replay_and_history::AlertState,
    _toast: &mut crate::surfaces::ToastSurface,
    _popup: &mut crate::surfaces::AgentPopupSurface,
) {
}

impl quantick_control_handlers::alerts::AlertDefinitionsPort for super::QuantickApp {
    fn alert_resolve_target(
        &mut self,
        selector: quantick_control_schema::alerts::DrawingSelector,
    ) -> Result<
        quantick_alerts::DurableDrawingRef,
        quantick_control_schema::alerts::TargetResolveError,
    > {
        use super::LayoutPort;
        self.layout_adapter().resolve_alert_target(selector)
    }
    fn alert_book(&self) -> &quantick_alerts::AlertBook {
        &self.runtime.alerts.core.book
    }
    fn alert_book_at(&mut self) -> (&mut quantick_alerts::AlertBook, i64) {
        (
            &mut self.runtime.alerts.core.book,
            crate::metrics::wall_clock_ms(),
        )
    }
}
