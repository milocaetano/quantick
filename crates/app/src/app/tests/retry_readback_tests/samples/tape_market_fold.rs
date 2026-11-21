use super::{Readback, Sample};
use serde_json::Value;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        17,
        (
            "orderflow.tape.market_fold.set",
            1,
            Value::Null,
            Readback::Moves,
        ),
    )]
}
