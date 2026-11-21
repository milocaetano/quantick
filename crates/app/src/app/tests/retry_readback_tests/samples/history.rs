use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            1, // First, while the tab's feed still pages: the feed rows below
            // retire it. The load starts a run; the cancel ends it.
            (
                "feed.history.load",
                1,
                json!({ "reach": "hours:2" }),
                Readback::Moves,
            ),
        ),
        (2, ("feed.history.cancel", 1, json!({}), Readback::Moves)),
    ]
}
