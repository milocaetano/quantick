use super::{Readback, Sample};
pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        96,
        (
            "fixture.magnet.set",
            1,
            serde_json::json!({"enabled": false}),
            Readback::Moves,
        ),
    )]
}
