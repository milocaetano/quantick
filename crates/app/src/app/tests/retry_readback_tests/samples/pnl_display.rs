use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        11, // A display preference: the first set moves the stored choice and
        // journals it; the retry under the same key never reaches the app.
        (
            "session.pnl_display.set",
            1,
            json!({ "primary": "points_times_qty" }),
            Readback::Moves,
        ),
    )]
}
