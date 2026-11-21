use super::{Readback, Sample};
use serde_json::json;

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![(
        78, // Opening the colour popover journals nothing: the colour stays.
        (
            "workspace.colour.popover",
            1,
            json!({ "act": "open" }),
            Readback::Stays,
        ),
    )]
}
