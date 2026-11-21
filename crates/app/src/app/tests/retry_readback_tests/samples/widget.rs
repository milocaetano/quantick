use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            36, // Each journals one `widget.changed` naming the pane's new state.
            ("widget.open", 1, json!({ "id": "depth" }), Readback::Moves),
        ),
        (
            37,
            ("widget.close", 1, json!({ "id": "depth" }), Readback::Moves),
        ),
    ]
}
