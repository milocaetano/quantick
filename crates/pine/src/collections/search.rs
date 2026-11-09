//! Searches require an ascending array; absent bounds are insertion neighbors.
use super::{Op, view};
use crate::{eval::arena::Arena, types::Value};
pub(super) fn execute(
    arena: &Arena,
    op: Op,
    args: &[Value],
    record: usize,
) -> Result<Value, String> {
    let needle = args[1].as_num().unwrap_or(f64::NAN);
    if needle.is_nan() {
        return Ok(Value::Num(-1.0));
    }
    let length = view::len(arena, record);
    let bound = |upper: bool| {
        let mut low = 0;
        let mut high = length;
        while low < high {
            let middle = low + (high - low) / 2;
            let value = view::get(arena, record, middle)
                .as_num()
                .unwrap_or(f64::NAN);
            if value < needle || upper && value == needle {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low
    };
    let lower = bound(false);
    let found = lower < length && view::get(arena, record, lower).as_num() == Some(needle);
    let result = match op {
        Op::BinarySearch => {
            if found {
                lower as f64
            } else {
                -1.0
            }
        }
        Op::BinaryLeft => {
            if found {
                lower as f64
            } else {
                lower as f64 - 1.0
            }
        }
        Op::BinaryRight => {
            if found {
                bound(true) as f64 - 1.0
            } else {
                lower as f64
            }
        }
        _ => unreachable!("binary search operation"),
    };
    Ok(Value::Num(result))
}
