//! The application adapter for widget control.

use crate::app::{GatewayPort as _, QuantickApp};
use quantick_control_handlers::widgets::{WidgetHost, WidgetOwner};

impl WidgetHost for QuantickApp {
    fn widgets(&mut self) -> Option<Box<dyn WidgetOwner + '_>> {
        let roots = crate::app::control_host::control_roots_mut(self);
        Some(Box::new(
            roots
                .runtime
                .widget_desk(&mut roots.surfaces.widgets, roots.config),
        ))
    }
}

/// A pane close button closes through `widget.close`, so the journal observes it.
pub(crate) fn close_widget(app: &mut QuantickApp, id: &str) {
    let close = quantick_control_schema::widget::WIDGET_CLOSE_CAPABILITY_ID;
    let input = serde_json::json!({ "id": id });
    if app
        .control_action(close, 1, crate::control::ActionOrigin::Human, input)
        .is_err()
    {
        let _ = quantick_control_handlers::widgets::widget(app, id, Some(false));
    }
}
