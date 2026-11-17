//! An order's life on the broker venue, through a scripted bridge: entry,
//! acknowledgement, fill, stop moves, partial close, reversal, flatten and
//! the broker's own refusals.

mod support;

use quantick_engine::Side;
use quantick_trading::fake::print_at;
use quantick_trading::{
    Bracket, BracketTarget, CancelReason, CloseAmount, ExitReason, FillRole, OrderIntent,
    RejectReason, TradingVenue, VenueEvent, VenueNotice, VenueText,
};
use support::{
    MAGIC, Snapshot, connected, deal, dec, notices, only, order, position, reply, with_stops,
};

#[test]
fn a_market_entry_is_in_flight_until_its_deal_arrives() {
    let (mut venue, bridge) = connected();
    venue.seed(&print_at(1, dec(125_000), 1_000));

    let events = venue.submit(OrderIntent::market(Side::Buy, dec(2)));
    let [VenueEvent::Placed(placed)] = events.as_slice() else {
        panic!("accepted and sent: {events:?}");
    };
    let id = placed.id;
    assert_eq!(venue.in_flight(), 1, "sent, not yet filled");
    let sent = bridge.take_sent();
    let submit = only(&sent, "submit");
    assert_eq!(submit["kind"], "market");
    assert_eq!(submit["side"], "buy");
    assert_eq!(submit["volume"], "2");
    assert_eq!(submit["magic"], MAGIC);
    assert_eq!(submit["deviation"], 10);
    assert!(
        submit.get("price").is_none(),
        "the bridge prices a market order at the touch"
    );

    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10009,
        "Request executed",
        501,
    ));
    let acked = venue.poll();
    assert_eq!(
        notices(&acked),
        vec![VenueNotice::Acknowledged { id, venue_ref: 501 }]
    );
    assert_eq!(venue.ticket(id), Some(501));
    assert_eq!(venue.in_flight(), 1, "acknowledged is not filled");

    bridge.push(
        &Snapshot::new()
            .deal(deal(901, 501, 501, "buy", "in", "expert", 2, 125_005))
            .position(position(501, "buy", 2, 125_005, MAGIC))
            .line(),
    );
    let filled = venue.on_trade(&print_at(2, dec(125_010), 2_000));
    assert!(
        filled.iter().any(|event| matches!(
            event,
            VenueEvent::Filled(fill) if fill.role == FillRole::Entry(id) && fill.price == dec(125_005)
        )),
        "the deal is the fill: {filled:?}"
    );
    assert_eq!(venue.in_flight(), 0);
    let held = venue.position().expect("open");
    assert_eq!(
        (held.side, held.quantity, held.avg_price),
        (Side::Buy, dec(2), dec(125_005))
    );
}

#[test]
fn a_resting_order_is_provisional_until_the_broker_acknowledges_it() {
    let (mut venue, bridge) = connected();
    venue.seed(&print_at(1, dec(125_000), 1_000));
    let events = venue.submit(
        OrderIntent::limit(Side::Buy, dec(1), dec(124_900))
            .with_bracket(Bracket::whole(Some(dec(124_700)), Some(dec(125_300)))),
    );
    let VenueEvent::Placed(placed) = &events[0] else {
        panic!("placed: {events:?}");
    };
    let id = placed.id;
    assert_eq!(venue.working_orders().len(), 1, "shown at once");
    assert!(
        !venue.is_acknowledged(id),
        "but drawn as not yet at the broker"
    );

    let submit = only(&bridge.take_sent(), "submit");
    assert_eq!(submit["kind"], "limit");
    assert_eq!(submit["price"], "124900");
    assert_eq!(
        (submit["sl"].as_str(), submit["tp"].as_str()),
        (Some("124700"), Some("125300"))
    );

    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10008,
        "Request placed",
        601,
    ));
    venue.poll();
    assert!(venue.is_acknowledged(id));
    assert_eq!(venue.ticket(id), Some(601));

    // The fill: a deal for the order, the position it opened, the order gone.
    bridge.push(
        &Snapshot::new()
            .deal(deal(902, 601, 601, "buy", "in", "expert", 1, 124_900))
            .position(with_stops(
                position(601, "buy", 1, 124_900, MAGIC),
                Some(124_700),
                Some(125_300),
            ))
            .closed(601, "filled")
            .line(),
    );
    let events = venue.poll();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, VenueEvent::Filled(f) if f.role == FillRole::Entry(id)))
    );
    assert!(
        venue.working_orders().is_empty(),
        "filled orders stop working"
    );
    let held = venue.position().expect("open");
    assert_eq!(
        (held.stop_loss, held.take_profit),
        (Some(dec(124_700)), Some(dec(125_300)))
    );
}

