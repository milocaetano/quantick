use super::*;
use quantick_session::script_strategies::SignalOrder;
use rust_decimal::Decimal;

fn row(id: u64, state: DeskState, link: DeskLink<DrawingId>) -> DeskRow<DrawingId> {
    DeskRow {
        tab_id: 7,
        pane_id: 1,
        id: InstanceRef::Script(id),
        name: "renko_poc_flip.pine".to_owned(),
        state,
        link,
        action: DeskAction::Alarm,
        status: "watching".to_owned(),
    }
}

fn asset() -> Asset {
    Asset {
        symbol: "WINZ26".to_owned(),
        bar_type: "Renko 10".to_owned(),
    }
}

fn window(state: DeskState, code: Option<&str>, history: Vec<usize>) -> StrategyWindow {
    let row = row(3, state, DeskLink::Chart);
    let mut window = StrategyWindow::open(&row, "WINZ26 · Renko 10".to_owned(), Page::Summary);
    window.detail = Some(Ok(StrategyDetail {
        row,
        description: "flip".to_owned(),
        side: Side::Sell,
        placement: ScriptPlacement::Range {
            a: Decimal::from(100),
            b: Decimal::from(110),
        },
        inputs: vec![("Min POC gap".to_owned(), "2".to_owned())],
        fires_today: 9,
        code: code.map(str::to_owned),
        history,
    }));
    window
}

#[test]
fn a_header_pauses_a_watching_asset_and_resumes_a_paused_one() {
    let watching = DeskGroup {
        asset: asset(),
        paused: false,
        rows: vec![row(1, DeskState::Watching, DeskLink::Chart)],
    };
    let header = group(&watching, &Actions::new()).header;
    assert_eq!(header.label, "WINZ26 · Renko 10");
    assert_eq!(header.count, "1");
    assert_eq!(header.toggle, Toggle::Pause);
    assert_eq!(header.send, (7, Command::PauseAsset(asset())));
    assert_eq!(header.hover, "pause every strategy on WINZ26 · Renko 10");

    let paused = DeskGroup {
        paused: true,
        ..watching
    };
    assert_eq!(
        group(&paused, &Actions::new()).header.send,
        (7, Command::ResumeAsset(asset()))
    );
}

#[test]
fn a_line_says_its_state_link_and_action_and_offers_its_toggle() {
    let drawn = DeskLink::Drawing {
        drawing: DrawingId(4),
        name: Some("zone-1".to_owned()),
    };
    let watching = line(&row(1, DeskState::Watching, drawn), "a", None);
    assert_eq!(watching.state, ("WATCHING", Ink::Buy));
    assert_eq!(watching.link, "zone-1");
    assert_eq!(watching.action, ("alarm", Ink::Violet));
    let pause = (7, Command::Pause(InstanceRef::Script(1)));
    assert_eq!(watching.toggle, Some((Toggle::Pause, pause)));
    assert_eq!(
        watching.remove,
        (7, Command::Remove(InstanceRef::Script(1)))
    );

    let paused = line(&row(1, DeskState::Paused, DeskLink::Chart), "a", None);
    assert_eq!(paused.state, ("PAUSED", Ink::Muted));
    assert_eq!(paused.link, "chart");
    let resume = (7, Command::Resume(InstanceRef::Script(1)));
    assert_eq!(paused.toggle, Some((Toggle::Resume, resume)));

    let reason = "the chart closed".to_owned();
    let ended = line(
        &row(1, DeskState::Ended { reason }, DeskLink::Chart),
        "a",
        None,
    );
    assert_eq!(ended.state, ("ENDED", Ink::Sell));
    assert_eq!(ended.toggle, None, "an ended instance arms from its window");

    let unnamed = DeskLink::Drawing {
        drawing: DrawingId(4),
        name: None,
    };
    assert_eq!(link(&unnamed), "drawing 4");
}

#[test]
fn a_page_word_opens_that_page_and_any_other_the_summary() {
    assert_eq!(Page::parse(" code "), Page::Code);
    assert_eq!(Page::parse("History"), Page::History);
    assert_eq!(Page::parse("nonsense"), Page::Summary);
}

#[test]
fn the_summary_lists_where_action_inputs_asset_and_fires_today() {
    let window = window(DeskState::Watching, Some("//@version=6"), vec![]);
    assert_eq!(window.title(), "renko_poc_flip.pine");
    assert!(window.readable());
    assert_eq!(window.description(), "flip");
    assert_eq!(window.code(), Some("//@version=6"));
    assert_eq!(window.save_name, "renko_poc_flip copy");
    let words = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    assert_eq!(
        window.summary(),
        [
            words("Where", "range 100–110"),
            words("Action", "alarm · sell"),
            words("Min POC gap", "2"),
            words("Asset and bar", "WINZ26 · Renko 10"),
            words("Fires today", "9"),
        ]
    );
}

#[test]
fn history_places_each_fire_along_the_loaded_bars() {
    let fired = window(DeskState::Watching, None, vec![2, 5, 10]);
    let (caption, marks) = fired.history();
    assert_eq!(
        caption,
        "3 fires on the loaded bars; the strip ends at the last fire"
    );
    assert_eq!(marks, [0.2, 0.5, 1.0]);
    let none = window(DeskState::Watching, None, vec![]);
    assert_eq!(none.history().1, Vec::<f32>::new());
}

