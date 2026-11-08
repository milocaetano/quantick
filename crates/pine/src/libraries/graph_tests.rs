//! Authored mixed-graph contracts: cycles, alias identity and preview relocation.
use super::*;
fn object(arena: &mut Arena, fields: &[Value]) -> Value {
    let record = arena.allocate(7);
    arena.words[record] = arena.words[31];
    arena.words[31] = record as u64;
    arena.replace(record + 1, fields.to_vec());
    arena.replace(record + 4, vec![Value::Bool(true); fields.len()]);
    Value::UserObject((record + 1) as u32)
}
fn handle(value: &Value) -> usize {
    match value {
        Value::UserObject(h) | Value::Collection(_, h) => *h as usize,
        _ => panic!("reference"),
    }
}
#[test]
fn mixed_cycle_reachability_reclamation_and_first_preview() {
    let mut state = crate::eval::ScriptState::new(2);
    let snapshot = state.snapshot();
    let arena = state.arena.make_mut();
    let array = allocate(arena);
    let root = object(arena, &[Value::Collection(false, array as u32)]);
    arena.replace(array, vec![root.clone(), root.clone()]);
    arena.set(GLOBALS, 0, root);
    state.restore(snapshot, &[VarMode::Varip, VarMode::Var]);
    let root = state.arena.get(GLOBALS, 0);
    let array = state.arena.get(handle(&root), 0);
    assert_eq!(state.arena.get(handle(&array), 0), root);
    assert_eq!(state.arena.get(handle(&array), 1), root);
    collect(state.arena.make_mut());
    assert_ne!(state.arena.words[30], 0);
    assert_ne!(state.arena.words[31], 0);
    let _snapshot = state.snapshot();
    assert!(matches!(state.arena, crate::collections::Image::Shared(_)));
    state.arena.make_mut().set(GLOBALS, 0, Value::Na);
    collect(state.arena.make_mut());
    assert_eq!(state.arena.words[30], 0);
    assert_eq!(state.arena.words[31], 0);
}
#[test]
fn object_only_snapshot_shares_and_varip_updates_existing_alias() {
    let mut state = crate::eval::ScriptState::new(2);
    let root = object(state.arena.make_mut(), &[Value::Num(1.0)]);
    state.arena.make_mut().set(GLOBALS, 0, root.clone());
    state.arena.make_mut().set(GLOBALS, 1, root.clone());
    let snapshot = state.snapshot();
    assert!(matches!(state.arena, crate::collections::Image::Shared(_)));
    state
        .arena
        .make_mut()
        .set(handle(&root), 0, Value::Num(9.0));
    state.restore(snapshot, &[VarMode::Varip, VarMode::Var]);
    assert_eq!(state.arena.get(GLOBALS, 0), state.arena.get(GLOBALS, 1));
    assert_eq!(state.arena.get(handle(&root), 0), Value::Num(9.0));
}

#[test]
fn unreachable_mixed_cycles_reuse_the_arena_over_many_boundaries() {
    let mut state = crate::eval::ScriptState::new(1);
    let mut capacity = 0;
    for iteration in 0..2_000 {
        let arena = state.arena.make_mut();
        let array = allocate(arena);
        let root = object(arena, &[Value::Collection(false, array as u32)]);
        arena.replace(array, vec![root]);
        collect(arena);
        assert_eq!(arena.words[30], 0);
        assert_eq!(arena.words[31], 0);
        if iteration == 0 {
            capacity = arena.words.len();
        }
        assert_eq!(arena.words.len(), capacity);
    }
}
