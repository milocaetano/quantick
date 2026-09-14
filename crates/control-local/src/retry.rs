//! Retry advice from authenticated registry metadata and outstanding requests.

use std::collections::BTreeMap;

use quantick_control::{
    error::{ControlError, codes},
    id::RequestId,
    wire::{RequestEnvelope, ResponseEnvelope, ResponseOutcome},
};

const DESCRIBE: &str = "control.describe";
/// Bound client bookkeeping even when a caller sends without draining replies.
const MAX_PENDING: usize = 1024;

#[derive(Debug, Default)]
pub(super) struct RetryState {
    reads: BTreeMap<(String, u32), bool>,
    pending: BTreeMap<RequestId, Pending>,
}

#[derive(Debug)]
struct Pending {
    read_only: bool,
    describe: bool,
    count: usize,
}

impl RetryState {
    pub(super) fn knows(&self, capability: &str, version: u32) -> bool {
        (capability == DESCRIBE && version == 1)
            || self.reads.contains_key(&(capability.to_owned(), version))
    }

    pub(super) fn begin(&mut self, request: &RequestEnvelope) -> Result<(), ControlError> {
        if self.pending.len() >= MAX_PENDING {
            return Err(ControlError::new(
                quantick_control::id::ErrorCode::new(codes::BACKPRESSURE)
                    .expect("known error code"),
                "read outstanding responses before sending more requests",
                true,
            ));
        }
        let describe =
            request.capability_id.as_str() == DESCRIBE && request.capability_version == 1;
        let read_only = describe
            || self.reads.get(&(
                request.capability_id.to_string(),
                request.capability_version,
            )) == Some(&true);
        self.pending
            .entry(request.request_id.clone())
            .and_modify(|pending| {
                // A duplicate-ID refusal may arrive before the original answer.
                pending.read_only &= read_only;
                pending.describe = false;
                pending.count = pending.count.saturating_add(1);
            })
            .or_insert(Pending {
                read_only,
                describe,
                count: 1,
            });
        Ok(())
    }

    pub(super) fn answered(&mut self, response: &ResponseEnvelope) {
        let Some(pending) = self.pending.get_mut(&response.request_id) else {
            return;
        };
        if pending.describe {
            // A refused or malformed refresh cannot keep old safety claims.
            self.reads.clear();
            if let ResponseOutcome::Success { result } = &response.outcome
                && let Some(rows) = result
                    .get("capabilities")
                    .and_then(serde_json::Value::as_array)
            {
                for row in rows {
                    if let (Some(id), Some(version), Some(read_only)) = (
                        row["id"].as_str(),
                        row["version"].as_u64(),
                        row["read_only"].as_bool(),
                    ) && let Ok(version) = u32::try_from(version)
                    {
                        self.reads
                            .entry((id.to_owned(), version))
                            .and_modify(|known| *known &= read_only)
                            .or_insert(read_only);
                    }
                }
            }
        }
        pending.count -= 1;
        if pending.count == 0 {
            self.pending.remove(&response.request_id);
        }
    }

    pub(super) fn transport_error(&self) -> ControlError {
        if self.pending.values().any(|request| !request.read_only) {
            ControlError::outcome_unknown(codes::INSTANCE_GONE)
        } else {
            super::client::instance_gone_error()
        }
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;
