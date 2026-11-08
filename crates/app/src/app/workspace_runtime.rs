//! Concrete workspace state and the native presentation adapter.
//!
//! The process keeps this payload while a view changes presentation. Existing
//! stores remain I/O adapters, including legacy installation-local fields.
//! Indicator settings retain their captured owner target with the workspace;
//! splitting that dialog from its bookkeeping is deferred until two views exist.
use eframe::egui;
use quantick_workspace::lifetime::{
    LifetimeError, PresentationId, ResourceError, ViewId, WorkspaceId, WorkspaceLifetime,
    WorkspaceView,
};

pub(super) struct WorkspaceRuntime {
    pub(super) chart_theme: quantick_chart::style::ChartThemeSync,
    pub(super) appearance: quantick_stores::appearance::AppearanceStore,
    pub(super) lifetime: WorkspaceLifetime,
    pub(super) tabs: super::ArrangementHost,
    pub(super) indicators: super::indicator_manager::IndicatorState,
    pub(super) footprint_config: crate::footprint_config::FootprintConfig,
    pub(super) style: crate::style::ChartStyle,
    pub(super) style_revision: u64,
    pub(super) history: super::tabs::HistorySettings,
    pub(super) tz: crate::timezone::TzOffset,
    pub(super) workspace: crate::workspace_store::WorkspaceStore,
}

impl WorkspaceRuntime {
    pub(super) fn new(
        tab: crate::tab::Tab,
        trades_dir: std::path::PathBuf,
        reach_span_minutes: u32,
        config: &crate::config::AppConfig,
    ) -> Self {
        // Resolve each startup path once: the owned store writes where it loaded.
        let symbols_path = crate::symbols_file::default_path();
        let footprint_settings_path = crate::footprint_config::settings_path();
        let indicator_presets_path = crate::indicators::preset_file::default_path();
        let layouts = super::layout_wiring::open_layout_store(config);
        Self {
            chart_theme: Default::default(),
            appearance: crate::theme::store::load(),
            lifetime: WorkspaceLifetime::new(super::WORKSPACE_ID),
            tabs: super::ArrangementHost::new(
                super::WORKSPACE_ID,
                quantick_workspace::arrangement::TabId(super::FIRST_TAB_ID),
                tab,
            ),
            indicators: super::indicator_manager::IndicatorState::new(
                super::WORKSPACE_ID,
                super::FIRST_TAB_ID,
                &indicator_presets_path,
            ),
            footprint_config: crate::footprint_config::load(&footprint_settings_path),
            style: crate::style::ChartStyle::default(),
            style_revision: 0,
            history: super::tabs::HistorySettings {
                progressive_history: true,
                history_reach: quantick_feed::history_reach::HistoryReach::default(),
                history_reach_span_minutes: reach_span_minutes,
                venue_lead_in: false,
            },
            tz: crate::timezone::TzOffset::default(),
            workspace: crate::workspace_store::WorkspaceStore::new(
                crate::workspace_store::StorePaths {
                    symbols: symbols_path,
                    chart_layers: crate::chart_layers::default_path(),
                    footprint_settings: footprint_settings_path,
                    indicator_presets: indicator_presets_path,
                    ui_state: crate::ui_state::default_path(),
                    bubble_assets: crate::bubble_presets::assets_path(),
                },
                layouts,
                trades_dir,
            ),
        }
    }

    /// Settings retain the pane they captured, regardless of window or focus.
    pub(super) fn draw_indicator_settings(
        &mut self,
        ctx: &egui::Context,
    ) -> Option<super::indicator_manager::SettingsChange> {
        let slot = self.indicators.indicator_settings.as_ref()?.slot;
        let target = self.indicators.indicator_settings_target;
        let valid = self.validate_target(target).is_ok();
        let view = self
            .tabs
            .by_id_mut(target.tab)
            .filter(|_| valid)
            .and_then(|tab| tab.pane_at_mut(target.side.index()))
            .filter(|pane| pane.id == target.pane)
            .and_then(|pane| pane.indicators.view_mut(slot));
        Some(
            self.indicators
                .draw_settings(ctx, view, self.workspace.indicator_presets_path()),
        )
    }

    /// Intake follows workspace ownership even while no native view can paint.
    pub(super) fn drain_tabs(&mut self, config: &crate::config::AppConfig) -> u64 {
        let active = self.tabs.active_id();
        let mut trades = 0_u64;
        for (tab_id, tab) in self.tabs.iter_with_ids_mut() {
            let before = tab.link.live_trades;
            let policy = self.history.policy(self.tz, tab_id == active);
            tab.drain_frame(tab_id, config, policy);
            trades += tab.link.live_trades - before;
        }
        trades
    }

    pub(super) fn id(&self) -> WorkspaceId {
        self.lifetime.id()
    }
    pub(super) fn resolve(&self, view: &WorkspaceView) -> Result<&Self, LifetimeError> {
        self.lifetime.resolve(view)?;
        Ok(self)
    }
    pub(super) fn resolve_mut(&mut self, view: &WorkspaceView) -> Result<&mut Self, LifetimeError> {
        self.lifetime.resolve(view)?;
        Ok(self)
    }

    /// Resolve a captured owner/tab/pane/slot, independently of current focus.
    pub(super) fn validate_target(&self, target: super::TabSlot) -> Result<(), ResourceError> {
        let address = target.address();
        self.lifetime.validate_address(address)?;
        let tab = self
            .tabs
            .by_id(address.tab)
            .ok_or(ResourceError::MissingTab)?;
        let pane = tab
            .pane_at(target.side.index())
            .filter(|pane| pane.id == address.pane)
            .ok_or(ResourceError::MissingPane)?;
        if !self.indicators.registry.contains(target)
            && !pane
                .indicators
                .all()
                .iter()
                .any(|view| view.slot.0 == address.slot)
        {
            return Err(ResourceError::MissingSlot);
        }
        Ok(())
    }
}

/// Native rendering identity stays outside the process-owned workspace payload.
pub(super) struct WindowPresentation {
    pub(super) context: Option<egui::Context>,
    pub(super) binding: WorkspaceView,
    pub(super) native: super::native_windows::NativeWindows,
}
impl WindowPresentation {
    pub(super) fn new(owner: &mut WorkspaceRuntime) -> Self {
        let binding = owner
            .lifetime
            .bind_view(ViewId(0))
            .expect("initial workspace view");
        let mut presentation = Self {
            binding,
            native: Default::default(),
            context: None,
        };
        presentation
            .rebind(owner, egui::ViewportId::ROOT)
            .expect("initial native binding");
        presentation
    }
    /// Change native placement and the headless binding together, before paint.
    pub(super) fn rebind(
        &mut self,
        owner: &WorkspaceRuntime,
        destination: egui::ViewportId,
    ) -> Result<(), LifetimeError> {
        let source = self.binding.presentation();
        let destination_id = PresentationId(destination.0.value());
        let plan = owner
            .lifetime
            .plan_rebind(&self.binding, source, destination_id)?;
        owner.lifetime.rebind(&mut self.binding, plan)
    }
    pub(super) fn resolve<'a>(
        &self,
        owner: &'a WorkspaceRuntime,
        ctx: &egui::Context,
    ) -> Result<&'a WorkspaceRuntime, LifetimeError> {
        if self.binding.presentation() != Some(PresentationId(ctx.viewport_id().0.value())) {
            return Err(LifetimeError::ForeignView);
        }
        owner.resolve(&self.binding)
    }
}
