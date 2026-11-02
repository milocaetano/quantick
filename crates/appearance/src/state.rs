//! Per-application state and explicit inheritance, independent of rendering.

use serde::{Deserialize, Serialize};

use crate::{ResolvedTheme, ThemeError};

/// A missing legacy provenance marker must be treated as explicit by its
/// persistence owner. Equality with a previous theme is never inheritance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Inheritance {
    Theme,
    #[default]
    Explicit,
}

impl Inheritance {
    pub fn resolve<T>(self, explicit: T, themed: impl FnOnce() -> T) -> T {
        match self {
            Self::Theme => themed(),
            Self::Explicit => explicit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    Unchanged,
    Changed { typography_changed: bool },
}

/// Owned by one application. The source is retained so an exact retry does no
/// parsing. A semantic retry does not increment revision or invalidate caches.
#[derive(Debug, Clone, Default)]
pub struct Appearance {
    resolved: ResolvedTheme,
    source: Option<String>,
    revision: u64,
}

impl Appearance {
    pub fn resolved(&self) -> &ResolvedTheme {
        &self.resolved
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn apply(&mut self, source: &str) -> Result<ApplyOutcome, ThemeError> {
        if self.source.as_deref() == Some(source) {
            return Ok(ApplyOutcome::Unchanged);
        }
        let candidate = ResolvedTheme::parse(source)?;
        let outcome = if candidate == self.resolved {
            ApplyOutcome::Unchanged
        } else {
            let typography_changed = candidate.typography != self.resolved.typography;
            self.resolved = candidate;
            self.revision = self.revision.wrapping_add(1);
            ApplyOutcome::Changed { typography_changed }
        };
        self.source = Some(source.to_owned());
        Ok(outcome)
    }
}
