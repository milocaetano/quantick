use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        5, // On by default: the call turns the market's line off.
        (
            "chart.price_tag.progress.set",
            1,
            Value::Null,
            Readback::Moves,
        ),
    )]
}
