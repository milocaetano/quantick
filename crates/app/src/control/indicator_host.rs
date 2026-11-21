//! The application adapter for indicator control.

use crate::app::QuantickApp;
use quantick_control_handlers::indicator_host::{IndicatorControlHost, IndicatorOwner};

impl IndicatorControlHost for QuantickApp {
    fn indicators(&mut self) -> Box<dyn IndicatorOwner + '_> {
        Box::new(self.indicator_wiring())
    }
}
