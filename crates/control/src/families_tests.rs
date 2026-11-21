use super::FAMILIES;

#[test]
fn family_contract_is_sorted() {
    assert!(!FAMILIES.is_empty());
    assert!(FAMILIES.windows(2).all(|pair| pair[0].id < pair[1].id));
    let context = FAMILIES
        .iter()
        .find(|row| row.id == "chart_context")
        .unwrap();
    assert_eq!(context.schemas, ["chart_context"]);
    assert_eq!(context.actions, ["chart_context"]);
    assert!(context.readbacks.is_empty());
    let scope = FAMILIES
        .iter()
        .find(|row| row.id == "drawing_scope")
        .unwrap();
    assert!(scope.actions.is_empty());
    assert_eq!(scope.readbacks, ["drawing_scope"]);
}
