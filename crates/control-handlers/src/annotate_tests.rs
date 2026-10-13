//! The annotate handlers driven through a fake window's panes and a fake
//! access, with no application behind them.

use quantick_chart_interaction::annotation::slot_at_position;
use quantick_control::annotation::{
    CAPABILITY_VERSION, HORIZONTAL_LEVELS_CAPABILITY_ID, REMOVE_CAPABILITY_ID,
};
use quantick_control_host::actions::ActionRegistry;
use serde_json::json;

use super::*;
use crate::test_support::FakeWindow as Actors;

/// One pane over three one-second bars opening at 1 s, 2 s and 3 s.
struct FakePane {
    id: u64,
    opens: Vec<i64>,
    drawings: Drawings<DrawingTool>,
    sweeps: usize,
}

impl FakePane {
    fn new(id: u64) -> Self {
        Self {
            id,
            opens: vec![1_000, 2_000, 3_000],
            drawings: Drawings::default(),
            sweeps: 0,
        }
    }
}

impl AnnotatedPane for FakePane {
    fn pane_id(&self) -> u64 {
        self.id
    }
    fn layout_id(&self) -> Option<u64> {
        None
    }
    fn pagination_revision(&self) -> u64 {
        0
    }
    fn slots(&self) -> usize {
        self.opens.len()
    }
    fn slot_at_time(&self, time_ms: i64) -> Option<usize> {
        self.opens.iter().rposition(|open| *open <= time_ms)
    }
    fn slot_open_time(&self, slot: usize) -> Option<i64> {
        self.opens.get(slot).copied()
    }
    fn anchor_time(&self, bar: f32) -> Option<i64> {
        slot_at_position(bar).and_then(|slot| self.slot_open_time(slot))
    }
    fn drawings(&self) -> &Drawings<DrawingTool> {
        &self.drawings
    }
    fn drawings_mut(&mut self) -> &mut Drawings<DrawingTool> {
        &mut self.drawings
    }
    fn sweep_orphans(&mut self) {
        self.sweeps += 1;
    }
}

/// One tab of id 7: its flow pane, and no context chart.
struct FakeWindow {
    flow: FakePane,
}

impl FakeWindow {
    fn new() -> Self {
        Self {
            flow: FakePane::new(70),
        }
    }
}

impl TabDirectory for FakeWindow {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        (tab_id == 7).then_some(0)
    }
    fn tab_id_at(&self, _index: usize) -> u64 {
        7
    }
}

impl AnnotatePort for FakeWindow {
    fn tab_count(&self) -> usize {
        1
    }
    fn drawing_side(&self, _index: usize) -> PaneSide {
        PaneSide::Flow
    }
    fn time_pane_count(&self, _index: usize) -> usize {
        0
    }
    fn sides(&self, _index: usize) -> Vec<PaneSide> {
        vec![PaneSide::Flow]
    }
    fn new_drawing(&self, tool: DrawingTool) -> NewDrawing {
        NewDrawing {
            style: tool.default_style(),
            payload: tool.default_payload(),
        }
    }
    fn pane_mut(&mut self, _index: usize, _side: PaneSide) -> &mut dyn AnnotatedPane {
        &mut self.flow
    }
}

#[derive(Default)]
struct FakeAccess {
    replayed: Option<(ActorKind, String)>,
    events: Vec<NewEvent>,
}

impl AnnotateAccess for FakeAccess {
    fn recorded_actor(&self) -> Option<(ActorKind, &str)> {
        self.replayed
            .as_ref()
            .map(|(kind, client)| (*kind, client.as_str()))
    }
    fn record_event(&mut self, event: NewEvent) {
        self.events.push(event);
    }
}

fn call(
    window: &mut FakeWindow,
    access: &mut FakeAccess,
    capability_id: &str,
    input: Value,
) -> Result<Value, ControlError> {
    let mut registry = ActionRegistry::<FakeWindow, FakeAccess>::new();
    register(&mut registry).expect("the annotate descriptors are valid");
    let action = registry
        .lookup(capability_id, CAPABILITY_VERSION)
        .expect("registered");
    (action.handler)(window, access, &Actors::assistant(), &input)
}

fn assistant_author() -> Option<DrawingAuthor> {
    Some(DrawingAuthor {
        actor_kind: "agent".to_owned(),
        client_name: "fake assistant".to_owned(),
    })
}

