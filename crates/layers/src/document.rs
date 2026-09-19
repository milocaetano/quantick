//! Versioned visibility documents. File I/O and diagnostics belong to consumers.
use crate::{ChartLayer, LayerRegistry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct LayerDocument {
    version: u32,
    #[serde(default)]
    layers: BTreeMap<String, bool>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum DocumentError {
    Version(u32),
    Invalid(String),
}
impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Version(version) => write!(
                f,
                "chart-layers format version {version} (this build reads {FORMAT_VERSION})"
            ),
            Self::Invalid(error) => f.write_str(error),
        }
    }
}
impl LayerDocument {
    pub fn parse(text: &str) -> Result<Self, DocumentError> {
        let file: Self =
            toml::from_str(text).map_err(|error| DocumentError::Invalid(error.to_string()))?;
        if file.version != FORMAT_VERSION {
            return Err(DocumentError::Version(file.version));
        }
        Ok(file)
    }
    /// Unknown identities and preset-owned state are deliberately ignored.
    pub fn resolve(self, registry: &LayerRegistry) -> BTreeMap<ChartLayer, bool> {
        self.layers
            .into_iter()
            .filter_map(|(id, visible)| {
                registry
                    .resolve(&id)
                    .filter(|layer| layer.persisted())
                    .map(|layer| (layer, visible))
            })
            .collect()
    }
    /// Explicit saved choices override shipped defaults; silence does not.
    pub fn restore(
        defaults: &str,
        stored: Option<&str>,
        registry: &LayerRegistry,
    ) -> BTreeMap<ChartLayer, bool> {
        let mut states = Self::parse(defaults)
            .map(|document| document.resolve(registry))
            .unwrap_or_default();
        if let Some(document) = stored.and_then(|text| Self::parse(text).ok()) {
            states.extend(document.resolve(registry));
        }
        states
    }
    pub fn encode(states: &BTreeMap<ChartLayer, bool>) -> Result<String, String> {
        let file = Self {
            version: FORMAT_VERSION,
            layers: states
                .iter()
                .filter(|(layer, _)| layer.persisted())
                .map(|(layer, visible)| (layer.id().to_owned(), *visible))
                .collect(),
        };
        toml::to_string_pretty(&file).map_err(|error| error.to_string())
    }
}
