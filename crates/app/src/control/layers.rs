//! The application adapter for chart layers.

use crate::app::chart_layers_wiring::LayerWiring;
use crate::app::{LayersPort, QuantickApp};

impl LayersPort for QuantickApp {
    fn layer_wiring(&mut self) -> LayerWiring<'_> {
        let roots = crate::app::control_host::control_roots_mut(self);
        let chart_settings = &mut roots.surfaces.chart_settings.model_state;
        LayerWiring::of(roots.runtime, chart_settings)
    }
}
