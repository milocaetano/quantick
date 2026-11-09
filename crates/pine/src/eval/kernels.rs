//! The stateful `ta.*` kernels: built-ins whose value on this bar depends on
//! the bars before it.
//!
//! One instance per `(call site, call path)` — the §2.5 identity rule — so a
//! kernel's ring is created here, pinned to its length on first use and
//! advanced exactly once per bar. `super::Eval::builtin_call` routes a call
//! here when the compiler gave it a call site; everything else goes to
//! [`super::pure`].
//!
//! Dispatch is a table: [`kernel_handler`] maps each stateful builtin to the
//! handler of its argument family, and every handler shares one signature.

use quantick_indicators::ta::packed::KernelKind;

use crate::ast::NodeId;
use crate::builtins::Builtin;
use crate::error::{ErrorCode, PineError};

use super::{Eval, Value};

/// One kernel call as the handlers see it: the node, the builtin, the
/// `(call site, call path)` identity and the arguments.
#[derive(Clone, Copy)]
pub(super) struct KernelCall<'c> {
    pub(super) id: NodeId,
    builtin: Builtin,
    pub(super) key: (u32, u32),
    pub(super) args: &'c [crate::ast::Arg],
}

/// The uniform handler signature every kernel family implements.
type KernelHandler<'a> = fn(&mut Eval<'a>, KernelCall<'_>) -> Result<Value, PineError>;

/// The dispatch table: the handler owning a stateful builtin's kernel, or
/// `None` for a builtin that owns no per-call-site state.
fn kernel_handler<'a>(builtin: Builtin) -> Option<KernelHandler<'a>> {
    Some(match builtin {
        Builtin::TaCrossover | Builtin::TaCrossunder | Builtin::TaCross => Eval::cross_kernel,
        Builtin::TaMacd => Eval::macd_kernel,
        Builtin::TaBarsSince => Eval::bars_since_kernel,
        Builtin::TaValueWhen => Eval::value_when_kernel,
        Builtin::Fixnan => Eval::fixnan_kernel,
        Builtin::TaTr => Eval::tr_kernel,
        Builtin::TaAtr => Eval::atr_kernel,
        Builtin::TaCum => Eval::cum_kernel,
        Builtin::TaChange | Builtin::TaMom => Eval::change_kernel,
        Builtin::TaVwma => Eval::vwma_kernel,
        Builtin::TaPivotHigh | Builtin::TaPivotLow => Eval::pivot_kernel,
        Builtin::TaSma
        | Builtin::TaEma
        | Builtin::TaRma
        | Builtin::TaWma
        | Builtin::TaRsi
        | Builtin::TaStdev
        | Builtin::TaHighest
        | Builtin::TaLowest
        | Builtin::TaHighestBars
        | Builtin::TaLowestBars
        | Builtin::MathSum => Eval::window_kernel,
        _ => return None,
    })
}

impl<'a> Eval<'a> {
    pub(super) fn kernel_call(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        site: u32,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        let Some(handler) = kernel_handler(builtin) else {
            unreachable!("{builtin:?} has a call site but no kernel handler");
        };
        let key = (site, self.path);
        handler(
            self,
            KernelCall {
                id,
                builtin,
                key,
                args,
            },
        )
    }

    /// A positive-int kernel length (re-validated here because inputs bind
    /// per instance).
    fn kernel_length(&self, v: f64, id: NodeId) -> Result<usize, PineError> {
        // The upper bound matters as much as the lower one: a kernel
        // allocates its ring up front, and a failed allocation aborts the
        // process instead of unwinding into this indicator's error state.
        // Inputs are bound per instance, so the load-time check in
        // `compile` is not the last word — this is.
        let in_range = v.is_finite()
            && v >= 1.0
            && v.fract() == 0.0
            && v <= crate::compile::MAX_KERNEL_LENGTH as f64;
        if in_range {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Ok(v as usize)
        } else {
            Err(self.err(
                id,
                ErrorCode::PineSeriesLength,
                format!(
                    "kernel length must be a positive integer no greater than {}, got {v}",
                    crate::compile::MAX_KERNEL_LENGTH
                ),
            ))
        }
    }

    /// The argument at `position` as a kernel length.
    pub(super) fn arg_length(
        &mut self,
        call: KernelCall<'_>,
        position: usize,
    ) -> Result<usize, PineError> {
        let v = self.arg_num(call.args, position)?;
        self.kernel_length(v, call.id)
    }

    /// The first argument as a condition, `false` when absent.
    fn first_cond(&mut self, args: &[crate::ast::Arg]) -> Result<bool, PineError> {
        if let Some(values) = &self.semantic_run.arguments {
            return values.first().map_or(Ok(false), |value| {
                value
                    .as_cond()
                    .ok_or_else(|| self.type_err(args[0].value, "a bool condition", value))
            });
        }
        if self.prepared_arguments {
            return args.first().map_or(Ok(false), |arg| {
                let value = self.register(arg.value);
                value
                    .as_cond()
                    .ok_or_else(|| self.type_err(arg.value, "a bool condition", &value))
            });
        }
        match args.first() {
            Some(arg) => self.cond(arg.value),
            None => Ok(false),
        }
    }

