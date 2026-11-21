use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(94, ("alerts.open", 1, json!({}), Readback::Moves))]
}
