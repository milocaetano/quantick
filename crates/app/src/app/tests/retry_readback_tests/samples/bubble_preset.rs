use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            24,
            (
                "orderflow.bubbles.preset.apply",
                1,
                Value::Null,
                Readback::Stays,
            ),
        ),
        (
            25,
            ("orderflow.bubbles.reset", 1, Value::Null, Readback::Moves),
        ),
    ]
}
