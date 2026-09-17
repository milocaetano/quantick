use super::*;
use crate::drawings::{ChartPoint, DrawingBand};

fn launch(values: &[(&str, &str)]) -> DrawingDemoLaunch {
    DrawingDemoLaunch::capture(|name| {
        values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.into())
    })
}
fn state(values: &[(&str, &str)]) -> DrawingDemoState {
    let mut state = DrawingDemoState::default();
    state.install(launch(values));
    state
}
fn facts() -> SeriesFacts {
    SeriesFacts {
        slots: 200,
        last_close: Some(100.0),
        auto_range: Some((90.0, 110.0)),
    }
}

#[test]
fn capture_retains_conditional_reads_and_exact_grammars() {
    for absent in [None, Some(""), Some("0"), Some("bad")] {
        let mut reads = Vec::new();
        let input = DrawingDemoLaunch::capture(|name| {
            reads.push(name.to_owned());
            absent.map(Into::into)
        });
        assert!(input.gallery.is_none() && input.profile.is_none() && input.draft.is_none());
        assert!(!input.avwap && !input.recut);
        assert_eq!(
            reads,
            [
                "QUANTICK_DRAWINGS_DEMO",
                "QUANTICK_DRAWINGS_DEMO_RECUT",
                "QUANTICK_FRVP_DEMO",
                "QUANTICK_AVWAP_DEMO",
                "QUANTICK_DRAWING_DRAFT"
            ]
        );
    }
    let input = launch(&[
        ("QUANTICK_DRAWINGS_DEMO", "bands"),
        ("QUANTICK_DRAWINGS_DEMO_SHARED", " 1 "),
        ("QUANTICK_DRAWINGS_DEMO_SELECT", " trend-line "),
        ("QUANTICK_DRAWINGS_DEMO_RECUT", "1"),
        ("QUANTICK_FRVP_DEMO", " compare "),
        ("QUANTICK_FRVP_DEMO_SELECT", " 1 "),
        ("QUANTICK_AVWAP_DEMO", " 1 "),
        ("QUANTICK_DRAWING_DRAFT", " 2 "),
        ("QUANTICK_DRAWING_CONSTRAIN", " 1 "),
    ]);
    let gallery = input.gallery.unwrap();
    assert!(gallery.bands && !gallery.shared);
    assert_eq!(gallery.select_tool.as_deref(), Some(" trend-line "));
    assert!(input.recut && input.avwap);
    assert_eq!(
        input.profile,
        Some(FrvpDemo {
            compare: true,
            stress: false,
            select: true
        })
    );
    assert_eq!(
        input.draft,
        Some(DrawingDraft {
            anchors: 2,
            constrain: false
        })
    );
    assert!(
        launch(&[("QUANTICK_DRAWINGS_DEMO", " 1 ")])
            .gallery
            .is_none()
    );
    assert!(launch(&[("QUANTICK_DRAWING_DRAFT", "0")]).draft.is_none());
}

#[cfg(windows)]
#[test]
fn non_unicode_inputs_are_absent_without_subordinate_reads() {
    use std::os::windows::ffi::OsStringExt;
    let mut reads = Vec::new();
    let input = DrawingDemoLaunch::capture(|name| {
        reads.push(name.to_owned());
        Some(OsString::from_wide(&[0xd800]))
    });
    assert_eq!(reads.len(), 5);
    assert!(input.gallery.is_none() && input.profile.is_none() && input.draft.is_none());
    assert!(!input.recut && !input.avwap);
}

#[derive(Default)]
struct RefusingRecipient {
    points: Vec<(&'static str, ChartPoint)>,
    selections: Vec<usize>,
    finishes: usize,
    shares: usize,
}
impl DrawingRecipient for RefusingRecipient {
    fn place(&mut self, tool: DrawingTool, _: &DrawingBand, point: ChartPoint, _: Look) -> bool {
        self.points.push((tool.id(), point));
        false
    }
    fn finish(&mut self) -> bool {
        self.finishes += 1;
        false
    }
    fn share_selected(&mut self) {
        self.shares += 1;
    }
    fn selected(&self) -> Option<usize> {
        None
    }
    fn select(&mut self, index: usize) {
        self.selections.push(index);
    }
    fn selected_center(&self) -> Option<f32> {
        None
    }
    fn len(&self) -> usize {
        0
    }
}

#[test]
fn ready_gallery_consumes_even_when_recipient_places_nothing_and_retains_recut_tail() {
    let mut state = state(&[
        ("QUANTICK_DRAWINGS_DEMO", "1"),
        ("QUANTICK_DRAWINGS_DEMO_SHARED", "1"),
        ("QUANTICK_DRAWINGS_DEMO_RECUT", "1"),
    ]);
    assert!(state.take_gallery(0).is_none() && state.gallery_requested());
    let request = state.take_gallery(200).unwrap();
    assert!(!state.gallery_requested());
    let mut plan = request.plan(facts(), &crate::indicators::IndicatorViews::default());
    let mut projected = Vec::new();
    plan.project_times(|slot| {
        projected.push(slot);
        Some(slot as i64 * 17)
    });
    let mut target = RefusingRecipient::default();
    assert!(plan.apply_main(&mut target).is_none());
    plan.apply_bands(&mut target);
    assert!(!target.points.is_empty());
    assert_eq!(target.points.len(), projected.len());
    for ((_, point), slot) in target.points.iter().zip(projected) {
        assert_eq!(point.time_ms, Some(slot as i64 * 17));
    }
    assert_eq!(target.shares, 0);
    assert!(target.finishes > 0);
    assert!(
        state.recut_requested(),
        "recut is owed after the ready take, independent of completion"
    );
    assert!(state.take_gallery(200).is_none());
}

#[test]
fn draft_waits_for_prerequisites_and_consumes_effective_zero() {
    let mut state = state(&[
        ("QUANTICK_DRAWING_DRAFT", "2"),
        ("QUANTICK_DRAWING_CONSTRAIN", "1"),
    ]);
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .unwrap();
    let chart = egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(800.0, 400.0));
    assert!(state.take_draft(None, Some(chart), facts()).is_none());
    assert!(state.take_draft(Some(tool), None, facts()).is_none());
    assert!(
        state
            .take_draft(
                Some(tool),
                Some(chart),
                SeriesFacts {
                    slots: 0,
                    ..facts()
                }
            )
            .is_none()
    );
    assert!(state.draft_requested().is_some());
    let plan = state.take_draft(Some(tool), Some(chart), facts()).unwrap();
    assert!(state.draft_requested().is_none());
    let mut target = RefusingRecipient::default();
    plan.apply(&mut target);
    assert!(target.points.is_empty());
    assert_eq!(plan.hand.position, egui::pos2(660.0, 240.0));
    assert_eq!(plan.hand.constrain, crate::drawings::Constrain::Level);
}

