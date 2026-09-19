use super::{refused, slot_of};
use quantick_chart_interaction::{
    annotation::{self, InputAnchor, Series},
    quick_range::{Owner, RangeContext},
};

/// Three bars and a gap: no bar covers any time asked of it.
struct Gap;

impl Series for Gap {
    fn context(&self) -> RangeContext {
        RangeContext {
            owner: Owner {
                tab: 1,
                pane: 2,
                layout: None,
            },
            revision: 0,
        }
    }
    fn slots(&self) -> usize {
        3
    }
    fn slot_at_time(&self, _: i64) -> Option<usize> {
        None
    }
    fn open_time(&self, _: usize) -> Option<i64> {
        None
    }
    fn time_at_position(&self, _: f32) -> Option<i64> {
        None
    }
    fn draft_in_progress(&self) -> bool {
        false
    }
}

#[test]
fn v1_and_v2_answer_an_uncovered_anchor_with_one_message_and_remedy() {
    let v1 = slot_of(&Gap, 5).expect_err("v1 refuses a time no bar covers");
    let anchor = InputAnchor {
        bar: None,
        price: 1.0,
        time_ms: Some(5),
    };
    let v2 = annotation::resolve(&Gap, &[anchor], None, 1)
        .map_err(refused)
        .expect_err("v2 refuses the same anchor");
    assert_eq!(v1, v2, "two wire versions, one answer");
    assert_eq!(v2.message, "no bar on that chart covers the anchor time");
    assert_eq!(
        v2.context.next_steps,
        [
            "Read a bar's open_time_unix_ms from chart.window.read or the cursor, and anchor to that."
        ]
    );
}
