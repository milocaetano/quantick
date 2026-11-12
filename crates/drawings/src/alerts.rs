//! Pure drawing projections for the reusable alert observer.
use quantick_alerts::{Geometry, Line, Point, Unavailable};

/// Supply mirror provenance from the existing durable drawing owner contract.
pub fn fact<'a, T: crate::ToolCatalog>(
    drawing: &'a crate::Drawing<T>,
    key: Option<&quantick_workspace::market_settings::DrawingKey>,
) -> Option<quantick_alerts::observer::Drawing<'a>> {
    let target = drawing.durable_identity.as_ref()?;
    let known_mirror = key.is_some_and(|key| {
        let key = if drawing.all_timeframes() {
            quantick_workspace::market_settings::DrawingKey::all_timeframes(key.market.clone())
        } else {
            key.clone()
        };
        quantick_workspace::drawing_identity::belongs_to(target, &key)
    });
    Some(quantick_alerts::observer::Drawing {
        target,
        geometry: project(drawing),
        known_mirror,
    })
}

pub fn project<T: crate::ToolCatalog>(
    drawing: &crate::Drawing<T>,
) -> Result<Geometry, Unavailable> {
    if drawing.foreign_market || drawing.off_series {
        return Err(Unavailable::OutsideTimeDomain);
    }
    let point = |point: &crate::ChartPoint| Point {
        coordinate: f64::from(point.bar),
        price: point.price,
    };
    let [first, second, rest @ ..] = drawing.points.as_slice() else {
        return Err(Unavailable::InvalidGeometry);
    };
    let mut line = Line {
        first: point(first),
        second: point(second),
        extend_left: false,
        extend_right: false,
    };
    match drawing.tool.id() {
        "trend-line" => Ok(Geometry::Line(line)),
        "rectangle" => {
            let (mut first, mut second) = (point(first), point(second));
            if first.coordinate > second.coordinate {
                std::mem::swap(&mut first, &mut second);
            }
            if let Some(payload) = drawing
                .payload
                .as_any()
                .downcast_ref::<crate::RectanglePayload>()
            {
                if payload.extend_left {
                    first.coordinate = -f64::MAX;
                }
                if payload.extend_right {
                    second.coordinate = f64::MAX;
                }
            }
            Ok(Geometry::Rectangle { first, second })
        }
        "parallel-channel" => {
            let third = rest.first().ok_or(Unavailable::InvalidGeometry)?;
            if let Some(payload) = drawing
                .payload
                .as_any()
                .downcast_ref::<crate::ChannelPayload>()
            {
                line.extend_left = payload.extend_left;
                line.extend_right = payload.extend_right;
            }
            let span = line.second.coordinate - line.first.coordinate;
            if span == 0.0 {
                return Err(Unavailable::InvalidGeometry);
            }
            let base = line.first.price
                + (f64::from(third.bar) - line.first.coordinate)
                    * (line.second.price - line.first.price)
                    / span;
            Ok(Geometry::Channel {
                line,
                offset: third.price - base,
            })
        }
        _ => Err(Unavailable::UnsupportedDrawing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ChartPoint, DrawingTool, Drawings};
    fn drawing(tool: &str, anchors: &[(f32, f64)]) -> crate::Drawing<DrawingTool> {
        let mut drawings = Drawings::default();
        let tool = DrawingTool::by_id(tool).unwrap();
        for &(bar, price) in anchors {
            drawings.place(tool, ChartPoint::at_time(bar, price, Some(1)));
        }
        drawings.items()[0].clone()
    }
    #[test]
    fn alert_projection_preserves_geometry_and_ignores_style() {
        let mut mark = drawing("trend-line", &[(0.0, 100.0), (10.0, 110.0)]);
        let geometry = project(&mark).unwrap();
        mark.hidden = true;
        mark.locked = true;
        mark.style.width_px = 3.0;
        assert_eq!(project(&mark), Ok(geometry));
        mark.foreign_market = true;
        assert_eq!(project(&mark), Err(Unavailable::OutsideTimeDomain));
        mark.foreign_market = false;
        mark.off_series = true;
        assert_eq!(project(&mark), Err(Unavailable::OutsideTimeDomain));
    }
    #[test]
    fn alert_projection_rectangle_and_channel_extensions() {
        let mut rectangle = drawing("rectangle", &[(10.0, 100.0), (0.0, 110.0)]);
        rectangle.payload = Box::new(crate::RectanglePayload {
            extend_left: true,
            extend_right: true,
        });
        assert_eq!(
            project(&rectangle),
            Ok(Geometry::Rectangle {
                first: Point {
                    coordinate: -f64::MAX,
                    price: 110.0
                },
                second: Point {
                    coordinate: f64::MAX,
                    price: 100.0
                },
            })
        );
        let mut channel = drawing(
            "parallel-channel",
            &[(0.0, 100.0), (10.0, 110.0), (5.0, 115.0)],
        );
        let Geometry::Channel { line, offset } = project(&channel).unwrap() else {
            panic!("channel projection");
        };
        assert_eq!(offset, 10.0);
        assert_eq!(line.first.coordinate, 0.0);
        channel.points[1].bar = 0.0;
        assert_eq!(project(&channel), Err(Unavailable::InvalidGeometry));
    }
}
