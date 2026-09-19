//! Bounded request dispatch, cancellation and correlated failure responses.

use quantick_control::wire::{RequestEnvelope, ResponseEnvelope, ResponseOutcome};
use quantick_control::{error::ControlError, id::ErrorCode};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const CANCELLED: u8 = 2;

/// Reserve one response slot without exceeding the host's configured bound.
pub fn try_reserve_in_flight(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < limit).then_some(current + 1)
        })
        .is_ok()
}

/// Answer a request with a failure while preserving its correlation identity.
pub fn failure_response(request: &RequestEnvelope, error: ControlError) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: request.protocol_version,
        request_id: request.request_id.clone(),
        instance_id: request.instance_id.clone(),
        capture_revision: None,
        module_revisions: Vec::new(),
        outcome: ResponseOutcome::Failure { error },
        warnings: Vec::new(),
    }
}

/// One shared request state. Cancellation proves no execution only when it
/// wins the same atomic transition the application must win before acting.
#[derive(Debug, Default)]
pub struct DispatchState(AtomicU8);

impl DispatchState {
    pub fn try_start(&self) -> bool {
        self.0
            .compare_exchange(QUEUED, STARTED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// True means this request cannot start, including an earlier cancellation.
    pub fn cancel_before_start(&self) -> bool {
        match self
            .0
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) | Err(CANCELLED) => true,
            Err(_) => false,
        }
    }

    pub fn has_started(&self) -> bool {
        self.0.load(Ordering::Acquire) == STARTED
    }

    /// Classify a missing result after atomically stopping any queued work.
    /// A key is not a promise about a future retry: a bounded terminal record
    /// may expire or be evicted before that retry arrives.
    pub fn interrupted(&self, code: &'static str, read_only: bool) -> ControlError {
        let cancelled = self.cancel_before_start();
        if !cancelled && !read_only {
            return ControlError::outcome_unknown(code);
        }
        let mut error = ControlError::new(
            ErrorCode::new(code).expect("known interruption code"),
            if cancelled {
                "request cancelled before application dispatch"
            } else {
                "request did not complete; execution may still be in progress"
            },
            true,
        );
        error.context.details = Some(serde_json::json!({
            "outcome": if cancelled { "not_started" } else { "unknown" },
        }));
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_control::limits::CONTROL_MAX_BUFFERED_RESPONSE_SLOTS;

    #[test]
    fn global_response_slots_enforce_the_reviewed_buffer_bound() {
        assert_eq!(
            CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
                * quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES,
            quantick_control::limits::CONTROL_MAX_BUFFERED_RESPONSE_BYTES
        );
        let in_flight = AtomicUsize::new(0);
        for _ in 0..CONTROL_MAX_BUFFERED_RESPONSE_SLOTS {
            assert!(try_reserve_in_flight(
                &in_flight,
                CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
            ));
        }
        assert!(!try_reserve_in_flight(
            &in_flight,
            CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
        ));
    }

    #[test]
    fn cancellation_prevents_even_a_later_dispatch() {
        let state = DispatchState::default();
        assert!(state.cancel_before_start());
        assert!(!state.try_start());
        assert!(!state.has_started());
        assert!(state.cancel_before_start());
    }

    #[test]
    fn dispatch_cannot_be_reported_as_cancelled() {
        let state = DispatchState::default();
        assert!(state.try_start());
        assert!(!state.cancel_before_start());
        assert!(state.has_started());
        assert!(!state.try_start());
    }

    #[test]
    fn interruption_advice_distinguishes_started_mutations_and_read_only_calls() {
        for (read_only, retryable) in [(false, false), (true, true)] {
            let state = DispatchState::default();
            assert!(state.try_start());
            let error = state.interrupted(quantick_control::error::codes::TIMEOUT, read_only);
            assert_eq!(error.retryable, retryable);
            assert_eq!(
                error.context.details.as_ref().unwrap()["outcome"],
                "unknown"
            );
            if !read_only {
                assert!(!error.context.next_steps.is_empty());
            }
        }
    }

    #[test]
    fn simultaneous_cancel_and_dispatch_have_exactly_one_winner() {
        for _ in 0..32 {
            let state = std::sync::Arc::new(DispatchState::default());
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let contender = std::sync::Arc::clone(&state);
            let ready = std::sync::Arc::clone(&barrier);
            let dispatch = std::thread::spawn(move || {
                ready.wait();
                contender.try_start()
            });
            barrier.wait();
            let cancelled = state.cancel_before_start();
            assert_ne!(cancelled, dispatch.join().unwrap());
            assert_eq!(state.has_started(), !cancelled);
        }
    }
}
