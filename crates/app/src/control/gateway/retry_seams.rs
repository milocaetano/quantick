//! Test seams the retry matrix's transport tests stand on.
//!
//! `docs/control-plane/retry-matrix.md` claims, per capability, what a client
//! gets when it retries a dropped call and what it reads to reconcile one it
//! cannot retry. Those claims are proven through the real local gateway — a
//! real socket, the real admission and idempotency store, the real
//! `execute_on_ui` — in `crate::app::tests::retry_readback_tests`. Three
//! things those tests need that the frame loop cannot give them live here,
//! compiled into the test build only:
//!
//! - **Serving the queue by hand, and seeing what it served.** "The retry acted
//!   once" is a statement about how many requests reached the application
//!   thread and got past its refusals, which is exactly what
//!   `UiRequest::started` records. [`ControlAccess::serve_queued_for_test`]
//!   runs each queued request through the same `execute_on_ui` the frame's
//!   drain calls, answers it the same way, and reports it.
//! - **An action that ran and has not answered.** The unknown-outcome branch of
//!   `gateway/idempotency.rs` is reached only when the application began a
//!   keyed call and had still not answered a full request window after its
//!   deadline. The frame loop always answers in the same breath it acts, so
//!   no production path can hold that state still long enough for a test to
//!   look at it; [`ControlAccess::serve_one_withholding_answer_for_test`]
//!   performs the action and keeps the answer, which is what an application
//!   thread stuck inside a slow action looks like from the response worker.
//! - **A ceiling no grant hands out.** The `trade.*` shaping calls declare
//!   `IdempotencyPolicy::Optional`, and no production connection can reach
//!   them to find out whether that holds: their `trader` ceiling is one
//!   `configured_profile` never returns. [`ControlAccess::enable_for_test_under_ceiling`]
//!   starts the gateway under a named ceiling so the store's behaviour for that
//!   family can be proven now, before a decision to hand it out exists — and
//!   widens nothing that ships.
//!
//! Nothing here is reachable from the binary: the module is declared under
//! `#[cfg(test)]`, and the one production change it leans on is
//! `request_enable` naming its ceiling through `request_enable_under`.

use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};

use crossbeam_channel::Sender;
use quantick_control::{error::ControlError, id::ProfileId};

use crate::app::QuantickApp;

use super::{AccessState, ControlAccess, GatewayOptions, UiReadExecution, UiRequest};

/// One request the application thread took off the queue through a seam.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServedRequest {
    pub capability_id: String,
    pub request_id: String,
    /// Whether it got past every pre-dispatch refusal — the moment after
    /// which it may have acted, and the flag the idempotency store's
    /// unknown-outcome branch reads.
    pub began: bool,
}

impl ServedRequest {
    fn of(request: &UiRequest) -> Self {
        Self {
            capability_id: request.prepared.envelope.capability_id.as_str().to_owned(),
            request_id: request.prepared.envelope.request_id.as_str().to_owned(),
            began: request.started.load(Ordering::Acquire),
        }
    }
}

/// An answer the application produced and has not handed back yet.
///
/// Holding one keeps the response worker's channel open, which is the whole
/// point: the worker cannot tell a withheld answer from an application thread
/// that is still inside the action.
pub(crate) struct WithheldAnswer {
    pub served: ServedRequest,
    response: Sender<Result<UiReadExecution, ControlError>>,
    result: Result<UiReadExecution, ControlError>,
}

impl WithheldAnswer {
    /// Hand the answer back, late. `false` means nobody was listening any
    /// more — the response worker gave up and settled without it.
    pub(crate) fn deliver(self) -> bool {
        self.response.try_send(self.result).is_ok()
    }
}

impl ControlAccess {
    /// Start the gateway under `ceiling` instead of the one the grant
    /// derives. Everything else — the grant, the handshake's intersection of
    /// ceiling, grant and request, every refusal — is production's.
    pub(crate) fn enable_for_test_under_ceiling(
        &mut self,
        ctx: &eframe::egui::Context,
        descriptor_directory: PathBuf,
        request_timeout: Duration,
        ceiling: &str,
    ) {
        let options = GatewayOptions {
            request_timeout,
            descriptor_directory: Some(descriptor_directory),
            ..GatewayOptions::default()
        };
        let ceiling = ProfileId::new(ceiling).expect("the test names a registered profile");
        self.request_enable_under(ctx, options, ceiling);
    }

    /// Serve everything queued, as the frame's drain would, and say what was
    /// served.
    pub(crate) fn serve_queued_for_test(&mut self, app: &mut QuantickApp) -> Vec<ServedRequest> {
        let mut served = Vec::new();
        while let Some(answer) = self.serve_one_withholding_answer_for_test(app) {
            served.push(answer.served.clone());
            let _ = answer.deliver();
        }
        served
    }

    /// Take the next queued request, run it through the production
    /// `execute_on_ui`, and keep its answer. `None` when nothing is queued.
    pub(crate) fn serve_one_withholding_answer_for_test(
        &mut self,
        app: &mut QuantickApp,
    ) -> Option<WithheldAnswer> {
        let (requests, generation) = match &self.state {
            AccessState::Enabled(runtime) => (runtime.requests.clone(), runtime.grant_generation),
            AccessState::Disabled | AccessState::Enabling | AccessState::Disabling(_) => {
                return None;
            }
        };
        let request = requests.try_recv().ok()?;
        let result = self.execute_on_ui(app, generation, &request);
        Some(WithheldAnswer {
            served: ServedRequest::of(&request),
            response: request.response.clone(),
            result,
        })
    }
}
