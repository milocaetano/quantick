use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(12, ("chart.tape_view.set", 1, Value::Null, Readback::Moves))]
}
