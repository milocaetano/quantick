//! Pure drawing projections for the reusable alert observer.
use quantick_alerts::{Geometry, Line, Point, TargetKind, Unavailable};

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

/// The alert target family a drawing tool projects to, or `None` when alerts
/// cannot address it. The single owner of "which drawings take an alert".
#[must_use]
pub fn target_kind(tool: &str) -> Option<TargetKind> {
    match tool {
        "trend-line" | "ray" | "extended-line" | "horizontal-line" | "horizontal-ray" => {
            Some(TargetKind::Line)
        }
        "parallel-channel" => Some(TargetKind::Channel),
        "rectangle" => Some(TargetKind::Rectangle),
        _ => None,
    }
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
    let tool = drawing.tool.id();
    if target_kind(tool).is_none() {
        return Err(Unavailable::UnsupportedDrawing);
    }
    let [first, rest @ ..] = drawing.points.as_slice() else {
        return Err(Unavailable::InvalidGeometry);
    };
    if let "horizontal-line" | "horizontal-ray" = tool {
        // One anchor names the level; a second point one bar on gives the
        // flat line a direction. The ray starts at its anchor and runs right.
        let first = point(first);
        return Ok(Geometry::Line(Line {
            first,
            second: Point {
                coordinate: first.coordinate + 1.0,
                price: first.price,
            },
            extend_left: tool == "horizontal-line",
            extend_right: true,
        }));
    }
    let [second, rest @ ..] = rest else {
        return Err(Unavailable::InvalidGeometry);
    };
    let mut line = Line {
        first: point(first),
        second: point(second),
        extend_left: false,
        extend_right: false,
    };
    match tool {
        "trend-line" => Ok(Geometry::Line(line)),
        // A ray runs past its second anchor, whichever way it was drawn.
        "ray" => {
            if line.second.coordinate < line.first.coordinate {
                line.extend_left = true;
            } else {
                line.extend_right = true;
            }
            Ok(Geometry::Line(line))
        }
        "extended-line" => {
            line.extend_left = true;
            line.extend_right = true;
            Ok(Geometry::Line(line))
        }
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
    use quantick_alerts::Operator;
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
    fn line_of(tool: &str, anchors: &[(f32, f64)]) -> Line {
        match project(&drawing(tool, anchors)) {
            Ok(Geometry::Line(line)) => line,
            other => panic!("{tool} should project to a line, got {other:?}"),
        }
    }
    fn ends(line: Line) -> (f64, f64, f64, f64, bool, bool) {
        (
            line.first.coordinate,
            line.first.price,
            line.second.coordinate,
            line.second.price,
            line.extend_left,
            line.extend_right,
        )
    }
    #[test]
    fn alert_projection_line_family_extends_as_painted() {
        let segment = line_of("trend-line", &[(0.0, 100.0), (10.0, 110.0)]);
        assert_eq!(ends(segment), (0.0, 100.0, 10.0, 110.0, false, false));
        let forward = line_of("ray", &[(0.0, 100.0), (10.0, 110.0)]);
        assert_eq!(ends(forward), (0.0, 100.0, 10.0, 110.0, false, true));
        let backward = line_of("ray", &[(10.0, 100.0), (0.0, 110.0)]);
        assert_eq!(ends(backward), (10.0, 100.0, 0.0, 110.0, true, false));
        let infinite = line_of("extended-line", &[(0.0, 100.0), (10.0, 110.0)]);
        assert_eq!(ends(infinite), (0.0, 100.0, 10.0, 110.0, true, true));
    }
    #[test]
    fn alert_projection_horizontal_tools_need_one_anchor() {
        let level = line_of("horizontal-line", &[(4.0, 105.5)]);
        assert_eq!(ends(level), (4.0, 105.5, 5.0, 105.5, true, true));
        let ray = line_of("horizontal-ray", &[(4.0, 105.5)]);
        assert_eq!(ends(ray), (4.0, 105.5, 5.0, 105.5, false, true));
        let mut empty = drawing("horizontal-line", &[(4.0, 105.5)]);
        empty.points.clear();
        assert_eq!(project(&empty), Err(Unavailable::InvalidGeometry));
    }
    /// Feed one projected drawing to a fresh alert book, one trade per
    /// `(coordinate, price)`, and count what fired.
    fn fired(mark: &crate::Drawing<DrawingTool>, op: Operator, path: &[(f64, f64)]) -> usize {
        use quantick_alerts::{
            AlertBook, AlertCommand, AlertId, AlertInput, AlertSpec, BarIdentity, BoundedEvents,
            DrawingFact, DurableDrawingRef, Expiration, FORMAT_VERSION, Frequency, MarketRef,
            Notification, ObservationKind, Predicate, SourceContext, TargetRef,
        };
        let target = DurableDrawingRef {
            workspace: "desk".into(),
            origin: "one".into(),
            pane: "main".into(),
            drawing: 1,
            incarnation: "mark".into(),
        };
        let spec = AlertSpec {
            version: FORMAT_VERSION,
            id: AlertId(0),
            name: "line".into(),
            market: MarketRef {
                source: SourceContext::Live {
                    feed: "test".into(),
                },
                symbol: "WIN".into(),
                aggregation: "tick:1".into(),
            },
            conditions: vec![Predicate {
                operator: op,
                target: TargetRef::Drawing(target.clone()),
            }],
            frequency: Frequency::PerBar,
            expiration: Expiration::Never,
            message: String::new(),
            notification: Notification {
                popup: false,
                toast: true,
                sound: None,
                sound_seconds: None,
            },
        };
        let mut book = AlertBook::default();
        book.command(AlertCommand::Create(spec.clone()), 0).unwrap();
        let facts = [DrawingFact {
            target: &target,
            revision: 1,
            geometry: project(mark),
        }];
        let mut out = BoundedEvents::new(16);
        for (sequence, &(coordinate, price)) in (0u64..).zip(path) {
            let input = AlertInput {
                market: &spec.market,
                utc_ms: 0,
                session_ms: sequence,
                bar: BarIdentity { epoch: 1, sequence },
                kind: ObservationKind::Trade,
                coordinate,
                price,
                availability: Ok(()),
                drawings: &facts,
            };
            book.evaluate(&input, &mut out);
        }
        out.len()
    }
    #[test]
    fn alert_line_family_fires_on_the_painted_line() {
        // A horizontal line holds its level on both sides of its anchor.
        let level = drawing("horizontal-line", &[(4.0, 100.0)]);
        let path = [(0.0, 99.0), (1.0, 101.0), (8.0, 99.0)];
        assert_eq!(fired(&level, Operator::Crossing, &path), 2);
        // A horizontal ray exists only from its anchor rightwards.
        let ray = drawing("horizontal-ray", &[(4.0, 100.0)]);
        let before = [(0.0, 99.0), (1.0, 101.0), (2.0, 99.0)];
        assert_eq!(fired(&ray, Operator::Crossing, &before), 0);
        let after = [(5.0, 99.0), (6.0, 101.0), (9.0, 99.0)];
        assert_eq!(fired(&ray, Operator::Crossing, &after), 2);
        // A sloped ray keeps rising past its second anchor: at bar 10 it
        // stands at 110, so 109 is below it and 111 above.
        let sloped = drawing("ray", &[(0.0, 100.0), (2.0, 102.0)]);
        let past = [(10.0, 109.0), (11.0, 112.0)];
        assert_eq!(fired(&sloped, Operator::CrossingUp, &past), 1);
        let segment = drawing("trend-line", &[(0.0, 100.0), (2.0, 102.0)]);
        assert_eq!(fired(&segment, Operator::CrossingUp, &past), 0);
    }
    #[test]
    fn alert_target_kind_names_every_addressable_tool() {
        for tool in [
            "trend-line",
            "ray",
            "extended-line",
            "horizontal-line",
            "horizontal-ray",
        ] {
            assert_eq!(target_kind(tool), Some(TargetKind::Line), "{tool}");
        }
        assert_eq!(target_kind("parallel-channel"), Some(TargetKind::Channel));
        assert_eq!(target_kind("rectangle"), Some(TargetKind::Rectangle));
        for tool in ["vertical-line", "fib-retracement", "triangle", "text"] {
            assert_eq!(target_kind(tool), None, "{tool}");
            assert_eq!(
                project(&drawing(tool, &[(0.0, 100.0), (10.0, 110.0), (5.0, 105.0)])),
                Err(Unavailable::UnsupportedDrawing),
                "{tool}"
            );
        }
    }
}
