//! The size rule and `orderflow.bubbles.size.set` through a fake pane: the
//! rule decides before anything is set, and a refused call edits nothing.

use super::*;
use serde_json::json;

const TARGETS: [BubbleScaleTarget; 2] = [BubbleScaleTarget::Candle, BubbleScaleTarget::Tape];

/// Every combination of the four facts: candle bubbles need a tick chart
/// drawing them beside its candles, Tape bubbles need the tape drawing them,
/// and neither target reads the other's facts.
#[test]
fn candle_size_needs_a_tick_chart_drawing_candle_bubbles_and_tape_size_needs_tape_bubbles() {
    for facts in 0..16_u8 {
        let pane = BubbleScalePane {
            tick_chart: facts & 1 != 0,
            candle_bubbles: facts & 2 != 0,
            tape_only: facts & 4 != 0,
            tape_bubbles: facts & 8 != 0,
        };
        let candle = pane.tick_chart && pane.candle_bubbles && !pane.tape_only;
        assert_eq!(
            pane.may_scale(BubbleScaleTarget::Candle),
            candle,
            "{pane:?}"
        );
        assert_eq!(
            pane.may_scale(BubbleScaleTarget::Tape),
            pane.tape_bubbles,
            "{pane:?}"
        );
        for target in TARGETS {
            assert_eq!(
                pane.admit(target).is_ok(),
                pane.may_scale(target),
                "{pane:?}"
            );
        }
    }
    let volume_chart = BubbleScalePane {
        candle_bubbles: true,
        tape_bubbles: true,
        ..BubbleScalePane::default()
    };
    assert_eq!(
        volume_chart.admit(BubbleScaleTarget::Candle),
        Err(ScaleRefusal::NotTickChart)
    );
}

/// Each refusal names the precondition it fails and the step that meets it,
/// so an agent branches on `violated_precondition`, never on the message.
#[test]
fn size_set_refusals_name_the_precondition_and_the_step_that_meets_it() {
    let all = BubbleScalePane {
        tick_chart: true,
        candle_bubbles: true,
        tape_only: false,
        tape_bubbles: true,
    };
    for (facts, target, precondition) in [
        (
            BubbleScalePane {
                tick_chart: false,
                ..all
            },
            "candle",
            TICK_CHART_PRECONDITION,
        ),
        (
            BubbleScalePane {
                candle_bubbles: false,
                ..all
            },
            "candle",
            DISPLAY_ACTIVE_PRECONDITION,
        ),
        (
            BubbleScalePane {
                tape_bubbles: false,
                ..all
            },
            "tape",
            DISPLAY_ACTIVE_PRECONDITION,
        ),
    ] {
        let mut pane = Pane::with(facts);
        let error = call(&mut pane, target, json!("2")).unwrap_err();
        assert_eq!(
            error
                .context
                .violated_precondition
                .as_ref()
                .map(|id| id.as_str()),
            Some(precondition),
            "{facts:?} {target}"
        );
        assert!(!error.retryable, "{facts:?} {target}");
        assert_eq!(error.context.next_steps.len(), 1, "{facts:?} {target}");
    }
    let declared: Vec<String> = descriptor()
        .preconditions
        .iter()
        .map(|declared| declared.id.as_str().to_owned())
        .collect();
    assert_eq!(
        declared,
        [TICK_CHART_PRECONDITION, DISPLAY_ACTIVE_PRECONDITION],
        "the descriptor declares every precondition a refusal names"
    );
}

struct Pane {
    facts: Result<BubbleScalePane, ControlError>,
    scale: [f32; 2],
    sets: usize,
}

impl Pane {
    fn with(facts: BubbleScalePane) -> Self {
        Self {
            facts: Ok(facts),
            scale: [1.0; 2],
            sets: 0,
        }
    }
}

impl BubbleScalePort for Pane {
    fn bubble_scale_pane(
        &mut self,
        _tab_id: u64,
        _pane_id: u64,
    ) -> Result<BubbleScalePane, ControlError> {
        self.facts.clone()
    }
    fn set_bubble_scale(
        &mut self,
        _tab_id: u64,
        _pane_id: u64,
        target: BubbleScaleTarget,
        scale: f32,
    ) -> Result<(bool, f32), ControlError> {
        self.sets += 1;
        let slot = &mut self.scale[usize::from(target == BubbleScaleTarget::Tape)];
        let changed = *slot != scale;
        *slot = scale;
        Ok((changed, scale))
    }
}

fn call(pane: &mut Pane, target: &str, scale: Value) -> Result<Value, ControlError> {
    let actor = crate::test_support::FakeWindow::assistant();
    let input = json!({"tab_id": "1", "pane_id": "2", "target": target, "scale": scale});
    set(pane, &mut (), &actor, &input)
}

#[test]
fn size_set_asks_the_rule_and_the_range_before_the_pane_sets_anything() {
    let all = BubbleScalePane {
        tick_chart: true,
        candle_bubbles: true,
        tape_only: false,
        tape_bubbles: true,
    };
    let mut pane = Pane::with(all);
    for scale in ["0.249", "3.001", "-1", "0"] {
        assert!(call(&mut pane, "tape", json!(scale)).is_err(), "{scale}");
    }
    for scale in [json!("NaN"), json!("1.0"), json!(2), Value::Null] {
        assert!(call(&mut pane, "tape", scale).is_err());
    }
    for facts in [
        BubbleScalePane {
            tick_chart: false,
            ..all
        },
        BubbleScalePane {
            candle_bubbles: false,
            ..all
        },
        BubbleScalePane {
            tape_only: true,
            ..all
        },
    ] {
        pane.facts = Ok(facts);
        assert!(call(&mut pane, "candle", json!("2")).is_err(), "{facts:?}");
    }
    pane.facts = Ok(BubbleScalePane {
        tape_bubbles: false,
        ..all
    });
    assert!(call(&mut pane, "tape", json!("2")).is_err());
    pane.facts = Err(ControlError::invalid_request("no order-flow view"));
    assert!(call(&mut pane, "tape", json!("2")).is_err());
    assert_eq!(pane.sets, 0, "every refusal came before any set");

    pane.facts = Ok(all);
    let first = call(&mut pane, "tape", json!("2")).unwrap();
    assert_eq!(first["target"], "tape");
    assert_eq!(first["scale"], "2");
    assert_eq!(first["changed"], true);
    let again = call(&mut pane, "tape", json!("2")).unwrap();
    assert_eq!(again["changed"], false);
    let candle = call(&mut pane, "candle", json!("0.5")).unwrap();
    assert_eq!(candle["scale"], "0.5");
    assert_eq!(pane.scale, [0.5, 2.0], "each target keeps its own gain");
    let tape_only = BubbleScalePane {
        tape_only: true,
        ..all
    };
    pane.facts = Ok(tape_only);
    assert_eq!(
        call(&mut pane, "tape", json!("3")).unwrap()["changed"],
        true,
        "Tape alone still sizes its bubbles"
    );
}
