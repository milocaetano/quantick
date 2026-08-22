//! Structured event and pagination cursors with explicit consistency rules.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    canonical::{Sha256Digest, canonical_sha256},
    error::{ControlError, codes},
    id::{InstanceId, ResourceId, SnapshotScopeId},
    limits::CONTROL_MAX_PAGE_ITEMS,
    wire::WireU64,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventCursor {
    pub instance_id: InstanceId,
    pub next_sequence: WireU64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventStart {
    Oldest,
    Latest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventJournalBounds {
    pub oldest_sequence: WireU64,
    pub next_sequence: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventReadPosition {
    pub next_sequence: WireU64,
    pub dropped_before: Option<EventCursor>,
}

pub fn resolve_event_read(
    instance_id: &InstanceId,
    cursor: Option<&EventCursor>,
    start: Option<EventStart>,
    bounds: EventJournalBounds,
) -> Result<EventReadPosition, ControlError> {
    if bounds.oldest_sequence > bounds.next_sequence {
        return Err(ControlError::invalid_request(
            "event journal bounds are inconsistent",
        ));
    }
    let requested = match (cursor, start) {
        (Some(cursor), None) => {
            if &cursor.instance_id != instance_id || cursor.next_sequence > bounds.next_sequence {
                return Err(ControlError::known(
                    codes::CURSOR_INVALID,
                    "event cursor does not belong to this journal position",
                    false,
                ));
            }
            cursor.next_sequence
        }
        (None, Some(EventStart::Oldest)) => bounds.oldest_sequence,
        (None, Some(EventStart::Latest)) => bounds.next_sequence,
        _ => {
            return Err(ControlError::known(
                codes::CURSOR_INVALID,
                "event read must supply either one cursor or one explicit start",
                false,
            ));
        }
    };
    if requested < bounds.oldest_sequence {
        return Ok(EventReadPosition {
            next_sequence: bounds.oldest_sequence,
            dropped_before: Some(EventCursor {
                instance_id: instance_id.clone(),
                next_sequence: bounds.oldest_sequence,
            }),
        });
    }
    Ok(EventReadPosition {
        next_sequence: requested,
        dropped_before: None,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaginationConsistency {
    RevisionLocked,
    AppendOnly,
    RetainedResource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PageCursor {
    pub instance_id: InstanceId,
    pub scope_id: SnapshotScopeId,
    pub query_digest: Sha256Digest,
    pub consistency_mode: PaginationConsistency,
    pub consistency_revision: WireU64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub high_water_position: Option<WireU64>,
    pub next_position: WireU64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<ResourceId>,
}

#[derive(Clone, Debug)]
pub struct PageContext<'a> {
    pub instance_id: &'a InstanceId,
    pub scope_id: &'a SnapshotScopeId,
    pub query: &'a Value,
    pub consistency_mode: PaginationConsistency,
    pub consistency_revision: WireU64,
    pub high_water_position: Option<WireU64>,
    pub resource_id: Option<&'a ResourceId>,
    pub resource_available: bool,
}

impl PageCursor {
    pub fn first(context: &PageContext<'_>, next_position: WireU64) -> Result<Self, ControlError> {
        validate_context_shape(context)?;
        validate_next_position(context, next_position)?;
        Ok(Self {
            instance_id: context.instance_id.clone(),
            scope_id: context.scope_id.clone(),
            query_digest: canonical_sha256(context.query).map_err(|error| {
                ControlError::invalid_request(format!("query is not canonicalizable: {error}"))
            })?,
            consistency_mode: context.consistency_mode,
            consistency_revision: context.consistency_revision,
            high_water_position: context.high_water_position,
            next_position,
            resource_id: context.resource_id.cloned(),
        })
    }

    pub fn validate_next(&self, context: &PageContext<'_>) -> Result<(), ControlError> {
        validate_context_shape(context)?;
        validate_next_position(context, self.next_position)?;
        let query_digest = canonical_sha256(context.query).map_err(|error| {
            ControlError::invalid_request(format!("query is not canonicalizable: {error}"))
        })?;
        if &self.instance_id != context.instance_id
            || &self.scope_id != context.scope_id
            || self.query_digest != query_digest
            || self.consistency_mode != context.consistency_mode
        {
            return Err(ControlError::known(
                codes::CURSOR_INVALID,
                "page cursor does not match the instance, scope, query, or consistency mode",
                false,
            ));
        }

        match self.consistency_mode {
            PaginationConsistency::RevisionLocked => {
                if self.consistency_revision != context.consistency_revision {
                    return Err(page_stale("the revision-locked source changed"));
                }
            }
            PaginationConsistency::AppendOnly => {
                if self.high_water_position.is_none()
                    || self.high_water_position != context.high_water_position
                {
                    return Err(ControlError::known(
                        codes::CURSOR_INVALID,
                        "append-only pagination requires the original high-water position",
                        false,
                    ));
                }
                if self.consistency_revision != context.consistency_revision {
                    return Err(page_stale(
                        "content at or before the append-only high-water position changed",
                    ));
                }
            }
            PaginationConsistency::RetainedResource => {
                if !context.resource_available {
                    return Err(ControlError::known(
                        codes::RESOURCE_GONE,
                        "the retained page resource expired",
                        false,
                    ));
                }
                if self.resource_id.as_ref() != context.resource_id {
                    return Err(ControlError::known(
                        codes::CURSOR_INVALID,
                        "page cursor names a different retained resource",
                        false,
                    ));
                }
                if self.consistency_revision != context.consistency_revision {
                    return Err(page_stale("retained resource consistency changed"));
                }
            }
        }
        Ok(())
    }
}

fn validate_next_position(
    context: &PageContext<'_>,
    next_position: WireU64,
) -> Result<(), ControlError> {
    if context
        .high_water_position
        .is_some_and(|high_water| next_position > high_water)
    {
        return Err(ControlError::known(
            codes::CURSOR_INVALID,
            "next page position exceeds the fixed high-water position",
            false,
        ));
    }
    Ok(())
}

fn validate_context_shape(context: &PageContext<'_>) -> Result<(), ControlError> {
    match context.consistency_mode {
        PaginationConsistency::RevisionLocked => {
            if context.high_water_position.is_some() || context.resource_id.is_some() {
                return Err(ControlError::invalid_request(
                    "revision-locked pagination cannot carry append or resource state",
                ));
            }
        }
        PaginationConsistency::AppendOnly => {
            if context.high_water_position.is_none() || context.resource_id.is_some() {
                return Err(ControlError::invalid_request(
                    "append-only pagination requires one high-water position",
                ));
            }
        }
        PaginationConsistency::RetainedResource => {
            if context.resource_id.is_none() || context.high_water_position.is_some() {
                return Err(ControlError::invalid_request(
                    "retained-resource pagination requires one resource ID",
                ));
            }
            if !context.resource_available {
                return Err(ControlError::known(
                    codes::RESOURCE_GONE,
                    "the retained page resource expired",
                    false,
                ));
            }
        }
    }
    Ok(())
}

fn page_stale(message: &str) -> ControlError {
    ControlError::page_stale(message)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Page<T> {
    #[schemars(length(max = CONTROL_MAX_PAGE_ITEMS))]
    pub items: Vec<T>,
    #[schemars(range(max = CONTROL_MAX_PAGE_ITEMS))]
    pub item_count: usize,
    pub has_more: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<PageCursor>,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, next_cursor: Option<PageCursor>) -> Result<Self, ControlError> {
        if items.len() > CONTROL_MAX_PAGE_ITEMS {
            return Err(ControlError::invalid_request(
                "page exceeds the reviewed item limit",
            ));
        }
        let item_count = items.len();
        Ok(Self {
            items,
            item_count,
            has_more: next_cursor.is_some(),
            next_cursor,
        })
    }

    pub fn validate(&self) -> Result<(), ControlError> {
        if self.items.len() > CONTROL_MAX_PAGE_ITEMS
            || self.item_count != self.items.len()
            || self.has_more != self.next_cursor.is_some()
        {
            return Err(ControlError::invalid_request(
                "page metadata is inconsistent or exceeds its item limit",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn instance() -> InstanceId {
        InstanceId::from_bytes([1; 16])
    }

    fn scope() -> SnapshotScopeId {
        SnapshotScopeId::new("fake.rows").unwrap()
    }

    #[test]
    fn revision_locked_cursor_rejects_revision_changes() {
        let instance = instance();
        let scope = scope();
        let query = json!({"symbol": "TEST"});
        let first = PageContext {
            instance_id: &instance,
            scope_id: &scope,
            query: &query,
            consistency_mode: PaginationConsistency::RevisionLocked,
            consistency_revision: WireU64::new(3),
            high_water_position: None,
            resource_id: None,
            resource_available: true,
        };
        let cursor = PageCursor::first(&first, WireU64::new(10)).unwrap();
        let changed = PageContext {
            consistency_revision: WireU64::new(4),
            ..first
        };
        assert_eq!(
            cursor.validate_next(&changed).unwrap_err().code.as_str(),
            codes::PAGE_STALE
        );
    }
}
