use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        0,
        (
            "appearance.apply",
            1,
            json!({"source": "version = 1\nname = 'Retry appearance'"}),
            Readback::Moves,
        ),
    )]
}