#[test]
fn a_label_call_and_its_drawing_intent_give_the_same_mark_and_effect() {
    let mut window = FakeWindow::new();
    let mut access = FakeAccess::default();
    let input = json!({
        "anchors": [{ "time_unix_ms": 2_500, "price": "101.5" }],
        "text": "absorption",
        "name": "bid wall",
    });
    let answer = call(&mut window, &mut access, LABEL_CAPABILITY_ID, input).expect("placed");
    let answer: AnnotationResult = serde_json::from_value(answer).expect("a v1 result");

    let tool = DrawingTool::by_id(LABEL_TOOL_ID).expect("registered");
    let mut mirror = Drawings::default();
    let effects = update(
        &mut mirror,
        DrawingIntent::Place(Placement {
            tool,
            points: vec![ChartPoint::at_time(1.5, 101.5, Some(2_000))],
            opening: window.new_drawing(tool),
            author: assistant_author(),
            name: Some("bid wall".to_owned()),
            text: Some("absorption".to_owned()),
        }),
    );
    assert_eq!(window.flow.drawings.items(), mirror.items(), "one mark");
    assert_eq!(
        effects,
        [DrawingEffect::Placed {
            id: DrawingId(answer.annotation_id.get()),
            label: answer.label.clone(),
        }],
        "one effect, answered as the call's id and label"
    );
    assert_eq!(answer.tab_id.get(), 7);
    assert_eq!(answer.pane_id.get(), 70);
    assert_eq!(access.events.len(), 1, "one created event");

    let id = DrawingId(answer.annotation_id.get());
    let removal = json!({ "annotation_id": answer.annotation_id });
    let removed = call(&mut window, &mut access, REMOVE_CAPABILITY_ID, removal).expect("removed");
    assert_eq!(removed["removed"], true);
    assert_eq!(
        update(&mut mirror, DrawingIntent::RemoveAuthored(id)),
        [DrawingEffect::Removed { id }]
    );
    assert_eq!(window.flow.drawings.items(), mirror.items(), "both gone");
    assert_eq!(window.flow.sweeps, 1, "what rode the mark is let go");
    assert_eq!(access.events.len(), 2, "one removed event");
}

#[test]
fn horizontal_levels_and_their_intent_place_the_same_lines() {
    let mut window = FakeWindow::new();
    let mut access = FakeAccess::default();
    let input = json!({
        "anchors": [
            { "time_unix_ms": 3_000, "bar_position": "2.5", "price": "99" },
            { "time_unix_ms": 1_000, "bar_position": "0.5", "price": "101" },
        ],
    });
    call(
        &mut window,
        &mut access,
        HORIZONTAL_LEVELS_CAPABILITY_ID,
        input,
    )
    .expect("placed");

    let tool = DrawingTool::by_id(HORIZONTAL_LINE_TOOL_ID).expect("registered");
    let line = |price| Placement {
        tool,
        points: vec![ChartPoint::at_time(0.5, price, Some(1_000))],
        opening: window.new_drawing(tool),
        author: assistant_author(),
        name: None,
        text: None,
    };
    let mut mirror = Drawings::default();
    let effects = update(
        &mut mirror,
        DrawingIntent::PlaceAll(vec![line(101.0), line(99.0)]),
    );
    assert_eq!(window.flow.drawings.items(), mirror.items());
    assert_eq!(effects.len(), 2);
    assert_eq!(
        access.events.len(),
        2,
        "each line journaled as its own object"
    );
}

#[test]
fn a_mark_drawn_by_hand_is_refused_and_stays() {
    let mut window = FakeWindow::new();
    let mut access = FakeAccess {
        replayed: Some((ActorKind::HumanUi, "trader".to_owned())),
        events: Vec::new(),
    };
    let input = json!({ "anchors": [{ "time_unix_ms": 1_000, "price": "100" }] });
    let answer = call(&mut window, &mut access, LABEL_CAPABILITY_ID, input).expect("placed");
    assert!(
        window.flow.drawings.items()[0].author.is_none(),
        "a replayed human run stamps nobody"
    );
    let removal = json!({ "annotation_id": answer["annotation_id"] });
    let error = call(&mut window, &mut access, REMOVE_CAPABILITY_ID, removal)
        .expect_err("the trader's mark is not the tier's to take");
    assert_eq!(error.code.as_str(), codes::PERMISSION_DENIED);
    assert_eq!(window.flow.drawings.items().len(), 1);
    assert_eq!(window.flow.sweeps, 0);
}

#[test]
fn a_draft_in_progress_refuses_before_anything_is_placed() {
    let mut window = FakeWindow::new();
    let tool = DrawingTool::by_id(TREND_LINE_TOOL_ID).expect("registered");
    let opening = window.new_drawing(tool);
    let point = ChartPoint::at_time(0.5, 100.0, None);
    let band = quantick_drawings::DrawingBand::Price;
    assert!(
        !window
            .flow
            .drawings
            .place_with(tool, &band, point, |_| opening)
    );
    let input = json!({ "anchors": [{ "time_unix_ms": 1_000, "price": "100" }] });
    let error = call(
        &mut window,
        &mut FakeAccess::default(),
        LABEL_CAPABILITY_ID,
        input,
    )
    .expect_err("the trader is mid-gesture");
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert!(error.retryable);
    assert!(window.flow.drawings.items().is_empty());
    assert_eq!(
        window.flow.drawings.draft_len(),
        1,
        "their draft is untouched"
    );
}

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
    let v2 = resolver::resolve(&Gap, &[anchor], None, 1)
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
