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
    let conditions: Vec<_> = spec
        .conditions
        .iter()
        .map(|p| {
            format!(
                "Price {:?} {}",
                p.operator,
                match &p.target {
                    TargetRef::Value(v) => v.to_string(),
                    TargetRef::Drawing(d) =>
                        format!("drawing {} ({}/{})", d.drawing, d.pane, d.incarnation),
                }
            )
        })
        .collect();
    format!(
        "All conditions: {}; {:?}",
        conditions.join(" AND "),
        spec.frequency
    )
}
