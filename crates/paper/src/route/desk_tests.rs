//! The broker desk over a broker double: where the venue sits and what
//! each press and frame does to it.

use super::*;

#[test]
fn arming_routes_the_active_chart_to_the_broker() {
    let (desk, mut tabs, _script) = armed(TradeMode::Real);
    assert_eq!(desk.route().route(), Route::Real);
    assert!(tabs.active().broker_routed());
    assert_eq!(tabs.active().route_kind(), Route::Real);
    let sheet = desk.sheet(&tabs, LIVE, None, HOLD_TO_ARM_MS);
    assert_eq!(sheet.title, "TRADING · REAL");
    assert_eq!(sheet.notice.text, "XP 51234567 · REAL");
    assert_eq!(sheet.notice.tone, RouteTone::Real);
}
#[test]
fn the_trading_tab_goes_by_the_route_word_the_panel_paints() {
    use quantick_workspace::dock::DockTab;
    assert_eq!(dock_title(DockTab::Trading, None), SIM_TITLE);
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let tabs = Tabs::new();
    desk.install(venue);
    let sim = desk.sheet(&tabs, LIVE, None, 0);
    assert_eq!(dock_title(DockTab::Trading, Some(&sim)), SIM_TITLE);
    for (mode, title) in [
        (TradeMode::Demo, "TRADING · DEMO"),
        (TradeMode::Real, "TRADING · REAL"),
    ] {
        let (desk, tabs, _script) = armed(mode);
        let sheet = desk.sheet(&tabs, LIVE, None, HOLD_TO_ARM_MS);
        assert_eq!(sheet.title, title);
        assert_eq!(dock_title(DockTab::Trading, Some(&sheet)), title);
        assert_eq!(
            dock_title(DockTab::Trades, Some(&sheet)),
            DockTab::Trades.title(),
            "the ledger keeps its own title"
        );
    }
}
#[test]
fn the_held_real_row_shows_its_facts_and_progress() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.hold_start(&tabs, LIVE, 0).expect("REAL is available");
    let money = quantick_trading::InstrumentMoney::declared(
        ("BRL", Decimal::new(20, 2), Decimal::ONE),
        None,
    )
    .expect("a declaration");
    let mut book = crate::risk_sizing::InstrumentBook::new();
    book.insert("WINZ26".to_owned(), money);
    tabs.active().set_instrument_money(book);
    let sheet = desk.sheet(&tabs, LIVE, Some(Decimal::ONE), 620);
    let real = &sheet.rows[1];
    assert_eq!(real.title, "XP · 51234567 · REAL");
    assert_eq!(real.hint, "holding…");
    assert_eq!(real.gesture, RowGesture::Hold);
    assert_eq!(real.progress, Some(0.62));
    assert_eq!(
        real.subs,
        [
            "real money · balance 12340.00 BRL",
            "WINZ26 · 0.20 BRL/pt · qty 1",
            "you: flat · other robots: 1 position",
            "Stop and target rest on the server.",
        ]
    );
    assert_eq!(sheet.rows[0].hint, "in use");
}
#[test]
fn a_replay_locks_the_selector_with_the_reason() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    let replay = SourceFacts {
        broker_enabled: true,
        source_routes: false,
        replay: true,
    };
    assert_eq!(
        desk.hold_start(&tabs, replay, 0),
        Err(Refusal::SourceCannotRoute)
    );
    let sheet = desk.sheet(&tabs, replay, None, 0);
    assert!(sheet.locked);
    assert_eq!(sheet.selector, "Simulated · replay");
    assert_eq!(sheet.rows[1].state, RowState::Off);
    assert_eq!(
        sheet.footer.as_deref(),
        Some("This source does not route orders.")
    );
    assert_eq!(sheet.rows[0].subs, ["Replay of WINZ26."]);
    desk.frame(&mut tabs, replay, 0);
    assert_eq!(desk.route().route(), Route::Sim);
}
#[test]
fn a_lost_link_with_a_position_parks_it_in_view_and_refuses_entries() {
    let (mut desk, mut tabs, script) = armed(TradeMode::Real);
    go_long(tabs.active());
    assert!(tabs.active().venue().position().is_some());

    script.borrow_mut().link = Some(LinkState::Lost);
    script
        .borrow_mut()
        .said
        .push(VenueEvent::Notice(VenueNotice::LinkLost));
    desk.frame(&mut tabs, LIVE, 5_000);
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(
        desk.take_toast().as_deref(),
        Some("REAL: no link with the broker - disarmed, orders now go to Simulated")
    );
    let account = tabs.active();
    assert!(!account.broker_routed());
    assert!(
        account.broker_shown(),
        "the broker position stays on the chart"
    );
    assert_eq!(account.route_kind(), Route::Real);
    assert_eq!(
        account.entry_block(),
        Some(crate::account::PARKED_ENTRY_BLOCK)
    );
    assert!(
        !account.entry_ready(),
        "the toolbar and the ticket both refuse"
    );
    let refused = account
        .venue_mut()
        .submit(OrderIntent::market(Side::Buy, Decimal::ONE));
    assert!(matches!(
        refused.as_slice(),
        [VenueEvent::Rejected(RejectReason::Blocked(_))]
    ));
    let sheet = desk.sheet(&tabs, LIVE, None, 19_000);
    assert_eq!(sheet.notice.text, "XP 51234567 · no link");
    assert_eq!(sheet.notice.dot, Dot::Hollow);
    assert_eq!(sheet.tone, RouteTone::Lost);
    assert_eq!(
        sheet.status.as_deref(),
        Some("no link with the broker for 14 s · disarmed · stop and target stay at the broker")
    );
}
#[test]
fn leaving_flat_detaches_the_broker_and_trades_the_simulator() {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    assert!(tabs.active().broker_routed());
    desk.request(
        &mut tabs,
        LIVE,
        RouteRequest::new(Route::Sim, Actor::Trader, None),
    )
    .expect("flat, nothing to ask");
    let account = tabs.active();
    assert!(account.broker().is_none());
    assert_eq!(account.route_kind(), Route::Sim);
    assert!(desk.has_venue());
}
#[test]
fn leaving_with_a_position_asks_then_keeps_its_plates() {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    go_long(tabs.active());
    assert_eq!(
        desk.request(
            &mut tabs,
            LIVE,
            RouteRequest::new(Route::Sim, Actor::Trader, None)
        ),
        Err(Refusal::ExposureNeedsChoice)
    );
    let sheet = desk.sheet(&tabs, LIVE, None, 0);
    assert!(sheet.leave_prompt);
    desk.request(
        &mut tabs,
        LIVE,
        RouteRequest::new(Route::Sim, Actor::Trader, Some(Leave::LeaveAtBroker)),
    )
    .expect("left at the broker");
    let account = tabs.active();
    assert!(account.broker_shown(), "the plates stay on the chart");
    assert!(account.venue().position().is_some());
    assert!(!account.broker_routed());
}
#[test]
fn switching_to_a_chart_that_cannot_route_disarms() {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    tabs.active = 2;
    let binance = SourceFacts {
        broker_enabled: true,
        source_routes: false,
        replay: false,
    };
    desk.frame(&mut tabs, binance, 3);
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(
        desk.route().last_disarm(),
        Some((Disarm::SourceCannotRoute, 3))
    );
    assert!(tabs.accounts[&2].broker().is_none());
}
#[test]
fn broker_closes_go_to_their_own_journal() {
    let (_desk, mut tabs, _script) = armed(TradeMode::Demo);
    let account = tabs.active();
    go_long(account);
    account.close_position();
    account.on_trade(&print(2, 103));
    assert_eq!(
        account.ledger_chart_selection(Some(0)),
        None,
        "a simulator ledger index cannot select a broker tape mark"
    );
    let real = account.broker_trades_dir().join("WINZ26");
    assert!(
        std::fs::read_dir(&real).is_ok_and(|mut files| files.next().is_some()),
        "a broker close lands under {}",
        real.display()
    );
    assert!(
        account.journal_path().is_none(),
        "the paper journal stayed empty"
    );
    let toast = account.take_toast().unwrap_or_default();
    assert!(toast.starts_with("DEMO closed: LONG 1"), "{toast}");
}
#[test]
fn plates_name_the_broker_ticket_once_it_has_one() {
    let (_desk, mut tabs, script) = armed(TradeMode::Demo);
    let account = tabs.active();
    account.seed(&print(0, 100));
    let placed = account.venue_mut().submit(OrderIntent::limit(
        Side::Buy,
        Decimal::ONE,
        Decimal::from(90),
    ));
    account.handle_events(placed);
    let id = account.venue().working_orders()[0].id;
    assert!(account.awaiting_ack(id), "sent, not yet held");
    assert_eq!(account.plate_id(id), id.0);
    script.borrow_mut().tickets.insert(id, 88_412_096);
    assert!(!account.awaiting_ack(id));
    assert_eq!(account.plate_id(id), 88_412_096);
    assert_eq!(account.venue_ref(BracketTarget::Position), Some(88_412_094));
}
#[test]
fn automated_strategies_never_reach_a_broker() {
    let (_desk, mut tabs, _script) = armed(TradeMode::Demo);
    let events = tabs
        .active()
        .apply_strategy_command(quantick_sim::Command::PlaceMarket {
            side: Side::Buy,
            quantity: Decimal::ONE,
            bracket: Bracket::none(),
        });
    assert_eq!(
        events,
        [VenueEvent::Rejected(RejectReason::Blocked(
            crate::account::STRATEGIES_SIM_ONLY
        ))]
    );
}
#[test]
fn a_press_on_the_real_row_arms_only_when_the_hold_completes() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.press(&mut tabs, LIVE, RoutePress::OpenList, 0);
    desk.press(&mut tabs, LIVE, RoutePress::HoldStart, 0);
    desk.frame(&mut tabs, LIVE, HOLD_TO_ARM_MS / 2);
    assert_eq!(desk.route().route(), Route::Sim);
    desk.press(&mut tabs, LIVE, RoutePress::HoldRelease, HOLD_TO_ARM_MS / 2);
    desk.frame(&mut tabs, LIVE, 5 * HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Sim, "released early");

    desk.press(&mut tabs, LIVE, RoutePress::HoldStart, 10_000);
    desk.frame(&mut tabs, LIVE, 10_000 + HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Real, "held through");
    assert!(!desk.sheet_open, "the list closes once armed");
}
#[test]
fn a_staged_scene_arms_as_the_trader_would_and_drops_its_link_once() {
    let scene = Scene::parse(Some("lost"), Some("1"), None).expect("a scene");
    assert!(scene.needs_broker() && scene.stands_in_live() && scene.lost);
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    assert!(!desk.stage(&mut tabs, LIVE, &scene, 5_000), "arming first");
    assert!(desk.sheet_open);
    assert_eq!(desk.route().route(), Route::Real, "through the full hold");
    assert!(
        desk.stage(&mut tabs, LIVE, &scene, 5_016),
        "then the link drops"
    );
    assert_eq!(Scene::parse(None, None, None), None);
    let idle = Scene::parse(Some("idle"), None, None).expect("a scene");
    assert!(idle.needs_broker() && !idle.stands_in_live());
}
// --- review fixes ---------------------------------------------------------

