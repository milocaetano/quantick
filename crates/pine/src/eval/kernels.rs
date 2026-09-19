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

use quantick_indicators::ta;

use crate::ast::NodeId;
use crate::builtins::Builtin;
use crate::error::{ErrorCode, PineError};

use super::{Eval, Kernel, Value};

/// One kernel call as the handlers see it: the node, the builtin, the
/// `(call site, call path)` identity and the arguments.
#[derive(Clone, Copy)]
struct KernelCall<'c> {
    id: NodeId,
    builtin: Builtin,
    key: (u32, u32),
    args: &'c [crate::ast::Arg],
}

/// The uniform handler signature every kernel family implements.
type KernelHandler<'a> = fn(&mut Eval<'a>, KernelCall<'_>) -> Result<Value, PineError>;

/// The dispatch table: the handler owning a stateful builtin's kernel, or
/// `None` for a builtin that owns no per-call-site state.
fn kernel_handler<'a>(builtin: Builtin) -> Option<KernelHandler<'a>> {
    Some(match builtin {
        Builtin::TaCrossover | Builtin::TaCrossunder | Builtin::TaCross => Eval::cross_kernel,
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
    fn arg_length(&mut self, call: KernelCall<'_>, position: usize) -> Result<usize, PineError> {
        let v = self.arg_num(call.args, position)?;
        self.kernel_length(v, call.id)
    }

    /// The first argument as a condition, `false` when absent.
    fn first_cond(&mut self, args: &[crate::ast::Arg]) -> Result<bool, PineError> {
        match args.first() {
            Some(arg) => self.cond(arg.value),
            None => Ok(false),
        }
    }

    fn cross_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let a = self.arg_num(call.args, 0)?;
        let b = self.arg_num(call.args, 1)?;
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| match call.builtin {
                Builtin::TaCrossover => Kernel::CrossOver(ta::CrossOver::new()),
                Builtin::TaCrossunder => Kernel::CrossUnder(ta::CrossUnder::new()),
                _ => Kernel::Cross(ta::Cross::new()),
            });
        let fired = match kernel {
            Kernel::CrossOver(k) => k.push(a, b),
            Kernel::CrossUnder(k) => k.push(a, b),
            Kernel::Cross(k) => k.push(a, b),
            _ => unreachable!("cross site holds a cross kernel"),
        };
        Ok(Value::Bool(fired))
    }

    fn bars_since_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let condition = self.first_cond(call.args)?;
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::BarsSince(ta::BarsSince::new()));
        let Kernel::BarsSince(k) = kernel else {
            unreachable!("barssince site holds a barssince kernel");
        };
        Ok(Value::Num(k.push(condition)))
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
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::ValueWhen(ta::ValueWhen::new(occurrence)));
        let Kernel::ValueWhen(k) = kernel else {
            unreachable!("valuewhen site holds a valuewhen kernel");
        };
        Ok(Value::Num(k.push(condition, source)))
    }

    fn fixnan_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let x = self.arg_num(call.args, 0)?;
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::Fixnan(f64::NAN));
        let Kernel::Fixnan(last) = kernel else {
            unreachable!("fixnan site holds a fixnan kernel");
        };
        if !x.is_nan() {
            *last = x;
        }
        Ok(Value::Num(*last))
    }

    fn tr_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let handle_na = self.first_cond(call.args)?;
        let (high, low, close) = (self.bar.high, self.bar.low, self.bar.close);
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::Tr(ta::Tr::new(handle_na)));
        let Kernel::Tr(k) = kernel else {
            unreachable!("tr site holds a tr kernel");
        };
        Ok(Value::Num(k.push(high, low, close)))
    }

    fn atr_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let len = self.arg_length(call, 0)?;
        self.pin_kernel_length(call.key, len, call.id)?;
        let (high, low, close) = (self.bar.high, self.bar.low, self.bar.close);
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::Atr(ta::Atr::new(len)));
        let Kernel::Atr(k) = kernel else {
            unreachable!("atr site holds an atr kernel");
        };
        Ok(Value::Num(k.push(high, low, close)))
    }

    fn cum_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let x = self.arg_num(call.args, 0)?;
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::Cum(ta::Cum::new()));
        let Kernel::Cum(k) = kernel else {
            unreachable!("cum site holds a cum kernel");
        };
        Ok(Value::Num(k.push(x)))
    }

    fn change_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let x = self.arg_num(call.args, 0)?;
        let n = match call.args.get(1) {
            Some(arg) => {
                let n = self.num(arg.value)?;
                self.kernel_length(n, call.id)?
            }
            None => 1,
        };
        self.pin_kernel_length(call.key, n, call.id)?;
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::Change(ta::Change::new(n)));
        let Kernel::Change(k) = kernel else {
            unreachable!("change site holds a change kernel");
        };
        Ok(Value::Num(k.push(x)))
    }

    fn vwma_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let src = self.arg_num(call.args, 0)?;
        let len = self.arg_length(call, 1)?;
        self.pin_kernel_length(call.key, len, call.id)?;
        let volume = self.bar.volume();
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| Kernel::Vwma(ta::Vwma::new(len)));
        let Kernel::Vwma(k) = kernel else {
            unreachable!("vwma site holds a vwma kernel");
        };
        Ok(Value::Num(k.push(src, volume)))
    }

    fn pivot_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let src = self.arg_num(call.args, 0)?;
        let left = self.arg_length(call, 1)?;
        let right = self.arg_length(call, 2)?;
        self.pin_kernel_length(call.key, left + right, call.id)?;
        let is_high = call.builtin == Builtin::TaPivotHigh;
        let kernel = self.state.kernels.entry(call.key).or_insert_with(|| {
            if is_high {
                Kernel::PivotHigh(ta::PivotHigh::new(left, right))
            } else {
                Kernel::PivotLow(ta::PivotLow::new(left, right))
            }
        });
        let value = match kernel {
            Kernel::PivotHigh(k) => k.push(src),
            Kernel::PivotLow(k) => k.push(src),
            _ => unreachable!("pivot site holds a pivot kernel"),
        };
        Ok(Value::Num(value))
    }

    /// The (src, len) family.
    fn window_kernel(&mut self, call: KernelCall<'_>) -> Result<Value, PineError> {
        let src = self.arg_num(call.args, 0)?;
        let len = self.arg_length(call, 1)?;
        self.pin_kernel_length(call.key, len, call.id)?;
        let kernel = self
            .state
            .kernels
            .entry(call.key)
            .or_insert_with(|| match call.builtin {
                Builtin::TaSma => Kernel::Sma(ta::Sma::new(len)),
                Builtin::TaEma => Kernel::Ema(ta::Ema::new(len)),
                Builtin::TaRma => Kernel::Rma(ta::Rma::new(len)),
                Builtin::TaWma => Kernel::Wma(ta::Wma::new(len)),
                Builtin::TaRsi => Kernel::Rsi(ta::Rsi::new(len)),
                Builtin::TaStdev => Kernel::Stdev(ta::Stdev::new(len)),
                Builtin::TaHighest => Kernel::Highest(ta::Highest::new(len)),
                Builtin::TaLowest => Kernel::Lowest(ta::Lowest::new(len)),
                Builtin::TaHighestBars => Kernel::HighestBars(ta::HighestBars::new(len)),
                Builtin::TaLowestBars => Kernel::LowestBars(ta::LowestBars::new(len)),
                Builtin::MathSum => Kernel::WindowSum(ta::WindowSum::new(len)),
                other => unreachable!("{other:?} is not a (src, len) kernel"),
            });
        let value = match kernel {
            Kernel::Sma(k) => k.push(src),
            Kernel::Ema(k) => k.push(src),
            Kernel::Rma(k) => k.push(src),
            Kernel::Wma(k) => k.push(src),
            Kernel::Rsi(k) => k.push(src),
            Kernel::Stdev(k) => k.push(src),
            Kernel::Highest(k) => k.push(src),
            Kernel::Lowest(k) => k.push(src),
            Kernel::HighestBars(k) => k.push(src),
            Kernel::LowestBars(k) => k.push(src),
            Kernel::WindowSum(k) => k.push(src),
            _ => unreachable!("(src, len) site holds a (src, len) kernel"),
        };
        Ok(Value::Num(value))
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
