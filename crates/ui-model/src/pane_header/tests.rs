use super::*;
use crate::Model as _;
use quantick_chart::pane_header::{LayerToggle, SideRefusal};
use quantick_engine::BarSpec;
use quantick_orderflow::{LaneView, LiveLaneStyle};

fn flow() -> Snapshot {
    Snapshot {
        identity: PaneIdentity::OrderFlow,
        bars: BarSpec::Tick(2000).into(),
        flow: Some(FlowHeaderState::of(
            &LiveLaneStyle::default(),
            Some(1.0),
            Some(1.0),
            None,
            Default::default(),
        )),
        chip: None,
        slot: None,
        settings_open: false,
        lights_on: false,
    }
}

#[test]
fn header_intents_preserve_behavior() {
    let mut state = State::default();
    let snapshot = flow();
    assert_eq!(
        Model::update(&mut state, Intent::Refresh(snapshot.clone())),
        Effects::default()
    );
    assert_eq!(
        state.header,
        Some(PaneHeader::of(PaneIdentity::OrderFlow, snapshot.bars))
    );
    for side in [LaneSide::Trail, LaneSide::Tape] {
        let segment = state.side(side).unwrap();
        assert_eq!(
            Model::update(&mut state, Intent::Side(segment.side)).request,
            segment.click()
        );
    }
    for toggle in LayerToggle::ALL {
        for on in [false, true] {
            let glyph = LayerGlyph {
                toggle,
                on,
                blocked: None,
            };
            assert_eq!(
                Model::update(
                    &mut state,
                    Intent::Layer {
                        glyph,
                        secondary: false
                    }
                ),
                Effects {
                    request: glyph.click(),
                    focus: false
                }
            );
            assert_eq!(
                Model::update(
                    &mut state,
                    Intent::Layer {
                        glyph,
                        secondary: true
                    }
                ),
                Effects {
                    request: Some(HeaderIntent::OpenLayerSettings(toggle)),
                    focus: true
                }
            );
        }
    }
    let mut held = snapshot;
    held.flow.as_mut().unwrap().market_held = true;
    Model::update(&mut state, Intent::Refresh(held));
    assert_eq!(
        state.side(LaneSide::Tape).unwrap().refusal,
        Some(SideRefusal::MarketClosed)
    );
    for side in [LaneSide::Trail, LaneSide::Tape] {
        let segment = state.side(side).unwrap();
        assert_eq!(
            Model::update(&mut state, Intent::Side(segment.side)),
            Effects::default()
        );
    }
    for request in [
        HeaderIntent::ToggleSettings,
        HeaderIntent::SetLights(true),
        HeaderIntent::SetLaneWindow(Default::default()),
    ] {
        assert_eq!(
            Model::update(&mut state, Intent::Request(request)),
            Effects {
                request: Some(request),
                focus: false
            }
        );
    }
}

// Migrated from app::tests::pane_header_tests: identity is independent of paint and focus.
#[test]
fn every_pane_reads_its_own_bars_without_a_click() {
    let mut flow_state = State::default();
    let mut time_state = State::default();
    Model::update(&mut flow_state, Intent::Refresh(flow()));
    Model::update(
        &mut time_state,
        Intent::Refresh(Snapshot::time(BarSpec::Tick(4).into())),
    );
    assert_eq!(flow_state.header.as_ref().unwrap().mark, Some("ORDER FLOW"));
    assert_eq!(flow_state.header.as_ref().unwrap().bars_text, "Tick · 2000");
    assert_eq!(time_state.header.as_ref().unwrap().bars_text, "Tick · 4");
    assert_eq!(time_state.header.as_ref().unwrap().mark, None);
    Model::update(
        &mut time_state,
        Intent::Refresh(Snapshot::time(BarSpec::Tick(8).into())),
    );
    assert_eq!(flow_state.header.as_ref().unwrap().bars_text, "Tick · 2000");
}

#[test]
fn refresh_reuses_identity_projection_and_updates_action_availability() {
    let mut state = State::default();
    let snapshot = flow();
    Model::update(&mut state, Intent::Refresh(snapshot.clone()));
    let allocation = state.header.as_ref().unwrap().bars_text.as_ptr();
    Model::update(&mut state, Intent::Refresh(snapshot.clone()));
    assert_eq!(
        state.header.as_ref().unwrap().bars_text.as_ptr(),
        allocation
    );
    let mut changed = snapshot;
    changed.flow.as_mut().unwrap().view = LaneView::CandlesOnly;
    Model::update(&mut state, Intent::Refresh(changed));
    assert_eq!(
        state.header.as_ref().unwrap().bars_text.as_ptr(),
        allocation
    );
    assert_eq!(
        state.side(LaneSide::Trail).unwrap().refusal,
        Some(SideRefusal::LastSideShowing)
    );
}