#[test]
fn profile_wait_missing_tool_prefix_retry_and_compare_rearm_are_distinct() {
    let mut state = state(&[("QUANTICK_FRVP_DEMO", "stress")]);
    assert!(matches!(
        state.prepare_profile_with_tool(None, None),
        ProfilePreparation::Wait
    ));
    assert!(state.profile_requested().is_some());
    assert!(matches!(
        state.prepare_profile_with_tool(Some(11), None),
        ProfilePreparation::Wait
    ));
    assert!(state.profile_requested().is_some());
    assert!(matches!(
        state.prepare_profile(Some(12)),
        ProfilePreparation::Prefix { candles: 25_000 }
    ));
    assert!(
        state.profile_requested().is_some(),
        "an undelivered prefix has not consumed anything"
    );
    assert!(matches!(
        state.prepare_profile_with_tool(Some(12), None),
        ProfilePreparation::Wait
    ));
    assert!(state.profile_requested().is_none());
    state.install(launch(&[
        ("QUANTICK_FRVP_DEMO", "compare"),
        ("QUANTICK_FRVP_DEMO_SELECT", "1"),
    ]));
    assert!(
        state
            .finish_profile(ProfileFacts {
                slots: 100,
                prefix: 41
            })
            .is_none()
    );
    assert!(state.profile_requested().is_some());
    let geometry = state
        .finish_profile(ProfileFacts {
            slots: 100,
            prefix: 40,
        })
        .unwrap();
    assert!(state.profile_requested().is_none());
    assert_eq!(
        geometry.close_slot, 64,
        "price still comes from seam geometry, not the compare endpoints"
    );
    let mut plan = geometry.plan(None);
    plan.project_times(|slot| Some(slot as i64));
    let mut target = RefusingRecipient::default();
    plan.apply(&mut target);
    assert_eq!(
        target.selections,
        [0, 0],
        "each range selects len.saturating_sub(1) even if placement failed"
    );
    assert_eq!(
        target
            .points
            .iter()
            .map(|(_, point)| point.bar)
            .collect::<Vec<_>>(),
        [50.0, 74.0, 75.0, 99.0]
    );
    assert!(target.points.iter().all(|(_, point)| point.price == 1.0));
}

#[test]
fn avwap_consumes_before_a_missing_tool_and_does_not_lookup_before_ready() {
    let mut state = state(&[("QUANTICK_AVWAP_DEMO", "1")]);
    assert!(
        state
            .take_avwap_with_lookup(11, || panic!("not ready"))
            .is_none()
    );
    assert!(state.avwap_requested());
    assert!(state.take_avwap_with_lookup(12, || None).is_none());
    assert!(!state.avwap_requested());
    assert!(
        state
            .take_avwap_with_lookup(200, || panic!("already consumed"))
            .is_none()
    );
}

#[test]
fn fallback_arithmetic_keeps_nonfinite_and_negative_values() {
    let (_, _, center, band) = SeriesFacts {
        auto_range: Some((f64::NEG_INFINITY, f64::INFINITY)),
        ..facts()
    }
    .window();
    assert!(center.is_nan() && band.is_infinite());
    let (_, _, center, band) = SeriesFacts {
        auto_range: Some((10.0, 5.0)),
        last_close: Some(-2.0),
        ..facts()
    }
    .window();
    assert_eq!((center, band), (-2.0, -0.008));
    assert_eq!(
        SeriesFacts {
            auto_range: None,
            last_close: None,
            ..facts()
        }
        .window(),
        (90, 110, 1.0, 0.004)
    );
}
