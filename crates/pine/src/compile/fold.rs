//! Pass 4 support: constant folding.
//!
//! What a script says at load time and cannot change per bar — a kernel
//! length, a plot colour, an `input()` default. The folder is deliberately
//! shallow: literals, the `color`/`plot`/`shape`/`location` member
//! constants, unary and binary arithmetic over those, and nothing that would
//! need the interpreter. Anything it cannot fold is reported by its caller
//! with the reason, never guessed.

use crate::ast::{BinOp, NodeId, NodeKind, UnOp};
use crate::builtins::Builtin;

use super::{Compiler, MAX_KERNEL_LENGTH, Resolution};

/// A constant value the folder can produce at load time.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Const {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Color(u32),
    Na,
    /// `plot.style_line`, `shape.circle`, `location.abovebar`, …
    Enum(&'static str),
}

impl Const {
    pub(super) fn as_f64(&self) -> Option<f64> {
        match self {
            Const::Int(v) => Some(*v as f64),
            Const::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub(super) fn as_positive_len(&self) -> Option<usize> {
        match self {
            Const::Int(v) if *v >= 1 => usize::try_from(*v).ok(),
            _ => None,
        }
    }

    /// A positive length that a kernel may actually allocate.
    pub(super) fn as_kernel_len(&self) -> Option<usize> {
        self.as_positive_len().filter(|v| *v <= MAX_KERNEL_LENGTH)
    }
}

/// Member constants of the `color`/`plot`/`shape`/`location` namespaces.
/// Colors fold to values; the rest fold to enum tags the interpreter maps.
pub(super) fn member_constant(namespace: &str, field: &str) -> Option<Const> {
    if namespace == "color" {
        let rgb = match field {
            "red" => 0xF236_45FF_u32,
            "green" => 0x0899_81FF,
            "blue" => 0x2962_FFFF,
            "orange" => 0xFF98_00FF,
            "aqua" => 0x00BC_D4FF,
            "yellow" => 0xFDD8_35FF,
            "purple" => 0x9C27_B0FF,
            "lime" => 0x00E6_76FF,
            "white" => 0xFFFF_FFFF,
            "black" => 0x0000_00FF,
            "gray" | "grey" => 0x7878_80FF,
            "silver" => 0xB2B5_BEFF,
            "maroon" => 0x8000_00FF,
            "navy" => 0x0000_80FF,
            "olive" => 0x8080_00FF,
            "teal" => 0x0089_7BFF,
            "fuchsia" => 0xE040_FBFF,
            _ => return None,
        };
        return Some(Const::Color(rgb));
    }
    let tag = match (namespace, field) {
        // format.* is accepted so `indicator(format=...)` resolves; the
        // header then warns that it is ignored.
        ("format", "price") => "format_price",
        ("format", "volume") => "format_volume",
        ("format", "percent") => "format_percent",
        ("format", "inherit") => "format_inherit",
        ("plot", "style_line") => "style_line",
        ("plot", "style_stepline") => "style_stepline",
        ("plot", "style_histogram") => "style_histogram",
        ("plot", "style_columns") => "style_columns",
        ("plot", "style_circles") => "style_circles",
        ("plot", "style_cross") => "style_cross",
        ("plot", "style_area") => "style_area",
        ("shape", "triangleup") => "triangleup",
        ("shape", "triangledown") => "triangledown",
        ("shape", "circle") => "circle",
        ("shape", "labelup") => "labelup",
        ("shape", "labeldown") => "labeldown",
        ("shape", "cross") => "cross",
        ("label", "style_label_up") => "style_label_up",
        ("label", "style_label_down") => "style_label_down",
        ("label", "style_none") => "style_none",
        ("location", "abovebar") => "abovebar",
        ("location", "belowbar") => "belowbar",
        ("location", "absolute") => "absolute",
        _ => return None,
    };
    Some(Const::Enum(tag))
}

impl Compiler {
    pub(super) fn fold(&self, id: NodeId) -> Option<Const> {
        match self.kind(id) {
            NodeKind::Int(v) => Some(Const::Int(*v)),
            NodeKind::Float(v) => Some(Const::Float(*v)),
            NodeKind::Str(v) => Some(Const::Str(v.clone())),
            NodeKind::Bool(v) => Some(Const::Bool(*v)),
            NodeKind::Color(v) => Some(Const::Color(*v)),
            NodeKind::Na => Some(Const::Na),
            NodeKind::Member { object, field } => match self.kind(*object) {
                NodeKind::Name(namespace) => member_constant(namespace, field),
                _ => None,
            },
            NodeKind::Unary {
                op: UnOp::Neg,
                operand,
            } => match self.fold(*operand)? {
                Const::Int(v) => Some(Const::Int(-v)),
                Const::Float(v) => Some(Const::Float(-v)),
                _ => None,
            },
            NodeKind::Binary { op, lhs, rhs } => {
                let l = self.fold(*lhs)?;
                let r = self.fold(*rhs)?;
                if let (Const::Int(a), Const::Int(b)) = (&l, &r) {
                    return Some(match op {
                        BinOp::Add => Const::Int(a.checked_add(*b)?),
                        BinOp::Sub => Const::Int(a.checked_sub(*b)?),
                        BinOp::Mul => Const::Int(a.checked_mul(*b)?),
                        BinOp::Div if *b != 0 && a % b == 0 => Const::Int(a / b),
                        BinOp::Div if *b != 0 => Const::Float(*a as f64 / *b as f64),
                        _ => return None,
                    });
                }
                let a = l.as_f64()?;
                let b = r.as_f64()?;
                Some(match op {
                    BinOp::Add => Const::Float(a + b),
                    BinOp::Sub => Const::Float(a - b),
                    BinOp::Mul => Const::Float(a * b),
                    // The interpreter maps division by zero to `na` as a
                    // documented divergence from IEEE; a folded expression
                    // that answered `inf` would evaluate differently from the
                    // same expression left unfolded.
                    BinOp::Div if b == 0.0 => Const::Na,
                    BinOp::Div => Const::Float(a / b),
                    _ => return None,
                })
            }
            // A name folds when it is a never-reassigned global whose
            // initialiser folds (this is how `len = input.int(9)` reaches a
            // kernel length).
            NodeKind::Name(_) => match self.resolutions[id.index()] {
                Resolution::Global(slot) => {
                    if self.reassigned.get(slot as usize).copied().unwrap_or(true) {
                        return None;
                    }
                    let init = self.global_init.get(slot as usize).copied().flatten()?;
                    self.fold(init)
                }
                Resolution::EnumConst(tag) => Some(Const::Enum(tag)),
                _ => None,
            },
            // `input.*(default, …)` folds to its default (inputs are bound
            // per instance; lengths re-checked at bind time).
            NodeKind::Call { callee, args } => {
                let name = self.callee_name(*callee)?;
                match Builtin::lookup(&name) {
                    Some(
                        Builtin::InputInt
                        | Builtin::InputFloat
                        | Builtin::InputBool
                        | Builtin::InputString,
                    ) => self.fold(self.arg(args, 0, "defval")?.value),
                    Some(Builtin::ColorNew) => {
                        let base = match self.fold(self.arg(args, 0, "color")?.value)? {
                            Const::Color(rgba) => rgba,
                            _ => return None,
                        };
                        let transparency =
                            self.fold(self.arg(args, 1, "transp")?.value)?.as_f64()?;
                        let alpha =
                            (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                        Some(Const::Color((base & 0xFFFF_FF00) | alpha))
                    }
                    Some(Builtin::ColorRgb) => {
                        let r = self.fold(self.arg(args, 0, "red")?.value)?.as_f64()? as u32;
                        let g = self.fold(self.arg(args, 1, "green")?.value)?.as_f64()? as u32;
                        let b = self.fold(self.arg(args, 2, "blue")?.value)?.as_f64()? as u32;
                        let transparency = match self.arg(args, 3, "transp") {
                            Some(arg) => self.fold(arg.value)?.as_f64()?,
                            None => 0.0,
                        };
                        let alpha =
                            (255.0 * (1.0 - (transparency / 100.0).clamp(0.0, 1.0))).round() as u32;
                        Some(Const::Color(
                            ((r & 0xFF) << 24) | ((g & 0xFF) << 16) | ((b & 0xFF) << 8) | alpha,
                        ))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
