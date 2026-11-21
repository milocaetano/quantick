use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            76, // Inside its ten seconds the delete comes back; then Manage opens.
            (
                "workspace.delete.undo",
                1,
                json!({ "name": "Renamed" }),
                Readback::Moves,
            ),
        ),
        (
            77,
            (
                "workspace.dialog.open",
                1,
                json!({ "mode": "manage" }),
                Readback::Moves,
            ),
        ),
    ]
}
