//! Load-time pure arithmetic used by input-derived HMA lengths.
//! This never accepts a series: every argument must already constant-fold.
use super::Compiler;
use super::fold::Const;
use crate::ast::Arg;
use crate::builtins::Builtin;

impl Compiler {
    pub(super) fn fold_indicator_length_call(
        &self,
        builtin: Builtin,
        args: &[Arg],
    ) -> Option<Const> {
        let values = args
            .iter()
            .map(|arg| self.fold(arg.value))
            .collect::<Option<Vec<_>>>()?;
        let first = values.first()?.as_f64()?;
        let value = match builtin {
            Builtin::MathSqrt if values.len() == 1 => first.sqrt(),
            Builtin::MathFloor if values.len() == 1 => first.floor(),
            Builtin::MathRound if values.len() == 1 => first.round(),
            Builtin::MathMax => values
                .iter()
                .try_fold(f64::NEG_INFINITY, |highest, value| {
                    let value = value.as_f64()?;
                    value.is_finite().then_some(highest.max(value))
                })?,
            _ => return None,
        };
        if !value.is_finite() {
            return None;
        }
        if matches!(builtin, Builtin::MathFloor | Builtin::MathRound)
            || builtin == Builtin::MathMax
                && values.iter().all(|value| matches!(value, Const::Int(_)))
        {
            if value < i64::MIN as f64 || value >= i64::MAX as f64 {
                return None;
            }
            Some(Const::Int(value as i64))
        } else {
            Some(Const::Float(value))
        }
    }
}
