use super::view;
// Array sequence operations and numeric reductions.
use super::{
    Op,
    runtime::{index, number, receiver},
};
use crate::{eval::arena::Arena, types::Value, version::Version};
use std::rc::Rc;
pub(super) fn execute(
    arena: &mut Arena,
    version: Version,
    op: Op,
    args: &[Value],
    record: usize,
    size: usize,
) -> Result<Value, String> {
    use Op::*;
    match op {
        BinarySearch | BinaryLeft | BinaryRight => super::search::execute(arena, op, args, record),
        Sort | SortIndices => super::sorting::execute(arena, op, args, record),
        Get(false) | Set | Remove(false) | Insert | First | Last | Pop | Shift => {
            let slot = match op {
                First | Shift => index(&Value::Num(0.0), size, false, version)?,
                Last | Pop => {
                    if size == 0 {
                        return Err("cannot read/remove an empty array".into());
                    }
                    size - 1
                }
                _ => index(&args[1], size, op == Insert, version)?,
            };
            let previous = if slot < size {
                view::get(arena, record, slot)
            } else {
                Value::Na
            };
            match op {
                Set => view::set(arena, record, slot, args[2].clone()),
                Insert => {
                    view::grow(arena, record, 1)?;
                    view::resize(arena, record, size + 1);
                    for i in (slot..size).rev() {
                        view::copy_value(arena, record, i, record, i + 1);
                    }
                    view::set(arena, record, slot, args[2].clone());
                }
                Remove(false) | Pop | Shift => {
                    for i in slot..size - 1 {
                        view::copy_value(arena, record, i + 1, record, i);
                    }
                    view::resize(arena, record, size - 1);
                }
                _ => {}
            }
            Ok(if matches!(op, Set | Insert) {
                Value::Na
            } else {
                previous
            })
        }
        Push | Unshift => {
            view::grow(arena, record, 1)?;
            view::resize(arena, record, size + 1);
            if op == Unshift {
                for i in (0..size).rev() {
                    view::copy_value(arena, record, i, record, i + 1);
                }
            }
            view::set(
                arena,
                record,
                if op == Push { size } else { 0 },
                args[1].clone(),
            );
            Ok(Value::Na)
        }
        Concat => {
            let other = receiver(&args[1], false)?;
            let values = view::values(arena, other);
            view::grow(arena, record, values.len())?;
            view::resize(arena, record, size + values.len());
            for (i, value) in values.into_iter().enumerate() {
                view::set(arena, record, size + i, value);
            }
            Ok(args[0].clone())
        }
        Reverse => {
            for i in 0..size / 2 {
                let left = view::get(arena, record, i);
                let right = view::get(arena, record, size - i - 1);
                view::set(arena, record, i, right);
                view::set(arena, record, size - i - 1, left);
            }
            Ok(Value::Na)
        }
        Fill => {
            let from = args.get(2).map_or(Ok(0), number)?;
            let to = args.get(3).map_or(Ok(size), number)?;
            if from > to || to > size {
                return Err("array.fill range out of bounds".into());
            }
            for i in from..to {
                view::set(arena, record, i, args[1].clone());
            }
            Ok(Value::Na)
        }
        Contains(false) | IndexOf | LastIndexOf => {
            let found = if op == LastIndexOf {
                (0..size).rfind(|i| view::get(arena, record, *i) == args[1])
            } else {
                (0..size).find(|i| view::get(arena, record, *i) == args[1])
            };
            Ok(if op == Contains(false) {
                Value::Bool(found.is_some())
            } else {
                Value::Num(found.map_or(-1.0, |i| i as f64))
            })
        }
        Sum | Avg | Min | Max | Abs | Covariance | Median | Mode | PercentileLinear
        | PercentileNearest | PercentRank | Range | Standardize | Stdev | Variance => {
            super::statistics::execute(arena, op, args, record)
        }
        Join => {
            let separator = match args.get(1) {
                Some(Value::Str(s)) => s.as_str(),
                _ => ",",
            };
            let values: Vec<_> = (0..size)
                .map(|i| match view::get(arena, record, i) {
                    Value::Str(s) => s.to_string(),
                    Value::Num(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    other => format!("{other:?}"),
                })
                .collect();
            Ok(Value::Str(Rc::new(values.join(separator))))
        }
        _ => unreachable!("array operation"),
    }
}
