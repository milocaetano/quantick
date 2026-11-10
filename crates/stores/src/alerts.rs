//! Validated workspace alert sidecar and persistence-before-success commands.
use quantick_alerts::{AlertBook, AlertCommand, CommandResult};
pub use quantick_alerts::{AlertDocument, AlertError};
use std::path::PathBuf;

pub const ALERTS_FILE: &str = "alerts.toml";
pub const BUNDLE_ENABLED: bool = true;

fn failure(field: &str, error: impl std::fmt::Display) -> AlertError {
    AlertError::Invalid {
        field: field.to_owned(),
        reason: error.to_string(),
    }
}

#[derive(Debug)]
pub struct AlertStore {
    pub path: PathBuf,
}

impl AlertStore {
    pub fn load(&self) -> Result<AlertDocument, AlertError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => decode(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                AlertBook::default().document()
            }
            Err(error) => Err(failure("storage", error)),
        }
    }
    pub fn save(&self, document: &AlertDocument) -> Result<(), AlertError> {
        AlertBook::default().restore(document.clone(), 0)?;
        let text = toml::to_string(document).map_err(|error| failure("document", error))?;
        crate::bundle::write_atomically(&self.path, &text)
            .map_err(|error| failure("storage", error))
    }
}

fn decode(text: &str) -> Result<AlertDocument, AlertError> {
    let document: AlertDocument =
        toml::from_str(text).map_err(|error| failure("document", error))?;
    AlertBook::default().restore(document.clone(), 0)?;
    Ok(document)
}

pub fn validate(text: &str) -> Result<(), String> {
    decode(text)
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}

/// Failed loads remain visible and block replacement until a successful reload.
#[derive(Debug)]
pub struct AlertRuntime {
    pub book: AlertBook,
    pub store: AlertStore,
    pub events: quantick_alerts::BoundedEvents,
    pub error: Option<AlertError>,
    checkpoint: Option<AlertDocument>,
}

impl AlertRuntime {
    #[must_use]
    pub fn open(path: PathBuf, utc_ms: i64) -> Self {
        let mut runtime = Self {
            book: AlertBook::default(),
            store: AlertStore { path },
            events: quantick_alerts::BoundedEvents::new(quantick_alerts::MAX_ALERTS),
            error: None,
            checkpoint: None,
        };
        let _ = runtime.reload(utc_ms);
        runtime
    }
    pub fn reload(&mut self, utc_ms: i64) -> Result<(), AlertError> {
        let result = self.store.load().and_then(|document| {
            self.book.restore(document.clone(), utc_ms)?;
            self.checkpoint = Some(document);
            self.events.drain().for_each(drop);
            Ok(())
        });
        self.error = result.as_ref().err().cloned();
        result
    }
    pub fn flush(&mut self) -> Result<(), AlertError> {
        if self.checkpoint.is_none() {
            return Err(self.error.clone().unwrap_or(AlertError::Unavailable));
        }
        let document = self.book.document()?;
        if self.checkpoint.as_ref() == Some(&document) {
            return Ok(());
        }
        let result = self.store.save(&document);
        if result.is_ok() {
            self.checkpoint = Some(document);
        }
        self.error = result.as_ref().err().cloned();
        result
    }
    /// The UI and admitted handler share this transaction boundary.
    pub fn command(
        &mut self,
        command: AlertCommand,
        utc_ms: i64,
    ) -> Result<CommandResult, AlertError> {
        if self.checkpoint.is_none() {
            return Err(self.error.clone().unwrap_or(AlertError::Unavailable));
        }
        let before = self.book.document()?;
        let result = self.book.command(command, utc_ms)?;
        if let Err(error) = self.flush() {
            self.book.restore(before, utc_ms)?;
            return Err(error);
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "alerts_tests.rs"]
mod tests;
