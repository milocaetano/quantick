use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        15,
        ("orderflow.tape.view.set", 1, Value::Null, Readback::Moves),
    )]
}
