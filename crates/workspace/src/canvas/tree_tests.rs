//! The registry and the geometry each preset resolves to.

use super::*;

const AREA: CanvasRect = CanvasRect::new(0.0, 0.0, 1600.0, 900.0);
const THIRD: f32 = 1600.0 / 3.0;

/// `(slot, x, y, w, h)` per pane, in slot order, on a 1600x900 canvas.
type Golden = &'static [(PaneSlot, f32, f32, f32, f32)];

/// The trader's mock, measured: context column 40 % beside the flow pane,
/// the flow pane 62 % high when it sits on top.
const GOLDEN: &[(&str, Golden)] = &[
    ("flow", &[(0, 0.0, 0.0, 1600.0, 900.0)]),
    (
        "time+flow",
        &[(0, 0.0, 0.0, 640.0, 900.0), (1, 640.0, 0.0, 960.0, 900.0)],
    ),
    (
        "time+time+flow",
        &[
            (0, 0.0, 0.0, 320.0, 900.0),
            (1, 320.0, 0.0, 320.0, 900.0),
            (2, 640.0, 0.0, 960.0, 900.0),
        ],
    ),
    (
        "stack2+flow",
        &[
            (0, 0.0, 0.0, 640.0, 450.0),
            (1, 0.0, 450.0, 640.0, 450.0),
            (2, 640.0, 0.0, 960.0, 900.0),
        ],
    ),
    (
        "stack3+flow",
        &[
            (0, 0.0, 0.0, 640.0, 300.0),
            (1, 0.0, 300.0, 640.0, 300.0),
            (2, 0.0, 600.0, 640.0, 300.0),
            (3, 640.0, 0.0, 960.0, 900.0),
        ],
    ),
    (
        "grid4+flow",
        &[
            (0, 0.0, 0.0, 320.0, 450.0),
            (1, 320.0, 0.0, 320.0, 450.0),
            (2, 0.0, 450.0, 320.0, 450.0),
            (3, 320.0, 450.0, 320.0, 450.0),
            (4, 640.0, 0.0, 960.0, 900.0),
        ],
    ),
    (
        "flow-over-time",
        &[(0, 0.0, 558.0, 1600.0, 342.0), (1, 0.0, 0.0, 1600.0, 558.0)],
    ),
    (
        "flow-over-2time",
        &[
            (0, 0.0, 558.0, 800.0, 342.0),
            (1, 800.0, 558.0, 800.0, 342.0),
            (2, 0.0, 0.0, 1600.0, 558.0),
        ],
    ),
    ("time", &[(0, 0.0, 0.0, 1600.0, 900.0)]),
    (
        "charts-2",
        &[(0, 0.0, 0.0, 800.0, 900.0), (1, 800.0, 0.0, 800.0, 900.0)],
    ),
    (
        "charts-3",
        &[
            (0, 0.0, 0.0, THIRD, 900.0),
            (1, THIRD, 0.0, THIRD, 900.0),
            (2, 2.0 * THIRD, 0.0, THIRD, 900.0),
        ],
    ),
    (
        "charts-4",
        &[
            (0, 0.0, 0.0, 800.0, 450.0),
            (1, 800.0, 0.0, 800.0, 450.0),
            (2, 0.0, 450.0, 800.0, 450.0),
            (3, 800.0, 450.0, 800.0, 450.0),
        ],
    ),
    (
        "charts-5",
        &[
            (0, 0.0, 0.0, THIRD, 450.0),
            (1, THIRD, 0.0, THIRD, 450.0),
            (2, 2.0 * THIRD, 0.0, THIRD, 450.0),
            (3, 0.0, 450.0, 800.0, 450.0),
            (4, 800.0, 450.0, 800.0, 450.0),
        ],
    ),
    (
        "charts-6",
        &[
            (0, 0.0, 0.0, THIRD, 450.0),
            (1, THIRD, 0.0, THIRD, 450.0),
            (2, 2.0 * THIRD, 0.0, THIRD, 450.0),
            (3, 0.0, 450.0, THIRD, 450.0),
            (4, THIRD, 450.0, THIRD, 450.0),
            (5, 2.0 * THIRD, 450.0, THIRD, 450.0),
        ],
    ),
    (
        "charts-7",
        &[
            (0, 0.0, 0.0, 400.0, 450.0),
            (1, 400.0, 0.0, 400.0, 450.0),
            (2, 800.0, 0.0, 400.0, 450.0),
            (3, 1200.0, 0.0, 400.0, 450.0),
            (4, 0.0, 450.0, THIRD, 450.0),
            (5, THIRD, 450.0, THIRD, 450.0),
            (6, 2.0 * THIRD, 450.0, THIRD, 450.0),
        ],
    ),
    (
        "charts-8",
        &[
            (0, 0.0, 0.0, 400.0, 450.0),
            (1, 400.0, 0.0, 400.0, 450.0),
            (2, 800.0, 0.0, 400.0, 450.0),
            (3, 1200.0, 0.0, 400.0, 450.0),
            (4, 0.0, 450.0, 400.0, 450.0),
            (5, 400.0, 450.0, 400.0, 450.0),
            (6, 800.0, 450.0, 400.0, 450.0),
            (7, 1200.0, 450.0, 400.0, 450.0),
        ],
    ),
];

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-2
}

