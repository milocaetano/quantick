use super::{Readback, Sample};
use serde_json::{Value, json};

pub(super) fn samples() -> Vec<(usize, Sample)> {
    vec![
        (
            79, // The canvas, in an order where each call has something to do: a
            // layout, one chart folded, maximized, restored and widened, a widget added,
            // a chart floated and docked back, the widget parked, popped out,
            // returned and removed.
            ("canvas.window_menu.open", 1, json!({}), Readback::Moves),
        ),
        (
            80,
            (
                "canvas.layout.pick",
                1,
                json!({ "preset_id": "charts-4" }),
                Readback::Moves,
            ),
        ),
        (
            81,
            ("canvas.pane.minimize", 1, Value::Null, Readback::Moves),
        ),
        (
            82,
            ("canvas.pane.maximize", 1, Value::Null, Readback::Moves),
        ),
        (83, ("canvas.pane.restore", 1, Value::Null, Readback::Moves)),
        (84, ("canvas.pane.resize", 1, Value::Null, Readback::Moves)),
        (85, ("canvas.seam.drag", 1, Value::Null, Readback::Moves)),
        (
            86,
            (
                "canvas.widget.add",
                1,
                json!({ "widget_id": "timeframe-chart" }),
                Readback::Moves,
            ),
        ),
        (87, ("canvas.pane.float", 1, Value::Null, Readback::Moves)),
        (88, ("canvas.pane.dock", 1, Value::Null, Readback::Moves)),
        (89, ("canvas.pane.park", 1, Value::Null, Readback::Moves)),
        (90, ("canvas.pane.pop_out", 1, Value::Null, Readback::Moves)),
        (91, ("canvas.pane.return", 1, Value::Null, Readback::Moves)),
        (92, ("canvas.pane.remove", 1, Value::Null, Readback::Moves)),
    ]
}
