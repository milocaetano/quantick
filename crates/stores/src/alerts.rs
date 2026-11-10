//! Versioned alerts sidecar. The host supplies its path and injected time.
pub use quantick_alerts::{AlertDocument, AlertError};
use std::path::PathBuf;

pub const ALERTS_FILE: &str = "alerts.toml";
/// Registered now, enabled only with the real persistence validator.
pub const BUNDLE_ENABLED: bool = false;

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

/// One workspace owns its book and sidecar; views only borrow this owner.
#[derive(Debug)]
pub struct AlertRuntime {
    pub book: quantick_alerts::AlertBook,
    pub store: AlertStore,
    pub events: quantick_alerts::BoundedEvents,
}

impl AlertRuntime {
    #[must_use]
    pub fn open(path: PathBuf, utc_ms: i64) -> Self {
        let mut runtime = Self {
            book: Default::default(),
            store: AlertStore { path },
            events: quantick_alerts::BoundedEvents::new(quantick_alerts::MAX_ALERTS),
        };
        let _ = runtime.reload(utc_ms);
        runtime
    }

    /// Reserved startup/import boundary; no file is read by the scaffold.
    pub fn reload(&mut self, _utc_ms: i64) -> Result<(), AlertError> {
        Err(AlertError::Unavailable)
    }

    /// Reserved dirty-only checkpoint; no file is written by the scaffold.
    pub fn flush(&mut self) -> Result<(), AlertError> {
        Err(AlertError::Unavailable)
    }
}
