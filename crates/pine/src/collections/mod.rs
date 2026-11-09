//! Typed collections backed solely by the interpreter word arena.
mod array;
mod image;
pub(crate) use image::Image;
mod runtime;
mod search;
mod sorting;
mod statistics;
pub(crate) use statistics::truncate;
mod storage;
mod view;
use crate::error::ErrorCode;
pub(crate) use runtime::execute;
pub(crate) use storage::{collect, restore_varip};

/// Element types admitted by the scalar collection constructors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Element {
    User(u32),
    Int,
    Float,
    Bool,
    String,
    Color,
}
/// A statically resolved collection instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    NewArray(Element),
    NewMap(Element, Element),
    From,
    Size(bool),
    Clear(bool),
    Copy(bool),
    Get(bool),
    Remove(bool),
    Contains(bool),
    Set,
    Push,
    Pop,
    Shift,
    Unshift,
    Insert,
    First,
    Last,
    Reverse,
    Concat,
    Fill,
    BinarySearch,
    BinaryLeft,
    BinaryRight,
    IndexOf,
    LastIndexOf,
    Abs,
    Covariance,
    Median,
    Mode,
    PercentileLinear,
    PercentileNearest,
    PercentRank,
    Range,
    Standardize,
    Stdev,
    Variance,
    Sum,
    Avg,
    Min,
    Max,
    Join,
    Slice,
    Sort,
    SortIndices,
    Put,
    PutAll,
    Keys,
    Values,
}
const REGISTRY: &[(&str, Op)] = &[
    ("array.new_int", Op::NewArray(Element::Int)),
    ("array.new_float", Op::NewArray(Element::Float)),
    ("array.new_bool", Op::NewArray(Element::Bool)),
    ("array.new_string", Op::NewArray(Element::String)),
    ("array.new_color", Op::NewArray(Element::Color)),
    ("map.new_int_int", Op::NewMap(Element::Int, Element::Int)),
    (
        "map.new_int_float",
        Op::NewMap(Element::Int, Element::Float),
    ),
    ("map.new_int_bool", Op::NewMap(Element::Int, Element::Bool)),
    (
        "map.new_int_string",
        Op::NewMap(Element::Int, Element::String),
    ),
    (
        "map.new_int_color",
        Op::NewMap(Element::Int, Element::Color),
    ),
    (
        "map.new_float_int",
        Op::NewMap(Element::Float, Element::Int),
    ),
    (
        "map.new_float_float",
        Op::NewMap(Element::Float, Element::Float),
    ),
    (
        "map.new_float_bool",
        Op::NewMap(Element::Float, Element::Bool),
    ),
    (
        "map.new_float_string",
        Op::NewMap(Element::Float, Element::String),
    ),
    (
        "map.new_float_color",
        Op::NewMap(Element::Float, Element::Color),
    ),
    ("map.new_bool_int", Op::NewMap(Element::Bool, Element::Int)),
    (
        "map.new_bool_float",
        Op::NewMap(Element::Bool, Element::Float),
    ),
    (
        "map.new_bool_bool",
        Op::NewMap(Element::Bool, Element::Bool),
    ),
    (
        "map.new_bool_string",
        Op::NewMap(Element::Bool, Element::String),
    ),
    (
        "map.new_bool_color",
        Op::NewMap(Element::Bool, Element::Color),
    ),
    (
        "map.new_string_int",
        Op::NewMap(Element::String, Element::Int),
    ),
    (
        "map.new_string_float",
        Op::NewMap(Element::String, Element::Float),
    ),
    (
        "map.new_string_bool",
        Op::NewMap(Element::String, Element::Bool),
    ),
    (
        "map.new_string_string",
        Op::NewMap(Element::String, Element::String),
    ),
    (
        "map.new_string_color",
        Op::NewMap(Element::String, Element::Color),
    ),
    (
        "map.new_color_int",
        Op::NewMap(Element::Color, Element::Int),
    ),
    (
        "map.new_color_float",
        Op::NewMap(Element::Color, Element::Float),
    ),
    (
        "map.new_color_bool",
        Op::NewMap(Element::Color, Element::Bool),
    ),
    (
        "map.new_color_string",
        Op::NewMap(Element::Color, Element::String),
    ),
    (
        "map.new_color_color",
        Op::NewMap(Element::Color, Element::Color),
    ),
    ("array.size", Op::Size(false)),
    ("array.clear", Op::Clear(false)),
    ("array.copy", Op::Copy(false)),
    ("array.get", Op::Get(false)),
    ("array.remove", Op::Remove(false)),
    ("array.includes", Op::Contains(false)),
    ("map.size", Op::Size(true)),
    ("map.clear", Op::Clear(true)),
    ("map.copy", Op::Copy(true)),
    ("map.get", Op::Get(true)),
    ("map.remove", Op::Remove(true)),
    ("map.contains", Op::Contains(true)),
    ("array.from", Op::From),
    ("array.set", Op::Set),
    ("array.push", Op::Push),
    ("array.pop", Op::Pop),
    ("array.shift", Op::Shift),
    ("array.unshift", Op::Unshift),
    ("array.insert", Op::Insert),
    ("array.first", Op::First),
    ("array.last", Op::Last),
    ("array.reverse", Op::Reverse),
    ("array.concat", Op::Concat),
    ("array.fill", Op::Fill),
    ("array.binary_search", Op::BinarySearch),
    ("array.binary_search_leftmost", Op::BinaryLeft),
    ("array.binary_search_rightmost", Op::BinaryRight),
    ("array.indexof", Op::IndexOf),
    ("array.lastindexof", Op::LastIndexOf),
    ("array.abs", Op::Abs),
    ("array.covariance", Op::Covariance),
    ("array.median", Op::Median),
    ("array.mode", Op::Mode),
    (
        "array.percentile_linear_interpolation",
        Op::PercentileLinear,
    ),
    ("array.percentile_nearest_rank", Op::PercentileNearest),
    ("array.percentrank", Op::PercentRank),
    ("array.range", Op::Range),
    ("array.standardize", Op::Standardize),
    ("array.stdev", Op::Stdev),
    ("array.variance", Op::Variance),
    ("array.sum", Op::Sum),
    ("array.avg", Op::Avg),
    ("array.min", Op::Min),
    ("array.max", Op::Max),
    ("array.join", Op::Join),
    ("array.slice", Op::Slice),
    ("array.sort", Op::Sort),
    ("array.sort_indices", Op::SortIndices),
    ("map.put", Op::Put),
    ("map.put_all", Op::PutAll),
    ("map.keys", Op::Keys),
    ("map.values", Op::Values),
];
pub(crate) fn lookup(name: &str) -> Option<Op> {
    REGISTRY
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, op)| *op)
}
pub(crate) fn names() -> impl Iterator<Item = &'static str> {
    REGISTRY.iter().map(|(name, _)| *name)
}
pub(crate) fn rejection() -> (ErrorCode, &'static str) {
    (
        ErrorCode::PineNoCollections,
        "this collection operation is not supported; see the dialect collection operation table",
    )
}
pub(crate) fn arguments(op: Op) -> &'static [&'static str] {
    use Op::*;
    match op {
        NewArray(_) => &["size", "initial_value"],
        NewMap(..) | From => &[],
        Get(true) | Remove(true) | Contains(true) => &["id", "key"],
        Get(false) | Remove(false) => &["id", "index"],
        Set | Insert => &["id", "index", "value"],
        Put => &["id", "key", "value"],
        PutAll => &["id", "id2"],
        Concat => &["id1", "id2"],
        BinarySearch | BinaryLeft | BinaryRight | Push | Unshift | Contains(false) | IndexOf
        | LastIndexOf => &["id", "value"],
        Fill => &["id", "value", "index_from", "index_to"],
        Min | Max => &["id", "nth"],
        Covariance => &["id1", "id2", "biased"],
        Stdev | Variance => &["id", "biased"],
        PercentileLinear | PercentileNearest => &["id", "percentage"],
        PercentRank => &["id", "index"],
        Slice => &["id", "index_from", "index_to"],
        Sort | SortIndices => &["id", "order"],
        Join => &["id", "separator"],
        _ => &["id"],
    }
}

/// Persistent collection mutations stay conditional during eager kernel advancement.
pub(crate) fn mutates(op: Op) -> bool {
    use Op::*;
    matches!(
        op,
        Clear(_)
            | Remove(_)
            | Set
            | Push
            | Pop
            | Shift
            | Unshift
            | Insert
            | Reverse
            | Concat
            | Fill
            | Sort
            | Put
            | PutAll
    )
}
