//! Runtime value and slot vocabulary. Owned by child 3 after skeleton integration.
use std::rc::Rc;

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
            Value::Na => Some(false),
            Value::Num(v) if v.is_nan() => Some(false),
            _ => None,
        }
    }

    pub(crate) fn is_na(&self) -> bool {
        matches!(self, Value::Na) || matches!(self, Value::Num(v) if v.is_nan())
    }
}
