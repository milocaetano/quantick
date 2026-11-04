//! Place measured FLOW chrome without moving or covering factual spheres.
use super::{FlowBounds, FlowRegionVisual};

/// Includes the painted separator rim and a small readable gap.
const CHROME_SPHERE_CLEARANCE_PX: f32 = 3.0;

/// Compact optional chrome stays beside its header instead of spanning price content.
pub const FLOW_CHROME_MAX_WIDTH_PX: f32 = 320.0;
/// A complete measured caption or key must fit within this header-local height.
pub const FLOW_CHROME_MAX_HEIGHT_PX: f32 = 96.0;

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

/// Keep optional chrome at its preferred header position. The caller reserves
/// indicator chips and the position HUD above that position. A collision or
/// incomplete fit omits chrome; searching lower would cover candles or price lines.
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
        || right - left > FLOW_CHROME_MAX_WIDTH_PX
        || height <= 0.0
        || height > FLOW_CHROME_MAX_HEIGHT_PX
        || top < area[0][1]
        || bottom > area[1][1]
    {
        return None;
    }
    let blocked = regions.iter().any(|region| {
        let disc = region.disc;
        let radius = disc.radius + CHROME_SPHERE_CLEARANCE_PX;
        let dx = (left - disc.center[0]).max(disc.center[0] - right).max(0.0);
        let dy = (top - disc.center[1]).max(disc.center[1] - bottom).max(0.0);
        dx * dx + dy * dy < radius * radius
    });
    (!blocked).then_some(panel)
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
    fn chrome_preserves_header_position_or_stands_down_in_any_order() {
        let area = [[0.0, 0.0], [600.0, 400.0]];
        let panel = [[8.0, 48.0], [328.0, 88.0]];
        let regions = [region(200.0, 55.0, 24.0), region(240.0, 100.0, 15.0)];
        assert_eq!(place_flow_chrome(area, panel, &regions), None);
        assert_eq!(
            place_flow_chrome(area, panel, &[regions[1], regions[0]]),
            None
        );
        assert_eq!(
            place_flow_chrome(area, panel, &[region(500.0, 55.0, 24.0)]),
            Some(panel)
        );
    }

    #[test]
    fn chrome_never_migrates_through_a_chain_of_spheres_into_price_content() {
        let area = [[0.0, 0.0], [480.0, 700.0]];
        let caption = [[8.0, 28.0], [328.0, 50.0]];
        let regions: Vec<_> = (0..12)
            .map(|index| region(200.0, 45.0 + index as f32 * 50.0, 24.0))
            .collect();
        assert_eq!(place_flow_chrome(area, caption, &regions), None);
        let compact = [[8.0, 28.0], [160.0, 72.0]];
        assert_eq!(place_flow_chrome(area, compact, &regions), Some(compact));
    }

    #[test]
    fn chrome_requires_a_complete_bounded_header_footprint() {
        let area = [[0.0, 0.0], [400.0, 80.0]];
        assert_eq!(
            place_flow_chrome(area, [[8.0, 28.0], [328.0, 81.0]], &[]),
            None
        );
        assert_eq!(
            place_flow_chrome(
                [[0.0, 0.0], [400.0, 200.0]],
                [[8.0, 10.0], [328.0, 107.0]],
                &[]
            ),
            None
        );
        let panel = [[8.0, 10.0], [328.0, 106.0]];
        assert_eq!(
            place_flow_chrome([[0.0, 0.0], [400.0, 200.0]], panel, &[]),
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
