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

/// Reserved admitted operation; the CRUD implementation registers it.
pub const TARGET_RESOLVE: &str = "alerts.target.resolve";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawingSelector {
    pub tab_id: u64,
    pub pane_id: u64,
    pub drawing_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetResolveError {
    Unavailable,
    Missing,
    Ambiguous,
    Busy,
    SourceChanged,
    WriteFailed(String),
}
