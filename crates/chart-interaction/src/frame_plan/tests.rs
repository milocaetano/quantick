use crate::frame_plan::HomeVisibility::{self, Always, Home, Market};
use crate::frame_plan::StageScope::{self, Settle, Tick, Window};
use crate::stage_registry::{dependency_pairs, hoisted};

/// Every harness family compiled in: the capture build's plan.
mod harness {
    crate::frame_stages!(scenario: all(), control: all(), scripted: all());
}

/// No harness family: the plan a release build runs.
mod release {
    crate::frame_stages!(scenario: any(), control: any(), scripted: any());
}

#[test]
fn the_frame_executes_its_registered_order() {
    use harness::{FramePlan, FrameStage};
    assert_eq!(FramePlan::stages().collect::<Vec<_>>(), FrameStage::ORDER);
    assert_eq!(FramePlan::stages().len(), FrameStage::COUNT);
    assert_eq!(FrameStage::NODES[0].name, "RecordFrameTime");
}

#[test]
fn the_frame_refuses_every_hoisted_dependency_in_either_build() {
    let pairs: Vec<_> = dependency_pairs(&harness::FrameStage::NODES).collect();
    assert_eq!(pairs.len(), 46, "{pairs:?}");
    for (earlier, later) in pairs {
        let order = hoisted(harness::FrameStage::ORDER, earlier, later);
        assert!(!harness::FrameStage::is_valid_order(&order), "{order:?}");
    }
    let pairs: Vec<_> = dependency_pairs(&release::FrameStage::NODES).collect();
    assert_eq!(pairs.len(), 41, "{pairs:?}");
    for (earlier, later) in pairs {
        let order = hoisted(release::FrameStage::ORDER, earlier, later);
        assert!(!release::FrameStage::is_valid_order(&order), "{order:?}");
    }
}

/// A release build registers no harness stage: not a no-op arm, no stage.
#[test]
fn a_release_frame_has_no_harness_stage() {
    let release: Vec<_> = release::FrameStage::NODES
        .iter()
        .map(|node| node.name)
        .collect();
    let harness: Vec<_> = harness::FrameStage::NODES
        .iter()
        .map(|node| node.name)
        .filter(|name| !release.contains(name))
        .collect();
    assert_eq!(
        harness,
        [
            "HistoryNoteHook",
            "EnableControlAccess",
            "TakeMark",
            "AnnotateHooks",
            "EvidenceHook",
            "ScenarioHooks",
            "WindowStartupHook",
        ]
    );
    assert_eq!(release::FrameStage::COUNT, 34);
    assert_eq!(harness::FrameStage::COUNT, 41);
}

#[test]
fn independent_frame_stages_stay_free_to_move() {
    // The drain and the frame clock declare nothing between them.
    let mut order: Vec<_> = release::FramePlan::stages().collect();
    order.swap(0, 1);
    assert!(release::FrameStage::is_valid_order(&order));
}

#[test]
fn owner_stages_continue_without_presentation_in_both_builds() {
    assert!(!harness::FrameStage::DrainSources.needs_presentation());
    assert!(!harness::FrameStage::IndicatorSurfaces.needs_presentation());
    assert!(harness::FrameStage::Canvas.needs_presentation());
    assert!(harness::FrameStage::IndicatorDialogs.needs_presentation());
    assert!(harness::FrameStage::EvidenceHook.needs_presentation());
    assert!(!release::FrameStage::DrainSources.needs_presentation());
    assert!(!release::FrameStage::IndicatorSurfaces.needs_presentation());
    assert!(release::FrameStage::Canvas.needs_presentation());
}

