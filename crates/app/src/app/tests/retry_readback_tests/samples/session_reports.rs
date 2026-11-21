use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(93, ("session.report.open", 1, Value::Null, Readback::Moves))]
}
