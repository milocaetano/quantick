use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        22,
        (
            "orderflow.bubbles.save_changes.set",
            1,
            Value::Null,
            Readback::Moves,
        ),
    )]
}
