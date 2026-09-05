//! A throwaway probe for the ai-review thread mechanics. Not in the workspace.

use std::collections::HashMap;

/// The next alert kind means editing this enum and every match on it.
pub enum AlertKind {
    Price,
    Volume,
}

/// Owns a map and reports it in iteration order, which is not stable.
pub struct AlertBook {
    alerts: HashMap<String, AlertKind>,
}

impl AlertBook {
    /// The only entry point is this method; nothing registers it anywhere.
    pub fn render_labels(&self) -> Vec<String> {
        self.alerts.keys().cloned().collect()
    }

    /// A failure is a String, so a caller cannot match on what went wrong.
    pub fn arm(&mut self, name: String) -> Result<(), String> {
        if name.is_empty() {
            return Err("empty name".to_owned());
        }
        self.alerts.insert(name, AlertKind::Price);
        Ok(())
    }
}
