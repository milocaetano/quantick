use super::{Readback, Sample};
use serde_json::{Value, json};

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            56, // A tab whose feed has left the feed table: the call runs, answers
            // `respawned: false`, and the generation says so by not moving.
            ("feed.reconnect", 1, json!({}), Readback::Stays),
        ),
        (57, ("feed.reload", 1, json!({}), Readback::Stays)),
        (58, ("feed.source.set", 1, Value::Null, Readback::Stays)),
        (59, ("tab.market.set", 1, Value::Null, Readback::Stays)),
        (
            60, // A tab already on the target: the retry a dropped answer sends.
            ("feed.contract.roll", 1, Value::Null, Readback::Stays),
        ),
        (
            61, // A tab whose feed declares no deal counter and has no recorded day:
            // stopping a recorder that is not running acts on nothing, and the
            // scope lists no recorder either side of the call.
            (
                "feed.deal_recording.set",
                1,
                json!({ "enabled": false }),
                Readback::Stays,
            ),
        ),
    ]
}