#[test]
fn sample_drag_is_scoped_and_reuses_the_press_origin() {
    let mut first = State::default();
    let mut second = State::default();
    let gesture = |scale, started, up_px, reset: bool, ended| Intent::SampleGesture {
        sample: BubbleSample::Candle,
        scale,
        started,
        dragged: !reset,
        up_px,
        reset,
        ended,
    };
    Model::update(&mut first, gesture(1.0, true, 0.0, false, false));
    assert_eq!(
        Model::update(&mut first, gesture(1.0, false, 40.0, false, false)).request,
        Some(HeaderIntent::SetSample(BubbleSample::Candle, 2.0))
    );
    assert_eq!(
        Model::update(&mut first, gesture(2.0, false, 80.0, false, false)).request,
        Some(HeaderIntent::SetSample(BubbleSample::Candle, 3.0))
    );
    assert_eq!(
        Model::update(&mut second, gesture(2.0, true, 40.0, false, false)).request,
        Some(HeaderIntent::SetSample(BubbleSample::Candle, 3.0))
    );
    assert_eq!(
        Model::update(&mut first, gesture(3.0, false, 0.0, true, true)).request,
        Some(HeaderIntent::SetSample(BubbleSample::Candle, 1.0))
    );
    assert_eq!(first.drag, None);
}

#[test]
fn the_side_switches_set_the_view_and_the_last_lit_stays() {
    let mut state = State::default();
    let mut snapshot = flow();
    Model::update(&mut state, Intent::Refresh(snapshot.clone()));
    for (side, wanted) in [
        (LaneSide::Tape, LaneView::CandlesOnly),
        (LaneSide::Trail, LaneView::CandlesOnly),
        (LaneSide::Tape, LaneView::Beside),
        (LaneSide::Trail, LaneView::TapeOnly),
        (LaneSide::Tape, LaneView::TapeOnly),
        (LaneSide::Trail, LaneView::Beside),
    ] {
        let effects = Model::update(&mut state, Intent::Side(side));
        if let Some(HeaderIntent::SetView(view)) = effects.request {
            let flow = snapshot.flow.as_mut().unwrap();
            flow.view = view;
            flow.shown_view = view;
            Model::update(&mut state, Intent::Refresh(snapshot.clone()));
        }
        assert_eq!(state.snapshot.as_ref().unwrap().flow.unwrap().view, wanted);
        assert!(!effects.focus);
    }
}

#[test]
fn a_dragged_sample_sizes_its_bubbles_and_a_double_click_returns_to_one() {
    let mut state = State::default();
    for (scale, up_px, reset, expected) in [
        (1.0, 40.0, false, 2.0),
        (2.0, 4000.0, false, 3.0),
        (3.0, -4000.0, false, 0.25),
        (0.25, 0.0, true, 1.0),
    ] {
        let effects = Model::update(
            &mut state,
            Intent::SampleGesture {
                sample: BubbleSample::Tape,
                scale,
                started: true,
                dragged: !reset,
                up_px,
                reset,
                ended: true,
            },
        );
        assert_eq!(
            effects.request,
            Some(HeaderIntent::SetSample(BubbleSample::Tape, expected))
        );
        assert!(!effects.focus);
    }
}

#[test]
fn window_picks_use_the_latest_flow_projection() {
    use quantick_orderflow::LaneWindow;
    let mut state = State::default();
    Model::update(&mut state, Intent::Refresh(flow()));
    assert!(state.window_options[0].selected);
    assert_eq!(
        Model::update(&mut state, Intent::WindowPick(0)),
        Effects::default()
    );
    assert_eq!(
        Model::update(&mut state, Intent::WindowPick(usize::MAX)),
        Effects::default()
    );
    let fixed = state.window_options[1].pick.unwrap();
    assert!(matches!(fixed, LaneWindow::Fixed { .. }));
    assert_eq!(
        Model::update(&mut state, Intent::WindowPick(1)).request,
        Some(HeaderIntent::SetLaneWindow(fixed))
    );
    let mut current = state.snapshot.as_ref().unwrap().flow.unwrap();
    current.window = fixed;
    current.window_reference_ms = Some(60000);
    Model::update(&mut state, Intent::RefreshFlow(Some(current)));
    assert!(state.window_options[1].selected);
    assert_eq!(
        Model::update(&mut state, Intent::WindowPick(1)).request,
        Some(HeaderIntent::SetLaneWindow(fixed))
    );
    assert_eq!(
        Model::update(&mut state, Intent::WindowPick(0)).request,
        Some(HeaderIntent::SetLaneWindow(LaneWindow::default()))
    );
    current.window = LaneWindow::default();
    Model::update(&mut state, Intent::RefreshFlow(Some(current)));
    assert_eq!(state.window_label, Some(window_label(&current)));
    let allocation = state.window_options[0].label.as_ptr();
    current.candle_sample = Some(2.0);
    Model::update(&mut state, Intent::RefreshFlow(Some(current)));
    assert_eq!(
        state.window_options[0].label.as_ptr(),
        allocation,
        "sample updates do not rebuild window choices"
    );
    Model::update(&mut state, Intent::RefreshFlow(None));
    assert!(state.window_label.is_none());
    assert!(state.window_options.is_empty());
}
