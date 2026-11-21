use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        26,
        (
            "orderflow.bubbles.tick.set",
            1,
            Value::Null,
            Readback::Stays,
        ),
    )]
}
