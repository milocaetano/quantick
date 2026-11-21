use super::{Readback, Sample};
use serde_json::{Value, json};

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            45,
            (
                "settings.open",
                1,
                json!({ "section": "data" }),
                Readback::Moves,
            ),
        ),
        (46, ("settings.set", 1, Value::Null, Readback::Moves)),
        (47, ("desk.save", 1, json!({}), Readback::Moves)),
        (48, ("desk.export", 1, json!({}), Readback::Stays)),
        (49, ("desk.import", 1, json!({}), Readback::Stays)),
        (50, ("desk.forget", 1, json!({}), Readback::Moves)),
        (
            51,
            (
                "desk.reopen_on_launch.set",
                1,
                json!({ "reopen": false }),
                Readback::Moves,
            ),
        ),
    ]
}
