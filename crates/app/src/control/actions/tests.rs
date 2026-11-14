use super::*;
use quantick_control_host::authority::{ANNOTATE_EFFECT_ID, CAPABILITY_VERSION, TRADE_EFFECT_ID};

/// The families that moved to `quantick_control_handlers` are still docked
/// here, behind the effects and permissions the contract names.
#[test]
fn the_standard_actions_still_carry_the_moved_families() {
    let registry = standard_actions().unwrap();
    let mark = registry
        .lookup(MARK_CAPABILITY_ID, CAPABILITY_VERSION)
        .unwrap();
    assert_eq!(mark.descriptor.effect.as_str(), ANNOTATE_EFFECT_ID);
    assert!(
        mark.descriptor
            .required_permissions
            .iter()
            .any(|permission| {
                permission.as_str()
                    == quantick_control_host::authority::ANNOTATE_ATTENTION_PERMISSION_ID
            })
    );
    for id in [
        quantick_control_handlers::trade::PLACE_CAPABILITY_ID,
        quantick_control_handlers::trade::BRACKET_CAPABILITY_ID,
        quantick_control_handlers::trade::CANCEL_CAPABILITY_ID,
    ] {
        let action = registry
            .lookup(id, CAPABILITY_VERSION)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(action.descriptor.effect.as_str(), TRADE_EFFECT_ID);
        assert!(!action.descriptor.read_only);
        assert!(
            action
                .descriptor
                .required_permissions
                .iter()
                .any(|permission| {
                    permission.as_str() == quantick_control_handlers::trade::TRADE_PERMISSION_ID
                })
        );
    }
    let cancel = registry
        .lookup(quantick_control_handlers::trade::CANCEL_CAPABILITY_ID, 1)
        .unwrap();
    assert!(!cancel.descriptor.destructive && cancel.descriptor.risk_reducing);
}
