//! Bundled source identity is explicit; an authored look is never inferred by RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Dark,
    Light,
}

impl Builtin {
    pub const ALL: [Self; 2] = [Self::Dark, Self::Light];
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|value| value.key() == key)
    }
    pub const fn key(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
    pub const fn source(self) -> &'static str {
        match self {
            Self::Dark => crate::DARK,
            Self::Light => crate::LIGHT,
        }
    }
    pub fn identify(source: Option<&str>) -> Option<Self> {
        match source {
            None => Some(Self::Dark),
            Some(source) => Self::ALL.into_iter().find(|value| value.source() == source),
        }
    }
}
