use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            9, // A listing changes nothing, so no `session.journal.changed` is
            // journaled either side of it; the edits are proven in
            // `quantick-paper` and `quantick-control-handlers`.
            (
                "session.journal.set",
                1,
                json!({ "action": "list" }),
                Readback::Stays,
            ),
        ),
        (
            10, // A delete of a session already gone: refused with
            // `session_not_found`, journaling nothing, the same answer twice.
            (
                "session.journal.delete",
                1,
                json!({ "session": "WINJ26/no-such-session.csv" }),
                Readback::Stays,
            ),
        ),
    ]
}