#[test]
fn every_preset_matches_its_golden_geometry() {
    assert_eq!(
        GOLDEN.len(),
        LAYOUT_PRESETS.len(),
        "one golden row per preset"
    );
    for (entry, (id, golden)) in LAYOUT_PRESETS.iter().zip(GOLDEN) {
        assert_eq!(entry.id, *id, "the golden table follows the registry order");
        let rects = entry.tree.rects(AREA);
        assert_eq!(rects.len(), golden.len(), "{id}: pane count");
        for ((slot, rect), (want_slot, x, y, w, h)) in rects.iter().zip(golden.iter()) {
            assert_eq!(slot, want_slot, "{id}: slot order");
            assert!(
                close(rect.x, *x) && close(rect.y, *y) && close(rect.w, *w) && close(rect.h, *h),
                "{id}: slot {slot} is {rect:?}, the mock draws ({x}, {y}, {w}, {h})"
            );
        }
        let area: f32 = rects.iter().map(|(_, rect)| rect.w * rect.h).sum();
        assert!(
            (area - AREA.w * AREA.h).abs() < 1.0,
            "{id}: the panes spend the canvas once"
        );
    }
}

#[test]
fn catalogue_is_eight_with_flow_then_eight_timeframes() {
    let ids: Vec<&str> = LAYOUT_PRESETS.iter().map(|preset| preset.id).collect();
    let labels: Vec<&str> = LAYOUT_PRESETS.iter().map(|preset| preset.label).collect();
    assert_eq!(
        labels,
        [
            "Flow",
            "1 TF + Flow",
            "2 TF + Flow",
            "2 stacked + Flow",
            "3 stacked + Flow",
            "4 TF + Flow",
            "Flow over TF",
            "Flow over 2 TF",
            "1 chart",
            "2 charts",
            "3 charts",
            "4 charts",
            "5 charts",
            "6 charts",
            "7 charts",
            "8 charts",
        ]
    );
    let groups: Vec<PresetGroup> = LAYOUT_PRESETS.iter().map(|preset| preset.group).collect();
    assert_eq!(groups[..8], [PresetGroup::WithFlow; 8]);
    assert_eq!(groups[8..], [PresetGroup::Timeframes; 8]);
    assert!(
        !labels.iter().any(|label| label.contains("2 Flows")),
        "2 Flows was rejected"
    );
    // The four ids configs and saved workspaces have always used.
    assert_eq!(preset("flow").unwrap().label, "Flow");
    assert_eq!(preset("time").unwrap().label, "1 chart");
    assert_eq!(preset("time+flow").unwrap().label, "1 TF + Flow");
    assert_eq!(preset("time+time+flow").unwrap().label, "2 TF + Flow");
    assert_eq!(MAX_CANVAS_PANES, 9);
    for (index, entry) in LAYOUT_PRESETS.iter().enumerate() {
        assert!(
            !ids[index + 1..].contains(&entry.id),
            "two presets answer to {}",
            entry.id
        );
        assert!(
            entry.kinds.len() <= MAX_CANVAS_PANES,
            "{} is too big",
            entry.id
        );
        let flows = entry
            .kinds
            .iter()
            .filter(|kind| **kind == PaneKind::Flow)
            .count();
        assert_eq!(
            flows,
            usize::from(entry.group == PresetGroup::WithFlow),
            "{}: one flow pane exactly when WITH FLOW",
            entry.id
        );
        if flows == 1 {
            assert_eq!(
                entry.kinds.last(),
                Some(&PaneKind::Flow),
                "{}: flow is the last slot",
                entry.id
            );
        }
        // `kinds[slot]` is the tree's pane `slot`.
        let mut panes = entry.tree.panes();
        panes.sort_by_key(|(slot, _)| *slot);
        let kinds: Vec<PaneKind> = panes.iter().map(|(_, kind)| *kind).collect();
        assert_eq!(kinds, entry.kinds, "{}: kinds and tree disagree", entry.id);
        let slots: Vec<PaneSlot> = panes.iter().map(|(slot, _)| *slot).collect();
        assert_eq!(
            slots,
            (0..entry.kinds.len() as PaneSlot).collect::<Vec<_>>()
        );
        // Each preset is recognised as itself and as no other.
        assert_eq!(matching_preset(entry.tree), Some(entry), "{}", entry.id);
    }
    assert_eq!(preset("no such layout"), None);
}