#[test]
fn the_desk_hands_the_frame_time_to_the_broker_wherever_it_sits() {
    let (venue, script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.frame(&mut tabs, LIVE, 1_000);
    assert_eq!(script.borrow().ticked, [1_000], "loose, still counted");

    desk.hold_start(&tabs, LIVE, 2_000)
        .expect("REAL is available");
    desk.hold_still(&mut tabs, LIVE, 2_000 + HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Real);
    // A hung bridge: the venue declares its silent link lost at the tick.
    let lost = VenueEvent::Notice(VenueNotice::LinkLost);
    script.borrow_mut().on_tick.push(lost);
    desk.frame(&mut tabs, LIVE, 9_000);
    assert_eq!(script.borrow().ticked.last(), Some(&9_000));
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(desk.route().last_disarm(), Some((Disarm::LinkLost, 9_000)));
}

#[test]
fn a_notice_a_print_drained_still_reaches_the_route() {
    let (mut desk, mut tabs, script) = armed(TradeMode::Real);
    tabs.active().seed(&print(0, 100));
    // The terminal changed account; the print drains the venue first, as the
    // MetaTrader venue's on_trade does, and only the toast saw it.
    let changed = VenueEvent::Notice(VenueNotice::AccountChanged);
    script.borrow_mut().said.push(changed);
    tabs.active().on_trade(&print(1, 100));
    desk.frame(&mut tabs, LIVE, 4_000);
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(
        desk.route().last_disarm(),
        Some((Disarm::AccountChanged, 4_000))
    );
}

#[test]
fn another_login_of_the_same_kind_disarms_without_a_notice() {
    let (mut desk, mut tabs, script) = armed(TradeMode::Real);
    desk.frame(&mut tabs, LIVE, 1_000);
    assert_eq!(desk.route().route(), Route::Real);
    // REAL A to REAL B, and no notice said so: the facts still differ.
    script.borrow_mut().login = Some(51_999_999);
    desk.frame(&mut tabs, LIVE, 2_000);
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(
        desk.route().last_disarm(),
        Some((Disarm::AccountChanged, 2_000))
    );
}

#[test]
fn a_hold_begun_on_one_account_never_arms_another() {
    let (venue, script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.hold_start(&tabs, LIVE, 0).expect("REAL is available");
    script.borrow_mut().login = Some(51_999_999);
    desk.frame(&mut tabs, LIVE, 10);
    desk.hold_still(&mut tabs, LIVE, HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Sim);
}

#[test]
fn a_final_parked_close_by_tape_or_poll_only_reaches_the_broker_journal() {
    for by_poll in [false, true] {
        let (_desk, mut tabs, script) = armed(TradeMode::Demo);
        let account = tabs.active();
        go_long(account);
        account.park_broker();
        account.close_position();
        if by_poll {
            script.borrow_mut().poll_print = Some(print(2, 105));
            account.poll_broker(2_000);
        } else {
            account.on_trade(&print(2, 105));
        }
        assert!(!account.broker_shown());
        assert!(account.simulator_trades().is_empty());
        assert!(account.session_trade_sources().is_empty());
        assert!(account.session_journal_paths().is_empty());
        assert!(account.take_journal_changed());
        let history = crate::report::journals::load(account.trades_dir(), None, &[]);
        assert_eq!(history.rows.len(), 1);
        assert!(history.rows[0].session.as_ref().unwrap().is_broker());
        assert_eq!(
            history.rows[0].source,
            Some(quantick_sim::history::SessionSource::Broker(
                TradeMode::Demo
            ))
        );
    }
}

#[test]
fn an_account_change_with_exposure_rearms_the_same_tab_and_journals_the_new_account() {
    let (mut desk, mut tabs, script) = armed(TradeMode::Demo);
    go_long(tabs.active());
    tabs.active().close_position();
    tabs.active().on_trade(&print(2, 105));
    go_long(tabs.active());
    script.borrow_mut().trade_mode = Some(TradeMode::Real);
    script.borrow_mut().login = Some(9876);
    desk.frame(&mut tabs, LIVE, 3_000);
    assert_eq!(desk.route().route(), Route::Sim);
    assert!(tabs.active().parked_broker().is_some());
    desk.hold_start(&tabs, LIVE, 4_000).unwrap();
    desk.hold_still(&mut tabs, LIVE, 4_000 + HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Real);
    tabs.active().close_position();
    tabs.active().on_trade(&print(6, 106));
    let history = crate::report::journals::load(tabs.active().trades_dir(), None, &[]);
    let sources: Vec<_> = history.rows.iter().map(|row| row.source).collect();
    assert_eq!(
        sources,
        [TradeMode::Demo, TradeMode::Real]
            .map(|mode| Some(quantick_sim::history::SessionSource::Broker(mode)))
    );
}

/// DEMO armed on the active tab, long one, then left at the broker.
fn parked() -> (BrokerDesk, Tabs) {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    go_long(tabs.active());
    let leave = RouteRequest::new(Route::Sim, Actor::Trader, Some(Leave::LeaveAtBroker));
    desk.request(&mut tabs, LIVE, leave)
        .expect("left at the broker");
    assert!(tabs.active().parked_broker().is_some());
    (desk, tabs)
}

#[test]
fn a_parked_broker_answers_only_the_traders_hand() {
    let (desk, mut tabs) = parked();
    assert_eq!(desk.route().route(), Route::Sim);
    for allow in [false, true] {
        assert_eq!(
            desk.write_block(&tabs, Actor::Agent, allow),
            Some(Refusal::ParkedAtBroker),
            "an agent never touches what the trader left at the broker"
        );
    }
    assert_eq!(desk.write_block(&tabs, Actor::Trader, false), None);
    tabs.active = 2;
    assert_eq!(
        desk.write_block(&tabs, Actor::Agent, false),
        None,
        "a tab trading the simulator is open to agents"
    );
}

#[test]
fn an_armed_broker_takes_agent_orders_only_where_the_config_allows() {
    let (desk, tabs, _script) = armed(TradeMode::Demo);
    assert_eq!(
        desk.write_block(&tabs, Actor::Agent, false),
        Some(Refusal::AgentOrdersOff)
    );
    assert_eq!(desk.write_block(&tabs, Actor::Agent, true), None);
}

#[test]
fn automated_strategies_never_reach_a_parked_broker() {
    let (_desk, mut tabs) = parked();
    let account = tabs.active();
    let refused = [
        quantick_sim::Command::ClosePosition,
        quantick_sim::Command::SetBracket {
            stop_loss: Some(Decimal::from(90)),
            take_profit: None,
        },
    ]
    .map(|command| account.apply_strategy_command(command));
    for events in refused {
        assert_eq!(
            events,
            [VenueEvent::Rejected(RejectReason::Blocked(
                crate::account::STRATEGIES_SIM_ONLY
            ))]
        );
    }
    account.on_trade(&print(2, 101));
    assert!(account.venue().position().is_some(), "nothing was closed");
}

#[test]
fn a_hold_whose_row_stopped_being_painted_never_arms() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.press(&mut tabs, LIVE, RoutePress::OpenList, 0);
    desk.press(&mut tabs, LIVE, RoutePress::HoldStart, 0);
    desk.frame(&mut tabs, LIVE, 16);
    desk.press(&mut tabs, LIVE, RoutePress::HoldStill, 32);
    desk.frame(&mut tabs, LIVE, 32);
    // The panel collapses mid-hold: no row is painted, so no report and no
    // release ever comes.
    desk.frame(&mut tabs, LIVE, 48);
    assert_eq!(desk.route().hold_progress(48), None, "the hold ended");
    desk.frame(&mut tabs, LIVE, 2 * HOLD_TO_ARM_MS);
    assert_eq!(desk.route().route(), Route::Sim);
}

#[test]
fn a_hold_reported_every_frame_arms_when_it_completes() {
    let (venue, _script) = broker(TradeMode::Real);
    let mut desk = BrokerDesk::new();
    let mut tabs = Tabs::new();
    desk.install(venue);
    desk.press(&mut tabs, LIVE, RoutePress::HoldStart, 0);
    let mut now = 0;
    while now < HOLD_TO_ARM_MS {
        now += 16;
        desk.press(&mut tabs, LIVE, RoutePress::HoldStill, now);
        desk.frame(&mut tabs, LIVE, now);
    }
    assert_eq!(desk.route().route(), Route::Real);
}

#[test]
fn a_broker_account_words_every_refusal_and_cancel_in_its_own_route() {
    let (_desk, mut tabs, _script) = armed(TradeMode::Demo);
    let account = tabs.active();
    go_long(account);
    account.take_toast();
    let mut book = crate::risk_sizing::InstrumentBook::new();
    let money = quantick_trading::InstrumentMoney::declared(
        ("BRL", Decimal::new(20, 2), Decimal::ONE),
        None,
    )
    .expect("a declaration");
    book.insert("WINZ26".to_owned(), money);
    account.set_instrument_money(book);
    let mut risk = account.risk_settings().clone();
    risk.lock = true;
    risk.basis = crate::risk_sizing::RiskBasis::Amount;
    risk.amount = Decimal::ONE;
    account.set_risk_settings(risk);

    // A short of one stopped 99 points above is past a 1 BRL budget.
    account.reverse_position(Bracket::whole(Some(Decimal::from(199)), None));
    let toast = account.take_toast().unwrap_or_default();
    assert!(toast.starts_with("DEMO: "), "{toast}");

    let intent = OrderIntent::market(Side::Sell, Decimal::from(1000))
        .with_bracket(Bracket::whole(Some(Decimal::from(199)), None));
    assert!(account.place_intent(intent).is_empty());
    let toast = account.take_toast().unwrap_or_default();
    assert!(toast.starts_with("DEMO: "), "{toast}");

    let order = quantick_trading::Order {
        id: OrderId(9),
        side: Side::Sell,
        kind: quantick_trading::EntryKind::Limit,
        price: Some(Decimal::from(110)),
        quantity: Decimal::ONE,
        bracket: Bracket::none(),
        cancel_at: None,
        flat_only: false,
        placed_ms: 0,
        role: quantick_trading::OrderRole::Entry,
        oco: None,
        reduce_only: false,
    };
    account.handle_events(vec![VenueEvent::Cancelled {
        order,
        reason: quantick_trading::CancelReason::BracketReplaced,
    }]);
    let toast = account.take_toast().unwrap_or_default();
    assert!(toast.starts_with("DEMO cancelled #9"), "{toast}");
}

#[test]
fn a_broker_account_words_the_tickets_complaints_in_its_own_route() {
    let (_broker, mut tabs, _script) = armed(TradeMode::Demo);
    let account = tabs.active();
    account.seed(&print(0, 100));
    account.take_toast();
    let mut desk = crate::desk::Desk::default();
    let limit = quantick_trading::EntryKind::Limit;

    desk.ticket.stop_offset_text = "abc".to_owned();
    assert_eq!(
        desk.place_resting(account, Side::Buy, limit, 90.0),
        Err("DEMO: the stop offset must be a positive number of points - got `abc`".to_owned())
    );
    desk.ticket.stop_offset_text.clear();
    desk.ticket.profit_offset_text = "x".to_owned();
    assert_eq!(
        desk.place_resting(account, Side::Buy, limit, 90.0),
        Err("DEMO: the profit offset must be a positive number of points - got `x`".to_owned())
    );
    desk.ticket.profit_offset_text.clear();

    desk.ticket.qty_text = "lots".to_owned();
    assert_eq!(
        desk.place_resting(account, Side::Buy, limit, 90.0),
        Ok(false)
    );
    assert_eq!(
        account.take_toast().as_deref(),
        Some("DEMO: quantity must be a positive number - got `lots`")
    );
    assert!(account.working_orders().is_empty(), "nothing was placed");
}

#[test]
fn a_disarm_toast_names_the_route_that_was_disarmed() {
    let (mut desk, mut tabs, _script) = armed(TradeMode::Demo);
    let replay = SourceFacts {
        broker_enabled: true,
        source_routes: false,
        replay: true,
    };
    desk.frame(&mut tabs, replay, 5_000);
    assert_eq!(desk.route().route(), Route::Sim);
    assert_eq!(
        desk.take_toast().as_deref(),
        Some("DEMO: this source does not route orders - disarmed, orders now go to Simulated")
    );
}

/// Reverse opens the other side, so a parked broker venue holds it as it
/// holds an entry; Close, Close 50% and Flatten only take exposure off and
/// stay pressable. The toolbar, the Trading panel and the chart HUD paint
/// this one answer.
#[test]
fn a_parked_broker_holds_reverse_and_never_the_exits() {
    let ticket = crate::desk::Desk::default();
    let (_desk, mut tabs) = parked();
    let account = tabs.active();
    let actions = ticket.position_actions(account);
    assert_eq!(
        actions.reverse_block.as_deref(),
        Some(crate::account::PARKED_ENTRY_BLOCK)
    );
    assert!(actions.exits, "the trader may still take exposure off");
    assert_eq!(
        ticket.trade_bar(account, "Shift+R").blocked,
        actions.reverse_block,
        "the toolbar reads the same answer"
    );
    let closed = account
        .venue_mut()
        .close(CloseAmount::Partial(Decimal::ONE));
    assert!(
        !closed
            .iter()
            .any(|event| matches!(event, VenueEvent::Rejected(_))),
        "a parked venue still answers a close: {closed:?}"
    );

    let (_desk, mut tabs, _script) = armed(TradeMode::Demo);
    go_long(tabs.active());
    let actions = ticket.position_actions(tabs.active());
    assert_eq!(actions.reverse_block, None, "armed, Reverse is open");
    assert!(actions.exits);

    let flat = Tabs::new();
    assert!(!ticket.position_actions(&flat.accounts[&1]).exits);
}

/// A row answers the pointer the way the panel reports it: a click on a
/// click row, and the REAL row's hold from its start to its release.
#[test]
fn a_row_turns_the_pointer_into_its_press() {
    let row = |gesture, progress| AccountRow {
        title: String::new(),
        hint: String::new(),
        subs: Vec::new(),
        state: RowState::Enabled,
        tone: RouteTone::Real,
        gesture,
        progress,
    };
    let demo = row(RowGesture::Click(Route::Demo), None);
    assert_eq!(
        demo.press(true, false),
        Some(RoutePress::Click(Route::Demo))
    );
    assert_eq!(demo.press(false, true), None);
    let real = row(RowGesture::Hold, None);
    assert_eq!(real.press(false, true), Some(RoutePress::HoldStart));
    assert_eq!(real.press(true, false), None, "a click never arms REAL");
    let holding = row(RowGesture::Hold, Some(0.4));
    assert_eq!(holding.press(false, true), Some(RoutePress::HoldStill));
    assert_eq!(holding.press(false, false), Some(RoutePress::HoldRelease));
    assert_eq!(row(RowGesture::None, None).press(true, true), None);
}

/// A price the hand drops on a broker route - a dragged stop or target, an
/// aimed entry - lands on the instrument's tick grid once the tick is known:
/// B3 refuses an off-grid stop as "Invalid stops" (10016). The simulator
/// keeps the tape's precision, and so does a broker with no tick known.
#[test]
fn a_broker_route_snaps_the_hand_to_the_instrument_tick() {
    let (_desk, mut tabs, _script) = armed(TradeMode::Real);
    let broker = tabs.active();
    broker.seed(&print(0, 172_140));
    assert_eq!(
        broker.snap(172_140.82),
        Decimal::from(172_141),
        "no tick known: the tape's precision"
    );
    broker.set_price_tick(Some(Decimal::from(5)));
    assert_eq!(broker.snap(172_140.82), Decimal::from(172_140), "a stop");
    assert_eq!(broker.snap(172_207.6), Decimal::from(172_210), "a target");
    assert_eq!(
        broker.snap(172_142.5),
        Decimal::from(172_140),
        "half a tick"
    );
    broker.set_price_tick(None);
    assert_eq!(broker.snap(172_140.82), Decimal::from(172_141));

    let sim = tabs.accounts.get_mut(&2).expect("the paper tab");
    sim.seed(&print(0, 172_140));
    sim.set_price_tick(Some(Decimal::from(5)));
    assert_eq!(sim.route_kind(), Route::Sim);
    assert_eq!(
        sim.snap(172_140.82),
        Decimal::from(172_141),
        "the simulator is unchanged"
    );
}
