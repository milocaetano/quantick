//! Collection views keep a parent handle and range in the authoritative image.
use crate::{eval::arena::Arena, types::Value};
pub(super) fn parent(arena: &Arena, record: usize) -> usize {
    arena.words[record + 3] as usize
}
pub(super) fn len(arena: &Arena, mut record: usize) -> usize {
    let mut range = Vec::new();
    while parent(arena, record) != 0 {
        range.push((
            arena.words[record + 4] as usize,
            arena.words[record + 5] as usize,
        ));
        record = parent(arena, record);
    }
    range
        .into_iter()
        .rev()
        .fold(arena.len(record), |size, (from, extent)| {
            size.saturating_sub(from).min(extent)
        })
}
pub(super) fn grow(arena: &Arena, mut record: usize, by: usize) -> Result<(), String> {
    loop {
        super::runtime::grow(len(arena, record), by)?;
        record = parent(arena, record);
        if record == 0 {
            return Ok(());
        }
    }
}
fn location(arena: &Arena, mut record: usize, mut slot: usize) -> (usize, usize) {
    while parent(arena, record) != 0 {
        slot += arena.words[record + 4] as usize;
        record = parent(arena, record);
    }
    (record, slot)
}
pub(super) fn get(arena: &Arena, record: usize, slot: usize) -> Value {
    let (record, slot) = location(arena, record, slot);
    arena.get(record, slot)
}
pub(super) fn set(arena: &mut Arena, record: usize, slot: usize, value: Value) {
    let (record, slot) = location(arena, record, slot);
    arena.set(record, slot, value);
}
pub(super) fn values(arena: &Arena, record: usize) -> Vec<Value> {
    (0..len(arena, record))
        .map(|i| get(arena, record, i))
        .collect()
}
pub(super) fn copy_value(arena: &mut Arena, from: usize, source: usize, to: usize, target: usize) {
    let (from, source) = location(arena, from, source);
    let (to, target) = location(arena, to, target);
    arena.copy_value(from, source, to, target);
}
// Resizing a view splices at its end, preserving the parent's suffix. Only
// operations through this view change its maximum range; parent edits do not.
pub(super) fn resize(arena: &mut Arena, record: usize, length: usize) {
    let old = len(arena, record);
    if parent(arena, record) == 0 {
        arena.resize(record, length);
        return;
    }
    if length > old {
        splice(arena, record, old, 0, length - old);
    } else {
        splice(arena, record, length, old - length, 0);
    }
}
fn splice(arena: &mut Arena, record: usize, at: usize, remove: usize, insert: usize) {
    let parent = parent(arena, record);
    if parent != 0 {
        let start = arena.words[record + 4] as usize;
        splice(
            arena,
            parent,
            start.min(len(arena, parent)) + at,
            remove,
            insert,
        );
        arena.words[record + 5] = (arena.words[record + 5] as usize + insert - remove) as u64;
        return;
    }
    let old = arena.len(record);
    let new = old + insert - remove;
    if new > old {
        arena.resize(record, new);
    }
    if insert > remove {
        for i in (at + remove..old).rev() {
            arena.copy_value(record, i, record, i + insert - remove);
        }
    } else {
        for i in at + remove..old {
            arena.copy_value(record, i, record, i + insert - remove);
        }
    }
    for i in at..at + insert {
        arena.set(record, i, Value::Na);
    }
    if new < old {
        arena.resize(record, new);
    }
}
pub(super) fn slice(
    arena: &mut Arena,
    source: usize,
    from: usize,
    to: usize,
) -> Result<Value, String> {
    if from >= to || to > len(arena, source) {
        return Err("array.slice requires 0 <= index_from < index_to <= size".into());
    }
    let target = super::storage::allocate(arena);
    arena.words[target + 3] = source as u64;
    arena.words[target + 4] = from as u64;
    arena.words[target + 5] = (to - from) as u64;
    Ok(Value::Collection(
        false,
        u32::try_from(target).expect("collection offset"),
    ))
}
