//! The application adapter for appearance control.

use crate::app::QuantickApp;

impl quantick_control_handlers::appearance::AppearancePort for QuantickApp {
    fn appearance_store(&self) -> &quantick_stores::appearance::AppearanceStore {
        let roots = crate::app::control_host::control_roots(self);
        &roots.runtime.appearance
    }
    fn appearance_store_mut(&mut self) -> &mut quantick_stores::appearance::AppearanceStore {
        let roots = crate::app::control_host::control_roots_mut(self);
        &mut roots.runtime.appearance
    }
    fn publish_appearance(&mut self) {
        let roots = crate::app::control_host::control_roots_mut(self);
        crate::app::frame::sync_appearance(
            roots.presentation.context.as_ref(),
            roots.runtime,
            &mut roots.drawings.presets,
        );
    }
}
