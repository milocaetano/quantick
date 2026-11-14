use super::*;
use crate::Model as _;
use crate::drawing_chrome::{Intent as ChromeIntent, Model, State as ChromeState};
use quantick_drawings::DRAWING_TOOLS;
fn step(state: &mut ChromeState, intent: Intent) -> Option<Effect> {
    Model::update(state, ChromeIntent::Scenario(intent)).take_scenario()
}
fn installed(input: DrawingDemoLaunch) -> ChromeState {
    let mut state = ChromeState::default();
    step(
        &mut state,
        Intent::QueueLaunch(Launch {
            demos: input,
            ..Default::default()
        }),
    );
    step(&mut state, Intent::ApplyLaunch);
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
fn queued_launch_applies_once_at_the_owner_phase() {
    let mut state = ChromeState::default();
    step(
        &mut state,
        Intent::QueueLaunch(Launch {
            demos: DrawingDemoLaunch {
                gallery: Some(DrawingsDemo::default()),
                avwap: true,
                ..Default::default()
            },
            manager: true,
            inspector: true,
            note: true,
            tab: Some(InspectorTab::Coordinates),
            inspector_position: Some([-4.0, 200.5]),
            bar_position: Some([0.0, -8.0]),
        }),
    );
    assert!(!state.scenario.gallery_requested() && !state.scenario.note_requested());
    assert!(!state.core.manager.open && !state.core.shared.open);
    assert_eq!(
        step(&mut state, Intent::ApplyLaunch),
        Some(Effect::Launch {
            inspector_position: Some([-4.0, 200.5]),
            bar_position: Some([0.0, -8.0]),
        })
    );
    assert!(state.scenario.gallery_requested() && state.scenario.avwap_requested());
    assert!(state.core.manager.open && state.core.shared.open && state.scenario.note_requested());
    assert_eq!(state.inspector.tab, InspectorTab::Coordinates);
    assert_eq!(state.inspector.remembered_position(), Some([-4.0, 200.5]));
    Model::update(&mut state, ChromeIntent::ManagerOpen(false));
    Model::update(&mut state, ChromeIntent::InspectorOpen(false));
    step(&mut state, Intent::NotePlaced(true));
    assert!(step(&mut state, Intent::ApplyLaunch).is_none());
    assert!(
        !state.core.manager.open && !state.core.shared.open && !state.scenario.note_requested()
    );
}
#[test]
fn ready_gallery_consumes_even_when_recipient_places_nothing_and_retains_recut_tail() {
    let mut state = installed(DrawingDemoLaunch {
        gallery: Some(DrawingsDemo {
            shared: Some(true),
            ..Default::default()
        }),
        recut: true,
        ..Default::default()
    });
    let ready = |slots| Intent::GalleryReady {
        series: SeriesFacts { slots, ..facts() },
        tools: DRAWING_TOOLS.to_vec(),
        samples: vec![],
    };
    assert!(step(&mut state, ready(0)).is_none() && state.scenario.gallery_requested());
    assert!(step(&mut state, ready(8 * DRAWING_TOOLS.len() - 1)).is_none());
    let Some(Effect::Gallery(plan)) = step(&mut state, ready(200)) else {
        panic!("ready gallery")
    };
    assert!(!state.scenario.gallery_requested());
    assert!(!plan.anchors.is_empty());
    assert!(plan.anchors.iter().any(|anchor| anchor.finish));
    for anchor in plan.anchors {
        assert!(
            step(
                &mut state,
                Intent::GalleryPlaced {
                    tool: anchor.tool,
                    completed: false,
                    selected: None
                }
            )
            .is_none()
        );
    }
    assert_eq!(
        step(&mut state, Intent::GalleryFinished),
        Some(Effect::GallerySelection(None))
    );
    assert!(step(&mut state, Intent::GalleryFinished).is_none());
    assert!(
        state.scenario.recut_requested(),
        "recut remains owed even without a successful receipt"
    );
    assert!(step(&mut state, ready(200)).is_none());
}
#[test]
fn draft_waits_for_prerequisites_and_consumes_effective_zero() {
    let mut state = installed(DrawingDemoLaunch {
        draft: Some(DrawingDraft {
            anchors: 2,
            constrain: true,
        }),
        ..Default::default()
    });
    let tool = DrawingTool::by_id("horizontal-line").unwrap();
    for (tool, chart_available, slots) in [
        (None, true, 200),
        (Some(tool), false, 200),
        (Some(tool), true, 0),
    ] {
        assert!(
            step(
                &mut state,
                Intent::DraftReady {
                    tool,
                    chart_available,
                    series: SeriesFacts { slots, ..facts() }
                }
            )
            .is_none()
        );
    }
    assert!(state.scenario.draft_requested().is_some());
    let Some(Effect::Draft(plan)) = step(
        &mut state,
        Intent::DraftReady {
            tool: Some(tool),
            chart_available: true,
            series: facts(),
        },
    ) else {
        panic!("ready draft")
    };
    assert!(state.scenario.draft_requested().is_none());
    assert!(plan.anchors.is_empty());
    assert_eq!(plan.hand, [0.2, -0.15]);
    assert!(plan.constrain);
}
#[test]
fn profile_wait_missing_tool_prefix_retry_and_compare_rearm_are_distinct() {
    let mut state = installed(DrawingDemoLaunch {
        profile: Some(FrvpDemo {
            stress: true,
            ..Default::default()
        }),
        ..Default::default()
    });
    for slots in [None, Some(11)] {
        assert_eq!(
            step(
                &mut state,
                Intent::PrepareProfile {
                    slots,
                    tool_available: false
                }
            ),
            Some(Effect::ProfilePreparation(ProfilePreparation::Wait))
        );
        assert!(state.scenario.profile_requested().is_some());
    }
    for _ in 0..2 {
        assert_eq!(
            step(
                &mut state,
                Intent::PrepareProfile {
                    slots: Some(12),
                    tool_available: true
                }
            ),
            Some(Effect::ProfilePreparation(ProfilePreparation::Prefix {
                candles: 25_000
            }))
        );
        assert!(
            state.scenario.profile_requested().is_some(),
            "failed prefix delivery leaves the request retryable"
        );
    }
    assert_eq!(
        step(
            &mut state,
            Intent::PrepareProfile {
                slots: Some(12),
                tool_available: false
            }
        ),
        Some(Effect::ProfilePreparation(ProfilePreparation::Wait))
    );
    assert!(state.scenario.profile_requested().is_none());
    let mut state = installed(DrawingDemoLaunch {
        profile: Some(FrvpDemo {
            compare: true,
            select: true,
            stress: false,
        }),
        ..Default::default()
    });
    assert!(
        step(
            &mut state,
            Intent::ProfileReady(ProfileFacts {
                slots: 100,
                prefix: 41
            })
        )
        .is_none()
    );
    assert!(state.scenario.profile_requested().is_some());
    let Some(Effect::Profile(geometry)) = step(
        &mut state,
        Intent::ProfileReady(ProfileFacts {
            slots: 100,
            prefix: 40,
        }),
    ) else {
        panic!("ready profile")
    };
    assert!(state.scenario.profile_requested().is_none());
    assert_eq!(geometry.close_slot, 64, "price comes from seam geometry");
    let plan = geometry.plan(
        DrawingTool::by_id(quantick_drawings::frvp::TOOL_ID).unwrap(),
        None,
    );
    assert!(plan.select);
    assert_eq!(
        plan.anchors
            .iter()
            .map(|a| a.select_after)
            .collect::<Vec<_>>(),
        [false, true, false, true]
    );
    assert_eq!(
        plan.anchors.iter().map(|a| a.point.bar).collect::<Vec<_>>(),
        [50.0, 74.0, 75.0, 99.0]
    );
    assert!(plan.anchors.iter().all(|a| a.point.price == 1.0));
    assert_eq!(
        plan.anchors.iter().map(|a| a.look).collect::<Vec<_>>(),
        [
            Look::Profile { outline: true },
            Look::Profile { outline: true },
            Look::Profile { outline: false },
            Look::Profile { outline: false }
        ]
    );
}
#[test]
fn avwap_consumes_before_a_missing_tool_and_does_not_lookup_before_ready() {
    let mut state = installed(DrawingDemoLaunch {
        avwap: true,
        ..Default::default()
    });
    assert!(step(&mut state, Intent::AvwapReady { slots: 11 }).is_none());
    assert!(state.scenario.avwap_requested());
    assert!(matches!(
        step(&mut state, Intent::AvwapReady { slots: 12 }),
        Some(Effect::Avwap(_))
    ));
    assert!(!state.scenario.avwap_requested());
    // No tool is available; the host performs no placement and sends no acknowledgement.
    assert!(step(&mut state, Intent::AvwapReady { slots: 200 }).is_none());
}
#[test]
fn placement_acknowledgement_depends_only_on_placed_not_editor_state() {
    let mut state = ChromeState::default();
    step(&mut state, Intent::RequestNote(true));
    for (placed, pending) in [(false, true), (true, false), (false, false)] {
        step(&mut state, Intent::NotePlaced(placed));
        assert_eq!(state.scenario.note_requested(), pending);
    }
}
#[test]
fn demo_recut_preserves_the_count_upper_bound() {
    for (count, expected) in [(0, 2), (50, 100), (u64::MAX, u64::MAX)] {
        assert_eq!(recut_count(count), expected);
    }
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

fn note_plan(
    chart_available: bool,
    slots: usize,
    close: Option<f64>,
    range: Option<(f64, f64)>,
) -> Option<NotePlacement> {
    let mut state = ChromeState::default();
    match step(
        &mut state,
        Intent::PlanNote {
            chart_available,
            series: SeriesFacts {
                slots,
                last_close: close,
                auto_range: range,
            },
        },
    ) {
        Some(Effect::Note(plan)) => Some(plan),
        _ => None,
    }
}
#[test]
fn note_geometry_preserves_original_slot_range_and_fallback_law() {
    assert!(note_plan(false, 100, Some(10.0), None).is_none());
    assert!(note_plan(true, 0, Some(10.0), None).is_none());
    for (slots, slot) in [(1, 0), (2, 1), (89, 45), (90, 45), (100, 55)] {
        let plan = note_plan(true, slots, None, None).unwrap();
        assert_eq!(plan.slot, slot);
        assert_eq!(plan.price, 1.0);
    }
    assert_eq!(
        note_plan(true, 2, Some(7.0), Some((5.0, 3.0)))
            .unwrap()
            .price,
        7.0
    );
    assert_eq!(
        note_plan(true, 2, Some(7.0), Some((2.0, 4.0)))
            .unwrap()
            .price,
        3.0
    );
    assert!(
        note_plan(true, 2, None, Some((0.0, f64::INFINITY)))
            .unwrap()
            .price
            .is_infinite()
    );
    assert!(
        note_plan(true, 2, None, Some((f64::NEG_INFINITY, f64::INFINITY)))
            .unwrap()
            .price
            .is_nan()
    );
}

#[test]
fn gallery_receipts_keep_local_only_on_completion_and_remember_matching_selection() {
    let tool = DrawingTool::by_id("trend-line").unwrap();
    let mut state = installed(DrawingDemoLaunch {
        gallery: Some(DrawingsDemo {
            shared: Some(false),
            select_tool: Some(tool.id().into()),
            ..Default::default()
        }),
        ..Default::default()
    });
    step(
        &mut state,
        Intent::GalleryReady {
            series: facts(),
            tools: DRAWING_TOOLS.to_vec(),
            samples: vec![],
        },
    );
    assert!(
        step(
            &mut state,
            Intent::GalleryPlaced {
                tool,
                completed: false,
                selected: Some(9)
            }
        )
        .is_none()
    );
    assert_eq!(
        step(
            &mut state,
            Intent::GalleryPlaced {
                tool,
                completed: true,
                selected: Some(3)
            }
        ),
        Some(Effect::KeepSelectedLocal)
    );
    let other = DrawingTool::by_id("horizontal-line").unwrap();
    assert_eq!(
        step(
            &mut state,
            Intent::GalleryPlaced {
                tool: other,
                completed: true,
                selected: Some(8)
            }
        ),
        Some(Effect::KeepSelectedLocal)
    );
    assert_eq!(
        step(&mut state, Intent::GalleryFinished),
        Some(Effect::GallerySelection(Some(3)))
    );
    assert!(step(&mut state, Intent::GalleryFinished).is_none());
}
#[test]
fn scenario_effect_merge_keeps_the_first_plan_without_idle_allocation() {
    use crate::drawing_chrome::DrawingChromeAsk;
    let mut ask = DrawingChromeAsk::default();
    assert!(ask.readbacks.is_none());
    ask.merge(DrawingChromeAsk::scenario(Some(Effect::GallerySelection(
        Some(3),
    ))));
    ask.merge(DrawingChromeAsk::scenario(Some(Effect::KeepSelectedLocal)));
    assert_eq!(ask.take_scenario(), Some(Effect::GallerySelection(Some(3))));
}
