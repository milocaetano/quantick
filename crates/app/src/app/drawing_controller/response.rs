//! The drawing response crosses the gateway between two borrows of its owners.
use super::{DrawingAccess, DrawingController, QUICK_RANGE_REFUSED};
use crate::app::{GatewayPort, QuantickApp, TabsPort, owners_desk::OwnersDesk};
use crate::surfaces::toast;
use quantick_chart_interaction::quick_range::conversion_plan::PlacementOutcome;
use std::time::Instant;

pub(crate) struct ResponseOwners<'a> {
    drawings: &'a mut DrawingController,
    runtime: &'a mut crate::app::workspace_runtime::WorkspaceRuntime,
    surfaces: &'a mut crate::surfaces::Surfaces,
    audio: &'a mut crate::app::replay_and_history::AlertState,
}
pub(crate) trait DrawingResponsePort: GatewayPort + TabsPort + OwnersDesk {
    fn drawing_response_owners(&mut self) -> ResponseOwners<'_>;
    /// The registered capability runs synchronously before the remaining drawing
    /// response. Invalid input still leaves those ordinary commands to execute.
    fn resolve_drawing_response(
        &mut self,
        mut ask: crate::surfaces::drawing_chrome::DrawingChromeAsk,
        now: Instant,
    ) {
        crate::app::alerts_runtime::open_drawing_target(&mut ask, || self.owners().into_wiring());
        let owners = self.drawing_response_owners();
        let mut drawings = owners.drawings.with(&mut owners.surfaces.drawing_chrome);
        if let Some(action) = drawings.begin_registered_action(&mut ask) {
            let pending = action.pending;
            let result = self.control_action(
                action.capability,
                action.version,
                crate::control::ActionOrigin::Human,
                action.input,
            );
            let outcome = PlacementOutcome::of(result.is_ok());
            let owners = self.drawing_response_owners();
            let mut drawings = owners.drawings.with(&mut owners.surfaces.drawing_chrome);
            let explain = drawings.finish_registered_action(pending, outcome);
            if let Err(error) = result {
                tracing::warn!(target:"quantick::control",event_code="QUICK_RANGE_ACTION_REFUSED",action=action.action_id,code=%error.code,error=%error.message,"the quick-range drawing could not be placed");
                if explain {
                    toast::note(
                        &mut self.drawing_response_owners().surfaces.toast,
                        QUICK_RANGE_REFUSED,
                        now,
                    );
                }
            }
        }
        let owners = self.drawing_response_owners();
        let mut drawings = owners.drawings.with(&mut owners.surfaces.drawing_chrome);
        let effects = drawings.apply_drawing_chrome(
            ask,
            &mut DrawingAccess::new(&mut owners.runtime.tabs),
            &mut *owners.audio.alerts,
            now,
        );
        crate::control::journal_own_magnet_flip(self, effects.own_magnet);
        let owners = self.drawing_response_owners();
        if effects.inspector_moved {
            owners.runtime.workspace.session_mut().inspector_moved();
        }
        effects.apply(&mut owners.surfaces.toast, &mut owners.surfaces.toolrail);
    }
}
impl DrawingResponsePort for QuantickApp {
    fn drawing_response_owners(&mut self) -> ResponseOwners<'_> {
        ResponseOwners {
            drawings: &mut self.drawings,
            runtime: &mut self.runtime,
            surfaces: &mut self.surfaces,
            audio: &mut self.audio,
        }
    }
}
