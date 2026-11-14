//! Accepted launch lifecycle; domain gestures remain on the production owner.
use super::*;

pub enum Staging {
    Wait,
    Refused,
    Ready {
        owner: Owner,
        anchors: [ChartPoint; 2],
        look: NewDrawing,
    },
}
impl State {
    pub(super) fn stage_demo(&mut self, staging: Staging) {
        if matches!(staging, Staging::Wait) {
            return;
        }
        let Some(request) = self.demo_requested.take() else {
            return;
        };
        let Staging::Ready {
            owner,
            anchors,
            look,
        } = staging
        else {
            return;
        };
        self.press(
            owner,
            [0.0, 0.0],
            anchors[0],
            core::GestureEligibility {
                pointer_tool: true,
                unoccluded: true,
                area: core::GestureArea {
                    min: [0.0; 2],
                    max: [20.0; 2],
                },
            },
            request.band,
        );
        if self.drag(owner, [10.0, 0.0], anchors[1], 4.0, false) {
            self.opened(look);
        }
        if request.ready {
            self.release(owner);
        }
    }
}
