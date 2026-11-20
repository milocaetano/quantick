use super::FAMILIES;

#[test]
fn family_contract_is_sorted() {
    assert!(!FAMILIES.is_empty());
    assert!(FAMILIES.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert_eq!(FAMILIES[0].id, "indicator_guide");
    assert_eq!(FAMILIES[1].id, "notify");
    assert_eq!(FAMILIES[0].actions, ["indicator_guide"]);
    assert_eq!(FAMILIES[1].readbacks, ["notify"]);
    assert!(FAMILIES[1].projections.is_empty());
    assert!(FAMILIES[1].behaviours.is_empty());
}

#[test]
fn consumers_resolve_bindings_locally() {
    fn indicator_guide() -> u32 {
        17
    }
    fn notify() -> u32 {
        29
    }
    macro_rules! local_actions {
        ($($id:literal {
            schemas: [$($schema:ident),*],
            actions: [$($action:ident),*],
            $($rest:ident: [$($binding:ident),*],)*
        })*) => {
            vec![$(($id, vec![$($action()),*])),*]
        };
    }
    let actions = crate::control_families!(local_actions);
    assert_eq!(
        actions,
        [("indicator_guide", vec![17]), ("notify", vec![29])]
    );
    assert_eq!(
        actions.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        FAMILIES.iter().map(|family| family.id).collect::<Vec<_>>()
    );
}