/// Every stage with the scope and Home visibility it declares, in the order
/// the capture build runs them. A stage that changes scope, Home visibility
/// or place fails here, not in a frame.
const DECLARED: [(&str, StageScope, HomeVisibility); 41] = [
    ("RecordFrameTime", Tick, Always),
    ("DrainSources", Tick, Always),
    ("ExpireHistoryNotes", Tick, Always),
    ("HistoryNoteHook", Tick, Always),
    ("EnableControlAccess", Tick, Always),
    ("ReplayTrace", Tick, Always),
    ("TakeMark", Tick, Always),
    ("AnnotateHooks", Tick, Always),
    ("EvidenceHook", Tick, Always),
    ("GatewayService", Tick, Always),
    ("ScenarioHooks", Tick, Always),
    ("WindowHousekeeping", Tick, Always),
    ("IndicatorSurfaces", Tick, Always),
    ("StrategyPopupRequests", Tick, Always),
    ("IndicatorMaintenance", Tick, Always),
    ("LayoutSwitches", Tick, Always),
    ("MirrorLoadingWaits", Tick, Always),
    ("RequestRepaint", Tick, Always),
    ("WindowStartupHook", Window, Always),
    ("TabKeys", Window, Always),
    ("MarketKeys", Window, Market),
    ("MenuBar", Window, Always),
    ("HomeToolbar", Window, Home),
    ("Toolbar", Window, Market),
    ("IndicatorDialogs", Window, Always),
    ("Surfaces", Window, Always),
    ("StatusLine", Window, Always),
    ("LayoutDialogs", Window, Always),
    ("ReplayBrowser", Window, Always),
    ("DrawingRail", Window, Market),
    ("Dock", Window, Market),
    ("PinnedInspector", Window, Market),
    ("FeedSwitch", Window, Always),
    ("HomeCanvas", Window, Home),
    ("Canvas", Window, Market),
    ("DrawingChrome", Window, Market),
    ("StrategyCleanup", Window, Always),
    ("Tail", Window, Always),
    ("BackgroundStrategyCleanup", Settle, Always),
    ("PlayAlerts", Settle, Always),
    ("BackgroundPaperPanels", Settle, Always),
];

#[test]
fn every_stage_declares_its_scope_and_home_visibility_in_either_build() {
    let harness: Vec<_> = harness::FrameStage::ORDER
        .iter()
        .map(|stage| (stage.name(), stage.scope(), stage.home()))
        .collect();
    assert_eq!(harness, DECLARED);
    let release: Vec<_> = release::FrameStage::ORDER
        .iter()
        .map(|stage| (stage.name(), stage.scope(), stage.home()))
        .collect();
    let without_harness: Vec<_> = DECLARED
        .into_iter()
        .filter(|(name, ..)| {
            release::FrameStage::NODES
                .iter()
                .any(|node| node.name == *name)
        })
        .collect();
    assert_eq!(release, without_harness);
}

#[test]
fn one_pass_per_scope_runs_the_declaration_in_order() {
    use harness::{FramePlan, FrameStage};
    let passes: Vec<_> = [Tick, Window, Settle]
        .into_iter()
        .flat_map(FramePlan::stages_in)
        .collect();
    assert_eq!(passes, FrameStage::ORDER, "a one-window tick is the plan");
    let release: Vec<_> = [Tick, Window, Settle]
        .into_iter()
        .flat_map(release::FramePlan::stages_in)
        .collect();
    assert_eq!(release, release::FrameStage::ORDER);
    let names = |scope| {
        FramePlan::stages_in(scope)
            .map(FrameStage::name)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(Settle),
        [
            "BackgroundStrategyCleanup",
            "PlayAlerts",
            "BackgroundPaperPanels"
        ]
    );
    assert!(
        names(Tick).contains(&"EvidenceHook"),
        "captured once a tick"
    );
    assert!(
        !names(Window).contains(&"DrainSources"),
        "drained once a tick"
    );
}

#[test]
fn a_stage_moved_out_of_its_scope_is_refused() {
    use harness::FrameStage;
    let order = FrameStage::ORDER;
    assert!(FrameStage::is_grouped_by_scope(&order));
    for scope in [Window, Settle] {
        let first = order
            .iter()
            .position(|stage| stage.scope() == scope)
            .expect("every scope has a stage");
        let moved = hoisted(order, first - 1, first);
        assert!(
            !FrameStage::is_grouped_by_scope(&moved),
            "{:?} ahead of {:?}",
            order[first],
            order[first - 1]
        );
    }
    let mut late_tick = order;
    late_tick[..].rotate_left(1);
    assert!(!FrameStage::is_grouped_by_scope(&late_tick));
}

#[test]
fn home_visibility_is_decided_once() {
    assert!(Always.runs(true) && Always.runs(false));
    assert!(!Market.runs(true) && Market.runs(false));
    assert!(Home.runs(true) && !Home.runs(false));
    assert!(Window.ends_with_its_window());
    assert!(!Tick.ends_with_its_window() && !Settle.ends_with_its_window());
}
