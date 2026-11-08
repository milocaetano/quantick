//! MACD validates each independently allocated window at compile time.
use super::Compiler;
use crate::{
    ast::{Arg, NodeId},
    error::{ErrorCode, PineError},
};
impl Compiler {
    pub(super) fn check_macd_lengths(&mut self, call: NodeId, args: &[Arg]) {
        if args.len() != 4 {
            self.errors.push(PineError::new(
                ErrorCode::PineArity,
                self.span(call),
                "ta.macd requires source, fast length, slow length and signal length",
            ));
            return;
        }
        for arg in &args[1..] {
            if self
                .fold(arg.value)
                .and_then(|c| c.as_kernel_len())
                .is_none()
            {
                self.errors.push(PineError::new(
                    ErrorCode::PineSeriesLength,
                    arg.span,
                    "each MACD length must fold to a valid fixed kernel length",
                ));
            }
        }
    }
}
