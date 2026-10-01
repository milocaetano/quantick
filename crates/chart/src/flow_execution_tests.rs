use super::*;
use quantick_engine::{BarSpec, Side, Trade};
use quantick_orderflow::projection::flow_tape::{FlowTapeFrame, FlowWorkerCache};
use std::{cell::RefCell, sync::Arc};

#[derive(Default)]
struct Runner {
    cache: RefCell<FlowWorkerCache>,
    request: RefCell<Option<FlowRequest>>,
    output: RefCell<Option<Arc<FlowTapeFrame>>>,
}
impl FlowRunner for Runner {
    fn request(&mut self, request: &FlowRequest, _: bool) -> bool {
        self.cache.borrow_mut().select_request(request);
        *self.request.borrow_mut() = Some(request.clone());
        false
    }
    fn submit(&self, chunk: FlowChunk) -> Result<(), Box<FlowChunk>> {
        let mut cache = self.cache.borrow_mut();
        cache.append(&chunk);
        *self.output.borrow_mut() = Some(cache.project(self.request.borrow().as_ref().unwrap()));
        Ok(())
    }
    fn finished(&self) -> Option<Arc<FlowTapeFrame>> {
        self.output.borrow_mut().take()
    }
}
#[test]
fn capture_preserves_same_ms_membership_forming_bar_view_and_empty_reset() {
    let mut state = ChartState::new(BarSpec::Tick(2));
    for quantity in 1..=5 {
        state.ingest_live(&Trade {
            agg_id: 1,
            timestamp_ms: 1000,
            price: 100.into(),
            quantity: quantity.into(),
            side: Side::Buy,
        });
    }
    let mut session = FlowSession::<Runner>::default();
    session.set_ignore_opening(true);
    for _ in 0..2 {
        project_flow_executions(
            true,
            &state,
            &mut session,
            0..3,
            (300.0, 200.0),
            (90.0, 110.0),
            (0.0, 3.0),
        );
    }
    let frame = session.frame().unwrap();
    assert_eq!(frame.requested_ordinals, 0..5);
    assert_eq!(frame.view.width_px, 300.0);
    assert_eq!(frame.view.radius_limit, 12.0);
    assert_eq!(frame.view.reference, FlowReference::VisibleRegions);
    assert!(frame.view.exclude_opening);
    assert_eq!(
        frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity)
            .sum::<Decimal>(),
        15.into()
    );
    let mut members: Vec<_> = frame
        .dots
        .iter()
        .flat_map(|dot| dot.members.iter())
        .map(|member| {
            (
                member.ordinal,
                member.candle_slot,
                member.accepted_ordinal,
                member.source_id,
            )
        })
        .collect();
    members.sort();
    assert_eq!(
        members,
        [
            (0, 0, 0, 1),
            (1, 0, 1, 1),
            (2, 1, 0, 1),
            (3, 1, 1, 1),
            (4, 2, 0, 1)
        ]
    );
    project_flow_executions(
        true,
        &state,
        &mut session,
        0..0,
        (300.0, 200.0),
        (90.0, 110.0),
        (0.0, 3.0),
    );
    assert!(session.frame().is_none());
    assert_eq!(session.progress().requested_executions, 0);
    assert!(session.ignore_opening());
}
#[test]
fn retained_flow_prices_follow_the_current_axis_while_layout_is_pending() {
    let old = PriceWindow::new(90.into(), 110.into()).unwrap();
    let current = PriceWindow::new(90.into(), 130.into()).unwrap();
    assert_eq!(flow_price_y(old, 100.into(), 10.0, 200.0, false), 110.0);
    assert_eq!(flow_price_y(current, 100.into(), 10.0, 200.0, false), 160.0);
    assert_eq!(flow_price_y(current, 100.into(), 10.0, 200.0, true), 60.0);
}
