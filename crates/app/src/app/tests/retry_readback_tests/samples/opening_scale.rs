use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        14,
        (
            "orderflow.tape.opening_scale.set",
            1,
            Value::Null,
            Readback::Moves,
        ),
    )]
}
