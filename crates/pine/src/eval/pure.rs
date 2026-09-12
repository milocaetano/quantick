//! The pure built-ins: the `math.*` family, `na()`/`nz()` and the two
//! colour constructors — every built-in whose value depends only on this
//! bar's arguments.
//!
//! No state, no ring, no call site — `super::Eval::builtin_call` falls
//! through to here once the stateful kernels of [`super::kernels`] and the
//! draw objects of [`super::objects`] have had their turn.

use quantick_indicators::fmath;

use crate::ast::NodeId;
use crate::builtins::Builtin;
use crate::error::{ErrorCode, PineError};

use super::{Eval, Value};

impl<'a> Eval<'a> {
    pub(super) fn pure_call(
        &mut self,
        id: NodeId,
        builtin: Builtin,
        args: &[crate::ast::Arg],
    ) -> Result<Value, PineError> {
        let value = match builtin {
            Builtin::NaCall => {
                let v = match args.first() {
                    Some(arg) => self.eval(arg.value)?,
                    None => Value::Na,
                };
                return Ok(Value::Bool(v.is_na()));
            }
            Builtin::Nz => {
                let v = match args.first() {
                    Some(arg) => self.eval(arg.value)?,
                    None => Value::Na,
                };
                if v.is_na() {
                    return match args.get(1) {
                        Some(replacement) => self.eval(replacement.value),
                        None => Ok(Value::Num(0.0)),
                    };
                }
                return Ok(v);
            }
            Builtin::ColorNew => {
                let base = match args.first() {
                    Some(arg) => self.eval(arg.value)?,
                    None => Value::Na,
                };
                let Value::Color(rgba) = base else {
                    return Err(self.type_err(id, "a color", &base));
                };
                let transparency = self.arg_num(args, 1)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let alpha = (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                return Ok(Value::Color((rgba & 0xFFFF_FF00) | alpha));
            }
            Builtin::ColorRgb => {
                let r = self.arg_num(args, 0)?;
                let g = self.arg_num(args, 1)?;
                let b = self.arg_num(args, 2)?;
                let transparency = match args.get(3) {
                    Some(arg) => self.num(arg.value)?,
                    None => 0.0,
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let pack = |v: f64| (v.clamp(0.0, 255.0)) as u32;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let alpha = (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                return Ok(Value::Color(
                    (pack(r) << 24) | (pack(g) << 16) | (pack(b) << 8) | alpha,
                ));
            }
            Builtin::MathAbs => self.arg_num(args, 0)?.abs(),
            Builtin::MathFloor => self.arg_num(args, 0)?.floor(),
            Builtin::MathCeil => self.arg_num(args, 0)?.ceil(),
            Builtin::MathRound => self.arg_num(args, 0)?.round(),
            Builtin::MathSign => {
                let v = self.arg_num(args, 0)?;
                if v.is_nan() {
                    f64::NAN
                } else if v > 0.0 {
                    1.0
                } else if v < 0.0 {
                    -1.0
                } else {
                    0.0
                }
            }
            Builtin::MathSqrt => self.arg_num(args, 0)?.sqrt(),
            Builtin::MathMax | Builtin::MathMin | Builtin::MathAvg => {
                // Streaming, so the per-bar path allocates nothing — these
                // appear inside plotted expressions. The identities are the
                // infinities, not `f64::MIN`/`MAX`: those are the extreme
                // *finite* values, so `math.max(math.log(0))` answered
                // -1.797e308 where -inf is the true maximum of its arguments.
                let mut max = f64::NEG_INFINITY;
                let mut min = f64::INFINITY;
                let mut sum = 0.0;
                let mut count = 0usize;
                let mut saw_nan = false;
                for arg in args {
                    let v = self.num(arg.value)?;
                    if v.is_nan() {
                        saw_nan = true;
                    }
                    max = max.max(v);
                    min = min.min(v);
                    sum += v;
                    count += 1;
                }
                if count == 0 || saw_nan {
                    f64::NAN
                } else {
                    match builtin {
                        Builtin::MathMax => max,
                        Builtin::MathMin => min,
                        _ => sum / count as f64,
                    }
                }
            }
            Builtin::MathPow => fmath::pow(self.arg_num(args, 0)?, self.arg_num(args, 1)?),
            Builtin::MathExp => fmath::exp(self.arg_num(args, 0)?),
            Builtin::MathLog => fmath::ln(self.arg_num(args, 0)?),
            Builtin::MathLog10 => fmath::log10(self.arg_num(args, 0)?),
            other => {
                return Err(self.err(
                    id,
                    ErrorCode::PineType,
                    format!("`{other:?}` cannot be called here"),
                ));
            }
        };
        Ok(Value::Num(value))
    }
}
