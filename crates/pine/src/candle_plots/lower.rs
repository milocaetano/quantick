//! Load-time registration, kept beside the owning candle behavior.
use super::{Compiler, fold::Const};
use crate::{
    ast::{Arg, NodeId},
    candle_plots::{BAR_ARGS, CANDLE_ARGS, argument},
    error::{ErrorCode, PineError},
};
use quantick_indicators::CandlePlotSpec;

impl Compiler {
    pub(super) fn validate_candle_scope(&mut self, call: NodeId) {
        if !self
            .pending_plots
            .iter()
            .any(|(registered, _, _)| *registered == call)
        {
            self.errors.push(PineError::new(
                ErrorCode::PineSyntax,
                self.span(call),
                "candle plots must be direct top-level declarations",
            ));
        }
    }

    pub(super) fn candle_call(&mut self, call: NodeId, args: &[Arg], is_bar: bool) {
        let names = if is_bar { BAR_ARGS } else { CANDLE_ARGS };
        let mut seen = std::collections::BTreeSet::new();
        let mut position = 0;
        let mut named = false;
        for arg in args {
            let name = if let Some(name) = arg.name.as_deref() {
                named = true;
                Some(name)
            } else {
                if named {
                    self.errors.push(PineError::new(
                        ErrorCode::PineSyntax,
                        arg.span,
                        "positional argument follows a named argument",
                    ));
                }
                let name = names.get(position).copied();
                position += 1;
                name
            };
            if name.is_none_or(|name| !names.contains(&name) || !seen.insert(name)) {
                self.errors.push(PineError::new(
                    ErrorCode::PineSyntax,
                    arg.span,
                    "unknown, duplicate, or excess candle plot argument",
                ));
            }
        }
        for name in ["open", "high", "low", "close"] {
            if argument(args, names, name).is_none() {
                self.errors.push(PineError::new(
                    ErrorCode::PineSyntax,
                    self.span(call),
                    format!("missing candle plot argument `{name}`"),
                ));
            }
        }
        for name in ["editable", "show_last", "display"] {
            if let Some(arg) = argument(args, names, name) {
                self.errors.push(PineError::new(
                    ErrorCode::PineUnsupported,
                    arg.span,
                    format!("candle plot `{name}` is not supported by the headless output"),
                ));
            }
        }
        let title = match argument(args, names, "title") {
            None => if is_bar { "PlotBar" } else { "PlotCandle" }.to_owned(),
            Some(arg) => match self.fold(arg.value) {
                Some(Const::Str(title)) => title,
                _ => {
                    self.errors.push(PineError::new(
                        ErrorCode::PineSyntax,
                        arg.span,
                        "candle plot title must be a constant string",
                    ));
                    String::new()
                }
            },
        };
        self.plot_of_call[call.index()] = Some(self.candle_plots.len());
        self.candle_plots.push(CandlePlotSpec { title, is_bar });
    }
}
