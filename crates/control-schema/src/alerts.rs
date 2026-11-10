//! Reserved alert schemas; unavailable scaffold capabilities are not advertised.
pub const MODULE_ID: &str = "alerts";
pub const LIST: &str = "alerts.list";
pub const VALIDATE: &str = "alerts.validate";
pub const CREATE: &str = "alerts.create";
pub const EDIT: &str = "alerts.edit";
pub const PAUSE: &str = "alerts.pause";
pub const RESUME: &str = "alerts.resume";
pub const REARM: &str = "alerts.rearm";
pub const DELETE: &str = "alerts.delete";
pub const HISTORY: &str = "alerts.history";
pub const CLEAR_HISTORY: &str = "alerts.clear_history";

#[must_use]
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    Vec::new()
}
