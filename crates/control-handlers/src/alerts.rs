//! Admitted alert operations over one persistent owner; no delivery on mutation.
use quantick_alerts::{AlertCommand, AlertError, AlertEvent, AlertSpec, AlertView, CommandResult};
use quantick_control::{error::ControlError, wire::ActorContext};
use quantick_control_schema::alerts as schema;
use serde_json::{Value, json};

pub trait AlertDefinitionsPort {
    fn alert_resolve_target(
        &mut self,
        _selector: schema::DrawingSelector,
    ) -> Result<quantick_alerts::DurableDrawingRef, schema::TargetResolveError> {
        Err(schema::TargetResolveError::Unavailable)
    }
    fn alert_book(&self) -> &quantick_alerts::AlertBook;
    fn alert_book_at(&mut self) -> (&mut quantick_alerts::AlertBook, i64);
    /// Native hosts override this boundary to persist before returning success.
    fn alert_command(&mut self, command: AlertCommand) -> Result<CommandResult, AlertError> {
        let (book, utc_ms) = self.alert_book_at();
        book.command(command, utc_ms)
    }
    fn alert_validate(&self, spec: &AlertSpec) -> Result<(), AlertError> {
        self.alert_book().validate(spec)
    }
    fn alert_views(&self) -> Result<Vec<AlertView>, AlertError> {
        self.alert_book().views()
    }
    fn alert_history(&self, after: u64, limit: usize) -> Result<Vec<AlertEvent>, AlertError> {
        self.alert_book().history(after, limit)
    }
}

pub fn register<H: AlertDefinitionsPort + ?Sized, A: crate::JournalAccess>(
    registry: &mut impl crate::dock::ActionDock<H, A>,
) -> Result<(), quantick_control::registry::RegistryError> {
    macro_rules! action {
        ($id:ident, $handler:ident) => {
            registry.register_outside_replay(schema::descriptor(schema::$id), $handler::<H, A>)?;
        };
    }
    action!(LIST, list);
    action!(READ, read);
    action!(VALIDATE, validate);
    action!(CREATE, create);
    action!(EDIT, edit);
    action!(PAUSE, pause);
    action!(RESUME, resume);
    action!(REARM, rearm);
    action!(DELETE, delete);
    action!(HISTORY, history);
    action!(CLEAR_HISTORY, clear_history);
    action!(TARGET_RESOLVE, resolve);
    Ok(())
}
fn parse<T: serde::de::DeserializeOwned>(input: &Value) -> Result<T, ControlError> {
    serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))
}
fn domain(error: AlertError) -> ControlError {
    use quantick_control::error::codes;
    let code = match error {
        AlertError::UnknownId => codes::RESOURCE_GONE,
        AlertError::StaleRevision { .. } => codes::REVISION_CONFLICT,
        AlertError::Unavailable => codes::CAPABILITY_UNAVAILABLE,
        AlertError::UnsupportedVersion(_) => codes::VERSION_UNSUPPORTED,
        _ => codes::INVALID_REQUEST,
    };
    let mut result = ControlError::new(
        quantick_control::id::ErrorCode::new(code).expect("static code"),
        format!("{error:?}"),
        false,
    );
    result.context.details = Some(match error {
        AlertError::Invalid { field, reason } => json!({"field":field,"reason":reason}),
        AlertError::StaleRevision { actual } => json!({"actual_revision":actual}),
        AlertError::Limit { resource, maximum } => json!({"resource":resource,"maximum":maximum}),
        _ => json!({"reason":format!("{error:?}")}),
    });
    result
}
fn result(value: CommandResult) -> Value {
    json!({"id":value.id.map(|id|id.0),"revision":value.revision})
}
fn view(value: AlertView) -> Value {
    let (status, reason) = match value.status {
        quantick_alerts::AlertStatus::Monitoring => ("monitoring", None),
        quantick_alerts::AlertStatus::Paused => ("paused", None),
        quantick_alerts::AlertStatus::Completed => ("completed", None),
        quantick_alerts::AlertStatus::Expired => ("expired", None),
        quantick_alerts::AlertStatus::Suspended(reason) => {
            ("suspended", Some(format!("{reason:?}")))
        }
    };
    json!({"spec":wire_spec(&value.spec),"revision":value.revision,"status":status,"reason":reason,"summary":value.summary})
}
fn page(input: &Value) -> Result<schema::PageInput, ControlError> {
    let page: schema::PageInput = parse(input)?;
    if page.limit == 0 || page.limit > 32 {
        return Err(ControlError::invalid_request(
            "limit must be between 1 and 32",
        ));
    }
    Ok(page)
}
fn list<H: AlertDefinitionsPort + ?Sized, A>(
    host: &mut H,
    _: &mut A,
    _: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let page = page(input)?;
    let views = bounded_page(
        host.alert_views()
            .map_err(domain)?
            .into_iter()
            .filter(|view| view.spec.id.0 > page.after)
            .take(page.limit)
            .map(view),
    )?;
    let next_after = views.last().and_then(|view| view["spec"]["id"].as_u64());
    Ok(json!({"alerts":views,"next_after":next_after}))
}
fn read<H: AlertDefinitionsPort + ?Sized, A>(
    host: &mut H,
    _: &mut A,
    _: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: schema::ReadInput = parse(input)?;
    host.alert_views()
        .map_err(domain)?
        .into_iter()
        .find(|view| view.spec.id.0 == input.id)
        .map(view)
        .ok_or_else(|| domain(AlertError::UnknownId))
}
fn validate<H: AlertDefinitionsPort + ?Sized, A>(
    host: &mut H,
    _: &mut A,
    _: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: schema::SpecInput = parse(input)?;
    host.alert_validate(&read_spec(input.spec)?)
        .map_err(domain)?;
    Ok(json!({"valid":true}))
}
fn create<H: AlertDefinitionsPort + ?Sized, A: crate::JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: schema::SpecInput = parse(input)?;
    host.alert_command(AlertCommand::Create(read_spec(input.spec)?))
        .map(|value| record(access, actor, schema::CREATE, result(value)))
        .map_err(domain)
}
fn edit<H: AlertDefinitionsPort + ?Sized, A: crate::JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: schema::EditInput = parse(input)?;
    host.alert_command(AlertCommand::Edit {
        spec: read_spec(input.spec)?,
        expected_revision: input.expected_revision,
    })
    .map(|value| record(access, actor, schema::EDIT, result(value)))
    .map_err(domain)
}
macro_rules! revision_action {
    ($function:ident,$variant:ident,$operation:ident) => {
        fn $function<H: AlertDefinitionsPort + ?Sized, A: crate::JournalAccess>(
            host: &mut H,
            access: &mut A,
            actor: &ActorContext,
            input: &Value,
        ) -> Result<Value, ControlError> {
            let input: schema::RevisionInput = parse(input)?;
            host.alert_command(AlertCommand::$variant {
                id: quantick_alerts::AlertId(input.id),
                expected_revision: input.expected_revision,
            })
            .map(|value| record(access, actor, schema::$operation, result(value)))
            .map_err(domain)
        }
    };
}
revision_action!(pause, Pause, PAUSE);
revision_action!(resume, Resume, RESUME);
revision_action!(rearm, Rearm, REARM);
revision_action!(delete, Delete, DELETE);
fn history<H: AlertDefinitionsPort + ?Sized, A>(
    host: &mut H,
    _: &mut A,
    _: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let page = page(input)?;
    let events = bounded_page(
        host.alert_history(page.after, page.limit)
            .map_err(domain)?
            .into_iter()
            .map(|event| {
                let mut value = json!(event);
                value["spec"] = wire_spec(&event.spec);
                value["price"] = json!(wire_price(event.price));
                value
            }),
    )?;
    let next_after = events.last().and_then(|event| event["sequence"].as_u64());
    Ok(json!({"events":events,"next_after":next_after}))
}
fn clear_history<H: AlertDefinitionsPort + ?Sized, A: crate::JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let _: quantick_control_host::authority::EmptyInput = parse(input)?;
    host.alert_command(AlertCommand::ClearHistory)
        .map(|value| record(access, actor, schema::CLEAR_HISTORY, result(value)))
        .map_err(domain)
}
fn resolve<H: AlertDefinitionsPort + ?Sized, A: crate::JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let target = host.alert_resolve_target(parse(input)?).map_err(|error| {
        let mut result = ControlError::invalid_request("drawing target could not be committed");
        result.context.details = Some(json!({"reason":error}));
        result
    })?;
    Ok(record(
        access,
        actor,
        schema::TARGET_RESOLVE,
        json!({"target":target}),
    ))
}

