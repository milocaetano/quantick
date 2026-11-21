use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        8,
        ("indicator.library.edit", 1, Value::Null, Readback::Moves),
    )]
}
