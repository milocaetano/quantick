use super::view;
// Ordered scalar records stored in the one authoritative arena.
use super::{Element, Op};
use crate::{eval::arena::Arena, types::Value, version::Version};
const MAX_ELEMENTS: usize = 100_000;
pub(crate) fn create(arena: &mut Arena, map: bool, values: Vec<Value>) -> Value {
    let record = super::storage::allocate(arena);
    arena.replace(record, values);
    Value::Collection(map, u32::try_from(record).expect("bounded arena"))
}
pub(super) fn number(value: &Value) -> Result<usize, String> {
    match value {
        Value::Num(n)
            if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n <= MAX_ELEMENTS as f64 =>
        {
            Ok(*n as usize)
        }
        _ => Err("collection size/index must be a nonnegative integer within 100000".into()),
    }
}
pub(super) fn index(
    value: &Value,
    size: usize,
    end: bool,
    version: Version,
) -> Result<usize, String> {
    let Value::Num(n) = value else {
        return Err("array index must be an integer".into());
    };
    let n = if *n < 0.0 && version == Version::V6 {
        size as f64 + *n
    } else {
        *n
    };
    let slot = number(&Value::Num(n))?;
    if slot < size || end && slot == size {
        Ok(slot)
    } else {
        Err(format!("array index {n} out of bounds for size {size}"))
    }
}
fn default_value(element: Element, version: Version) -> Value {
    match element {
        Element::Bool if version == Version::V6 => Value::Bool(false),
        Element::Bool => Value::NaBool,
        Element::Color => Value::NaColor,
        Element::String => Value::NaString,
        _ => Value::Na,
    }
}
pub(super) fn receiver(value: &Value, map: bool) -> Result<usize, String> {
    if let Value::Collection(kind, offset) = value
        && *kind == map
    {
        Ok(*offset as usize)
    } else {
        Err("missing or wrong collection handle".into())
    }
}
pub(super) fn grow(size: usize, by: usize) -> Result<(), String> {
    if size.saturating_add(by) > MAX_ELEMENTS {
        Err("collection exceeds 100000 elements (50000 key/value pairs)".into())
    } else {
        Ok(())
    }
}
pub(crate) fn execute(
    arena: &mut Arena,
    version: Version,
    op: Op,
    args: &[Value],
) -> Result<Value, String> {
    use Op::*;
    match op {
        NewArray(element) => {
            let size = args.first().map_or(Ok(0), number)?;
            let initial = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| default_value(element, version));
            return Ok(create(arena, false, vec![initial; size]));
        }
        NewMap(_, element) => {
            let value = create(arena, true, vec![]);
            let record = receiver(&value, true)?;
            arena.set_at(record - 2, default_value(element, version));
            return Ok(value);
        }
        From => {
            grow(0, args.len())?;
            return Ok(create(arena, false, args.to_vec()));
        }
        _ => {}
    }
    let map = matches!(
        op,
        Size(true)
            | Clear(true)
            | Copy(true)
            | Get(true)
            | Remove(true)
            | Contains(true)
            | Put
            | PutAll
            | Keys
            | Values
    );
    let record = receiver(&args[0], map)?;
    let size = view::len(arena, record);
    if op == Slice {
        return view::slice(arena, record, number(&args[1])?, number(&args[2])?);
    }
    match op {
        Size(_) => Ok(Value::Num((if map { size / 2 } else { size }) as f64)),
        Clear(_) => {
            view::resize(arena, record, 0);
            Ok(Value::Na)
        }
        Copy(_) => {
            let values = view::values(arena, record);
            let copy = create(arena, map, values);
            let target = receiver(&copy, map)?;
            let missing = arena.get_at(record - 2);
            arena.set_at(target - 2, missing);
            Ok(copy)
        }
        Keys | Values => {
            let values = (if op == Keys { 0 } else { 1 }..size)
                .step_by(2)
                .map(|i| view::get(arena, record, i))
                .collect();
            Ok(create(arena, false, values))
        }
        Get(true) | Remove(true) | Contains(true) | Put => {
            let found = (0..size)
                .step_by(2)
                .find(|i| view::get(arena, record, *i) == args[1]);
            if op == Contains(true) {
                return Ok(Value::Bool(found.is_some()));
            }
            let previous = found.map_or_else(
                || arena.get_at(record - 2),
                |i| view::get(arena, record, i + 1),
            );
            if op == Put {
                if args[1].is_na() {
                    return Err("map key cannot be na".into());
                }
                let slot = match found {
                    Some(i) => i,
                    None => {
                        grow(size, 2)?;
                        view::resize(arena, record, size + 2);
                        view::set(arena, record, size, args[1].clone());
                        size
                    }
                };
                view::set(arena, record, slot + 1, args[2].clone());
            } else if op == Remove(true)
                && let Some(slot) = found
            {
                for i in slot..size - 2 {
                    view::copy_value(arena, record, i + 2, record, i);
                }
                view::resize(arena, record, size - 2);
            }
            Ok(previous)
        }
        PutAll => {
            let other = receiver(&args[1], true)?;
            let values = view::values(arena, other);
            for pair in values.as_chunks::<2>().0 {
                execute(
                    arena,
                    version,
                    Put,
                    &[args[0].clone(), pair[0].clone(), pair[1].clone()],
                )?;
            }
            Ok(Value::Na)
        }
        _ => super::array::execute(arena, version, op, args, record, size),
    }
}
