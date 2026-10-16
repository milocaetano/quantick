use super::*;

const DASH: f32 = 5.0;
const GAP: f32 = 4.0;
const STEP: f32 = DASH + GAP;

fn stroke() -> Stroke {
    Stroke::new(1.0, crate::WHITE)
}

fn clip() -> PxRect {
    PxRect::from_min_max(Px::new(0.0, 0.0), Px::new(100.0, 100.0))
}

/// Every dash's endpoints, in painting order.
fn dashes(scene: &MarkScene) -> Vec<[Px; 2]> {
    scene
        .shapes
        .iter()
        .map(|shape| match shape {
            MarkShape::Line { from, to, .. } => [*from, *to],
            other => panic!("a dashed segment is plain lines, not {other:?}"),
        })
        .collect()
}

/// A trader's channel can run far past the chart: a midline a million
/// pixels long is still painted every frame. Only the dashes that can
/// reach the clip are walked, at most one per step across it plus the
/// partial ones on each edge.
#[test]
fn a_dashed_segment_far_past_the_clip_walks_only_its_visible_part() {
    let mut scene = MarkScene::default();
    let (from, to) = (Px::new(-1.0e6, 50.0), Px::new(1.0e6, 50.0));
    dashed_segment_within(&mut scene, clip(), from, to, stroke(), DASH, GAP);
    let dashes = dashes(&scene);
    let across = (clip().width() / STEP).ceil() as usize;
    assert!(
        !dashes.is_empty() && dashes.len() <= across + 3,
        "{} dashes for a {} px clip",
        dashes.len(),
        clip().width()
    );
    // The dash holding each edge may start a step before it.
    let reach = clip().expand(DASH + STEP);
    for [start, end] in &dashes {
        assert!(
            reach.contains(*start) && reach.contains(*end),
            "{start:?}-{end:?}"
        );
    }
}

/// The clipped walk is a window onto the whole walk: the same dashes land
/// on the same pixels, so a dash never crawls as the chart pans.
#[test]
fn a_clipped_dashed_segment_keeps_the_whole_segment_phase() {
    for (from, to) in [
        (Px::new(-47.0, 50.0), Px::new(260.0, 50.0)),
        (Px::new(-130.0, -80.0), Px::new(310.0, 240.0)),
        (Px::new(310.0, 240.0), Px::new(-130.0, -80.0)),
    ] {
        let (mut whole, mut clipped) = (MarkScene::default(), MarkScene::default());
        dashed_segment(&mut whole, from, to, stroke(), DASH, GAP);
        dashed_segment_within(&mut clipped, clip(), from, to, stroke(), DASH, GAP);
        let whole = dashes(&whole);
        let clipped = dashes(&clipped);
        assert!(clipped.len() < whole.len(), "{from:?}-{to:?}");
        for dash in &clipped {
            assert!(
                whole.iter().any(|known| {
                    (known[0] - dash[0]).length() < 1e-2 && (known[1] - dash[1]).length() < 1e-2
                }),
                "{dash:?} is not a dash of the whole walk {from:?}-{to:?}"
            );
        }
        let inside = |dash: &[Px; 2]| clip().contains(dash[0]) || clip().contains(dash[1]);
        assert_eq!(
            clipped.iter().filter(|dash| inside(dash)).count(),
            whole.iter().filter(|dash| inside(dash)).count(),
            "every dash touching the clip is kept {from:?}-{to:?}"
        );
    }
}

/// A visible span that starts far enough out that one `f32` step no
/// longer moves the position: the walk must still end, with at most one
/// dash per stride across the clip. Accumulating the step there spins
/// forever.
#[test]
fn a_dashed_segment_starting_past_f32_step_resolution_still_ends() {
    let mut scene = MarkScene::default();
    let (from, to) = (Px::new(-3.0e8, 50.0), Px::new(3.0e8, 50.0));
    dashed_segment_within(&mut scene, clip(), from, to, stroke(), DASH, GAP);
    let across = (clip().width() / STEP).ceil() as usize;
    assert!(
        !scene.shapes.is_empty() && scene.shapes.len() <= across + 3,
        "{} dashes for a {} px clip",
        scene.shapes.len(),
        clip().width()
    );
}

/// A segment the clip never meets, or one that cannot be walked, paints
/// nothing.
#[test]
fn a_dashed_segment_outside_the_clip_paints_nothing() {
    let mut scene = MarkScene::default();
    for (from, to) in [
        (Px::new(-1.0e6, 150.0), Px::new(1.0e6, 150.0)),
        (Px::new(120.0, -500.0), Px::new(120.0, 500.0)),
        (Px::new(f32::NAN, 50.0), Px::new(50.0, 50.0)),
        (Px::new(50.0, 50.0), Px::new(f32::INFINITY, 50.0)),
    ] {
        dashed_segment_within(&mut scene, clip(), from, to, stroke(), DASH, GAP);
    }
    assert!(scene.shapes.is_empty());
}
