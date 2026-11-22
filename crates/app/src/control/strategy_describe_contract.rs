//! The one window operation used by the Panels menu and all five control calls.
use quantick_control::error::ControlError;
use quantick_ui_model::strategy_describe::{Intent, Status};

pub(crate) trait DescribeHost {
    fn dispatch(&mut self, intent: Intent) -> Result<Status, ControlError>;
}
