//! Immutable bar definitions resolved once, before a builder sees any trades.
//!
//! Consumers render parameter descriptors, hold configurations and call `build`.
//! Neither the registry nor a legacy enum participates in per-trade dispatch.
pub mod definitions;
mod parameters;
pub use parameters::*;

use crate::BarBuilder;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::borrow::Cow;

#[derive(Debug, Clone, Copy)]
pub struct BarDefinition {
    pub id: &'static str,
    pub parameter: ParameterDescriptor,
    pub choice_parameter: Option<&'static str>,
    pub choices: &'static [ChoiceDescriptor],
    pub default_choice: Option<&'static str>,
    pub requirements: InputRequirements,
    pub progress_unit: &'static str,
    /// Closed bars partition a fixed duration, as required by candle/viewport consumers.
    /// A duration-shaped parameter alone does not imply this capability.
    pub fixed_time_interval: bool,
    pub factory: fn(Decimal, Option<&str>) -> Box<dyn BarBuilder>,
}

impl BarDefinition {
    pub fn requirements_for(&self, choice: Option<&str>) -> InputRequirements {
        let mut requirements = self.requirements;
        if let Some(choice) = self.choices.iter().find(|entry| Some(entry.id) == choice) {
            requirements.traded_volume |= choice.requirements.traded_volume;
            requirements.deal_counter |= choice.requirements.deal_counter;
        }
        requirements
    }
    pub fn label(&self) -> &'static str {
        self.id
    }
    pub fn needs_deal_counter(&self) -> bool {
        self.requirements.deal_counter
    }
    pub fn needs_traded_volume(&self) -> bool {
        self.requirements.traded_volume
    }
    pub fn progress_unit(&self) -> &'static str {
        self.progress_unit
    }
    pub fn default_config(&'static self) -> BarConfiguration {
        BarConfiguration {
            definition: self,
            parameter: self.parameter.default,
            choice: self.default_choice,
        }
    }

    pub fn configure(
        &'static self,
        parameter: Decimal,
        choice: Option<&str>,
    ) -> Result<BarConfiguration, BarConfigurationError> {
        self.parameter.validate(self.id, parameter)?;
        let choice = match choice {
            None => self.default_choice,
            Some(id) => Some(
                self.choices
                    .iter()
                    .find(|entry| entry.id == id)
                    .ok_or_else(|| BarConfigurationError::UnknownChoice {
                        choice: id.to_owned(),
                    })?
                    .id,
            ),
        };
        Ok(BarConfiguration {
            definition: self,
            parameter,
            choice,
        })
    }

    fn parse(&'static self, text: &str) -> Result<BarConfiguration, BarConfigurationError> {
        let (choice, parameter) = if self.choice_parameter.is_some() {
            match text.split_once(':') {
                Some((choice, parameter)) => {
                    let choice = choice.trim();
                    let found = self
                        .choices
                        .iter()
                        .find(|entry| entry.id == choice)
                        .ok_or_else(|| BarConfigurationError::UnknownChoice {
                            choice: choice.to_owned(),
                        })?;
                    (Some(found.id), parameter.trim())
                }
                None => (self.default_choice, text),
            }
        } else {
            (None, text)
        };
        let parameter = self.parameter.parse(self.id, parameter)?;
        Ok(BarConfiguration {
            definition: self,
            parameter,
            choice,
        })
    }
}

/// A resolved configuration is small and Copy. Its definition outlives builders.
#[derive(Debug, Clone, Copy)]
pub struct BarConfiguration {
    definition: &'static BarDefinition,
    parameter: Decimal,
    choice: Option<&'static str>,
}

impl PartialEq for BarConfiguration {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.definition, other.definition)
            && self.parameter == other.parameter
            && self.choice == other.choice
    }
}
impl Eq for BarConfiguration {}

