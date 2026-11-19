//! A compiled script ready to arm, and the reasons one is refused.

use std::fmt;

use quantick_engine::Side;
use quantick_pine::{AlertTitle, CompiledScript, PineError, RuntimeFault, alert_titles, compile};

/// The words a title may open with to name a side, any case.
const BUY_WORDS: [&str; 3] = ["bull", "buy", "long"];
const SELL_WORDS: [&str; 3] = ["bear", "sell", "short"];

/// The side an alert title names by its first word — `bull`, `buy` or
/// `long` for a buy, `bear`, `sell` or `short` for a sell, in any case — or
/// `None`. The whole word: `Bullish` names nothing.
#[must_use]
pub fn side_of_title(title: &str) -> Option<Side> {
    let first = title.split_whitespace().next()?.to_ascii_lowercase();
    if BUY_WORDS.contains(&first.as_str()) {
        Some(Side::Buy)
    } else if SELL_WORDS.contains(&first.as_str()) {
        Some(Side::Sell)
    } else {
        None
    }
}

/// A script failure as the trader reads it: `line N: <CODE> <message>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFault {
    /// 1-based source line.
    pub line: usize,
    /// The stable Pine error code (`PINE_LOOP_BUDGET`, …).
    pub code: &'static str,
    pub message: String,
}

impl ScriptFault {
    /// The first of a compile's errors, located in `source`.
    fn compiled(errors: &[PineError], source: &str) -> Self {
        let first = errors.first().expect("a failed compile reports an error");
        Self {
            line: first.span.line_col(source).0,
            code: first.code.as_str(),
            message: first.message.clone(),
        }
    }

    pub(crate) fn runtime(fault: &RuntimeFault) -> Self {
        Self {
            line: fault.line,
            code: fault.code.as_str(),
            message: fault.message.clone(),
        }
    }
}

impl fmt::Display for ScriptFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {} {}", self.line, self.code, self.message)
    }
}

/// Why a script cannot be armed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArmRefusal {
    /// The script does not compile; the first error.
    Compile(ScriptFault),
    /// No `alertcondition` at all: nothing could ever ring.
    NoAlerts,
    /// An `alertcondition` with no title fires under a default that names
    /// no side.
    DefaultTitle,
    /// A title computed per bar: its side cannot be checked before it fires.
    ComputedTitle { line: usize },
    /// A literal title whose first word names no side.
    UnsidedTitle(String),
}

impl fmt::Display for ArmRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sides = "its first word must be bull, buy, long, bear, sell or short";
        match self {
            Self::Compile(fault) => write!(f, "the script does not compile: {fault}"),
            Self::NoAlerts => f.write_str("the script has no alertcondition, so it can never ring"),
            Self::DefaultTitle => write!(f, "an alertcondition has no title; {sides}"),
            Self::ComputedTitle { line } => write!(
                f,
                "line {line}: an alertcondition title is computed per bar; write it as a string \
                 literal whose first word names the side"
            ),
            Self::UnsidedTitle(title) => {
                write!(f, "alertcondition title `{title}` names no side; {sides}")
            }
        }
    }
}

impl std::error::Error for ArmRefusal {}

/// A script that compiled and whose every alert title names a side.
#[derive(Clone)]
pub struct Script {
    name: String,
    source: String,
    compiled: CompiledScript,
    /// Each alert title the script declares, with the side it names.
    sides: Vec<(String, Side)>,
}

impl fmt::Debug for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Script")
            .field("name", &self.name)
            .field("sides", &self.sides)
            .finish_non_exhaustive()
    }
}

impl Script {
    /// Compile `source` and read its alert titles.
    ///
    /// # Errors
    ///
    /// The first compile error, or the first title that names no side.
    pub fn load(name: &str, source: &str) -> Result<Self, ArmRefusal> {
        let compiled = compile(source, name)
            .map_err(|errors| ArmRefusal::Compile(ScriptFault::compiled(&errors, source)))?;
        let titles = alert_titles(&compiled, source);
        if titles.is_empty() {
            return Err(ArmRefusal::NoAlerts);
        }
        let sides = titles
            .into_iter()
            .map(|title| match title {
                AlertTitle::Default => Err(ArmRefusal::DefaultTitle),
                AlertTitle::Computed { line } => Err(ArmRefusal::ComputedTitle { line }),
                AlertTitle::Literal(title) => match side_of_title(&title) {
                    Some(side) => Ok((title, side)),
                    None => Err(ArmRefusal::UnsidedTitle(title)),
                },
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            name: name.to_owned(),
            source: source.to_owned(),
            compiled,
            sides,
        })
    }

    /// The name it was loaded under: its library file name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Each alert title with the side it names, in source order.
    #[must_use]
    pub fn sides(&self) -> &[(String, Side)] {
        &self.sides
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn compiled(&self) -> &CompiledScript {
        &self.compiled
    }
}