#[test]
fn only_an_ended_instance_arms_and_only_a_script_saves() {
    let watching = window(DeskState::Watching, None, vec![]);
    assert!(!watching.arms());
    assert!(!watching.saves(), "a drawn preset has no script");
    let reason = "stopped".to_owned();
    let ended = window(DeskState::Ended { reason }, Some("src"), vec![]);
    assert!(ended.arms());
    assert!(ended.saves());
    let id = InstanceRef::Script(3);
    assert_eq!(ended.arm(), (7, Command::Arm(id)));
    assert_eq!(ended.duplicate(), (7, Command::Duplicate(id)));
    let name = "renko_poc_flip copy".to_owned();
    let save = Command::SaveAs { instance: id, name };
    assert_eq!(ended.save_as(), (7, save));

    let mut forgotten = ended;
    forgotten.forget();
    assert!(forgotten.detail().is_none() && !forgotten.arms());
    forgotten.detail = Some(Err("gone".to_owned()));
    assert!(!forgotten.readable());
    assert_eq!(forgotten.description(), "gone");
    assert_eq!(forgotten.title(), "Strategy");
}

#[test]
fn a_save_toasts_its_file_and_a_refusal_its_reason() {
    let result = |saved_as: Option<&str>| CommandResult {
        capability: "strategy.desk.save_as".to_owned(),
        tab_id: None,
        changed: 1,
        created: None,
        saved_as: saved_as.map(str::to_owned),
    };
    assert_eq!(
        toast(Ok(result(Some("flip copy.pine")))),
        Some("saved to the library as flip copy.pine".to_owned())
    );
    assert_eq!(toast(Ok(result(None))), None);
    let refused = ControlError::invalid_request("the strategy has ended");
    assert_eq!(
        toast(Err(refused)),
        Some("the strategy has ended".to_owned())
    );
}

#[test]
fn a_window_reads_again_once_its_listed_row_moves_on() {
    let mut window = window(DeskState::Watching, None, vec![]);
    let listed = row(3, DeskState::Watching, DeskLink::Chart);
    assert!(
        !window.stale(Some(&listed)),
        "the row it read is still listed"
    );

    let reason = "fired once".to_owned();
    let ended = row(3, DeskState::Ended { reason }, DeskLink::Chart);
    assert!(window.stale(Some(&ended)), "it ended on its own");
    assert!(window.stale(None), "another caller removed it");

    window.detail = Some(Err("gone".to_owned()));
    assert!(
        !window.stale(None),
        "a refusal stands while nothing is listed"
    );
    assert!(window.stale(Some(&listed)), "it is listed again");
    window.forget();
    assert!(window.stale(Some(&listed)), "never read");
}

#[test]
fn strategy_rail_shows_action_quantity_and_protection() {
    let order = |quantity, stop: i64, gain: i64| {
        let order = SignalOrder {
            quantity,
            stop: Decimal::from(stop),
            gain: Decimal::from(gain),
        };
        Some((1, SignalAction::Order(order)))
    };
    let watching = row(1, DeskState::Watching, DeskLink::Chart);

    let alarm = line(&watching, "a", Some((1, SignalAction::Alarm)));
    assert_eq!(alarm.action, ("alarm", Ink::Violet));
    assert_eq!((alarm.quantity, alarm.protection), (None, None));
    assert_eq!(alarm.chip, "alarm");
    let draft = alarm.draft.expect("a script row edits its action");
    assert!(
        !draft.order && draft.ticket,
        "an alarm drafts a ticket-sized order"
    );

    let ordering = DeskRow {
        action: DeskAction::Order,
        ..watching.clone()
    };
    let fixed = line(
        &ordering,
        "a",
        order(OrderQuantity::Fixed(Decimal::from(3)), 0, 0),
    );
    assert_eq!(fixed.action, ("order", Ink::Accent));
    assert_eq!(fixed.quantity.as_deref(), Some("3 ct"));
    assert_eq!(fixed.protection.as_deref(), Some("—"), "no leg either side");

    let ticket = line(&ordering, "a", order(OrderQuantity::Ticket, 100, 200));
    assert_eq!(ticket.quantity.as_deref(), Some("Ticket"));
    assert_eq!(ticket.protection.as_deref(), Some("SL 100 / TP 200"));
    assert_eq!(ticket.chip, "order · Ticket · SL 100 / TP 200");
    let stop_only = line(&ordering, "a", order(OrderQuantity::Ticket, 100, 0));
    assert_eq!(stop_only.protection.as_deref(), Some("SL 100"));

    let drawn = DeskRow {
        id: InstanceRef::Drawing {
            pane_id: 1,
            drawing: DrawingId(4),
            id: 2,
        },
        ..ordering
    };
    let preset = line(&drawn, "a", None);
    assert_eq!(
        (preset.quantity, preset.draft),
        (None, None),
        "its preset decides"
    );
}

#[test]
fn the_action_chip_sends_one_set_action_or_says_what_is_missing() {
    let mut draft = ActionDraft::of(7, 3, SignalAction::Alarm);
    let labels = draft.fields().map(|(label, _)| label);
    assert_eq!(labels, ["quantity", "stop", "gain"]);
    let set = |action| {
        let id = InstanceRef::Script(3);
        (7, Command::SetAction { id, action })
    };
    assert_eq!(draft.sent(), Ok(set(SignalAction::Alarm)));

    *draft.switches()[0].1 = true;
    *draft.fields()[1].1 = " 100 ".to_owned();
    let ticket = SignalOrder {
        quantity: OrderQuantity::Ticket,
        stop: Decimal::from(100),
        gain: Decimal::ZERO,
    };
    assert_eq!(draft.sent(), Ok(set(SignalAction::Order(ticket))));
    assert_eq!(
        ActionDraft::of(7, 3, SignalAction::Order(ticket)).stop,
        "100"
    );

    draft.ticket = false;
    let missing = draft.sent().expect_err("no quantity typed");
    assert!(missing.contains("quantity"), "{missing}");
    draft.quantity = "two".to_owned();
    let refused = draft.sent().expect_err("not a number");
    assert!(refused.contains("`two`"), "{refused}");
}
