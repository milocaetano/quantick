//! The `alertcondition` titles a script declares, read off the compiled tree
//! before any bar runs.
//!
//! A consumer that decides something from a title — a strategy reading the
//! side a title names — has to refuse a script at load, not discover on the
//! first live alarm that a title says nothing it can use. Only a string
//! literal can be judged that early; a title computed per bar is reported as
//! such, with its line, so the refusal can point at it.

use crate::ast::NodeKind;
use crate::builtins::{ALERT_ARGS, Builtin};
use crate::candle_plots::argument;
use crate::ir::{CompiledScript, Resolution};

/// One `alertcondition` call's title, as the source writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertTitle {
    /// A string literal: the title every alarm of this call fires under.
    Literal(String),
    /// No title argument: the call fires under the runtime's default title.
    Default,
    /// An expression evaluated per bar; its value is unknown until it runs.
    Computed {
        /// 1-based line of the title argument.
        line: usize,
    },
}

/// Every `alertcondition` call in `compiled`, in source order; `source` is
/// the text it was compiled from, which a computed title's line is read in.
#[must_use]
pub fn alert_titles(compiled: &CompiledScript, source: &str) -> Vec<AlertTitle> {
    let alert = Resolution::Builtin(Builtin::AlertCondition);
    let mut calls: Vec<_> = (compiled.ast.all())
        .filter_map(|node| match &node.kind {
            NodeKind::Call { callee, args }
                if compiled.resolutions.get(callee.index()) == Some(&alert) =>
            {
                Some((node.span.start, args))
            }
            _ => None,
        })
        .collect();
    // The arena is in push order, children first; the source order is the
    // order a reader numbers the calls in.
    calls.sort_by_key(|(start, _)| *start);
    calls
        .into_iter()
        .map(|(_, args)| match argument(args, ALERT_ARGS, "title") {
            None => AlertTitle::Default,
            Some(arg) => match &compiled.ast.node(arg.value).kind {
                NodeKind::Str(text) => AlertTitle::Literal(text.clone()),
                _ => AlertTitle::Computed {
                    line: arg.span.line_col(source).0,
                },
            },
        })
        .collect()
}
