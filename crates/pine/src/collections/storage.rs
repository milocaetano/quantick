//! Reachability and varip transplantation; every persistent record stays in words.
use crate::{
    ast::VarMode,
    eval::arena::{Arena, GLOBALS, HISTORIES, LOCALS, REGISTERS},
    types::Value,
};
use std::collections::{BTreeMap, BTreeSet};
const COLLECTIONS: usize = 30;
const OBJECTS: usize = 31;
pub(super) fn allocate(arena: &mut Arena) -> usize {
    allocate_record(arena, false)
}
fn allocate_record(arena: &mut Arena, object: bool) -> usize {
    let (root, size, header) = if object {
        (OBJECTS, 7, 1)
    } else {
        (COLLECTIONS, 9, 3)
    };
    let record = arena.allocate(size);
    arena.words[record] = arena.words[root];
    arena.words[root] = record as u64;
    record + header
}
fn mark(value: &Value, live: &mut BTreeMap<usize, bool>) {
    match value {
        Value::Collection(_, offset) => {
            live.insert(*offset as usize, false);
        }
        Value::UserObject(offset) => {
            live.insert(*offset as usize, true);
        }
        Value::Tuple(values) => {
            for value in values.iter() {
                mark(value, live);
            }
        }
        _ => {}
    }
}
fn children(arena: &Arena, handle: usize, object: bool) -> BTreeMap<usize, bool> {
    let mut refs = BTreeMap::new();
    for value in arena.values(handle) {
        mark(&value, &mut refs);
    }
    if !object {
        mark(&arena.get_at(handle - 2), &mut refs);
        let parent = super::view::parent(arena, handle);
        if parent != 0 {
            refs.insert(parent, false);
        }
    }
    refs
}
pub(crate) fn collect(arena: &mut Arena) {
    if arena.words[COLLECTIONS] == 0 && arena.words[OBJECTS] == 0 {
        return;
    }
    let mut live = BTreeMap::new();
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
    let mut pending: Vec<_> = live.iter().map(|(h, o)| (*h, *o)).collect();
    while let Some((handle, object)) = pending.pop() {
        for (child, object) in children(arena, handle, object) {
            if live.insert(child, object).is_none() {
                pending.push((child, object));
            }
        }
    }
    for (root, header) in [(COLLECTIONS, 3), (OBJECTS, 1)] {
        let mut link = root;
        let mut record = arena.words[link] as usize;
        while record != 0 {
            let next = arena.words[record];
            if live.contains_key(&(record + header)) {
                link = record;
            } else {
                arena.resize(record + header, 0);
                let payload = arena.offset(record + header);
                arena.release(payload);
                if root == OBJECTS {
                    let modes = arena.offset(record + 4);
                    arena.release(modes);
                }
                arena.release(record);
                arena.words[link] = next;
            }
            record = next as usize;
        }
    }
}
pub(crate) fn restore_varip(current: &Arena, snapshot: &mut Arena, modes: &[VarMode]) {
    if current.words[COLLECTIONS] == 0 && current.words[OBJECTS] == 0 {
        for (slot, mode) in modes.iter().enumerate() {
            if *mode == VarMode::Varip {
                snapshot.set(GLOBALS, slot, current.get(GLOBALS, slot));
            }
        }
        return;
    }
    let mut existing = BTreeSet::new();
    for (root, header) in [(COLLECTIONS, 3), (OBJECTS, 1)] {
        let mut record = snapshot.words[root] as usize;
        while record != 0 {
            existing.insert(record + header);
            record = snapshot.words[record] as usize;
        }
    }
    let mut reachable = BTreeMap::new();
    for (slot, mode) in modes.iter().enumerate() {
        if *mode == VarMode::Varip {
            mark(&current.get(GLOBALS, slot), &mut reachable);
        }
    }
    let mut record = snapshot.words[OBJECTS] as usize;
    while record != 0 {
        let handle = record + 1;
        if current.values(handle + 3).contains(&Value::Bool(true)) {
            reachable.insert(handle, true);
        }
        record = snapshot.words[record] as usize;
    }
    let mut pending: Vec<_> = reachable.iter().map(|(h, o)| (*h, *o)).collect();
    let mut copied = BTreeMap::new();
    while let Some((source, object)) = pending.pop() {
        let target = if existing.contains(&source) {
            source
        } else {
            allocate_record(snapshot, object)
        };
        copied.insert(source, target);
        let mut refs = BTreeMap::new();
        if object && existing.contains(&source) {
            for i in 0..current.len(source) {
                if current.get(source + 3, i) == Value::Bool(true) {
                    mark(&current.get(source, i), &mut refs);
                }
            }
        } else {
            refs = children(current, source, object);
        }
        for (child, object) in refs {
            if reachable.insert(child, object).is_none() {
                pending.push((child, object));
            }
        }
    }
    // Allocate the whole graph before writing any edge, including back edges.
    for (source, object) in reachable {
        let target = copied[&source];
        if object && existing.contains(&source) {
            for i in 0..current.len(source) {
                if current.get(source + 3, i) == Value::Bool(true) {
                    snapshot.set(target, i, remap(current.get(source, i), &copied));
                }
            }
        } else {
            let values = current
                .values(source)
                .into_iter()
                .map(|v| remap(v, &copied))
                .collect();
            snapshot.replace(target, values);
        }
        if object {
            snapshot.replace(target + 3, current.values(source + 3));
        }
        if !object {
            snapshot.set_at(target - 2, remap(current.get_at(source - 2), &copied));
            let parent = super::view::parent(current, source);
            snapshot.words[target + 3] = copied.get(&parent).copied().unwrap_or(0) as u64;
            snapshot.words[target + 4] = current.words[source + 4];
            snapshot.words[target + 5] = current.words[source + 5];
        }
    }
    for (slot, mode) in modes.iter().enumerate() {
        if *mode == VarMode::Varip {
            snapshot.set(GLOBALS, slot, remap(current.get(GLOBALS, slot), &copied));
        }
    }
}
fn remap(value: Value, copied: &BTreeMap<usize, usize>) -> Value {
    match value {
        Value::Collection(map, source) => Value::Collection(map, copied[&(source as usize)] as u32),
        Value::UserObject(source) => Value::UserObject(copied[&(source as usize)] as u32),
        Value::Tuple(values) => Value::Tuple(std::rc::Rc::new(
            values.iter().cloned().map(|v| remap(v, copied)).collect(),
        )),
        value => value,
    }
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
            state.arena.make_mut(),
            Version::V6,
            Op::From,
            &[Value::Num(1.0)],
        )
        .unwrap();
        state.arena.make_mut().set(GLOBALS, 0, root.clone());
        let snapshot = state.snapshot();
        let super::super::Image::Shared(committed) = state.arena.clone() else {
            panic!("shared collection image");
        };
        assert_eq!(Rc::strong_count(&committed), 3);
        execute(
            state.arena.make_mut(),
            Version::V6,
            Op::Push,
            &[root.clone(), Value::Num(2.0)],
        )
        .unwrap();
        let super::super::Image::Shared(current) = &state.arena else {
            panic!("shared collection image");
        };
        assert!(!Rc::ptr_eq(&committed, current));
        let Value::Collection(_, handle) = root else {
            panic!("array handle");
        };
        assert_eq!(committed.len(handle as usize), 1);
        assert_eq!(state.arena.len(handle as usize), 2);
        state.restore(snapshot, &[VarMode::Var]);
        assert_eq!(state.arena.len(handle as usize), 1);
    }
    #[test]
    fn first_varip_allocation_promotes_next_snapshot() {
        use super::super::Image;
        let mut state = crate::eval::ScriptState::new(1);
        let snapshot = state.snapshot();
        assert!(matches!(state.arena, Image::Owned(_)));
        let root = execute(
            state.arena.make_mut(),
            Version::V6,
            Op::From,
            &[Value::Num(1.0)],
        )
        .unwrap();
        state.arena.make_mut().set(GLOBALS, 0, root);
        state.restore(snapshot, &[VarMode::Varip]);
        assert!(matches!(state.arena, Image::Owned(_)));
        let snapshot = state.snapshot();
        assert!(matches!(state.arena, Image::Shared(_)));
        let root = state.arena.get(GLOBALS, 0);
        execute(
            state.arena.make_mut(),
            Version::V6,
            Op::Push,
            &[root, Value::Num(2.0)],
        )
        .unwrap();
        state.restore(snapshot, &[VarMode::Varip]);
        let root = state.arena.get(GLOBALS, 0);
        assert_eq!(
            execute(
                state.arena.make_mut(),
                Version::V6,
                Op::Size(false),
                &[root]
            )
            .unwrap(),
            Value::Num(2.0)
        );
        state.arena.make_mut().set(GLOBALS, 0, Value::Na);
        collect(state.arena.make_mut());
        assert_eq!(state.arena.words[COLLECTIONS], 0);
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

#[cfg(test)]
#[path = "../libraries/graph_tests.rs"]
mod mixed_graph_tests;
