//! Scalar types, qualifier inference, overloads and runtime value vocabulary.
use std::rc::Rc;

mod arguments;
mod calls;
mod check;
mod model;
#[path = "../version/types.rs"]
mod version_policy;
pub(crate) use check::validate;

pub(crate) use crate::ir::TypeProgram as Checked;

/// A runtime value. `na` for numbers is NaN inside `Num`; the distinct `Na`
/// covers non-numeric contexts.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A number (NaN = na).
    Num(f64),
    /// A boolean.
    Bool(bool),
    /// A packed RGBA color.
    Color(u32),
    /// A string (thin Rc: fat pointers would push Value past 16 bytes,
    /// and strings never ride the hot numeric path).
    Str(Rc<String>),
    /// A function's multi-return (thin Rc, same reasoning).
    Tuple(Rc<Vec<Value>>),
    /// A draw-object handle (`line.new` & co.).
    Obj(ObjKind, u32),
    /// `na` outside numeric context.
    Na,
    /// Missing boolean.
    NaBool,
    /// Missing color.
    NaColor,
    /// Missing string.
    NaString,
}

/// Which store a [`Value::Obj`] handle points into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjKind {
    /// A `line` handle.
    Line,
    /// A `box` handle.
    Box,
    /// A `label` handle.
    Label,
}

impl Value {
    /// Numeric view: numbers pass through, `na` is NaN, anything else is a
    /// type error at the caller.
    pub(crate) fn as_num(&self) -> Option<f64> {
        match self {
            Value::Num(v) => Some(*v),
            Value::Na => Some(f64::NAN),
            _ => None,
        }
    }

    /// Condition view: bools pass, `na` is false (Pine's rule).
    pub(crate) fn as_cond(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Na | Value::NaBool => Some(false),
            Value::Num(v) => Some(!v.is_nan() && *v != 0.0),
            _ => None,
        }
    }

    pub(crate) fn missing_like(&self) -> Self {
        match self {
            Value::Bool(_) | Value::NaBool => Value::NaBool,
            Value::Color(_) | Value::NaColor => Value::NaColor,
            Value::Str(_) | Value::NaString => Value::NaString,
            _ => Value::Na,
        }
    }

    pub(crate) fn coerce(self, kind: u8) -> Self {
        match kind {
            4 => Value::Bool(self.as_cond().unwrap_or(false)),
            1 if self.is_na() => Value::NaBool,
            1 => Value::Bool(self.as_cond().unwrap_or(false)),
            2 if self.is_na() => Value::NaColor,
            3 if self.is_na() => Value::NaString,
            _ => self,
        }
    }

    pub(crate) fn is_na(&self) -> bool {
        matches!(
            self,
            Value::Na | Value::NaBool | Value::NaColor | Value::NaString
        ) || matches!(self, Value::Num(v) if v.is_nan())
    }
}
