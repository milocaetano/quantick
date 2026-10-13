//! Place measured FLOW chrome without moving or covering factual spheres.
use super::{FlowBounds, FlowRegionVisual};

/// Includes the painted separator rim and a small readable gap.
const CHROME_SPHERE_CLEARANCE_PX: f32 = 3.0;

/// Rows of measured legend entries, independent of a font or painter.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowChromeLayout {
    pub positions: Vec<[f32; 2]>,
    pub size: [f32; 2],
}

pub fn flow_chrome_layout(
    widths: &[f32],
    max_width: f32,
    row_height: f32,
    gap: f32,
) -> FlowChromeLayout {
    let max_width = max_width.max(1.0);
    let row_height = row_height.max(1.0);
    let gap = gap.max(0.0);
    let mut positions = Vec::with_capacity(widths.len());
    let mut x = 0.0;
    let mut y = 0.0;
    let mut widest: f32 = 0.0;
    for &raw_width in widths {
        let width = raw_width.max(0.0);
        if x > 0.0 && x + width > max_width {
            widest = widest.max((x - gap).max(0.0));
            x = 0.0;
            y += row_height;
        }
        positions.push([x, y]);
        x += width + gap;
    }
    widest = widest.max((x - gap).max(0.0)).min(max_width);
    let height = if widths.is_empty() {
        0.0
    } else {
        y + row_height
    };
    FlowChromeLayout {
        positions,
        size: [widest, height],
    }
}

/// Keep the panel's measured width and height, finding the first clear band
/// at or below its preferred top. The caller has already reserved the chart
/// header, indicator chips and position HUD above that top. A full canvas
/// omits optional chrome, never a sphere or its quantity.
pub fn place_flow_chrome(
    area: FlowBounds,
    panel: FlowBounds,
    regions: &[FlowRegionVisual],
) -> Option<FlowBounds> {
    let [[left, top], [right, bottom]] = panel;
    let height = bottom - top;
    if !area
        .iter()
        .chain(&panel)
        .flatten()
        .all(|value| value.is_finite())
        || left < area[0][0]
        || right > area[1][0]
        || right <= left
        || height <= 0.0
        || top < area[0][1]
    {
        return None;
    }
    // Each circle excludes one interval of possible panel-top positions.
    // Merge those intervals in one sorted sweep, independent of mark order.
    let mut blocked: Vec<_> = regions
        .iter()
        .filter_map(|region| {
            let disc = region.disc;
            let radius = disc.radius + CHROME_SPHERE_CLEARANCE_PX;
            let dx = (left - disc.center[0]).max(disc.center[0] - right).max(0.0);
            if dx >= radius {
                return None;
            }
            let half_height = (radius * radius - dx * dx).sqrt();
            Some((
                disc.center[1] - half_height - height,
                disc.center[1] + half_height,
            ))
        })
        .collect();
    blocked.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut placed_top = top;
    for (start, end) in blocked {
        if start >= placed_top {
            break;
        }
        placed_top = placed_top.max(end);
    }
    (placed_top + height <= area[1][1])
        .then_some([[left, placed_top], [right, placed_top + height]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow_execution::{FlowDisc, FlowRegionRole};

    #[test]
    fn compact_legend_wraps_without_exceeding_width() {
        let layout = flow_chrome_layout(&[90.0, 90.0, 90.0], 190.0, 17.0, 3.0);
        assert_eq!(layout.positions, [[0.0, 0.0], [93.0, 0.0], [0.0, 17.0]]);
        assert!(layout.size[0] <= 190.0);
        assert_eq!(layout.size[1], 34.0);
    }

    fn region(x: f32, y: f32, radius: f32) -> FlowRegionVisual {
        let disc = FlowDisc {
            center: [x, y],
            radius,
        };
        FlowRegionVisual {
            dot_index: 0,
            role: FlowRegionRole::Peak,
            source_disc: disc,
            disc,
            unresolved_overlap: false,
        }
    }

    #[test]
    fn chrome_preserves_header_clearance_and_skips_spheres_in_any_order() {
        let area = [[0.0, 0.0], [600.0, 400.0]];
        let panel = [[8.0, 48.0], [390.0, 88.0]];
        let regions = [region(200.0, 55.0, 24.0), region(240.0, 100.0, 15.0)];
        let placed = place_flow_chrome(area, panel, &regions).unwrap();
        assert_eq!(placed, [[8.0, 118.0], [390.0, 158.0]]);
        assert_eq!(
            place_flow_chrome(area, panel, &[regions[1], regions[0]]),
            Some(placed)
        );
        assert!(regions.iter().all(|region| !region.disc.visible(placed)));
        assert_eq!(
            place_flow_chrome(area, panel, &[region(500.0, 55.0, 24.0)]),
            Some(panel)
        );
    }

    #[test]
    fn wrapped_chrome_stands_down_when_no_gap_fits_without_changing_marks() {
        let area = [[0.0, 0.0], [200.0, 100.0]];
        let panel = [[8.0, 30.0], [190.0, 80.0]];
        let regions = [region(100.0, 75.0, 30.0)];
        assert_eq!(place_flow_chrome(area, panel, &regions), None);
        assert_eq!(place_flow_chrome(area, panel, &[]), Some(panel));
        assert_eq!(regions[0].disc.center, [100.0, 75.0]);
        assert_eq!(regions[0].disc.radius, 30.0);
    }
}
