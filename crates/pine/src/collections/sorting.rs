use super::view;
// Stable scalar sorting; returned indices are an independent collection.
use super::{Op, runtime::create};
use crate::{eval::arena::Arena, types::Value};
use std::cmp::Ordering;
fn missing(value: &Value) -> bool {
    value.is_na() || matches!(value, Value::Str(s) if s.is_empty())
}
fn compare(a: &Value, b: &Value) -> Ordering {
    match (missing(a), missing(b)) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        _ => match (a, b) {
            (Value::Num(a), Value::Num(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Value::Str(a), Value::Str(b)) => a.cmp(b),
            _ => Ordering::Equal,
        },
    }
}
pub(super) fn execute(
    arena: &mut Arena,
    op: Op,
    args: &[Value],
    record: usize,
) -> Result<Value, String> {
    let descending = match args.get(1) {
        None => false,
        Some(Value::Str(s)) if s.as_str() == "ascending" => false,
        Some(Value::Str(s)) if s.as_str() == "descending" => true,
        _ => return Err("array sort order must be order.ascending or order.descending".into()),
    };
    let values = view::values(arena, record);
    let mut indices: Vec<_> = (0..values.len()).collect();
    indices.sort_by(|a, b| {
        let ordering = compare(&values[*a], &values[*b]);
        if descending {
            ordering.reverse()
        } else {
            ordering
        }
    });
    if op == Op::SortIndices {
        Ok(create(
            arena,
            false,
            indices.into_iter().map(|i| Value::Num(i as f64)).collect(),
        ))
    } else {
        for (slot, index) in indices.into_iter().enumerate() {
            view::set(arena, record, slot, values[index].clone());
        }
        Ok(Value::Na)
    }
}
