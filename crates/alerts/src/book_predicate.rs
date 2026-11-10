//! Per-condition observation state and geometry evaluation.
use crate::{input::*, spec::*};
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Baseline {
    side: i8,
    inside: Option<bool>,
    revision: Option<u64>,
}
fn line_value(line: Line, x: f64) -> Result<f64, Unavailable> {
    let a = line.first;
    let b = line.second;
    if ![a.coordinate, a.price, b.coordinate, b.price, x]
        .iter()
        .all(|n| n.is_finite())
        || a.coordinate == b.coordinate
    {
        return Err(Unavailable::InvalidGeometry);
    }
    if (x < a.coordinate.min(b.coordinate) && !line.extend_left)
        || (x > a.coordinate.max(b.coordinate) && !line.extend_right)
    {
        return Err(Unavailable::OutsideTimeDomain);
    }
    let value =
        a.price + (b.price - a.price) * ((x - a.coordinate) / (b.coordinate - a.coordinate));
    if value.is_finite() {
        Ok(value)
    } else {
        Err(Unavailable::InvalidGeometry)
    }
}
fn bounds(
    target: &TargetRef,
    input: &AlertInput<'_>,
) -> Result<(f64, f64, TargetKind, u64), Unavailable> {
    match target {
        TargetRef::Value(v) => Ok((*v, *v, TargetKind::Value, 0)),
        TargetRef::Drawing(target) => {
            let mut matching = input.drawings.iter().filter(|fact| fact.target == target);
            let fact = matching.next().ok_or(Unavailable::MissingDrawing)?;
            if matching.next().is_some() {
                return Err(Unavailable::AmbiguousDrawing);
            }
            let (low, high, kind) = match fact.geometry? {
                Geometry::Line(line) => {
                    let v = line_value(line, input.coordinate)?;
                    (v, v, TargetKind::Line)
                }
                Geometry::Channel { line, offset } => {
                    if !offset.is_finite() {
                        return Err(Unavailable::InvalidGeometry);
                    }
                    let v = line_value(line, input.coordinate)?;
                    let other = v + offset;
                    (v.min(other), v.max(other), TargetKind::Channel)
                }
                Geometry::Rectangle { first, second } => {
                    if ![
                        first.coordinate,
                        second.coordinate,
                        first.price,
                        second.price,
                    ]
                    .iter()
                    .all(|n| n.is_finite())
                    {
                        return Err(Unavailable::InvalidGeometry);
                    }
                    if input.coordinate < first.coordinate.min(second.coordinate)
                        || input.coordinate > first.coordinate.max(second.coordinate)
                    {
                        return Err(Unavailable::OutsideTimeDomain);
                    }
                    (
                        first.price.min(second.price),
                        first.price.max(second.price),
                        TargetKind::Rectangle,
                    )
                }
            };
            if !low.is_finite() || !high.is_finite() {
                return Err(Unavailable::InvalidGeometry);
            }
            Ok((low, high, kind, fact.revision))
        }
    }
}
pub(super) fn judge(
    predicate: &Predicate,
    input: &AlertInput<'_>,
    state: &mut Baseline,
) -> Result<bool, Unavailable> {
    let (low, high, kind, revision) = bounds(&predicate.target, input)?;
    if !crate::operators::OPERATORS
        .iter()
        .any(|op| op.operator == predicate.operator && op.targets.contains(&kind))
    {
        return Err(Unavailable::UnsupportedDrawing);
    }
    if state.revision != Some(revision) {
        *state = Baseline {
            revision: Some(revision),
            ..Baseline::default()
        };
    }
    let side = if input.price > high {
        1
    } else if input.price < low {
        -1
    } else {
        0
    };
    let inside = side == 0;
    let result = match predicate.operator {
        Operator::Crossing => side != 0 && state.side != 0 && side != state.side,
        Operator::CrossingUp => side == 1 && state.side == -1,
        Operator::CrossingDown => side == -1 && state.side == 1,
        Operator::GreaterThan => side == 1,
        Operator::LessThan => side == -1,
        Operator::Inside => inside,
        Operator::Outside => !inside,
        Operator::Entering => state.inside == Some(false) && inside,
        Operator::Exiting => state.inside == Some(true) && !inside,
    };
    if side != 0 {
        state.side = side;
    }
    state.inside = Some(inside);
    Ok(result)
}
