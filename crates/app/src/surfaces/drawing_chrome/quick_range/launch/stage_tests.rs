use super::*;
use crate::drawings;
use crate::pane::PaneSide;

#[test]
fn each_captured_mode_stages_the_real_owner_and_is_consumed_once() {
    for (raw, ready, future) in [
        ("active", false, false),
        ("1", true, false),
        ("ready", true, false),
        ("future", true, true),
    ] {
        let owner = Owner {
            tab: 1,
            pane: 2,
            side: PaneSide::Flow,
            revision: 3,
            layout: Some(4),
        };
        let mut model = quantick_ui_model::drawing_chrome::State::default();
        let session = super::super::super::Session::Host(&mut model);
        let mut view = super::super::View::default();
        let mut quick = QuickRange::new(session, &mut view);
        quick.queue_launch(QuickRangeLaunch::capture(|_| Some(raw.into())));
        assert!(
            quick.model().demo_requested().is_none(),
            "construction must not stage a demo"
        );
        quick.stage_demo(owner, |_, _| {
            panic!("the launch phase has not applied its input")
        });
        quick.apply_launch();
        assert!(
            !quick.model().present(),
            "launch waits for the staging phase"
        );
        quick.stage_demo(owner, |band, requested_future| {
            assert_eq!(requested_future, future);
            assert_eq!(*band, DrawingBand::Price, "no `@`: the candles");
            let tool = drawings::DrawingTool::by_id("measure").unwrap();
            Ok(Some((
                [ChartPoint::at(1.5, 100.0), ChartPoint::at(10.5, 101.0)],
                NewDrawing {
                    style: tool.default_style(),
                    payload: tool.default_payload(),
                },
            )))
        });
        assert_eq!(quick.model().view().unwrap().actionable(), ready);
        quick.dismiss();
        quick.apply_launch();
        quick.stage_demo(owner, |_, _| {
            panic!("the captured demo was already consumed")
        });
        assert!(!quick.model().present());
    }
}
