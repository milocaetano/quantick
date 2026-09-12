//! The stateful `ta.*` kernels: built-ins whose value on this bar depends on
//! the bars before it.
//!
//! One instance per `(call site, call path)` — the §2.5 identity rule — so a
//! kernel's ring is created here, pinned to its length on first use and
//! advanced exactly once per bar. `super::Eval::builtin_call` routes a call
//! here when the compiler gave it a call site; everything else goes to
//! [`super::pure`].

use quantick_indicators::ta;

use crate::ast::NodeId;
use crate::builtins::Builtin;
use crate::error::{ErrorCode, PineError};

use super::{Eval, Kernel, Value};

impl<'a> Eval<'a> {
    pub(super) fn kernel_call(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        site: u32,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        let key = (site, self.path);
        // Bool-family kernels take their own paths below.
        match builtin {
            Builtin::TaCrossover | Builtin::TaCrossunder | Builtin::TaCross => {
                let a = self.arg_num(args, 0)?;
                let b = self.arg_num(args, 1)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| match builtin {
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
                return Ok(Value::Bool(fired));
            }
            Builtin::TaBarsSince => {
                let condition = match args.first() {
                    Some(arg) => self.cond(arg.value)?,
                    None => false,
                };
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::BarsSince(ta::BarsSince::new()));
                let Kernel::BarsSince(k) = kernel else {
                    unreachable!("barssince site holds a barssince kernel");
                };
                return Ok(Value::Num(k.push(condition)));
            }
            Builtin::TaValueWhen => {
                let condition = match args.first() {
                    Some(arg) => self.cond(arg.value)?,
                    None => false,
                };
                let source = self.arg_num(args, 1)?;
                let occurrence = self.arg_num(args, 2)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let occurrence = if occurrence.is_nan() {
                    0
                } else {
                    occurrence.max(0.0) as usize
                };
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::ValueWhen(ta::ValueWhen::new(occurrence)));
                let Kernel::ValueWhen(k) = kernel else {
                    unreachable!("valuewhen site holds a valuewhen kernel");
                };
                return Ok(Value::Num(k.push(condition, source)));
            }
            Builtin::Fixnan => {
                let x = self.arg_num(args, 0)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Fixnan(f64::NAN));
                let Kernel::Fixnan(last) = kernel else {
                    unreachable!("fixnan site holds a fixnan kernel");
                };
                if !x.is_nan() {
                    *last = x;
                }
                return Ok(Value::Num(*last));
            }
            _ => {}
        }

        // Numeric kernels: gather inputs, then a positive-int length where
        // the kernel takes one (re-validated here because inputs bind per
        // instance).
        let length = |v: f64, id: NodeId, this: &Self| -> Result<usize, PineError> {
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
                Err(this.err(
                    id,
                    ErrorCode::PineSeriesLength,
                    format!(
                        "kernel length must be a positive integer no greater than {}, got {v}",
                        crate::compile::MAX_KERNEL_LENGTH
                    ),
                ))
            }
        };
        let value = match builtin {
            Builtin::TaTr => {
                let handle_na = match args.first() {
                    Some(arg) => self.cond(arg.value)?,
                    None => false,
                };
                let (high, low, close) = (self.bar.high, self.bar.low, self.bar.close);
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Tr(ta::Tr::new(handle_na)));
                let Kernel::Tr(k) = kernel else {
                    unreachable!("tr site holds a tr kernel");
                };
                k.push(high, low, close)
            }
            Builtin::TaAtr => {
                let len = length(self.arg_num(args, 0)?, id, self)?;
                self.pin_kernel_length(key, len, id)?;
                let (high, low, close) = (self.bar.high, self.bar.low, self.bar.close);
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Atr(ta::Atr::new(len)));
                let Kernel::Atr(k) = kernel else {
                    unreachable!("atr site holds an atr kernel");
                };
                k.push(high, low, close)
            }
            Builtin::TaCum => {
                let x = self.arg_num(args, 0)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Cum(ta::Cum::new()));
                let Kernel::Cum(k) = kernel else {
                    unreachable!("cum site holds a cum kernel");
                };
                k.push(x)
            }
            Builtin::TaChange | Builtin::TaMom => {
                let x = self.arg_num(args, 0)?;
                let n = match args.get(1) {
                    Some(arg) => length(self.num(arg.value)?, id, self)?,
                    None => 1,
                };
                self.pin_kernel_length(key, n, id)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Change(ta::Change::new(n)));
                let Kernel::Change(k) = kernel else {
                    unreachable!("change site holds a change kernel");
                };
                k.push(x)
            }
            Builtin::TaVwma => {
                let src = self.arg_num(args, 0)?;
                let len = length(self.arg_num(args, 1)?, id, self)?;
                self.pin_kernel_length(key, len, id)?;
                let volume = self.bar.volume();
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| Kernel::Vwma(ta::Vwma::new(len)));
                let Kernel::Vwma(k) = kernel else {
                    unreachable!("vwma site holds a vwma kernel");
                };
                k.push(src, volume)
            }
            Builtin::TaPivotHigh | Builtin::TaPivotLow => {
                let src = self.arg_num(args, 0)?;
                let left = length(self.arg_num(args, 1)?, id, self)?;
                let right = length(self.arg_num(args, 2)?, id, self)?;
                self.pin_kernel_length(key, left + right, id)?;
                let is_high = builtin == Builtin::TaPivotHigh;
                let kernel = self.state.kernels.entry(key).or_insert_with(|| {
                    if is_high {
                        Kernel::PivotHigh(ta::PivotHigh::new(left, right))
                    } else {
                        Kernel::PivotLow(ta::PivotLow::new(left, right))
                    }
                });
                match kernel {
                    Kernel::PivotHigh(k) => k.push(src),
                    Kernel::PivotLow(k) => k.push(src),
                    _ => unreachable!("pivot site holds a pivot kernel"),
                }
            }
            // The (src, len) family.
            _ => {
                let src = self.arg_num(args, 0)?;
                let len = length(self.arg_num(args, 1)?, id, self)?;
                self.pin_kernel_length(key, len, id)?;
                let kernel = self
                    .state
                    .kernels
                    .entry(key)
                    .or_insert_with(|| match builtin {
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
                match kernel {
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
                }
            }
        };
        Ok(Value::Num(value))
    }
}
