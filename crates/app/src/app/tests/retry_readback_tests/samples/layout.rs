use super::{LAYOUT_V2, Readback, Sample};
use serde_json::{Value, json};

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            27,
            (
                "layout.preset.apply",
                LAYOUT_V2,
                json!({ "preset_id": "stack2+flow" }),
                Readback::Moves,
            ),
        ),
        (
            28,
            (
                "layout.focus.set",
                LAYOUT_V2,
                json!({ "pane": "2" }),
                Readback::Moves,
            ),
        ),
        (
            29,
            (
                "layout.pane.set_interval",
                LAYOUT_V2,
                json!({ "pane": "1", "interval_ms": 7_200_000 }),
                Readback::Moves,
            ),
        ),
        (
            30,
            (
                "layout.pane.set_bar_spec",
                LAYOUT_V2,
                json!({ "pane": "0", "spec": "tick:77" }),
                Readback::Moves,
            ),
        ),
        (
            31,
            (
                "layout.pane.move",
                LAYOUT_V2,
                json!({ "from": "1", "to": "2" }),
                Readback::Moves,
            ),
        ),
        (
            32,
            (
                "layout.pane.collapse",
                LAYOUT_V2,
                json!({}),
                Readback::Moves,
            ),
        ),
        (
            33,
            ("layout.pane.expand", LAYOUT_V2, json!({}), Readback::Moves),
        ),
        (
            34,
            (
                "layout.flow.collapse",
                LAYOUT_V2,
                json!({}),
                Readback::Moves,
            ),
        ),
        (
            35,
            ("layout.flow.expand", LAYOUT_V2, json!({}), Readback::Moves),
        ),
        (
            38,
            ("layout.pane.resize_pair", 1, Value::Null, Readback::Moves),
        ),
        (
            39, // Seven places cannot come back as they were sent, so v2 refuses
            // them — and the readback stays where it was.
            (
                "layout.pane.resize",
                LAYOUT_V2,
                json!({ "fraction": "0.1234567" }),
                Readback::Stays,
            ),
        ),
        (
            40,
            (
                "layout.pane.resize",
                LAYOUT_V2,
                json!({ "fraction": "0.5" }),
                Readback::Moves,
            ),
        ),
        (53, ("layout.tab.create", 1, json!({}), Readback::Moves)),
        (
            54,
            (
                "layout.tab.rename",
                1,
                json!({ "new_name": "Renamed on retry" }),
                Readback::Moves,
            ),
        ),
        (
            55, // Filled in when it runs: the layout that was active before the create.
            ("layout.tab.switch", 1, Value::Null, Readback::Moves),
        ),
    ]
}
