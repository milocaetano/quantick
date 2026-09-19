//! Portable bundle documents and consuming storage protocols. The host performs IO.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod capture;
mod import;
pub use capture::*;
pub use import::*;

pub const FORMAT_VERSION: u32 = 1;

/// A document is untrusted input; import validates it before staging any store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub sections: BTreeMap<String, toml::Value>,
}
impl Bundle {
    pub fn len(&self) -> usize {
        self.sections.len()
    }
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// Why one section was refused or could not be written, kept typed so a
/// headless importer branches on the kind rather than on message text.
/// The bundle's own version is [`ImportFailure::Version`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionError {
    /// The text is not a document the store accepts.
    Malformed(String),
    /// The host could not stage or install the section's file.
    Io {
        kind: std::io::ErrorKind,
        message: String,
    },
}
impl std::fmt::Display for SectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(message) | Self::Io { message, .. } => f.write_str(message),
        }
    }
}
impl std::error::Error for SectionError {}
impl From<std::io::Error> for SectionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

/// One immutable registration supplies selection, local-key and parser policy.
pub trait BundleStore {
    fn key(&self) -> &str;
    fn included(&self) -> bool;
    fn local_keys(&self) -> &[&str];
    fn validate_text(&self, text: &str) -> Result<(), SectionError>;
}

fn strip_local_keys(value: &mut toml::Value, keys: &[&str]) {
    if let Some(table) = value.as_table_mut() {
        for key in keys {
            table.remove(*key);
        }
    }
}

fn merge_local_keys(
    section: &toml::Value,
    current: CurrentText,
    keys: &[&str],
) -> Result<String, String> {
    if keys.is_empty() {
        return toml::to_string_pretty(section).map_err(|error| error.to_string());
    }
    let mut merged = section.clone();
    if let (Some(table), CurrentText::Available(text)) = (merged.as_table_mut(), current)
        && let Ok(current) = toml::from_str::<toml::Value>(&text)
        && let Some(current) = current.as_table()
    {
        for key in keys {
            if let Some(value) = current.get(*key) {
                table.insert((*key).to_owned(), value.clone());
            }
        }
    }
    toml::to_string_pretty(&merged).map_err(|error| error.to_string())
}
