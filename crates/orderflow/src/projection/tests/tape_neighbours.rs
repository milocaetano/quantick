//! Search-box pruning must retain every geometrically eligible neighbour.
use super::*;

#[test]
fn actual_reach_matches_brute_force_across_negative_and_grid_boundary_positions() {
    let geometry = TapeDotGeometry {
        left_x: 0.0,
        right_x: 1.0,
        width_px: 1.0,
        height_px: 1.0,
    };
    let mut neighbours = Neighbours::new(24.0, geometry);
    let axis = [
        -48.001, -48.0, -24.001, -24.0, -0.001, 0.0, 0.001, 23.999, 24.0, 24.001, 48.0,
    ];
    let points: Vec<_> = axis
        .iter()
        .flat_map(|&x| axis.iter().map(move |&y| (x, y)))
        .collect();
    for (index, &(x, y)) in points.iter().enumerate() {
        neighbours.insert(index, x, y);
    }
    for &(x, y) in &points {
        for reach in [0.001, 12.0, 13.0, 24.0] {
            let inside = |index: &usize| {
                let (other_x, other_y) = points[*index];
                (x - other_x).hypot(y - other_y) <= reach
            };
            let mut actual: Vec<_> = neighbours.candidates(x, y, reach).filter(inside).collect();
            actual.sort_unstable();
            let expected: Vec<_> = (0..points.len()).filter(inside).collect();
            assert_eq!(actual, expected, "search at ({x}, {y}), reach {reach}");
        }
    }
    // A tiny search does not scan the entire adjacent cell on the far side.
    let mut small = Neighbours::new(24.0, geometry);
    small.insert(0, 12.0, 12.0);
    small.insert(1, 25.0, 12.0);
    assert_eq!(small.candidates(12.0, 12.0, 1.0).collect::<Vec<_>>(), [0]);
}
