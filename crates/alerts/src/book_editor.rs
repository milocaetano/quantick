//! Cold-path validation and editor text.
use crate::{book::AlertError, spec::*};
pub(super) fn invalid(field: &str, reason: &str) -> AlertError {
    AlertError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}
pub(super) fn validate(spec: &AlertSpec) -> Result<(), AlertError> {
    if spec.version != FORMAT_VERSION {
        return Err(AlertError::UnsupportedVersion(spec.version));
    }
    for (field, value) in [
        ("name", spec.name.as_str()),
        ("symbol", spec.market.symbol.as_str()),
        ("aggregation", spec.market.aggregation.as_str()),
    ] {
        if value.trim().is_empty() || value.len() > MAX_MESSAGE_BYTES {
            return Err(invalid(field, "must be nonempty and bounded"));
        }
    }
    let source = match &spec.market.source {
        SourceContext::Live { feed } => feed,
        SourceContext::Replay { recording } => recording,
    };
    if source.trim().is_empty() || source.len() > MAX_MESSAGE_BYTES {
        return Err(invalid("source", "must be nonempty and bounded"));
    }
    if spec.conditions.is_empty() || spec.conditions.len() > MAX_CONDITIONS {
        return Err(invalid(
            "conditions",
            "must contain one to eight conditions",
        ));
    }
    if spec.message.len() > MAX_MESSAGE_BYTES {
        return Err(invalid("message", "too long"));
    }
    if let Some(token) = &spec.notification.sound
        && crate::sound::AlertSound::from_token(token).is_none()
    {
        return Err(invalid("sound", "unknown sound token"));
    }
    if spec.notification.sound_seconds == Some(0) {
        return Err(invalid("sound_seconds", "must be positive"));
    }
    if let Expiration::At {
        display_offset_minutes,
        ..
    } = spec.expiration
    {
        offset(display_offset_minutes)?;
    }
    for condition in &spec.conditions {
        match &condition.target {
            TargetRef::Value(value) => {
                if !value.is_finite() {
                    return Err(invalid("target", "must be finite"));
                }
                if !crate::operators::OPERATORS.iter().any(|d| {
                    d.operator == condition.operator && d.targets.contains(&TargetKind::Value)
                }) {
                    return Err(invalid("operator", "incompatible with value"));
                }
            }
            TargetRef::Drawing(target) => {
                if [
                    &target.workspace,
                    &target.origin,
                    &target.pane,
                    &target.incarnation,
                ]
                .iter()
                .any(|s| s.trim().is_empty() || s.len() > MAX_MESSAGE_BYTES)
                    || target.drawing == 0
                {
                    return Err(invalid(
                        "target",
                        "requires complete durable drawing identity",
                    ));
                }
            }
        }
    }
    Ok(())
}
pub(super) fn offset(value: i32) -> Result<(), AlertError> {
    if !(-720..=840).contains(&value) {
        Err(invalid(
            "display_offset_minutes",
            "outside supported civil offsets",
        ))
    } else {
        Ok(())
    }
}
pub(super) fn summary(spec: &AlertSpec) -> String {
    summary_with(&spec.conditions, spec.frequency, |_| None)
}
/// The conditions and cadence in the editor's own words. `name` resolves a
/// drawing target to its display name and kind when the caller can; an
/// unresolved drawing is named by its id alone, never by its identity keys.
pub fn summary_with(
    conditions: &[Predicate],
    frequency: Frequency,
    name: impl Fn(&DurableDrawingRef) -> Option<(String, TargetKind)>,
) -> String {
    let conditions: Vec<_> = conditions
        .iter()
        .map(|p| {
            let (target, kind) = match &p.target {
                TargetRef::Value(v) => (v.to_string(), Some(TargetKind::Value)),
                TargetRef::Drawing(d) => match name(d) {
                    Some((name, kind)) => (name, Some(kind)),
                    None => (format!("drawing {}", d.drawing), None),
                },
            };
            format!(
                "Price {} {target}",
                crate::operators::label(kind, p.operator)
            )
        })
        .collect();
    format!(
        "All conditions: {}; {}",
        conditions.join(" AND "),
        frequency.label()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_workspace::drawing_identity::DurableDrawingRef;

    fn drawing(id: u64) -> DurableDrawingRef {
        DurableDrawingRef {
            workspace: "6".into(),
            origin: "replay9:ALERTTEST14:all-timeframes".into(),
            pane: "4d6426481f043c9827acd9397e8d0945".into(),
            drawing: id,
            incarnation: "1".into(),
        }
    }

    /// Native C11 read `Price Entering drawing 8 (6:replay9:…/4d64…); PerBar`
    /// under Summary: enum debug names and identity keys, not the words the
    /// editor's own combos show.
    #[test]
    fn the_summary_speaks_the_editors_words() {
        let conditions = [
            Predicate {
                operator: Operator::CrossingUp,
                target: TargetRef::Value(99.5),
            },
            Predicate {
                operator: Operator::GreaterThan,
                target: TargetRef::Drawing(drawing(12)),
            },
            Predicate {
                operator: Operator::Entering,
                target: TargetRef::Drawing(drawing(8)),
            },
        ];
        let named = |target: &DurableDrawingRef| match target.drawing {
            12 => Some(("Rectangle 12".to_owned(), TargetKind::Rectangle)),
            _ => None,
        };
        assert_eq!(
            summary_with(&conditions, Frequency::PerBar, named),
            "All conditions: Price Crossing Up 99.5 AND Price Above upper boundary \
             Rectangle 12 AND Price Entering drawing 8; Once per bar"
        );
        for (frequency, label) in [
            (Frequency::Once, "Only once"),
            (Frequency::PerBar, "Once per bar"),
            (Frequency::PerBarClose, "Once per bar close"),
            (Frequency::PerMinute, "Once per minute"),
        ] {
            assert_eq!(frequency.label(), label);
        }
    }
}
