use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            67,
            ("workspace.new", 1, json!({ "name": "  " }), Readback::Moves),
        ),
        (
            68, // Last: deleting a workspace lays its tab out as a new tab, and the rows
            // above work on the charts the earlier rows opened.
            (
                "workspace.save_as",
                1,
                json!({ "name": "Retried" }),
                Readback::Moves,
            ),
        ),
        (69, ("workspace.save_to", 1, json!({}), Readback::Stays)),
        (
            70, // Refused since revert left the menus: nothing moves.
            ("workspace.revert", 1, json!({}), Readback::Stays),
        ),
        (
            71,
            (
                "workspace.autosave.set",
                1,
                json!({ "on": false }),
                Readback::Moves,
            ),
        ),
        (
            72,
            (
                "tab.workspace.set",
                1,
                json!({ "name": "Retried" }),
                Readback::Stays,
            ),
        ),
        (
            73,
            (
                "workspace.rename",
                1,
                json!({ "name": "Retried", "to": "Renamed" }),
                Readback::Moves,
            ),
        ),
        (
            74,
            (
                "workspace.colour.set",
                1,
                json!({ "name": "Renamed", "colour": "ink" }),
                Readback::Moves,
            ),
        ),
        (
            75,
            (
                "workspace.delete",
                1,
                json!({ "name": "Renamed" }),
                Readback::Moves,
            ),
        ),
        (
            95, // Last of all: it closes the active tab, edits or not.
            ("tab.close", 1, json!({ "save": false }), Readback::Moves),
        ),
    ]
}