/// Decimal strings are the control wire contract; only the domain sees f64.
fn read_spec(mut value: Value) -> Result<AlertSpec, ControlError> {
    if let Some(conditions) = value["conditions"].as_array_mut() {
        for condition in conditions {
            if let Some(price) = condition["target"].get_mut("Value") {
                let decimal: quantick_control::wire::CanonicalDecimal = parse(price)?;
                let number: f64 = decimal.as_str().parse().map_err(|_| {
                    ControlError::invalid_request(
                        "target price is outside the supported finite range",
                    )
                })?;
                if !number.is_finite() {
                    return Err(ControlError::invalid_request("target price must be finite"));
                }
                if wire_price(number) != decimal.as_str() {
                    return Err(ControlError::invalid_request(
                        "target price must round-trip exactly through the supported price precision",
                    ));
                }
                *price = json!(number);
            }
        }
    }
    parse(&value)
}
fn wire_spec(spec: &AlertSpec) -> Value {
    let mut value = json!(spec);
    for (condition, wire) in spec.conditions.iter().zip(
        value["conditions"]
            .as_array_mut()
            .expect("serialized conditions"),
    ) {
        if let quantick_alerts::TargetRef::Value(price) = condition.target {
            wire["target"]["Value"] = json!(wire_price(price));
        }
    }
    value
}

#[cfg(test)]
#[path = "alerts_tests.rs"]
mod tests;

fn wire_price(price: f64) -> String {
    if price == 0.0 {
        "0".to_owned()
    } else {
        price.to_string()
    }
}

/// Reserve envelope space and stop before the next item exceeds the wire cap.
/// The last returned identity is the continuation cursor, so no item is lost.
fn bounded_page(values: impl IntoIterator<Item = Value>) -> Result<Vec<Value>, ControlError> {
    const BUDGET: usize = quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES - 1024;
    let mut bytes = 0;
    let mut page = Vec::new();
    for value in values {
        let size = serde_json::to_vec(&value)
            .map_err(|error| ControlError::invalid_request(error.to_string()))?
            .len()
            + 1;
        if bytes + size > BUDGET {
            if page.is_empty() {
                return Err(quantick_control_host::admission::known_error(
                    quantick_control::error::codes::PAYLOAD_TOO_LARGE,
                    "one alert exceeds the response byte limit",
                    false,
                ));
            }
            break;
        }
        bytes += size;
        page.push(value);
    }
    Ok(page)
}

fn record(
    access: &mut impl crate::JournalAccess,
    actor: &ActorContext,
    operation: &str,
    receipt: Value,
) -> Value {
    access.record_event(quantick_control_host::journal::NewEvent::by(
        schema::MODULE_ID,
        schema::CHANGED_EVENT,
        actor,
        json!({"operation":operation,"request_id":actor.request_id,"receipt":receipt}),
    ));
    receipt
}
