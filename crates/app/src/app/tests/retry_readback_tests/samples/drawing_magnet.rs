use super::{Readback, Sample};
use serde_json::{Value, json};

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            63,
            (
                "annotate.magnet.set",
                1,
                json!({ "enabled": true }),
                Readback::Moves,
            ),
        ),
        (
            64, // Filled in when it runs: a line placed for the call to act on.
            (
                "annotate.drawing_magnet.set",
                1,
                Value::Null,
                Readback::Moves,
            ),
        ),
    ]
}