impl BarConfiguration {
    pub fn kind(self) -> &'static BarDefinition {
        self.definition
    }
    pub fn definition(self) -> &'static BarDefinition {
        self.definition
    }
    pub fn id(self) -> &'static str {
        self.definition.id
    }
    pub fn parameter(self) -> Decimal {
        self.parameter
    }
    pub fn choice(self) -> Option<&'static str> {
        self.choice
    }
    pub fn build(self) -> Box<dyn BarBuilder> {
        (self.definition.factory)(self.parameter, self.choice)
    }
    pub fn time_interval_ms(self) -> Option<i64> {
        self.definition
            .fixed_time_interval
            .then(|| self.parameter.to_i64().expect("interval representation"))
    }
    pub fn requirements(self) -> InputRequirements {
        self.definition.requirements_for(self.choice)
    }
    pub fn to_config_string(self) -> String {
        let value = self.definition.parameter.format(self.parameter);
        match self
            .choice
            .filter(|choice| Some(*choice) != self.definition.default_choice)
        {
            Some(choice) => format!("{}:{choice}:{value}", self.id()),
            None => format!("{}:{value}", self.id()),
        }
    }
    pub fn summary(self) -> String {
        let value = self.definition.parameter.format(self.parameter);
        match self
            .choice
            .filter(|choice| Some(*choice) != self.definition.default_choice)
        {
            Some(choice) => format!("{}({choice} {value})", self.id()),
            None => format!("{}({value})", self.id()),
        }
    }
    pub fn parameter_summary(self) -> String {
        let value = match self.definition.parameter.editor.decimals {
            Some(places) => format!("{:.*}", places, self.parameter.to_f64().unwrap_or_default()),
            None => self.definition.parameter.format(self.parameter),
        };
        match self
            .choice
            .filter(|choice| Some(*choice) != self.definition.default_choice)
        {
            Some(choice) => format!("{choice} {value}"),
            None => value,
        }
    }
    /// Legacy in-memory/config restore used positive floors, including time <100ms.
    /// Strict text/command construction goes through `configure` instead.
    pub fn clamped(self) -> Self {
        Self {
            parameter: self.parameter.max(
                if self.definition.parameter.kind == NumberKind::Decimal {
                    DECIMAL_PARAM_FLOOR
                } else {
                    Decimal::ONE
                },
            ),
            ..self
        }
    }
    pub fn with_parameter(self, name: &str, value: Decimal) -> Result<Self, BarConfigurationError> {
        if name != self.definition.parameter.name {
            return Err(BarConfigurationError::UnknownParameter {
                parameter: name.to_owned(),
            });
        }
        self.definition.configure(value, self.choice)
    }
    pub fn with_choice(self, name: &str, choice: &str) -> Result<Self, BarConfigurationError> {
        if Some(name) != self.definition.choice_parameter {
            return Err(BarConfigurationError::UnknownParameter {
                parameter: name.to_owned(),
            });
        }
        self.definition.configure(self.parameter, Some(choice))
    }
    /// Only the closed legacy adapter may import historical unchecked values.
    pub(crate) fn legacy(
        definition: &'static BarDefinition,
        parameter: Decimal,
        choice: Option<&'static str>,
    ) -> Self {
        Self {
            definition,
            parameter,
            choice,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BarRegistry {
    definitions: Cow<'static, [&'static BarDefinition]>,
}
impl PartialEq for BarRegistry {
    fn eq(&self, other: &Self) -> bool {
        self.definitions.len() == other.definitions.len()
            && self
                .definitions
                .iter()
                .zip(other.definitions.iter())
                .all(|(a, b)| std::ptr::eq(*a, *b))
    }
}
impl Eq for BarRegistry {}
impl BarRegistry {
    pub fn new(
        definitions: impl IntoIterator<Item = &'static BarDefinition>,
    ) -> Result<Self, BarConfigurationError> {
        let definitions: Vec<_> = definitions.into_iter().collect();
        for (i, definition) in definitions.iter().enumerate() {
            definition
                .parameter
                .validate(definition.id, definition.parameter.default)?;
            let invalid = |reason| BarConfigurationError::InvalidDefinition {
                kind: definition.id.to_owned(),
                reason,
            };
            if definition.fixed_time_interval && definition.parameter.kind != NumberKind::Duration {
                return Err(invalid(
                    "a fixed time partition requires a duration parameter",
                ));
            }
            if definition.id.is_empty()
                || definition.id.contains(':')
                || definition.id.trim() != definition.id
            {
                return Err(invalid("the stable ID must be a nonempty spec token"));
            }
            if definition.choice_parameter.is_some() != !definition.choices.is_empty()
                || definition.default_choice.is_some() != !definition.choices.is_empty()
                || definition
                    .default_choice
                    .is_some_and(|id| !definition.choices.iter().any(|choice| choice.id == id))
            {
                return Err(invalid("choice schema and default disagree"));
            }
            for (i, choice) in definition.choices.iter().enumerate() {
                if definition.choices[..i]
                    .iter()
                    .any(|other| other.id == choice.id)
                {
                    return Err(invalid("duplicate parameter choice"));
                }
            }
            if definitions[..i]
                .iter()
                .any(|other| other.id == definition.id)
            {
                return Err(BarConfigurationError::DuplicateKind {
                    kind: definition.id.to_owned(),
                });
            }
        }
        Ok(Self {
            definitions: Cow::Owned(definitions),
        })
    }
    pub fn definitions(&self) -> &[&'static BarDefinition] {
        &self.definitions
    }
    pub fn find(&self, id: &str) -> Result<&'static BarDefinition, BarConfigurationError> {
        self.definitions
            .iter()
            .copied()
            .find(|definition| definition.id == id)
            .ok_or_else(|| BarConfigurationError::UnknownKind {
                kind: id.to_owned(),
            })
    }
    pub fn parse(&self, text: &str) -> Result<BarConfiguration, BarConfigurationError> {
        let (id, parameter) =
            text.split_once(':')
                .ok_or_else(|| BarConfigurationError::NotKindParameter {
                    text: text.to_owned(),
                })?;
        self.find(id.trim())?.parse(parameter.trim())
    }
}

/// The production catalog is assembled by the definitions' registration point.
pub const BUILTIN_BARS: BarRegistry = BarRegistry {
    definitions: Cow::Borrowed(definitions::DEFINITIONS),
};
