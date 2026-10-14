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
/// #10: a History install recuts the series from older prints. The spheres
/// on screen stay drawn at their own prints on the frame after the install,
/// before the new series has refilled anything, and the refilled frame puts
/// them at the same places.
#[test]
fn a_history_install_keeps_the_spheres_on_screen_at_their_prints() {
    let trade = |id: u64| Trade {
        agg_id: id,
        timestamp_ms: id as i64 * 1_000,
        price: (100 + id % 3).into(),
        quantity: 1.into(),
        side: Side::Buy,
    };
    let mut state = ChartState::new(BarSpec::Tick(2));
    state.ingest_backfill(&(10..20).map(trade).collect::<Vec<_>>());
    let mut session = FlowSession::<Runner>::default();
    let show =
        |state: &ChartState, session: &mut FlowSession<Runner>, slots: std::ops::Range<usize>| {
            let clip = (slots.start as f64, slots.end as f64);
            let width = slots.len() as f32 * 100.0;
            project_flow_executions(
                true,
                state,
                session,
                slots,
                (width, 200.0),
                (90.0, 110.0),
                clip,
            );
        };
    for _ in 0..2 {
        show(&state, &mut session, 0..5);
    }
    // Where each sphere stands, by the venue ids of its prints.
    let placed = |frame: &FlowTapeFrame| {
        let mut placed: Vec<(Vec<u64>, Decimal)> = frame
            .dots
            .iter()
            .map(|dot| {
                let mut ids: Vec<u64> = dot.members.iter().map(|m| m.source_id).collect();
                ids.sort_unstable();
                (ids, dot.candle_position)
            })
            .collect();
        placed.sort();
        placed
    };
    let before = placed(session.frame().unwrap());
    assert!(!before.is_empty());
    // Two older bars arrive: every print of the screen moves two candles on.
    state.prepend_history(&(6..10).map(trade).collect::<Vec<_>>());
    let shifted: Vec<_> = before
        .iter()
        .map(|(ids, position)| (ids.clone(), *position + Decimal::TWO))
        .collect();
    show(&state, &mut session, 2..7);
    let carried = session
        .frame()
        .expect("the spheres stay on screen across the install");
    assert_eq!(placed(carried), shifted);
    show(&state, &mut session, 2..7);
    assert_eq!(placed(session.frame().unwrap()), shifted);
}

#[test]
fn retained_flow_prices_follow_the_current_axis_while_layout_is_pending() {
    let old = PriceWindow::new(90.into(), 110.into()).unwrap();
    let current = PriceWindow::new(90.into(), 130.into()).unwrap();
    assert_eq!(flow_price_y(old, 100.into(), 10.0, 200.0, false), 110.0);
    assert_eq!(flow_price_y(current, 100.into(), 10.0, 200.0, false), 160.0);
    assert_eq!(flow_price_y(current, 100.into(), 10.0, 200.0, true), 60.0);
}
