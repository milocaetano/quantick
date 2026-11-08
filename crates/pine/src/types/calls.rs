//! Builtin overload selection and argument contracts.
use super::check::Checker;
use super::model::{Base, Qualifier, Type, accepts, common};
use crate::ast::{Arg, NodeId, NodeKind};
use crate::builtins::Builtin;

pub(super) fn variable(b: Builtin) -> Type {
    use Builtin::*;
    let base = match b {
        BarIndex | LastBarIndex | Time | TimeClose | Calendar(_) => Base::Int,
        BarstateIsConfirmed | BarstateIsLast => Base::Bool,
        Open | High | Low | Close | Volume | BuyVolume | SellVolume | Delta | Cvd | TradeCount
        | Hl2 | Hlc3 | Ohlc4 | Hlcc4 | TaTr => Base::Float,
        _ => Base::Unknown,
    };
    Type::new(base, Qualifier::Series)
}
impl Checker<'_> {
    fn arity(&mut self, id: NodeId, count: usize, min: usize, max: usize) {
        if !(min..=max).contains(&count) {
            self.error(id, format!("expected {min}..={max} arguments, got {count}"));
        }
    }
    fn require(&mut self, id: NodeId, ty: &Type, base: Base, max: Qualifier) {
        if !self.accepts(&base, &ty.base) || ty.qualifier > max {
            self.error(id, format!("expected at most {max:?} {base:?}, got {ty:?}"));
        }
        if base == Base::Bool && ty.numeric() {
            self.coercions[id.index()] = self.version.bool_coercion();
        }
    }
    fn arguments(&mut self, args: &[Arg], values: &[Type], expected: &[Base]) {
        for ((arg, ty), base) in args.iter().zip(values).zip(expected) {
            self.require(arg.value, ty, base.clone(), Qualifier::Series);
        }
    }
    pub fn builtin(&mut self, id: NodeId, b: Builtin, args: &[Arg], values: &[Type]) -> Type {
        use Builtin::*;
        let q = values
            .iter()
            .fold(Qualifier::Const, |q, t| q.max(t.qualifier));
        let first = values.first().cloned().unwrap_or_else(Type::unknown);
        let n = args.len();
        if self.version.two_state_bool()
            && matches!(b, NaCall | Nz | Fixnan)
            && first.base == Base::Bool
        {
            self.error(id, "v6 na/nz/fixnan do not accept bool");
        }
        let base = match b {
            IntCast | FloatCast | BoolCast | StringCast | ColorCast => {
                self.arity(id, n, 1, 1);
                let base = match b {
                    IntCast => Base::Int,
                    FloatCast => Base::Float,
                    BoolCast => Base::Bool,
                    StringCast => Base::String,
                    _ => Base::Color,
                };
                let valid = match b {
                    IntCast | FloatCast => first.numeric(),
                    BoolCast => first.condition(),
                    _ => accepts(&base, &first.base),
                };
                if !valid {
                    self.error(id, "no matching scalar conversion");
                }
                base
            }
            NaCall => {
                self.arity(id, n, 1, 1);
                Base::Bool
            }
            Nz => {
                self.arity(id, n, 1, 2);
                if !matches!(
                    first.base,
                    Base::Int | Base::Float | Base::Bool | Base::Color | Base::Na | Base::Unknown
                ) {
                    self.error(id, "nz supports numbers, bools and colors");
                }
                let fallback = values.get(1).cloned().unwrap_or_else(|| first.clone());
                match common(&first, &fallback) {
                    Some(t) => t.base,
                    None => {
                        self.error(id, "no matching nz overload");
                        Base::Unknown
                    }
                }
            }
            MathMax | MathMin | MathAvg => {
                self.arity(id, n, 1, usize::MAX);
                for (arg, ty) in args.iter().zip(values) {
                    self.require(arg.value, ty, Base::Float, Qualifier::Series);
                }
                if b == MathAvg || values.iter().any(|t| t.base == Base::Float) {
                    Base::Float
                } else {
                    Base::Int
                }
            }
            MathAbs | MathSign | MathFloor | MathCeil | MathRound | MathSqrt | MathExp
            | MathLog | MathLog10 | MathPow => {
                self.arity(
                    id,
                    n,
                    if b == MathPow { 2 } else { 1 },
                    if b == MathPow { 2 } else { 1 },
                );
                for (arg, ty) in args.iter().zip(values) {
                    self.require(arg.value, ty, Base::Float, Qualifier::Series);
                }
                match b {
                    MathFloor | MathCeil | MathRound => Base::Int,
                    MathAbs | MathSign => first.base,
                    _ => Base::Float,
                }
            }
            ColorNew => {
                self.arity(id, n, 2, 2);
                self.arguments(args, values, &[Base::Color, Base::Float]);
                Base::Color
            }
            ColorRgb => {
                self.arity(id, n, 3, 4);
                for (arg, ty) in args.iter().zip(values) {
                    self.require(arg.value, ty, Base::Float, Qualifier::Series);
                }
                Base::Color
            }
            Calendar(_) => {
                self.arity(id, n, 1, 2);
                self.arguments(args, values, &[Base::Int, Base::String]);
                return Type::new(Base::Int, Qualifier::Series);
            }
            _ => return self.surface(id, b, args, values),
        };
        Type::new(base, q)
    }
    fn surface(&mut self, id: NodeId, b: Builtin, args: &[Arg], values: &[Type]) -> Type {
        use Builtin::*;
        let first = values.first().cloned().unwrap_or_else(Type::unknown);
        let q = values
            .iter()
            .fold(Qualifier::Const, |q, t| q.max(t.qualifier));
        let base = match b {
            InputInt | InputFloat | InputBool | InputString | InputColor | InputSource => {
                let base = match b {
                    InputInt => Base::Int,
                    InputBool => Base::Bool,
                    InputString => Base::String,
                    InputColor => Base::Color,
                    _ => Base::Float,
                };
                let default = args
                    .iter()
                    .position(|arg| arg.name.as_deref() == Some("defval"))
                    .or_else(|| args.iter().position(|arg| arg.name.is_none()));
                if let Some(index) = default {
                    let arg = &args[index];
                    self.require(
                        arg.value,
                        &values[index],
                        base.clone(),
                        if b == InputSource {
                            Qualifier::Series
                        } else {
                            Qualifier::Const
                        },
                    );
                }
                return Type::new(
                    base,
                    if b == InputSource {
                        Qualifier::Series
                    } else {
                        Qualifier::Input
                    },
                );
            }
            PlotCandle | PlotBar => {
                let names = if b == PlotCandle {
                    crate::candle_plots::CANDLE_ARGS
                } else {
                    crate::candle_plots::BAR_ARGS
                };
                for name in [
                    "open",
                    "high",
                    "low",
                    "close",
                    "color",
                    "wickcolor",
                    "bordercolor",
                    "title",
                ] {
                    if let Some(arg) = crate::candle_plots::argument(args, names, name)
                        && let Some((_, ty)) = args
                            .iter()
                            .zip(values)
                            .find(|(candidate, _)| candidate.value == arg.value)
                    {
                        let base = match name {
                            "color" | "wickcolor" | "bordercolor" => Base::Color,
                            "title" => Base::String,
                            _ => Base::Float,
                        };
                        self.require(
                            arg.value,
                            ty,
                            base,
                            if name == "title" {
                                Qualifier::Const
                            } else {
                                Qualifier::Series
                            },
                        );
                    }
                }
                Base::Na
            }
            Plot | Hline => {
                if let Some(arg) = args.first() {
                    self.require(arg.value, &first, Base::Float, Qualifier::Series);
                }
                Base::Handle("plot")
            }
            PlotShape | PlotChar => {
                if !first.condition() {
                    self.error(id, "marker requires a numeric or bool series");
                }
                Base::Na
            }
            AlertCondition => {
                if !self.valid_condition(&first) {
                    self.error(id, "alert requires a condition");
                }
                Base::Na
            }
            Barcolor | Bgcolor => {
                if let Some(arg) = args.first() {
                    self.require(arg.value, &first, Base::Color, Qualifier::Series);
                }
                Base::Na
            }
            Indicator | Fill => Base::Na,
            LineNew | BoxNew | LabelNew => {
                for (i, (arg, ty)) in args.iter().zip(values).enumerate() {
                    let base = match arg.name.as_deref() {
                        Some("color" | "bgcolor" | "border_color" | "textcolor") => {
                            Some(Base::Color)
                        }
                        Some("text") => Some(Base::String),
                        Some("x" | "x1" | "x2" | "left" | "right") => Some(Base::Int),
                        Some("y" | "y1" | "y2" | "top" | "bottom" | "width") => Some(Base::Float),
                        None if b == LabelNew && i == 2 => Some(Base::String),
                        None if i < 4 && b != LabelNew || i < 2 => {
                            Some(if i % 2 == 0 { Base::Int } else { Base::Float })
                        }
                        _ => None,
                    };
                    if let Some(base) = base {
                        self.require(arg.value, ty, base, Qualifier::Series);
                    }
                }
                return Type::new(
                    Base::Handle(match b {
                        LineNew => "line",
                        BoxNew => "box",
                        _ => "label",
                    }),
                    Qualifier::Series,
                );
            }
            _ if b.is_stateful_call() => return self.kernel(id, b, args, values),
            _ => {
                self.error(id, "a series variable is not callable");
                Base::Unknown
            }
        };
        Type::new(base, q)
    }
    fn kernel(&mut self, id: NodeId, b: Builtin, args: &[Arg], values: &[Type]) -> Type {
        use Builtin::*;
        let (min, max) = match b {
            TaTr => (0, 1),
            TaAtr | TaCum | TaBarsSince | Fixnan => (1, 1),
            TaChange | TaMom => (1, 2),
            TaValueWhen => (3, 3),
            TaMacd => (4, 4),
            TaVwma => (3, 3),
            TaPivotHigh | TaPivotLow => (3, 3),
            _ => (2, 2),
        };
        self.arity(id, args.len(), min, max);
        for (i, (arg, ty)) in args.iter().zip(values).enumerate() {
            let condition = (b == TaBarsSince || b == TaTr || b == TaValueWhen) && i == 0;
            let length = match b {
                TaAtr => true,
                TaMacd => i > 0,
                TaVwma => i == 2,
                TaValueWhen => i == 2,
                TaPivotHigh | TaPivotLow => i > 0,
                TaSma | TaEma | TaRma | TaWma | TaRsi | TaStdev | TaHighest | TaLowest
                | TaHighestBars | TaLowestBars | TaChange | TaMom | MathSum => i == 1,
                _ => false,
            };
            self.require(
                arg.value,
                ty,
                if condition {
                    Base::Bool
                } else if length {
                    Base::Int
                } else {
                    Base::Float
                },
                if length {
                    Qualifier::Simple
                } else {
                    Qualifier::Series
                },
            );
        }
        let base = match b {
            TaCrossover | TaCrossunder | TaCross => Base::Bool,
            TaBarsSince | TaHighestBars | TaLowestBars => Base::Int,
            TaMacd => Base::Tuple(vec![Type::new(Base::Float, Qualifier::Series); 3]),
            _ => Base::Float,
        };
        Type::new(base, Qualifier::Series)
    }
    pub fn method(&mut self, id: NodeId, callee: NodeId, args: &[Arg], values: &[Type]) -> Type {
        if let NodeKind::Member { object, field } = &self.ast.node(callee).kind {
            let object = *object;
            let field = field.clone();
            let ty = self.infer(object);
            if !matches!(
                ty.base,
                Base::Handle("line" | "box" | "label") | Base::Unknown
            ) {
                self.error(id, "method requires a drawing handle");
            }
            let expected = match (&ty.base, field.as_str()) {
                (Base::Handle("line"), "set_xy1" | "set_xy2")
                | (Base::Handle("box"), "set_lefttop" | "set_rightbottom")
                | (Base::Handle("label"), "set_xy") => vec![Base::Int, Base::Float],
                (Base::Handle("line"), "set_x1" | "set_x2")
                | (Base::Handle("box"), "set_left" | "set_right")
                | (Base::Handle("label"), "set_x") => vec![Base::Int],
                (Base::Handle("line"), "set_y1" | "set_y2" | "set_width")
                | (Base::Handle("box"), "set_top" | "set_bottom")
                | (Base::Handle("label"), "set_y") => vec![Base::Float],
                (Base::Handle("line" | "label"), "set_color")
                | (Base::Handle("box"), "set_bgcolor" | "set_border_color")
                | (Base::Handle("label"), "set_textcolor") => vec![Base::Color],
                (Base::Handle("label"), "set_text") => vec![Base::String],
                (Base::Handle("line" | "box" | "label"), "delete") => vec![],
                (Base::Unknown, _) => return Type::unknown(),
                _ => {
                    self.error(id, "unknown method for this handle type");
                    return Type::unknown();
                }
            };
            self.arity(id, args.len(), expected.len(), expected.len());
            self.arguments(args, values, &expected);
        }
        Type::new(Base::Na, Qualifier::Series)
    }
}
