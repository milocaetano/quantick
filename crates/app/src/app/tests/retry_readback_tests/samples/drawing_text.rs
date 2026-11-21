use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(65, ("annotate.text.set", 1, Value::Null, Readback::Moves))]
}
