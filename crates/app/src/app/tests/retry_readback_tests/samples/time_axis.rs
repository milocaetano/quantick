use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        13,
        ("chart.time_axis.reset", 1, Value::Null, Readback::Moves),
    )]
}
