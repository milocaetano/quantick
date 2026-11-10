//! Discoverable editor choices; target compatibility has one domain owner.
use crate::spec::{Operator, OperatorDescriptor, TargetKind};

const LINE: &[TargetKind] = &[TargetKind::Value, TargetKind::Line];
const COMPARE: &[TargetKind] = &[TargetKind::Value, TargetKind::Line, TargetKind::Rectangle];
const REGION: &[TargetKind] = &[TargetKind::Channel, TargetKind::Rectangle];

/// Preserve a compatible choice when editing a target, otherwise use its first
/// catalog choice. Every supported target kind has at least one operator.
pub fn for_target(current: Operator, target: TargetKind) -> Operator {
    if OPERATORS
        .iter()
        .any(|item| item.operator == current && item.targets.contains(&target))
    {
        current
    } else {
        OPERATORS
            .iter()
            .find(|item| item.targets.contains(&target))
            .expect("every target kind has an operator")
            .operator
    }
}

pub const OPERATORS: &[OperatorDescriptor] = &[
    OperatorDescriptor {
        operator: Operator::Crossing,
        label: "Crossing",
        targets: LINE,
    },
    OperatorDescriptor {
        operator: Operator::CrossingUp,
        label: "Crossing Up",
        targets: LINE,
    },
    OperatorDescriptor {
        operator: Operator::CrossingDown,
        label: "Crossing Down",
        targets: LINE,
    },
    OperatorDescriptor {
        operator: Operator::GreaterThan,
        label: "Greater Than",
        targets: COMPARE,
    },
    OperatorDescriptor {
        operator: Operator::LessThan,
        label: "Less Than",
        targets: COMPARE,
    },
    OperatorDescriptor {
        operator: Operator::Entering,
        label: "Entering",
        targets: REGION,
    },
    OperatorDescriptor {
        operator: Operator::Exiting,
        label: "Exiting",
        targets: REGION,
    },
    OperatorDescriptor {
        operator: Operator::Inside,
        label: "Inside",
        targets: REGION,
    },
    OperatorDescriptor {
        operator: Operator::Outside,
        label: "Outside",
        targets: REGION,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_changes_preserve_only_compatible_operators() {
        assert_eq!(
            for_target(Operator::Crossing, TargetKind::Channel),
            Operator::Entering
        );
        assert_eq!(
            for_target(Operator::Crossing, TargetKind::Rectangle),
            Operator::GreaterThan
        );
        assert_eq!(
            for_target(Operator::Inside, TargetKind::Value),
            Operator::Crossing
        );
        assert_eq!(
            for_target(Operator::LessThan, TargetKind::Rectangle),
            Operator::LessThan
        );
        for kind in [
            TargetKind::Value,
            TargetKind::Line,
            TargetKind::Channel,
            TargetKind::Rectangle,
        ] {
            for item in OPERATORS {
                let selected = for_target(item.operator, kind);
                assert!(
                    OPERATORS
                        .iter()
                        .any(|entry| entry.operator == selected && entry.targets.contains(&kind))
                );
            }
        }
    }
}
