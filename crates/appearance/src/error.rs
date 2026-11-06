//! Field-specific configuration errors.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeError {
    pub field: String,
    pub message: String,
}

impl ThemeError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ThemeError {}