/// Opens a long of `volume` at 125000 with broker ticket 77.
fn long(
    volume: i64,
) -> (
    quantick_broker_mt5::Mt5Venue,
    quantick_broker_mt5::fake::FakeBridgeHandle,
) {
    let (mut venue, bridge) = connected();
    venue.seed(&print_at(1, dec(125_000), 1_000));
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", volume, 125_000, MAGIC))
            .line(),
    );
    venue.poll();
    bridge.take_sent();
    (venue, bridge)
}

#[test]
fn moving_the_stop_is_a_native_sltp_on_the_position() {
    let (mut venue, bridge) = long(1);
    let events = venue.amend_bracket(
        BracketTarget::Position,
        Bracket::whole(Some(dec(124_800)), Some(dec(125_400))),
    );
    assert_eq!(
        events,
        vec![VenueEvent::BracketSet {
            stop_loss: Some(dec(124_800)),
            take_profit: Some(dec(125_400))
        }]
    );
    let sltp = only(&bridge.take_sent(), "sltp");
    assert_eq!(sltp["position"], 77);
    assert_eq!(
        (sltp["sl"].as_str(), sltp["tp"].as_str()),
        (Some("124800"), Some("125400"))
    );
    assert!(!venue.core().position_stops_acknowledged());

    // A snapshot taken before the broker saw the change does not undo it.
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 1, 125_000, MAGIC))
            .line(),
    );
    venue.poll();
    assert_eq!(
        venue.position().and_then(|p| p.stop_loss),
        Some(dec(124_800))
    );

    bridge.push(&reply(
        sltp["id"].as_str().unwrap(),
        10009,
        "Request executed",
        0,
    ));
    venue.poll();
    assert!(venue.core().position_stops_acknowledged());
}

#[test]
fn a_refused_stop_move_goes_back_and_says_why_in_the_brokers_words() {
    let (mut venue, bridge) = long(1);
    venue.amend_bracket(
        BracketTarget::Position,
        Bracket::whole(Some(dec(125_100)), None),
    );
    let sltp = only(&bridge.take_sent(), "sltp");
    bridge.push(&reply(
        sltp["id"].as_str().unwrap(),
        10016,
        "Invalid stops",
        0,
    ));
    let events = venue.poll();
    assert_eq!(
        events,
        vec![
            VenueEvent::Rejected(RejectReason::Venue {
                code: Some(10016),
                message: VenueText::new("Invalid stops"),
            }),
            VenueEvent::BracketSet {
                stop_loss: None,
                take_profit: None
            },
        ]
    );
    let VenueEvent::Rejected(reason) = &events[0] else {
        unreachable!()
    };
    assert!(reason.to_string().contains("Invalid stops"), "{reason}");
    assert_eq!(venue.position().and_then(|p| p.stop_loss), None);
}

#[test]
fn moving_a_resting_order_is_a_modify_and_a_refusal_puts_it_back() {
    let (mut venue, bridge) = connected();
    venue.seed(&print_at(1, dec(125_000), 1_000));
    let id = match venue
        .submit(OrderIntent::limit(Side::Sell, dec(1), dec(125_300)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    let submit = only(&bridge.take_sent(), "submit");
    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10008,
        "Request placed",
        602,
    ));
    venue.poll();

    let events = venue.amend_price(id, dec(125_250));
    assert!(matches!(&events[..], [VenueEvent::Updated(o)] if o.price == Some(dec(125_250))));
    assert!(!venue.is_acknowledged(id), "the move is provisional");
    let modify = only(&bridge.take_sent(), "modify");
    assert_eq!(
        (modify["order"].as_u64(), modify["price"].as_str()),
        (Some(602), Some("125250"))
    );
    assert!(
        venue.amend_price(id, dec(125_200))[0]
            == VenueEvent::Rejected(RejectReason::Blocked(
                quantick_broker_mt5::machine::CHANGE_PENDING
            )),
        "one change at a time"
    );

    bridge.push(&reply(
        modify["id"].as_str().unwrap(),
        10015,
        "Invalid price",
        0,
    ));
    let events = venue.poll();
    assert!(matches!(&events[1], VenueEvent::Updated(o) if o.price == Some(dec(125_300))));
    assert_eq!(venue.working_orders()[0].price, Some(dec(125_300)));
    assert!(venue.is_acknowledged(id));
}

