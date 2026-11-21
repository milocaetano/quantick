use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        66, // Filled in when it runs: a dated line placed for the call to keep.
        (
            "annotate.drawing_scope.set",
            1,
            Value::Null,
            Readback::Moves,
        ),
    )]
}