    fn cross_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let a = self.arg_num(call.args, 0)?;
        let b = self.arg_num(call.args, 1)?;
        let kind = match call.builtin {
            Builtin::TaCrossover => KernelKind::CrossOver,
            Builtin::TaCrossunder => KernelKind::CrossUnder,
            _ => KernelKind::Cross,
        };
        Ok(Value::Bool(
            self.state
                .arena
                .push_kernel(call.key, kind, 0, 0, [a, b, 0.0])
                != 0.0,
        ))
    }

    fn bars_since_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let condition = self.first_cond(call.args)?;
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::BarsSince,
            0,
            0,
            [f64::from(condition), 0.0, 0.0],
        )))
    }

    fn value_when_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let condition = self.first_cond(call.args)?;
        let source = self.arg_num(call.args, 1)?;
        let occurrence = self.arg_num(call.args, 2)?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let occurrence = if occurrence.is_nan() {
            0
        } else {
            occurrence.max(0.0) as usize
        };
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::ValueWhen,
            occurrence,
            0,
            [f64::from(condition), source, 0.0],
        )))
    }

    fn fixnan_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let x = self.arg_num(call.args, 0)?;
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::Fixnan,
            0,
            0,
            [x, 0.0, 0.0],
        )))
    }

    fn tr_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let handle_na = self.first_cond(call.args)?;
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::Tr,
            usize::from(handle_na),
            0,
            [self.bar.high, self.bar.low, self.bar.close],
        )))
    }

    fn atr_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let len = self.arg_length(call, 0)?;
        self.pin_kernel_length(call.key, len, call.id)?;
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::Atr,
            len,
            0,
            [self.bar.high, self.bar.low, self.bar.close],
        )))
    }

    fn cum_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let x = self.arg_num(call.args, 0)?;
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::Cum,
            0,
            0,
            [x, 0.0, 0.0],
        )))
    }

    fn change_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let x = self.arg_num(call.args, 0)?;
        let n = if call.args.get(1).is_some() {
            let n = self.arg_num(call.args, 1)?;
            self.kernel_length(n, call.id)?
        } else {
            1
        };
        self.pin_kernel_length(call.key, n, call.id)?;
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::Change,
            n,
            0,
            [x, 0.0, 0.0],
        )))
    }

    fn vwma_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let src = self.arg_num(call.args, 0)?;
        let len = self.arg_length(call, 1)?;
        self.pin_kernel_length(call.key, len, call.id)?;
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            KernelKind::Vwma,
            len,
            0,
            [src, self.bar.volume(), 0.0],
        )))
    }

    fn pivot_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let src = self.arg_num(call.args, 0)?;
        let left = self.arg_length(call, 1)?;
        let right = self.arg_length(call, 2)?;
        self.pin_kernel_length(call.key, left + right, call.id)?;
        let kind = if call.builtin == Builtin::TaPivotHigh {
            KernelKind::PivotHigh
        } else {
            KernelKind::PivotLow
        };
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            kind,
            left,
            right,
            [src, 0.0, 0.0],
        )))
    }

    /// The (src, len) family.
    fn window_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let src = self.arg_num(call.args, 0)?;
        let len = self.arg_length(call, 1)?;
        self.pin_kernel_length(call.key, len, call.id)?;
        let kind = match call.builtin {
            Builtin::TaSma => KernelKind::Sma,
            Builtin::TaEma => KernelKind::Ema,
            Builtin::TaRma => KernelKind::Rma,
            Builtin::TaWma => KernelKind::Wma,
            Builtin::TaRsi => KernelKind::Rsi,
            Builtin::TaStdev => KernelKind::Stdev,
            Builtin::TaHighest => KernelKind::Highest,
            Builtin::TaLowest => KernelKind::Lowest,
            Builtin::TaHighestBars => KernelKind::HighestBars,
            Builtin::TaLowestBars => KernelKind::LowestBars,
            Builtin::MathSum => KernelKind::WindowSum,
            other => unreachable!("{other:?} is not a window kernel"),
        };
        Ok(Value::Num(self.state.arena.push_kernel(
            call.key,
            kind,
            len,
            0,
            [src, 0.0, 0.0],
        )))
    }
}

#[cfg(test)]
mod kernel_dispatch_tests {
    use super::kernel_handler;
    use crate::builtins::Builtin;

    #[test]
    fn every_stateful_builtin_has_a_kernel_handler_and_no_other_does() {
        let mut stateful = 0;
        for name in Builtin::all_names() {
            let builtin = Builtin::lookup(name).expect("every listed name resolves");
            assert_eq!(
                kernel_handler(builtin).is_some(),
                builtin.is_stateful_call(),
                "{name}: the kernel dispatch table and is_stateful_call disagree"
            );
            stateful += usize::from(builtin.is_stateful_call());
        }
        assert!(stateful > 0, "the builtin table lists stateful kernels");
    }
}
