//! Headless candle/bar plot argument vocabulary.
use crate::ast::Arg;

/// Positional arguments through the supported color fields.
pub(crate) const CANDLE_ARGS: &[&str] = &[
    "open",
    "high",
    "low",
    "close",
    "title",
    "color",
    "wickcolor",
    "editable",
    "show_last",
    "bordercolor",
    "display",
];
pub(crate) const BAR_ARGS: &[&str] = &[
    "open",
    "high",
    "low",
    "close",
    "title",
    "color",
    "editable",
    "show_last",
    "display",
];

pub(crate) fn argument<'a>(args: &'a [Arg], names: &[&str], name: &str) -> Option<&'a Arg> {
    let position = names.iter().position(|candidate| *candidate == name)?;
    args.iter()
        .find(|arg| arg.name.as_deref() == Some(name))
        .or_else(|| args.iter().filter(|arg| arg.name.is_none()).nth(position))
}
