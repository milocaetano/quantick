use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (44, ("home.show", 1, json!({}), Readback::Moves)),
        (52, ("home.leave", 1, json!({}), Readback::Moves)),
    ]
}
