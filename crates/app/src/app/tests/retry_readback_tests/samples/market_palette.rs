use super::{Readback, Sample};
use serde_json::{Value, json};

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            41, // The market on screen, on its own bar and workspace: focused, so
            // no tab is added.
            ("market.open", 1, Value::Null, Readback::Stays),
        ),
        (
            42,
            (
                "market.palette.open",
                1,
                json!({ "query": "btc" }),
                Readback::Moves,
            ),
        ),
        (43, ("catalog.remove", 1, Value::Null, Readback::Moves)),
    ]
}
