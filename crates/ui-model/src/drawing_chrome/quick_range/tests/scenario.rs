use super::*;

#[test]
fn queued_launch_waits_retries_and_consumes_once() {
    let mut state = State::default();
    let owner = Owner {
        tab: 1,
        pane: 2,
        side: PaneSide::Flow,
        revision: 3,
        layout: Some(4),
    };
    for ready in [false, true] {
        let request = core::DemoRequest {
            ready,
            future: true,
            band: DrawingBand::Price,
        };
        update(&mut state, Intent::QueueLaunch(Some(request)));
        assert!(state.demo_requested().is_none());
        update(&mut state, Intent::ApplyLaunch);
        assert!(!state.present());
        update(&mut state, Intent::StageDemo(Staging::Wait));
        assert!(state.demo_requested().is_some());
        update(&mut state, Intent::ApplyLaunch);
        assert!(state.demo_requested().is_some());
        let tool = drawings::DrawingTool::by_id("measure").unwrap();
        update(
            &mut state,
            Intent::StageDemo(Staging::Ready {
                owner,
                anchors: [
                    ChartPoint::at_time(1.5, 100.0, Some(1)),
                    ChartPoint::at_time(10.5, 101.0, None),
                ],
                look: NewDrawing {
                    style: tool.default_style(),
                    payload: tool.default_payload(),
                },
            }),
        );
        assert!(state.demo_requested().is_none());
        assert_eq!(state.view().unwrap().actionable(), ready);
        assert_eq!(state.paint_anchors().unwrap()[0].time_ms, Some(1));
        update(&mut state, Intent::Dismiss);
        update(&mut state, Intent::ApplyLaunch);
        assert!(state.demo_requested().is_none());
        assert!(!state.present());
    }
    update(
        &mut state,
        Intent::QueueLaunch(core::DemoRequest::parse("ready")),
    );
    update(&mut state, Intent::ApplyLaunch);
    update(&mut state, Intent::StageDemo(Staging::Refused));
    assert!(state.demo_requested().is_none());
    assert!(!state.present());
    update(&mut state, Intent::ApplyLaunch);
    assert!(state.demo_requested().is_none());
}
