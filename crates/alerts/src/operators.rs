//! Discoverable editor choices; target compatibility has one domain owner.
use crate::spec::{Operator, OperatorDescriptor, TargetKind};

const LINE: &[TargetKind] = &[TargetKind::Value, TargetKind::Line];
const COMPARE: &[TargetKind] = &[TargetKind::Value, TargetKind::Line, TargetKind::Rectangle];
const REGION: &[TargetKind] = &[TargetKind::Channel, TargetKind::Rectangle];

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
