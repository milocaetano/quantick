use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            18,
            ("orderflow.settings.open", 1, Value::Null, Readback::Moves),
        ),
        (
            19,
            (
                "orderflow.settings.section.set",
                1,
                Value::Null,
                Readback::Moves,
            ),
        ),
        (
            20,
            ("orderflow.settings.close", 1, Value::Null, Readback::Moves),
        ),
        (
            21,
            (
                "orderflow.settings.lights.set",
                1,
                Value::Null,
                Readback::Moves,
            ),
        ),
    ]
}
