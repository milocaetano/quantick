//! Canonical argument order shared by typing and positional runtime dispatch.
use super::check::Checker;
use crate::{
    ast::{Arg, NodeId},
    builtins::Builtin,
};
impl Checker<'_> {
    pub fn ordered(&mut self, id: NodeId, b: Builtin, args: Vec<Arg>) -> Vec<Arg> {
        use Builtin::*;
        let names: &[&str] = match b {
            IntCast | FloatCast | BoolCast | StringCast | ColorCast | NaCall | Fixnan => &["x"],
            Nz => &["source", "replacement"],
            MathAbs | MathFloor | MathCeil | MathRound | MathSign | MathSqrt | MathExp
            | MathLog | MathLog10 => &["number"],
            MathPow => &["base", "exponent"],
            ColorNew => &["color", "transp"],
            ColorRgb => &["red", "green", "blue", "transp"],
            Collection(crate::collections::Op::From) => {
                if args.iter().any(|arg| arg.name.is_some()) {
                    self.error(id, "array.from accepts positional arguments only");
                }
                return args;
            }
            Collection(op) => crate::collections::arguments(op),
            Calendar(_) => &["time", "timezone"],
            TaMacd => &["source", "fastlen", "slowlen", "siglen"],
            TaVwma => &["source", "volume", "length"],
            TaAtr => &["length"],
            TaCum => &["source"],
            TaTr => &["handle_na"],
            TaBarsSince => &["condition"],
            TaValueWhen => &["condition", "source", "occurrence"],
            TaCrossover | TaCrossunder | TaCross => &["source1", "source2"],
            TaPivotHigh | TaPivotLow => &["source", "leftbars", "rightbars"],
            TaSma | TaEma | TaRma | TaWma | TaRsi | TaStdev | TaHighest | TaLowest
            | TaHighestBars | TaLowestBars | TaChange | TaMom | MathSum => &["source", "length"],
            _ => return args,
        };
        let args = if let Collection(op) = b {
            self.collection_defaults(id, op, args, false)
        } else {
            args
        };
        let mut slots = vec![None; names.len()];
        let mut position = 0;
        for (index, arg) in args.iter().enumerate() {
            let target = match &arg.name {
                Some(name) => names.iter().position(|n| *n == name),
                None => {
                    let target = position;
                    position += 1;
                    Some(target)
                }
            };
            let Some(target) = target.filter(|t| *t < slots.len() && slots[*t].is_none()) else {
                self.error(arg.value, "unknown, duplicate or excess argument");
                return args;
            };
            slots[target] = Some(index);
        }
        let order: Vec<_> = slots.iter().flatten().copied().collect();
        if slots.iter().take(order.len()).any(Option::is_none) {
            self.error(id, "missing required argument before an optional argument");
            return args;
        }
        self.ast.order_args(id, &order);
        order.into_iter().map(|i| args[i].clone()).collect()
    }
}