#[test]
fn a_partial_close_sends_the_part_and_journals_the_round_trip_from_the_deal() {
    let (mut venue, bridge) = long(3);
    assert!(venue.close(CloseAmount::Partial(dec(1))).is_empty());
    let close = only(&bridge.take_sent(), "close");
    assert_eq!(
        (close["position"].as_u64(), close["volume"].as_str()),
        (Some(77), Some("1"))
    );
    assert_eq!(venue.in_flight(), 1, "the close waits for its deal");

    bridge.push(&reply(
        close["id"].as_str().unwrap(),
        10009,
        "Request executed",
        801,
    ));
    venue.poll();
    assert_eq!(venue.in_flight(), 1);
    bridge.push(
        &Snapshot::new()
            .deal(deal(903, 801, 77, "sell", "out", "expert", 1, 125_050))
            .position(position(77, "buy", 2, 125_000, MAGIC))
            .line(),
    );
    let events = venue.on_trade(&print_at(2, dec(125_050), 2_000));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, VenueEvent::Filled(f) if f.role == FillRole::Close))
    );
    assert_eq!(venue.in_flight(), 0);
    assert_eq!(venue.closed_trades().len(), 1);
    let trade = &venue.closed_trades()[0];
    assert_eq!(
        (trade.quantity, trade.pnl_points, trade.exit_reason),
        (dec(1), dec(50), ExitReason::Manual)
    );
    assert_eq!(
        trade.entry_agg_id, None,
        "a broker deal is not a print on this tape"
    );
    assert_eq!(venue.position().map(|p| p.quantity), Some(dec(2)));

    // Asking for more than is open closes what is open.
    venue.close(CloseAmount::Partial(dec(9)));
    assert_eq!(only(&bridge.take_sent(), "close")["volume"], "2");
}

#[test]
fn a_reverse_is_one_opposite_entry_the_broker_nets() {
    let (mut venue, bridge) = long(1);
    let id = match venue
        .submit(OrderIntent::market(Side::Sell, dec(2)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    let submit = only(&bridge.take_sent(), "submit");
    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10009,
        "Request executed",
        503,
    ));
    bridge.push(
        &Snapshot::new()
            .deal(deal(904, 503, 77, "sell", "inout", "expert", 2, 124_980))
            .position(position(77, "sell", 1, 124_980, MAGIC))
            .line(),
    );
    let events = venue.poll();
    let closed: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            VenueEvent::Closed(trade) => Some(trade.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(closed.len(), 1);
    assert_eq!(
        (closed[0].quantity, closed[0].exit_reason),
        (dec(1), ExitReason::Reversal)
    );
    assert_eq!(closed[0].pnl_points, dec(-20));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, VenueEvent::Filled(f) if f.role == FillRole::Entry(id)))
    );
    let held = venue.position().expect("reversed");
    assert_eq!((held.side, held.quantity), (Side::Sell, dec(1)));
}

