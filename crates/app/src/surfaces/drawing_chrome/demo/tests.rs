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
    assert!(gallery.bands && gallery.shared.is_none());
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
    fn keep_selected_to_chart(&mut self) {
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
fn gallery_executor_projects_every_anchor_and_preserves_failed_placement() {
    use quantick_ui_model::Model as _;
    let mut owner = quantick_ui_model::drawing_chrome::State::default();
    use quantick_ui_model::drawing_chrome::{Intent, Model};
    Model::update(
        &mut owner,
        Intent::Scenario(scenario::Intent::RequestGallery(DrawingsDemo {
            shared: Some(true),
            ..Default::default()
        })),
    );
    let effect = Model::update(
        &mut owner,
        Intent::Scenario(scenario::Intent::GalleryReady {
            series: facts(),
            tools: DRAWING_TOOLS.to_vec(),
            samples: vec![],
        }),
    )
    .take_scenario()
    .unwrap();
    let scenario::Effect::Gallery(mut plan) = effect else {
        panic!("gallery plan")
    };
    let mut projected = Vec::new();
    plan.project_times(|slot| {
        projected.push(slot);
        Some(slot as i64 * 17)
    });
    let mut target = RefusingRecipient::default();
    assert!(
        plan.apply_main(&mut target, |intent| {
            let mut effect = quantick_ui_model::drawing_chrome::Model::update(
                &mut owner,
                quantick_ui_model::drawing_chrome::Intent::Scenario(intent),
            );
            effect.take_scenario()
        })
        .is_none()
    );
    plan.apply_bands(&mut target);
    assert!(!target.points.is_empty());
    assert_eq!(target.points.len(), projected.len());
    for ((_, point), slot) in target.points.iter().zip(projected) {
        assert_eq!(point.time_ms, Some(slot as i64 * 17));
    }
    assert_eq!(target.shares, 0);
    assert!(target.finishes > 0);
}
#[test]
fn draft_adapter_maps_the_normalized_hand_and_applies_empty_plan() {
    let tool = DrawingTool::by_id("horizontal-line").unwrap();
    let plan = DrawingDraft {
        anchors: 2,
        constrain: true,
    }
    .plan(tool, facts());
    let mut target = RefusingRecipient::default();
    plan.apply(&mut target);
    assert!(target.points.is_empty());
    let chart = eframe::egui::Rect::from_min_size(
        eframe::egui::pos2(100.0, 100.0),
        eframe::egui::vec2(800.0, 400.0),
    );
    assert_eq!(
        plan.hand(chart).position,
        crate::drawings::px(eframe::egui::pos2(660.0, 240.0))
    );
    assert_eq!(
        plan.hand(chart).constrain,
        crate::drawings::Constrain::Level
    );
}
#[test]
fn profile_executor_selects_after_each_pair_even_when_placement_fails() {
    use quantick_ui_model::Model as _;
    use quantick_ui_model::drawing_chrome::{Intent, Model, State};
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Scenario(scenario::Intent::QueueLaunch(scenario::Launch {
            demos: scenario::DrawingDemoLaunch {
                profile: Some(FrvpDemo {
                    compare: true,
                    select: true,
                    stress: false,
                }),
                ..Default::default()
            },
            ..Default::default()
        })),
    );
    Model::update(&mut state, Intent::Scenario(scenario::Intent::ApplyLaunch));
    let effect = Model::update(
        &mut state,
        Intent::Scenario(scenario::Intent::ProfileReady(ProfileFacts {
            slots: 100,
            prefix: 40,
        })),
    )
    .take_scenario()
    .unwrap();
    let scenario::Effect::Profile(geometry) = effect else {
        panic!("profile geometry")
    };
    let mut plan = geometry.plan(DrawingTool::by_id(crate::frvp::TOOL_ID).unwrap(), None);
    plan.project_times(|slot| Some(slot as i64));
    let mut target = RefusingRecipient::default();
    plan.apply(&mut target);
    assert_eq!(
        target.selections,
        [0, 0],
        "select len.saturating_sub(1) even when placement fails"
    );
    assert_eq!(
        target
            .points
            .iter()
            .map(|(_, p)| p.time_ms)
            .collect::<Vec<_>>(),
        [Some(50), Some(74), Some(75), Some(99)]
    );
}

#[test]
fn avwap_lookup_waits_for_model_readiness_and_is_not_retried_after_missing_tool() {
    use quantick_ui_model::Model as _;
    use quantick_ui_model::drawing_chrome::{Intent, Model, State};
    let mut owner = State::default();
    Model::update(
        &mut owner,
        Intent::Scenario(scenario::Intent::QueueLaunch(scenario::Launch {
            demos: scenario::DrawingDemoLaunch {
                avwap: true,
                ..Default::default()
            },
            ..Default::default()
        })),
    );
    Model::update(&mut owner, Intent::Scenario(scenario::Intent::ApplyLaunch));
    let mut advance = |slots| {
        Model::update(
            &mut owner,
            Intent::Scenario(scenario::Intent::AvwapReady { slots }),
        )
        .take_scenario()
    };
    assert!(resolve_avwap(advance(11), || panic!("not ready")).is_none());
    assert!(resolve_avwap(advance(12), || None).is_none());
    assert!(resolve_avwap(advance(200), || panic!("already consumed")).is_none());
}
