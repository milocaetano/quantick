//! Versioned alerts sidecar. The host supplies its path and injected time.
pub use quantick_alerts::{AlertDocument, AlertError};
use std::path::PathBuf;

pub const ALERTS_FILE: &str = "alerts.toml";

#[derive(Debug)]
pub struct AlertStore {
    pub path: PathBuf,
}

impl AlertStore {
    pub fn load(&self) -> Result<AlertDocument, AlertError> {
        Err(AlertError::Unavailable)
    }
    pub fn save(&self, _document: &AlertDocument) -> Result<(), AlertError> {
        Err(AlertError::Unavailable)
    }
}

pub fn validate(_text: &str) -> Result<(), String> {
    Err("alert persistence is not installed".to_owned())
}