#[test]
fn flatten_cancels_every_own_order_and_closes_the_position() {
    let (mut venue, bridge) = long(1);
    let acked = match venue
        .submit(OrderIntent::limit(Side::Buy, dec(1), dec(124_000)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    let first = only(&bridge.take_sent(), "submit");
    bridge.push(&reply(
        first["id"].as_str().unwrap(),
        10008,
        "Request placed",
        610,
    ));
    venue.poll();
    let unacked = match venue
        .submit(OrderIntent::limit(Side::Buy, dec(1), dec(123_000)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    let second = only(&bridge.take_sent(), "submit");

    assert!(
        venue.flatten().is_empty(),
        "everything flatten does is on its way"
    );
    let sent = bridge.take_sent();
    assert_eq!(
        only(&sent, "cancel")["order"],
        610,
        "the acknowledged one by its ticket"
    );
    assert_eq!(only(&sent, "close")["position"], 77);

    // The other is cancelled the moment it has a ticket to cancel by.
    bridge.push(&reply(
        second["id"].as_str().unwrap(),
        10008,
        "Request placed",
        611,
    ));
    venue.poll();
    assert_eq!(only(&bridge.take_sent(), "cancel")["order"], 611);

    let cancel = only(&sent, "cancel");
    bridge.push(&reply(
        cancel["id"].as_str().unwrap(),
        10009,
        "Request executed",
        610,
    ));
    let events = venue.poll();
    assert!(events.iter().any(|e| matches!(
        e,
        VenueEvent::Cancelled { order, reason: CancelReason::Flatten } if order.id == acked
    )));
    assert!(venue.working_orders().iter().all(|o| o.id == unacked));
}

#[test]
fn a_broker_refusal_removes_the_order_and_carries_its_words() {
    let (mut venue, bridge) = connected();
    venue.submit(OrderIntent::market(Side::Buy, dec(500)));
    let submit = only(&bridge.take_sent(), "submit");
    bridge.push(&reply(submit["id"].as_str().unwrap(), 10019, "No money", 0));
    let events = venue.poll();
    assert_eq!(
        events,
        vec![VenueEvent::Rejected(RejectReason::Venue {
            code: Some(10019),
            message: VenueText::new("No money")
        })]
    );
    assert!(venue.is_idle(), "nothing in flight, nothing working");
}

#[test]
fn an_order_the_broker_removes_on_its_own_is_cancelled_by_the_venue() {
    let (mut venue, bridge) = connected();
    bridge.push(
        &Snapshot::new()
            .order(order(620, "buy", "limit", 1, 124_000, MAGIC))
            .line(),
    );
    venue.poll();
    let adopted = venue.working_orders()[0].id;
    assert!(
        venue.is_acknowledged(adopted),
        "an own order from before is shown, with its ticket"
    );
    assert_eq!(venue.ticket(adopted), Some(620));

    bridge.push(&Snapshot::new().closed(620, "expired").line());
    let events = venue.poll();
    assert!(matches!(
        events.as_slice(),
        [VenueEvent::Cancelled {
            reason: CancelReason::Venue,
            ..
        }]
    ));
    assert!(venue.working_orders().is_empty());
}

#[test]
fn a_stop_hit_at_the_broker_is_a_stop_loss_exit() {
    let (mut venue, bridge) = long(1);
    bridge.push(
        &Snapshot::new()
            .deal(deal(905, 0, 77, "sell", "out", "sl", 1, 124_800))
            .line(),
    );
    let events = venue.poll();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, VenueEvent::Filled(f) if f.role == FillRole::StopLoss))
    );
    assert_eq!(venue.closed_trades()[0].exit_reason, ExitReason::StopLoss);
    assert!(venue.position().is_none());
}

#[test]
fn deals_from_before_the_session_are_history_not_news() {
    let (mut venue, bridge) = support::venue();
    bridge.connect("bridge-1");
    bridge.push(
        &Snapshot::new()
            .deal(deal(1, 2, 3, "buy", "in", "expert", 1, 100))
            .line(),
    );
    let events = venue.poll();
    assert!(
        !events.iter().any(|e| matches!(e, VenueEvent::Filled(_))),
        "{events:?}"
    );
}

#[test]
fn a_snapshot_taken_before_a_cancel_does_not_bring_the_order_back() {
    let (mut venue, bridge) = connected();
    let id = match venue
        .submit(OrderIntent::limit(Side::Buy, dec(1), dec(124_000)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    let submit = only(&bridge.take_sent(), "submit");
    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10008,
        "Request placed",
        630,
    ));
    venue.poll();
    venue.cancel(id);
    let cancel = only(&bridge.take_sent(), "cancel");
    bridge.push(&reply(
        cancel["id"].as_str().unwrap(),
        10009,
        "Request executed",
        630,
    ));
    let events = venue.poll();
    assert!(matches!(
        events.as_slice(),
        [VenueEvent::Cancelled {
            reason: CancelReason::User,
            ..
        }]
    ));
    bridge.push(
        &Snapshot::new()
            .order(order(630, "buy", "limit", 1, 124_000, MAGIC))
            .line(),
    );
    venue.poll();
    assert!(
        venue.working_orders().is_empty(),
        "a late snapshot is not a resurrection"
    );
}

#[test]
fn a_close_the_broker_drops_unfilled_is_reported_and_stops_being_in_flight() {
    let (mut venue, bridge) = long(1);
    venue.close(CloseAmount::All);
    let close = only(&bridge.take_sent(), "close");
    bridge.push(&reply(
        close["id"].as_str().unwrap(),
        10009,
        "Request executed",
        810,
    ));
    venue.poll();
    assert_eq!(venue.in_flight(), 1);
    bridge.push(
        &Snapshot::new()
            .position(position(77, "buy", 1, 125_000, MAGIC))
            .closed(810, "canceled")
            .line(),
    );
    let events = venue.poll();
    assert!(
        matches!(
            &events[..],
            [VenueEvent::Rejected(RejectReason::Venue { code: None, .. })]
        ),
        "{events:?}"
    );
    assert_eq!(venue.in_flight(), 0);
    assert!(venue.position().is_some(), "still open, and shown as open");
}

#[test]
fn a_market_reply_without_a_ticket_is_still_answered_by_its_deal() {
    let (mut venue, bridge) = connected();
    let id = match venue
        .submit(OrderIntent::market(Side::Sell, dec(1)))
        .as_slice()
    {
        [VenueEvent::Placed(order)] => order.id,
        other => panic!("{other:?}"),
    };
    let submit = only(&bridge.take_sent(), "submit");
    bridge.push(&reply(
        submit["id"].as_str().unwrap(),
        10009,
        "Request executed",
        0,
    ));
    bridge.push(
        &Snapshot::new()
            .deal(deal(906, 555, 555, "sell", "in", "expert", 1, 125_000))
            .position(position(555, "sell", 1, 125_000, MAGIC))
            .line(),
    );
    let events = venue.poll();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, VenueEvent::Filled(f) if f.role == FillRole::Entry(id)))
    );
    assert_eq!(venue.in_flight(), 0);
    assert_eq!(venue.ticket(id), Some(555));
}
