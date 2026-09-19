//! Selection state and debounce policy, independent of screens and transports.
use crate::bar_registry::{
    BUILTIN_BARS, BarConfiguration, BarConfigurationError, BarRegistry, InputRequirements,
};
use rust_decimal::Decimal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarInputAvailability {
    pub traded_volume: bool,
    pub deal_counter: bool,
    pub deal_count: bool,
}
impl BarInputAvailability {
    pub const PRINTS: Self = Self {
        traded_volume: true,
        deal_counter: false,
        deal_count: false,
    };
    /// Restore may retain a temporarily unavailable selection. Only use at a
    /// boundary that already established its source capabilities.
    pub const ALL: Self = Self {
        traded_volume: true,
        deal_counter: true,
        deal_count: true,
    };
    pub fn refusal(self, requirements: InputRequirements) -> Option<BarUnavailable> {
        if requirements.traded_volume && !self.traded_volume {
            Some(BarUnavailable::TradedVolume)
        } else if requirements.deal_counter && !self.deal_count {
            Some(if self.deal_counter {
                BarUnavailable::DealCount
            } else {
                BarUnavailable::DealCounter
            })
        } else {
            None
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarUnavailable {
    TradedVolume,
    DealCounter,
    DealCount,
}
impl BarUnavailable {
    pub fn reason(self) -> &'static str {
        match self {
            Self::TradedVolume => "this source quotes prices but prints no traded volume",
            Self::DealCounter => "this source has no deal counter — MetaTrader B3 only",
            Self::DealCount => {
                "no deal count yet — press REC to count from now, or load a recorded day"
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    Configuration(BarConfigurationError),
    Unavailable(BarUnavailable),
}
impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(error) => error.fmt(f),
            Self::Unavailable(error) => f.write_str(error.reason()),
        }
    }
}
impl std::error::Error for SelectionError {}
impl From<BarConfigurationError> for SelectionError {
    fn from(value: BarConfigurationError) -> Self {
        Self::Configuration(value)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SelectionCommand<'a> {
    Select(&'a str),
    Parameter { name: &'a str, value: Decimal },
    Choice { name: &'a str, value: &'a str },
    Replace(BarConfiguration),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionEffect {
    Unchanged,
    Changed,
    Pending,
    Rebuild(BarConfiguration),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarSelection {
    registry: BarRegistry,
    retained: Vec<BarConfiguration>,
    selected: usize,
    pending: Option<BarConfiguration>,
}
impl Default for BarSelection {
    fn default() -> Self {
        Self::new(
            BUILTIN_BARS
                .find("tick")
                .expect("registered tick")
                .default_config(),
        )
    }
}
impl BarSelection {
    pub fn new(spec: impl Into<BarConfiguration>) -> Self {
        Self::with_registry(BUILTIN_BARS, spec.into().clamped())
            .expect("registered built-in configuration")
    }
    pub fn with_registry(
        registry: BarRegistry,
        initial: BarConfiguration,
    ) -> Result<Self, BarConfigurationError> {
        let selected = registry
            .definitions()
            .iter()
            .position(|definition| std::ptr::eq(*definition, initial.definition()))
            .ok_or_else(|| BarConfigurationError::UnknownKind {
                kind: initial.id().to_owned(),
            })?;
        let mut retained: Vec<_> = registry
            .definitions()
            .iter()
            .map(|definition| definition.default_config())
            .collect();
        retained[selected] = initial;
        Ok(Self {
            registry,
            retained,
            selected,
            pending: None,
        })
    }
    pub fn registry(&self) -> &BarRegistry {
        &self.registry
    }
    pub fn spec(&self) -> BarConfiguration {
        self.retained[self.selected].clamped()
    }
    pub fn selected_id(&self) -> &'static str {
        self.spec().id()
    }
    pub fn pending(&self) -> Option<BarConfiguration> {
        self.pending
    }
    pub fn retained(&self, id: impl AsRef<str>) -> &BarConfiguration {
        self.retained
            .iter()
            .find(|config| config.id() == id.as_ref())
            .expect("registered retained kind")
    }
    /// In-memory legacy restore intentionally keeps its historical positive floors.
    pub fn set(&mut self, spec: impl Into<BarConfiguration>) {
        let spec = spec.into().clamped();
        self.selected = self
            .slot(spec)
            .expect("configuration belongs to this registry");
        self.retained[self.selected] = spec;
    }
    pub fn retain(&mut self, spec: impl Into<BarConfiguration>) {
        let spec = spec.into().clamped();
        let slot = self
            .slot(spec)
            .expect("configuration belongs to this registry");
        self.retained[slot] = spec;
    }
    fn slot(&self, spec: BarConfiguration) -> Result<usize, BarConfigurationError> {
        self.retained
            .iter()
            .position(|entry| std::ptr::eq(entry.definition(), spec.definition()))
            .ok_or_else(|| BarConfigurationError::UnknownKind {
                kind: spec.id().to_owned(),
            })
    }
    pub fn availability(
        &self,
        id: &str,
        inputs: BarInputAvailability,
    ) -> Result<Option<BarUnavailable>, BarConfigurationError> {
        self.registry.find(id)?;
        Ok(inputs.refusal(self.retained(id).requirements()))
    }
    pub fn update(
        &mut self,
        command: SelectionCommand<'_>,
        inputs: BarInputAvailability,
    ) -> Result<SelectionEffect, SelectionError> {
        let spec = match command {
            SelectionCommand::Select(id) => {
                self.registry.find(id)?;
                *self.retained(id)
            }
            SelectionCommand::Parameter { name, value } => {
                self.spec().with_parameter(name, value)?
            }
            SelectionCommand::Choice { name, value } => self.spec().with_choice(name, value)?,
            SelectionCommand::Replace(spec) => {
                self.slot(spec)?;
                spec.definition()
                    .configure(spec.parameter(), spec.choice())?
            }
        };
        if let Some(reason) = inputs.refusal(spec.requirements()) {
            return Err(SelectionError::Unavailable(reason));
        }
        // Validation above remains strict. Equality and retention use the same
        // effective value as legacy restore, including positive decimal floors.
        let spec = spec.clamped();
        if spec == self.spec() {
            return Ok(SelectionEffect::Unchanged);
        }
        self.set(spec);
        Ok(SelectionEffect::Changed)
    }
    /// First frame paints loading, a changing selector defers again, and only
    /// a second stable frame requests a rebuild. The host executes that effect.
    pub fn settle(&mut self, applied: BarConfiguration) -> SelectionEffect {
        let desired = self.spec();
        if desired == applied {
            self.pending = None;
            return SelectionEffect::Unchanged;
        }
        match self.pending.replace(desired) {
            Some(pending) if pending == desired => {
                self.pending = None;
                SelectionEffect::Rebuild(desired)
            }
            _ => SelectionEffect::Pending,
        }
    }
}
