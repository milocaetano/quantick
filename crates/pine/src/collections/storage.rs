//! Reachability and varip transplantation; every persistent record stays in words.
use crate::{
    ast::VarMode,
    eval::arena::{Arena, GLOBALS, HISTORIES, LOCALS, REGISTERS},
    types::Value,
};
use std::collections::{BTreeMap, BTreeSet};
const COLLECTIONS: usize = 30;
pub(super) fn allocate(arena: &mut Arena) -> usize {
    let record = arena.allocate(9);
    arena.words[record] = arena.words[COLLECTIONS];
    arena.words[COLLECTIONS] = record as u64;
    record + 3
}
fn mark(value: &Value, live: &mut BTreeSet<usize>) {
    match value {
        Value::Collection(_, offset) => {
            live.insert(*offset as usize);
        }
        Value::Tuple(values) => {
            for value in values.iter() {
                mark(value, live);
            }
        }
        _ => {}
    }
}
pub(crate) fn collect(arena: &mut Arena) {
    if arena.words[COLLECTIONS] == 0 {
        return;
    }
    let mut live = BTreeSet::new();
    for list in [GLOBALS, LOCALS, REGISTERS] {
        for i in 0..arena.len(list) {
            mark(&arena.get(list, i), &mut live);
        }
    }
    let mut history = arena.words[HISTORIES] as usize;
    while history != 0 {
        for list in [history + 5, history + 8] {
            for i in 0..arena.len(list) {
                mark(&arena.get(list, i), &mut live);
            }
        }
        history = arena.words[history] as usize;
    }
    let mut pending: Vec<_> = live.iter().copied().collect();
    while let Some(handle) = pending.pop() {
        let parent = super::view::parent(arena, handle);
        if parent != 0 && live.insert(parent) {
            pending.push(parent);
        }
    }
    let mut link = COLLECTIONS;
    let mut record = arena.words[link] as usize;
    while record != 0 {
        let next = arena.words[record];
        if live.contains(&(record + 3)) {
            link = record;
        } else {
            arena.resize(record + 3, 0);
            let payload = arena.offset(record + 3);
            arena.release(payload);
            arena.release(record);
            arena.words[link] = next;
        }
        record = next as usize;
    }
}
pub(crate) fn restore_varip(current: &Arena, snapshot: &mut Arena, modes: &[VarMode]) {
    if current.words[COLLECTIONS] == 0 {
        return;
    }
    let mut existing = BTreeSet::new();
    let mut record = snapshot.words[COLLECTIONS] as usize;
    while record != 0 {
        existing.insert(record + 3);
        record = snapshot.words[record] as usize;
    }
    let mut copied = BTreeMap::new();
    for (slot, mode) in modes.iter().enumerate() {
        if *mode != VarMode::Varip {
            continue;
        }
        if let Value::Collection(map, offset) = current.get(GLOBALS, slot) {
            let target = transplant(current, snapshot, offset as usize, &existing, &mut copied);
            snapshot.set(
                GLOBALS,
                slot,
                Value::Collection(map, u32::try_from(target).expect("collection offset")),
            );
        }
    }
}
fn transplant(
    current: &Arena,
    snapshot: &mut Arena,
    source: usize,
    existing: &BTreeSet<usize>,
    copied: &mut BTreeMap<usize, usize>,
) -> usize {
    let mut pending = Vec::new();
    let mut cursor = source;
    while cursor != 0 && !copied.contains_key(&cursor) {
        let target = if existing.contains(&cursor) {
            cursor
        } else {
            allocate(snapshot)
        };
        copied.insert(cursor, target);
        pending.push(cursor);
        cursor = super::view::parent(current, cursor);
    }
    for source in pending.into_iter().rev() {
        let target = copied[&source];
        snapshot.replace(target, current.values(source));
        snapshot.set_at(target - 2, current.get_at(source - 2));
        let parent = super::view::parent(current, source);
        snapshot.words[target + 3] = if parent == 0 {
            0
        } else {
            copied[&parent] as u64
        };
        snapshot.words[target + 4] = current.words[source + 4];
        snapshot.words[target + 5] = current.words[source + 5];
    }
    copied[&source]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{Element, Op, execute};
    use crate::version::Version;
    #[test]
    fn collection_snapshot_shares_until_execution_write() {
        use std::rc::Rc;
        let mut state = crate::eval::ScriptState::new(1);
        let root = execute(
            Rc::make_mut(&mut state.arena),
            Version::V6,
            Op::From,
            &[Value::Num(1.0)],
        )
        .unwrap();
        Rc::make_mut(&mut state.arena).set(GLOBALS, 0, root.clone());
        let committed = state.arena.clone();
        let snapshot = state.snapshot();
        assert_eq!(Rc::strong_count(&state.arena), 3);
        execute(
            Rc::make_mut(&mut state.arena),
            Version::V6,
            Op::Push,
            &[root.clone(), Value::Num(2.0)],
        )
        .unwrap();
        assert!(!Rc::ptr_eq(&committed, &state.arena));
        let Value::Collection(_, handle) = root else {
            panic!("array handle");
        };
        assert_eq!(committed.len(handle as usize), 1);
        assert_eq!(state.arena.len(handle as usize), 2);
        state.restore(snapshot, &[VarMode::Var]);
        assert_eq!(state.arena.len(handle as usize), 1);
    }
    #[test]
    fn collection_image_growth_reclamation_and_cow() {
        let mut arena = Arena::default();
        arena.resize(GLOBALS, 1);
        let root = execute(&mut arena, Version::V5, Op::NewArray(Element::Float), &[]).unwrap();
        arena.set(GLOBALS, 0, root.clone());
        let snapshot = arena.clone();
        for i in 0..1000 {
            execute(
                &mut arena,
                Version::V5,
                Op::Push,
                &[root.clone(), Value::Num(f64::from(i))],
            )
            .unwrap();
        }
        assert_eq!(
            execute(
                &mut snapshot.clone(),
                Version::V5,
                Op::Size(false),
                std::slice::from_ref(&root)
            )
            .unwrap(),
            Value::Num(0.0)
        );
        assert_eq!(
            execute(
                &mut arena,
                Version::V5,
                Op::Size(false),
                std::slice::from_ref(&root)
            )
            .unwrap(),
            Value::Num(1000.0)
        );
        for _ in 0..100 {
            execute(&mut arena, Version::V5, Op::From, &[Value::Num(5.0)]).unwrap();
            collect(&mut arena);
        }
        let size = arena.words.len();
        for _ in 0..1000 {
            execute(&mut arena, Version::V5, Op::From, &[Value::Num(5.0)]).unwrap();
            collect(&mut arena);
        }
        assert_eq!(arena.words.len(), size);
    }
}
