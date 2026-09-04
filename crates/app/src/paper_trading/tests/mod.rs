// The `paper_trading.rs` unit tests, moved out of the file so a session
// opening the ticket to change one control no longer reads 3,651 lines of
// tests it did not ask for.
//
// They stay a child module of `crate::paper_trading` rather than moving to an
// integration test: a child sees its ancestor's private items, so the move
// widens no visibility in production code, and the `use super::*` below is
// the line the module already had inline.
//
// The file's second test module, `risk_tests`, sits beside this one in
// `risk_tests.rs`. It reaches this module's scope through its own
// `use super::*`, which carries `paper_trading`'s imports down the same
// way -- the shape `app/tests/` already uses across twelve files.

mod risk_tests;

use super::*;
use crate::paper_account::{elide_path, export_csv, utc_compact};
use crate::paper_report::HistoryRow;
// Journalling tests read back the folders the writer created; the
// helper that lists them lives with the rest of the shared chrome.
use crate::paper_chrome::list_symbol_folders;
use crate::paper_report::{load_history, report_from_history};

/// One journal row: the trade, the folder it came from, the source its
/// file recorded.
fn row(symbol: &str, source: Option<history::SessionSource>, trade: ClosedTrade) -> HistoryRow {
    HistoryRow {
        symbol: symbol.to_owned(),
        source,
        trade,
    }
}

fn print(agg_id: u64, price: i64) -> Trade {
    Trade {
        agg_id,
        timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

#[test]
fn utc_compact_matches_known_timestamps() {
    // 2026-03-16 13:01:08 UTC.
    assert_eq!(utc_compact(1_773_666_068_000), "20260316-130108");
    // The epoch itself.
    assert_eq!(utc_compact(0), "19700101-000000");
}

#[test]
fn an_in_session_rerun_opens_its_own_file() {
    // Seek-to-start / reopen-same-recording is a timeline reset with
    // the source unchanged: run 2 must not append into run 1's file.
    let mut paper = PaperTrading::new();
    paper.set_symbol("RESEEK");
    paper
        .account_mut()
        .set_session_source(history::SessionSource::Replay);
    for _ in 0..2 {
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 103));
        paper.on_timeline_reset();
    }
    let folder = paper.account.dir.join("RESEEK");
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect();
    names.sort();
    assert_eq!(names.len(), 2, "each run has its own file: {names:?}");
    assert!(names[1].contains(".rerun-1."), "{names:?}");
}

#[test]
fn export_rows_remember_the_source_each_trade_closed_under() {
    let mut paper = PaperTrading::new();
    paper.set_symbol("SRCX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));
    paper
        .account_mut()
        .set_session_source(history::SessionSource::Replay);
    assert_eq!(
        paper.account.session_trade_sources,
        vec![history::SessionSource::Live],
        "a trade keeps the source it closed under, not the current one"
    );
}

#[test]
fn the_export_csv_carries_readable_stamps_and_running_equity() {
    let trade = |closed_ms: i64, pnl: i64, mae: Option<i64>| ClosedTrade {
        side: Side::Buy,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(100),
        exit_price: Decimal::from(100 + pnl),
        opened_ms: closed_ms - 60_000,
        closed_ms,
        pnl_points: Decimal::from(pnl),
        exit_reason: quantick_sim::ExitReason::Manual,
        entry_agg_id: mae.map(|_| 1),
        exit_agg_id: mae.map(|_| 2),
        mae_points: mae.map(Decimal::from),
        mfe_points: mae.map(Decimal::from),
    };
    let rows = vec![
        row(
            "BTCUSDT",
            Some(history::SessionSource::Live),
            trade(1_773_666_068_000, 5, Some(2)),
        ),
        row("WINQ26", None, trade(1_773_666_368_000, -2, None)),
    ];
    let text = export_csv(&rows);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "header plus two rows");
    assert!(lines[0].starts_with("symbol,side,quantity,opened_ms,opened_utc"));
    assert!(lines[0].ends_with(",source"), "{}", lines[0]);
    assert!(
        lines[1].contains("2026-03-16T13:01:08Z"),
        "human-readable UTC beside the epoch: {}",
        lines[1]
    );
    assert!(lines[1].ends_with(",manual,1,2,2,2,live"), "{}", lines[1]);
    assert!(
        lines[2].contains(",3,"),
        "running equity 5 + (-2): {}",
        lines[2]
    );
    assert!(
        lines[2].ends_with(",manual,,,,,"),
        "unknown v1 fields and an unrecorded source stay empty: {}",
        lines[2]
    );
}

#[test]
fn long_export_paths_elide_to_the_file_name() {
    let short = Path::new("paper-trades/export-1.csv");
    assert_eq!(elide_path(short), short.display().to_string());
    let long = Path::new(
        "C:/some/extremely/long/path/that/never/ends/and/keeps/going/paper-trades/export-20260805-141233.csv",
    );
    let elided = elide_path(long);
    assert!(elided.starts_with('…'), "{elided}");
    assert!(elided.ends_with("export-20260805-141233.csv"), "{elided}");
}

#[test]
fn utc_dates_format_from_the_same_civil_math() {
    assert_eq!(fmt_utc_date(1_773_666_068_000), "2026-03-16");
}

#[test]
fn market_offsets_become_a_bracket_around_the_reference() {
    let mut paper = PaperTrading::new();
    paper.stop_offset_text = "5".to_owned();
    paper.profit_offset_text = "10".to_owned();
    let bracket = paper
        .parse_bracket(Side::Buy, Decimal::from(100))
        .expect("both parse");
    assert_eq!(bracket.stop_loss(), Some(Decimal::from(95)));
    assert_eq!(bracket.take_profit(), Some(Decimal::from(110)));
    let bracket = paper
        .parse_bracket(Side::Sell, Decimal::from(100))
        .expect("both parse");
    assert_eq!(bracket.stop_loss(), Some(Decimal::from(105)));
    assert_eq!(bracket.take_profit(), Some(Decimal::from(90)));
}

#[test]
fn a_bad_offset_toasts_and_blocks_the_order() {
    let mut paper = PaperTrading::new();
    paper.stop_offset_text = "abc".to_owned();
    assert!(paper.parse_bracket(Side::Buy, Decimal::from(100)).is_none());
    assert!(
        paper.account.peek_toast().is_some(),
        "the refusal teaches, never silent"
    );
}

/// The headline gesture: a working order, hovered, offers labelled
/// SL/TP handles; pressing one and dragging sets that leg; the tape
/// then fills the order and the position opens already protected.
///
/// This is the whole promise in one test — the handle the trader
/// presses, the venue call it produces, and the fill that arms it.
#[test]
fn dragging_a_working_orders_handle_arms_the_position_it_opens() {
    let mut paper = PaperTrading::new();
    paper.account.venue.seed(&print(0, 100));
    // A buy limit at 95, below the market: the kind the tape can fill.
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
    let id = paper.working_orders()[0].id;

    // 80..120 over 400 px, price falling with y: 95 is y 250, 90 is y 300.
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let order_center = clamp_tag_center(250.0, chart.top(), chart.bottom());
    // The SL handle sits on the losing side of a buy — below it.
    let handle = bracket_handle_rect(chart.right(), order_center, false);
    assert_eq!(
        paper.control_at(handle.center(), chart, &scale),
        Some(PaperControl::Handle {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        }),
        "a working order offers the same handles the position does"
    );

    // Press the handle, drag down to 90, release.
    paper.handle_chart_input(&frame_at(chart, &scale, handle.center(), true, true, false));
    assert_eq!(
        paper.drag,
        PaperDrag::CreateLeg {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        },
        "the press started a create-drag on the order, not the position"
    );
    paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false));
    paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true));

    assert_eq!(
        paper.working_orders()[0].bracket.stop_loss(),
        Some(Decimal::from(90)),
        "the drag set the order's own stop, before it ever filled"
    );
    assert!(
        paper.account.venue.position().is_none(),
        "and opened no position doing it"
    );

    // The tape reaches the limit: the position arrives protected.
    paper.on_trade(&print(1, 95));
    let position = paper.account.venue.position().expect("the limit filled");
    assert_eq!(
        position.stop_loss,
        Some(Decimal::from(90)),
        "the leg armed itself on the fill - no window without a stop"
    );
}

/// A pane that is not feeding paper input paints the order and its
/// lines — an order is a fact about the account, true on whichever
/// chart you are looking at — but **not** its bracket handles.
///
/// The tag opens on every pane at once by design (one hover, two
/// surfaces), so `reveal` is true over there too. Without the pointer
/// gate the other pane drew a pressable-looking `SL`/`TP` beside an
/// order whose presses it does not take.
#[test]
fn a_pane_without_the_pointer_paints_no_bracket_handles() {
    let mut paper = PaperTrading::new();
    paper.account.venue.seed(&print(0, 100));
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));

    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let order_center = clamp_tag_center(250.0, chart.top(), chart.bottom());
    let handle = bracket_handle_rect(chart.right(), order_center, false);

    // The pane with the pointer offers the handle...
    assert!(
        paper
            .control_at(handle.center(), chart, &scale)
            .is_some_and(|control| matches!(control, PaperControl::Handle { .. })),
        "the input pane can be pressed there"
    );

    // ...and the paint asks one predicate whether to draw them, so the
    // rule is readable in one place rather than inferred from a paint
    // this test would have to reproduce to check.
    assert!(
        !handles_visible(None, true, false),
        "revealed, but no hand on this pane: nothing is drawn"
    );
    assert!(
        handles_visible(Some(handle.center()), true, false),
        "with the hand here, a revealed owner shows its handles"
    );
    assert!(
        handles_visible(Some(handle.center()), false, true),
        "and reaching straight for a handle keeps it up"
    );
}

/// A leg that exists is its own handle: its line is grabbable, and its
/// tag cross clears it without touching the other leg.
#[test]
fn a_working_orders_legs_are_draggable_and_clearable() {
    let mut paper = PaperTrading::new();
    paper.account.venue.seed(&print(0, 100));
    paper.stop_offset_text = "5".to_owned();
    paper.profit_offset_text = "15".to_owned();
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
    let id = paper.working_orders()[0].id;
    assert_eq!(
        paper.working_orders()[0].bracket,
        Bracket::whole(Some(Decimal::from(90)), Some(Decimal::from(110)),),
        "the ticket offsets rode along on the resting order"
    );

    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // Price 90 is y 300: the order's stop line.
    assert_eq!(
        paper.line_at(egui::pos2(400.0, 300.0), &scale),
        Some(PaperDrag::Leg {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        }),
        "the order's stop line is grabbable like the position's"
    );

    // Its tag cross clears that leg and leaves the target alone.
    let stop_center = clamp_tag_center(300.0, chart.top(), chart.bottom());
    let cross = close_button_rect(chart.right(), stop_center);
    assert_eq!(
        paper.control_at(cross.center(), chart, &scale),
        Some(PaperControl::ClearLeg {
            owner: BracketTarget::Order(id),
            leg: Leg::StopLoss,
        })
    );
    paper
        .account
        .amend_leg(BracketTarget::Order(id), Leg::StopLoss, None);
    assert_eq!(
        paper.working_orders()[0].bracket,
        Bracket::whole(None, Some(Decimal::from(110)),),
        "clearing one leg never drops the other"
    );
}

/// A leg the venue refuses snaps back and says why — the order's own
/// price is the reference, so a buy limit at 95 cannot take a stop at
/// 96 even though 96 is below the market at 100.
#[test]
fn a_working_orders_leg_is_judged_against_the_order_not_the_market() {
    let mut paper = PaperTrading::new();
    paper.account.venue.seed(&print(0, 100));
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 95.0));
    let id = paper.working_orders()[0].id;

    paper.account.amend_leg(
        BracketTarget::Order(id),
        Leg::StopLoss,
        Some(Decimal::from(96)),
    );
    assert_eq!(
        paper.working_orders()[0].bracket.stop_loss(),
        None,
        "the refusal left the order as it was"
    );
    let toast = paper.account.peek_toast().expect("the refusal teaches");
    assert!(
        toast.contains("stop loss"),
        "and says which leg was wrong: {}",
        toast
    );
}

/// The stated kind wins where both are conceivable to a trader but only
/// one can rest — which is every price except the mark.
///
/// The pairing to read here is the second and third assertion: at 95,
/// with the market at 100, `Auto` yields a limit. Ask for a stop at that
/// same price and the aim stands down instead of handing you the limit.
/// That is the whole feature: the click that lands is the order you came
/// to place, or no click at all.
#[test]
fn a_stated_entry_kind_is_honoured_or_the_aim_stands_down() {
    let mark = Decimal::from(100);
    let below = Decimal::from(95);
    let above = Decimal::from(105);

    // Auto reads the market, exactly as it always has.
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Auto, Side::Buy, below, mark),
        Some(EntryKind::Limit),
        "a buy below the market waits at a limit"
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Auto, Side::Buy, above, mark),
        Some(EntryKind::Stop),
        "and above it stops in"
    );

    // A stated kind takes the price where it is valid...
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Buy, below, mark),
        Some(EntryKind::Limit)
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Stop, Side::Buy, above, mark),
        Some(EntryKind::Stop)
    );

    // ...and stands the aim down where it is not, rather than silently
    // placing the other kind. A trader who came to buy a pullback must
    // never be handed a breakout stop.
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Stop, Side::Buy, below, mark),
        None,
        "a buy stop cannot arm below the market, so nothing is offered"
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Buy, above, mark),
        None,
        "and a buy limit above it would fill at once"
    );

    // A sell mirrors, on every choice.
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Sell, above, mark),
        Some(EntryKind::Limit)
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Stop, Side::Sell, below, mark),
        Some(EntryKind::Stop)
    );
    assert_eq!(
        resolve_cmd_kind(CmdEntryKind::Limit, Side::Sell, below, mark),
        None
    );

    // On the mark nothing rests, whatever was asked for: a resting order
    // there fills on the next print, which is a market order wearing the
    // wrong name.
    for choice in CmdEntryKind::ALL {
        assert_eq!(
            resolve_cmd_kind(choice, Side::Buy, mark, mark),
            None,
            "{choice:?} rests nothing on the mark"
        );
    }
}

/// The choice survives a restart, and an unknown token in a
/// hand-edited sidecar falls back rather than refusing to open.
#[test]
fn the_entry_kind_choice_is_remembered_and_unknown_tokens_fall_back() {
    let state = crate::paper_state::PaperState {
        cmd_entry_kind: Some("stop".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        CmdTradingSettings::from_state(&state).kind,
        CmdEntryKind::Stop
    );

    let state = crate::paper_state::PaperState {
        cmd_entry_kind: Some("teleport".to_owned()),
        ..Default::default()
    };
    assert_eq!(
        CmdTradingSettings::from_state(&state).kind,
        CmdEntryKind::Auto,
        "a token this build does not know is the default, not a crash"
    );
}

/// The aim itself obeys the choice: same pointer, same market, one
/// preview and one silence.
#[test]
fn the_aim_obeys_the_stated_kind() {
    let mut paper = PaperTrading::new();
    paper.account.venue.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    // y 250 is price 95 — below the market, where a buy limit rests.
    let aim = egui::pos2(400.0, 250.0);

    paper.account.cmd_trading.kind = CmdEntryKind::Auto;
    paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, false));
    assert_eq!(
        paper.cmd_preview.map(|preview| preview.kind),
        Some(EntryKind::Limit),
        "auto offers the kind that can rest there"
    );

    paper.account.cmd_trading.kind = CmdEntryKind::Stop;
    paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, false));
    assert!(
        paper.cmd_preview.is_none(),
        "a trader who asked for a stop is shown no limit"
    );

    // And the press places nothing where the aim shows nothing.
    assert!(!paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, true)));
    assert!(paper.working_orders().is_empty());
}

/// A 800×400 chart over the given price range, plus the input for one
/// pointer frame at `(x, y)`.
/// The strategy from the trader's own editor, halved.
fn halves() -> crate::order_strategies::OrderStrategy {
    use crate::order_strategies::{OrderStrategy, StrategyRow};
    OrderStrategy {
        name: "halves".to_owned(),
        rows: vec![
            StrategyRow {
                share_percent: Decimal::from(50),
                gain_ticks: Some(8),
                loss_ticks: Some(4),
            },
            StrategyRow {
                share_percent: Decimal::from(50),
                gain_ticks: Some(2),
                loss_ticks: Some(5),
            },
        ],
    }
}

/// The projection and the placement go through one function, so what the
/// aim showed is exactly what rested. Proven by comparing the two
/// brackets rather than by reading the code that builds them.
#[test]
fn the_strategys_ladder_is_both_projected_and_placed() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("halves"));
    paper.qty_text = "2".to_owned();
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // y = 250 is 95: below the mark, so a buy rests as a limit there.
    let aim = egui::pos2(400.0, 250.0);

    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
    let projected = paper.cmd_preview.expect("the aim is up").bracket;
    let parts: Vec<_> = projected.parts().copied().collect();
    assert_eq!(parts.len(), 2, "both rungs are projected: {parts:?}");
    assert_eq!(parts[0].quantity, Some(Decimal::ONE));
    assert_eq!(parts[0].take_profit, Some(Decimal::from(103)));
    assert_eq!(parts[0].stop_loss, Some(Decimal::from(91)));
    assert_eq!(parts[1].take_profit, Some(Decimal::from(97)));
    assert_eq!(parts[1].stop_loss, Some(Decimal::from(90)));

    let mut press = ruler_frame(chart, &scale, aim, 0.0);
    press.primary_pressed = true;
    press.primary_down = true;
    assert!(paper.handle_chart_input(&press), "the aim placed");

    assert_eq!(
        paper.working_orders()[0].bracket,
        projected,
        "the order carries the very bracket the aim projected"
    );
}

/// The ruler is a compass, and a trader wants it most when they already
/// have a ladder in mind: rolling the wheel works with a strategy armed,
/// and rolling back to zero hands the ladder its projection again.
///
/// This module used to stand the ruler down under a strategy. That was a
/// rule it invented and the trader never asked for, and it made the
/// wheel look broken in the configuration they actually use.
#[test]
fn the_ruler_works_with_a_strategy_armed_and_yields_when_it_is_put_away() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("halves"));
    paper.qty_text = "2".to_owned();
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);

    // With the ruler at zero the armed ladder is what the aim projects.
    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
    assert_eq!(
        paper.cmd_preview.expect("aim up").bracket.parts().count(),
        2,
        "the ladder projects while the ruler is put away"
    );

    // Three notches: the wheel is the ruler's, strategy or no strategy.
    for _ in 0..3 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
    }
    assert!(paper.consumed_scroll(), "the wheel belonged to the ruler");
    assert_eq!(paper.ruler_notches, 3);
    let preview = paper.cmd_preview.expect("aim up");
    assert_eq!(
        preview.bracket.stop_loss(),
        Some(Decimal::from(92)),
        "and the ruler's symmetric pair is what it now shows"
    );
    assert_eq!(preview.bracket.take_profit(), Some(Decimal::from(98)));

    // Roll it back to zero and the ladder returns; neither gesture costs
    // the other.
    for _ in 0..3 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -40.0));
    }
    assert_eq!(paper.ruler_notches, 0);
    assert_eq!(
        paper.cmd_preview.expect("aim up").bracket.parts().count(),
        2,
        "the armed ladder is back"
    );
}

/// A name the strategies no longer carry selects nothing rather than
/// quietly arming a different ladder.
#[test]
fn a_selection_naming_a_missing_strategy_selects_nothing() {
    let mut paper = PaperTrading::new();
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("a strategy that was deleted"));
    assert!(paper.account().selected_order_strategy().is_none());
    assert_eq!(
        paper.account().order_strategies().len(),
        1,
        "the list is intact"
    );
}

/// The named call and the wheel leave the ruler in the same place, and
/// neither can put it somewhere the other cannot reach.
#[test]
fn setting_the_ruler_by_name_lands_where_the_wheel_would() {
    let mut by_name = PaperTrading::new();
    by_name.seed(&print(0, 100));
    assert_eq!(by_name.set_ruler_ticks(3), 3);

    let mut by_wheel = PaperTrading::new();
    by_wheel.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);
    for _ in 0..3 {
        by_wheel.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
    }

    assert_eq!(by_name.ruler_notches, by_wheel.ruler_notches);
    // And the bound is the same bound: a caller cannot reach past what
    // the wheel itself clamps to.
    assert_eq!(by_name.set_ruler_ticks(u32::MAX), RULER_MAX_NOTCHES);
}

/// The ticket's own buttons honour the ticket's own strategy.
///
/// The Strategy row sits directly above BUY/SELL and prints what is
/// armed; a button under it that placed a bare order would be two
/// surfaces disagreeing about the very next order.
#[test]
fn the_market_buttons_honour_the_selected_strategy() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("halves"));
    paper.qty_text = "2".to_owned();

    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));

    let legs: Vec<_> = paper
        .working_orders()
        .iter()
        .filter(|order| order.is_protective())
        .collect();
    assert_eq!(
        legs.len(),
        4,
        "both rungs armed from the button, not a bare order: {legs:?}"
    );
}

/// The tick is the finest move the tape has shown, not the last print's
/// own decoration.
///
/// A venue that quotes `78112.57000000` has a raw scale of eight and a
/// real step of two; the next print at `78100` normalizes to zero. Both
/// readings would make the ruler step by a different amount under the
/// trader's hand.
#[test]
fn the_tick_is_the_finest_step_the_tape_has_shown() {
    let mut paper = PaperTrading::new();
    // Trailing zeros are decoration, not precision.
    paper.seed(&Trade {
        agg_id: 0,
        timestamp_ms: 0,
        price: Decimal::new(7_811_257_000_000, 8),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    assert_eq!(
        paper.account.tick(),
        Decimal::new(1, 2),
        "two places, not eight"
    );

    // A rounder print does not coarsen an instrument already seen finer.
    paper.on_trade(&print(1, 78_100));
    assert_eq!(
        paper.account.tick(),
        Decimal::new(1, 2),
        "the tick never grows back under a round print"
    );

    // A genuinely finer print does refine it.
    paper.on_trade(&Trade {
        agg_id: 2,
        timestamp_ms: 2_000,
        price: Decimal::new(78_100_123, 3),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    assert_eq!(paper.account.tick(), Decimal::new(1, 3));
}

/// A laddered position shows its rungs and offers no create-handle.
///
/// Its own `stop_loss`/`take_profit` are `None` under a ladder, and a
/// surface reading only those drew the position as unprotected *and*
/// offered the handles of an unprotected one - where a single drag
/// replaces every rung with one level.
#[test]
fn a_laddered_position_reads_as_protected_and_offers_no_handle() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("halves"));
    paper.qty_text = "2".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));

    let position = paper.account.venue.position().expect("long").clone();
    let bracket = paper.account.position_bracket(&position);
    assert!(
        bracket.is_laddered(),
        "the legs fold back into the ladder that armed them: {bracket:?}"
    );
    assert_eq!(bracket.parts().count(), 2, "one rung per OCO pair");
    assert_eq!(
        bracket.stop_loss(),
        None,
        "and it still refuses to name one stop for two"
    );

    // The handles are what a drag would destroy the ladder through.
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let entry_y = 200.0;
    for above in [true, false] {
        let handle = bracket_handle_rect(chart.right(), entry_y, above).center();
        assert_eq!(
            paper.control_at(handle, chart, &scale),
            None,
            "a laddered position offers no create-handle (above: {above})"
        );
    }
}

/// A rung of a resting order can be hauled, and hauling it edits *that
/// order* - never the named ladder that shaped it.
///
/// The strategy is a template. Once an order is on the chart it is the
/// trader's, and a stop they cannot move is not a stop.
#[test]
fn a_rung_of_a_resting_order_moves_and_leaves_the_strategy_alone() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("halves"));
    paper.qty_text = "2".to_owned();
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // y = 250 is 95: below the mark, so a buy rests as a limit there.
    let aim = egui::pos2(400.0, 250.0);
    let mut press = ruler_frame(chart, &scale, aim, 0.0);
    press.primary_pressed = true;
    press.primary_down = true;
    assert!(paper.handle_chart_input(&press), "the aim placed");

    let id = paper.working_orders()[0].id;
    let before: Vec<_> = paper.working_orders()[0].bracket.parts().copied().collect();
    assert_eq!(before.len(), 2, "a two-rung ladder rests: {before:?}");
    // The first rung's stop is 91 (95 - 4 ticks), at y = 290.
    let stop = before[0].stop_loss.expect("the first rung is stopped");
    assert_eq!(stop, Decimal::from(91));

    let stop_y = scale.y(91.0);
    assert_eq!(
        paper.line_at(egui::pos2(400.0, stop_y), &scale),
        Some(PaperDrag::Rung {
            order: id,
            index: 0,
            leg: Leg::StopLoss
        }),
        "the rung is a line the trader can grab"
    );
    assert!(paper.handle_chart_input(&frame(chart, &scale, stop_y, true, true, false)));
    // Pull it down to 88 (y = 320) and let go.
    assert!(paper.handle_chart_input(&frame(chart, &scale, 320.0, false, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 320.0, false, false, true)));

    let after: Vec<_> = paper.working_orders()[0].bracket.parts().copied().collect();
    assert_eq!(
        after[0].stop_loss,
        Some(Decimal::from(88)),
        "the rung moved where it was dropped"
    );
    assert_eq!(
        after[0].take_profit, before[0].take_profit,
        "its own target is untouched"
    );
    assert_eq!(after[1], before[1], "and the other rung never moved");

    // The template is exactly as the trader saved it.
    let strategy = paper
        .account()
        .selected_order_strategy()
        .expect("still armed");
    assert_eq!(strategy.rows[0].loss_ticks, Some(4));
    assert_eq!(strategy.rows[0].gain_ticks, Some(8));
}

/// The state the editor's own `+ Row` used to create: a third rung at
/// 0%, which fails `ShareNotPositive` and silently stopped the chart
/// projecting a ladder the ticket still said was armed.
///
/// The trader hit `+ Row`, held the modifier and saw nothing, with no
/// word anywhere saying why. A row that arrives broken is the bug.
#[test]
fn adding_a_row_never_breaks_the_strategy_that_was_working() {
    use crate::order_strategies::StrategyRow;

    let mut strategy = halves();
    assert!(strategy.validate().is_ok(), "50/50 is usable");

    // What `+ Row` does, as the button does it.
    let assigned: Decimal = strategy.rows.iter().map(|row| row.share_percent).sum();
    assert_eq!(assigned, Decimal::ONE_HUNDRED, "nothing is left over");
    let last = strategy.rows.last_mut().expect("has rows");
    let half = (last.share_percent / Decimal::TWO).round_dp(2);
    last.share_percent -= half;
    strategy.rows.push(StrategyRow {
        share_percent: half,
        gain_ticks: Some(NEW_RUNG_TICKS),
        loss_ticks: Some(NEW_RUNG_TICKS),
    });

    assert_eq!(strategy.rows.len(), 3);
    assert!(
        strategy.validate().is_ok(),
        "the third rung splits the last one instead of arriving at zero: {:?}",
        strategy.rows
    );
    let shares: Decimal = strategy.rows.iter().map(|row| row.share_percent).sum();
    assert_eq!(shares, Decimal::ONE_HUNDRED, "and they still add up");
}

/// An armed ladder that cannot resolve must say so where it is armed.
#[test]
fn an_unusable_strategy_is_named_in_the_ticket_not_left_silent() {
    use crate::order_strategies::StrategyRow;

    let mut broken = halves();
    broken.rows.push(StrategyRow {
        share_percent: Decimal::ZERO,
        gain_ticks: Some(20),
        loss_ticks: Some(20),
    });
    let error = broken.validate().expect_err("a zero share is not a share");
    assert!(
        !error.advice().is_empty(),
        "and the reason is a sentence the ticket can print"
    );

    // The chart is silent by design in this state - that is exactly why
    // the ticket has to speak.
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper
        .account_mut()
        .set_order_strategies(vec![broken], Some("halves"));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 0.0));
    let preview = paper.cmd_preview.expect("the aim is still up");
    assert!(
        preview.bracket.is_empty(),
        "an unusable ladder projects nothing - which is why it must be named"
    );
}

/// Reproduction: the ticket at its default quantity of one, with a
/// two-rung 50/50 ladder armed. This is what the trader actually had.
#[test]
fn repro_the_aim_projects_a_ladder_at_quantity_one() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("halves"));
    // qty_text is left at its default.
    assert_eq!(paper.qty_text, "1");
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);
    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));

    let preview = paper.cmd_preview.expect("the aim is up");
    let parts: Vec<_> = preview.bracket.parts().copied().collect();
    println!("quantity one -> parts: {parts:?}");
    assert!(
        !preview.bracket.is_empty(),
        "the aim must project the armed ladder, not nothing"
    );
}

/// A wheel that reports 40 px a notch must move the ruler, and so must
/// one that reports 50, or 120, or 13.
///
/// This build assumed 50 and met a mouse that reports 40: every roll
/// computed zero ticks and the ruler silently refused to move, which is
/// indistinguishable from the feature not existing. The notch is the
/// device's to declare, not ours to assume.
#[test]
fn the_first_roll_moves_the_ruler_whatever_the_wheel_reports() {
    for notch in [13.0_f32, 40.0, 50.0, 120.0] {
        let mut paper = PaperTrading::new();
        paper.seed(&print(0, 100));
        let (chart, scale) = chart_and_scale(80.0, 120.0);
        let aim = egui::pos2(400.0, 250.0);

        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, notch));
        assert_eq!(
            paper.ruler_notches, 1,
            "one notch of {notch} px is one tick"
        );

        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, notch * 3.0));
        assert_eq!(paper.ruler_notches, 4, "three more notches at {notch} px");

        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -notch * 2.0));
        assert_eq!(paper.ruler_notches, 2, "and it walks back");
    }
}

/// Esc puts the ruler away - but only once the gestures it was already
/// for have nothing to cancel. A standing distance must not shadow
/// disarming an order.
#[test]
fn escape_clears_the_ruler_but_only_after_an_armed_placement() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 40.0));
    assert_eq!(paper.ruler_notches, 1, "the ruler stands");
    paper.account.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });

    assert!(paper.cancel_interaction(), "the first press has work to do");
    assert!(
        paper.account.armed.is_none(),
        "and it disarmed the placement"
    );
    assert_eq!(
        paper.ruler_notches, 1,
        "the distance survives - it was not what the trader was cancelling"
    );

    assert!(
        paper.cancel_interaction(),
        "the second press reaches the ruler"
    );
    assert_eq!(paper.ruler_notches, 0);
    assert!(
        !paper.cancel_interaction(),
        "and then there is nothing left"
    );
}

/// The old, narrower guarantee, kept: with nothing else in flight the
/// first press is the ruler's.
#[test]
fn escape_clears_the_ruler() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 200.0));
    assert!(paper.ruler_notches > 0, "the ruler is standing");
    assert!(paper.cancel_interaction(), "escape had something to cancel");
    assert_eq!(paper.ruler_notches, 0, "and it put the ruler away");
}

/// The gesture the ruler is made of is the one Windows reports sideways.
///
/// Holding a modifier turns a vertical wheel into horizontal scroll, so
/// `raw_scroll_delta` arrives as `x = 40, y = 0` for exactly the roll the
/// ruler exists to serve. Reading only `y` meant the ruler saw nothing
/// whenever the trader was actually holding the key - the one case that
/// matters. The pane hands over whichever axis carried it; this proves
/// the ruler steps on what it is handed.
#[test]
fn the_ruler_steps_on_the_travel_the_pane_hands_it() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);
    // 40 px is what this machine's wheel reports, on whichever axis.
    for _ in 0..3 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
    }
    assert_eq!(paper.ruler_notches, 3);
    let preview = paper.cmd_preview.expect("the aim is up");
    assert_eq!(preview.bracket.stop_loss(), Some(Decimal::from(92)));
    assert_eq!(preview.bracket.take_profit(), Some(Decimal::from(98)));
}

/// Rolling back to zero takes the stop and the target off the chart.
///
/// Zero is not "a very small bracket": it is the bare order the trader
/// started with, and the aim has to look like one. Anything still drawn
/// there is a level they did not ask for and would carry into the click.
#[test]
fn rolling_back_to_zero_leaves_no_stop_and_no_target() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);

    for _ in 0..4 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
    }
    assert_eq!(paper.ruler_notches, 4);
    assert!(
        !paper.cmd_preview.expect("aim up").bracket.is_empty(),
        "the ruler is drawing a pair"
    );

    // All the way back down, and one notch past it for good measure.
    for _ in 0..5 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -40.0));
    }
    assert_eq!(paper.ruler_notches, 0, "it reaches zero and stops there");
    assert!(
        paper.cmd_preview.expect("aim up").bracket.is_empty(),
        "and nothing is left on the chart to place"
    );

    // Esc is the other way home, and lands in the same place.
    for _ in 0..3 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
    }
    assert!(paper.cancel_interaction());
    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
    assert!(
        paper.cmd_preview.expect("aim up").bracket.is_empty(),
        "escape leaves the aim as bare as rolling back does"
    );
}

/// Selecting a strategy must change what the aim draws, and choosing
/// `<None>` must take it away again. The trader reported "selecting a
/// strategy changes nothing"; this is the contract that claim is about.
#[test]
fn the_strategy_combo_changes_what_the_aim_draws() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.qty_text = "2".to_owned();
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);

    // `<None>`: the modifier alone draws nothing.
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], None);
    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
    assert!(
        paper.cmd_preview.expect("aim up").bracket.is_empty(),
        "with no strategy the aim is bare until the wheel is rolled"
    );

    // Selected: the ladder is there on the modifier alone.
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], Some("halves"));
    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
    assert_eq!(
        paper.cmd_preview.expect("aim up").bracket.parts().count(),
        2,
        "selecting a strategy puts its rungs on the aim"
    );

    // Back to `<None>`: gone again.
    paper
        .account_mut()
        .set_order_strategies(vec![halves()], None);
    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
    assert!(
        paper.cmd_preview.expect("aim up").bracket.is_empty(),
        "and choosing none takes it away"
    );
}

/// A notch is worth what the instrument's step says, and the default is
/// derived so the very first roll feels right without configuration.
#[test]
fn the_default_step_scales_with_the_instrument() {
    // A one-cent instrument near 78,000: half a basis point is 3.9,
    // which the 1-2-5 ladder rounds up to 5 points.
    let mut btc = PaperTrading::new();
    btc.seed(&Trade {
        agg_id: 0,
        timestamp_ms: 0,
        price: Decimal::new(7_800_057, 2),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    assert_eq!(btc.account.tick(), Decimal::new(1, 2));
    assert_eq!(
        btc.ruler_step(),
        Decimal::from(5),
        "twenty to forty points is four to eight rolls away"
    );

    // The mini index near 138,000 prints in whole five-point steps.
    let mut win = PaperTrading::new();
    win.seed(&print(0, 138_000));
    win.on_trade(&print(1, 138_005));
    assert_eq!(win.ruler_step(), Decimal::from(10), "two ticks a notch");
}

/// A typed step is the trader's, saved per instrument, and switching
/// away and back keeps it.
#[test]
fn a_typed_step_is_kept_per_symbol() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 78_000));
    paper.set_symbol("BTCUSDT");
    paper.set_ruler_step(Some(Decimal::from(25)));
    assert_eq!(paper.ruler_step(), Decimal::from(25));

    paper.set_symbol("WIN$N");
    assert_ne!(
        paper.ruler_step(),
        Decimal::from(25),
        "another instrument does not inherit it"
    );

    paper.set_symbol("BTCUSDT");
    assert_eq!(paper.ruler_step(), Decimal::from(25), "and it comes back");

    // Clearing puts the instrument back on its derived default.
    paper.set_ruler_step(None);
    assert_eq!(paper.ruler_step(), paper.account.derived_ruler_step());
}

/// Switching instrument drops the standing ruler and the tick it was
/// measured in. A distance chosen on one market means nothing on the
/// next, and would otherwise arm the first order placed there.
#[test]
fn a_symbol_switch_forgets_the_ruler_and_the_tick() {
    let mut paper = PaperTrading::new();
    // The symbol first: switching to it is what clears the tick, and a
    // tape seeded before the switch would be cleared with it.
    paper.set_symbol("BTCUSDT");
    paper.seed(&Trade {
        agg_id: 0,
        timestamp_ms: 0,
        price: Decimal::new(7_800_057, 2),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    let (chart, scale) = chart_and_scale(77_000.0, 79_000.0);
    paper.handle_chart_input(&ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 40.0));
    assert!(paper.ruler_notches > 0);
    assert_eq!(paper.account.tick(), Decimal::new(1, 2));

    paper.set_symbol("WIN$N");
    assert_eq!(paper.ruler_notches, 0, "the distance does not travel");
    // The tick falls back to the coarsest until the new tape prints:
    // erring coarse means a wider step, never a phantom precision the
    // new market has not shown.
    assert_eq!(
        paper.account.tick(),
        Decimal::ONE,
        "the old market's precision does not travel either"
    );
    // And the first print of the new one refines it honestly.
    paper.on_trade(&Trade {
        agg_id: 9,
        timestamp_ms: 9_000,
        price: Decimal::new(1_380_055, 1),
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    assert_eq!(paper.account.tick(), Decimal::new(1, 1));
}

/// The opening instrument is arrival, not departure. The app names the
/// symbol a frame after construction, so a ruler standing before that
/// first call — the launch hook's, or a session restored into a fresh
/// simulator — must survive it. Only leaving an instrument forgets.
#[test]
fn the_first_symbol_of_a_session_keeps_a_standing_ruler() {
    let mut paper = PaperTrading::new();
    paper.ruler_notches = 6;
    paper.set_symbol("BTCUSDT");
    assert_eq!(
        paper.ruler_notches, 6,
        "arriving at the opening instrument is not a switch"
    );
    paper.set_symbol("WIN$N");
    assert_eq!(paper.ruler_notches, 0, "leaving one still forgets");
}

/// Pressing the wheel puts the ruler away, and only while an aim is up.
#[test]
fn pressing_the_wheel_puts_the_ruler_away() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);
    for _ in 0..3 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
    }
    assert_eq!(paper.ruler_notches, 3);

    let mut press = ruler_frame(chart, &scale, aim, 0.0);
    press.middle_pressed = true;
    paper.handle_chart_input(&press);
    assert_eq!(
        paper.ruler_notches, 0,
        "the wheel that walked it out puts it away"
    );
    assert!(
        paper.cmd_preview.expect("aim up").bracket.is_empty(),
        "and the aim is bare again"
    );

    // With no aim up the press is nobody's business here.
    for _ in 0..2 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 40.0));
    }
    let mut bare = ruler_frame(chart, &scale, aim, 0.0);
    bare.modifiers = egui::Modifiers::default();
    bare.middle_pressed = true;
    paper.handle_chart_input(&bare);
    assert_eq!(paper.ruler_notches, 2, "no aim, no claim on the press");
}

/// A frame with the aim's modifier held and wheel travel to spend.
fn ruler_frame<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    pointer: egui::Pos2,
    scroll_y: f32,
) -> ChartInput<'a> {
    ChartInput {
        chart,
        scale: Some(scale),
        pointer: Some(pointer),
        primary_pressed: false,
        primary_down: false,
        primary_released: false,
        modifiers: egui::Modifiers {
            shift: true,
            ..Default::default()
        },
        canvas_claimed: false,
        scroll_y,
        middle_pressed: false,
        layer_visible: true,
    }
}

/// The ruler walks both legs out together, one tick per notch, and says
/// how far in the units the trader reads.
#[test]
fn the_wheel_walks_the_projected_bracket_out_symmetrically() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // y = 250 is 95: below the mark, so a buy rests as a limit there.
    let aim = egui::pos2(400.0, 250.0);

    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 0.0));
    let preview = paper.cmd_preview.expect("the aim is up");
    assert_eq!(preview.kind, EntryKind::Limit);
    assert_eq!(preview.ruler_ticks, 0, "the ruler starts off");
    assert_eq!(preview.bracket.stop_loss(), None, "and projects nothing");

    // Three notches up, one roll each - which is what a wheel does.
    for _ in 0..3 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 50.0));
    }
    assert!(paper.consumed_scroll(), "the wheel belonged to the ruler");
    assert_eq!(paper.ruler_notches, 3);
    let preview = paper.cmd_preview.expect("the aim is still up");
    assert_eq!(
        preview.bracket.stop_loss(),
        Some(Decimal::from(92)),
        "three ticks below the aim"
    );
    assert_eq!(
        preview.bracket.take_profit(),
        Some(Decimal::from(98)),
        "and three ticks above it - the same distance, which is the 1:1"
    );

    // One notch back down.
    paper.handle_chart_input(&ruler_frame(chart, &scale, aim, -50.0));
    assert_eq!(paper.ruler_notches, 2);
    let preview = paper.cmd_preview.expect("still aiming");
    assert_eq!(preview.bracket.stop_loss(), Some(Decimal::from(93)));
    assert_eq!(preview.bracket.take_profit(), Some(Decimal::from(97)));
}

/// One unreadable offset box stands the whole bracket down, rather than
/// projecting the other one on its own.
///
/// The two boxes are a pair. A ticket whose stop says `abc` and whose
/// target says `5` must project nothing: showing a target-only bracket
/// there would put protection on the chart that the trader never typed,
/// and it is the trade they would take it for. `ticket_bracket` read both
/// with `?` and one bad box failed the call; the seam carries that as
/// `TicketForm::offsets` being all-or-nothing, and this is what says so.
#[test]
fn one_unreadable_offset_stands_the_whole_bracket_down() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let reference = Decimal::from(100);

    paper.stop_offset_text = "2".to_owned();
    paper.profit_offset_text = "5".to_owned();
    let both = paper.armed_bracket(Side::Buy, reference, Decimal::ONE);
    assert_eq!(both.stop_loss(), Some(Decimal::from(98)));
    assert_eq!(both.take_profit(), Some(Decimal::from(105)));

    paper.stop_offset_text = "abc".to_owned();
    let spoiled = paper.armed_bracket(Side::Buy, reference, Decimal::ONE);
    assert_eq!(
        spoiled.stop_loss(),
        None,
        "an unreadable stop projects no stop"
    );
    assert_eq!(
        spoiled.take_profit(),
        None,
        "and takes the readable target down with it"
    );
}

/// A short's ruler mirrors: the stop goes above the aim, the target
/// below, and both stay the same distance from it.
#[test]
fn the_ruler_mirrors_for_a_sell() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // y = 250 is 95: below the mark, so a sell arms as a stop there.
    let aim = egui::pos2(400.0, 250.0);
    let mut frame = ruler_frame(chart, &scale, aim, 50.0);
    frame.modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    // Four notches, one roll each.
    for _ in 0..4 {
        paper.handle_chart_input(&frame);
    }

    let preview = paper.cmd_preview.expect("the sell aim is up");
    assert_eq!(preview.side, Side::Sell);
    assert_eq!(
        preview.bracket.stop_loss(),
        Some(Decimal::from(99)),
        "above a short"
    );
    assert_eq!(
        preview.bracket.take_profit(),
        Some(Decimal::from(91)),
        "below it"
    );
}

/// The wheel with no aim up is the chart's, not the ruler's.
#[test]
fn without_an_aim_the_wheel_is_left_to_the_chart() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let mut frame = ruler_frame(chart, &scale, egui::pos2(400.0, 250.0), 150.0);
    frame.modifiers = egui::Modifiers::default();
    paper.handle_chart_input(&frame);
    assert!(paper.cmd_preview.is_none(), "no modifier, no aim");
    assert!(!paper.consumed_scroll(), "so the wheel is not the ruler's");
    assert_eq!(paper.ruler_notches, 0);
}

/// What the ruler shows is what the click places.
#[test]
fn the_order_the_click_places_carries_the_rulers_bracket() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 250.0);
    for _ in 0..5 {
        paper.handle_chart_input(&ruler_frame(chart, &scale, aim, 50.0));
    }
    assert_eq!(paper.ruler_notches, 5);

    let mut press = ruler_frame(chart, &scale, aim, 0.0);
    press.primary_pressed = true;
    press.primary_down = true;
    assert!(paper.handle_chart_input(&press), "the aim placed");

    let order = &paper.working_orders()[0];
    assert_eq!(order.price, Some(Decimal::from(95)));
    assert_eq!(
        order.bracket.stop_loss(),
        Some(Decimal::from(90)),
        "the stop the ruler was showing"
    );
    assert_eq!(
        order.bracket.take_profit(),
        Some(Decimal::from(100)),
        "and the target beside it"
    );
}

fn chart_and_scale(lo: f64, hi: f64) -> (egui::Rect, PriceScale) {
    let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    (chart, PriceScale::from_range(lo, hi, 0.0, 400.0))
}

/// One pointer frame mid-chart, where nothing right-anchored lives.
fn frame<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    y: f32,
    pressed: bool,
    down: bool,
    released: bool,
) -> ChartInput<'a> {
    frame_at(chart, scale, egui::pos2(400.0, y), pressed, down, released)
}

/// One pointer frame at an exact position — for the controls that live
/// against the plot's right edge, which `frame`'s mid-chart x misses.
fn frame_at<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    pointer: egui::Pos2,
    pressed: bool,
    down: bool,
    released: bool,
) -> ChartInput<'a> {
    ChartInput {
        chart,
        scale: Some(scale),
        pointer: Some(pointer),
        primary_pressed: pressed,
        primary_down: down,
        primary_released: released,
        modifiers: egui::Modifiers::default(),
        canvas_claimed: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    }
}

/// A `frame` with held modifiers and a free pointer — the cmd-trading
/// gesture's shape of input.
fn cmd_frame<'a>(
    chart: egui::Rect,
    scale: &'a PriceScale,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    pressed: bool,
) -> ChartInput<'a> {
    ChartInput {
        chart,
        scale: Some(scale),
        pointer: Some(pointer),
        primary_pressed: pressed,
        primary_down: pressed,
        primary_released: false,
        modifiers,
        canvas_claimed: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    }
}

/// Escape (routed through the app's escape stack) cancels exactly one
/// paper interaction per press, and a cancelled drag submits nothing.
#[test]
fn escape_cancels_the_armed_placement_then_the_grabbed_line() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.account.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    assert!(paper.cancel_interaction(), "the armed placement dies first");
    assert!(paper.account.armed.is_none());
    assert!(!paper.cancel_interaction(), "nothing left to cancel");

    paper.stop_offset_text = "10".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // Grab the stop at 90 (y = 300), cancel, then let go: no submit.
    assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false)));
    assert!(paper.cancel_interaction(), "the grabbed line is released");
    assert!(!paper.handle_chart_input(&frame(chart, &scale, 250.0, false, false, true)));
    assert_eq!(
        paper
            .account
            .venue
            .position()
            .expect("still long")
            .stop_loss,
        Some(Decimal::from(90)),
        "a cancelled drag never moves the stop"
    );
}

#[test]
fn an_armed_click_places_the_order_at_the_clicked_price_and_disarms() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.account.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    let (chart, scale) = chart_and_scale(90.0, 110.0);
    // y = 300 sits at price 95 on this scale.
    let consumed = paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false));
    assert!(consumed, "the armed click never reaches the chart pan");
    assert!(
        paper.account.armed.is_none(),
        "a successful placement disarms"
    );
    assert_eq!(paper.account.venue.working_orders().len(), 1);
    assert_eq!(
        paper.account.venue.working_orders()[0].price,
        Some(Decimal::from(95))
    );
}

#[test]
fn a_rejected_armed_click_stays_armed_and_teaches() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.account.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    let (chart, scale) = chart_and_scale(90.0, 110.0);
    // y = 100 sits at price 105 — a buy limit above the market.
    let consumed = paper.handle_chart_input(&frame(chart, &scale, 100.0, true, true, false));
    assert!(consumed);
    assert!(
        paper.account.armed.is_some(),
        "the user clicks again after the toast"
    );
    assert!(paper.account.venue.working_orders().is_empty());
    assert!(
        paper.account.peek_toast().is_some(),
        "the refusal explains itself"
    );
}

/// The lines say what a press would do before it happens (audit M3/M4):
/// draggable levels wear the resize cursor, an entry line with a
/// missing leg offers the create-drag, and empty tape asks nothing.
#[test]
fn hover_cursors_announce_draggable_and_creatable_lines() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.stop_offset_text = "10".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 300.0), chart, &scale),
        Some(egui::CursorIcon::ResizeVertical),
        "the stop at 90 sits at y 300 and drags"
    );
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 200.0), chart, &scale),
        Some(egui::CursorIcon::ResizeVertical),
        "the entry at 100 offers the missing take profit by drag"
    );
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 40.0), chart, &scale),
        None,
        "empty tape belongs to the chart"
    );
}

#[test]
fn the_cmd_gesture_previews_and_a_label_click_places_the_order() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let ctrl = egui::Modifiers {
        command: true,
        ..Default::default()
    };
    let both = egui::Modifiers {
        shift: true,
        command: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);

    // Hold buy above the mark: a stop. Below: a limit.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 100.0),
        shift,
        false,
    ));
    let preview = paper.cmd_preview.expect("preview above the mark");
    assert_eq!((preview.side, preview.kind), (Side::Buy, EntryKind::Stop));
    assert_eq!(preview.price, Decimal::from(110));
    assert_eq!(
        preview.pointer,
        egui::pos2(400.0, 100.0),
        "the aim is the hand"
    );
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 300.0),
        shift,
        false,
    ));
    let preview = paper.cmd_preview.expect("preview below the mark");
    assert_eq!((preview.side, preview.kind), (Side::Buy, EntryKind::Limit));

    // The sell key mirrors the table.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 100.0),
        ctrl,
        false,
    ));
    let preview = paper.cmd_preview.expect("sell above the mark");
    assert_eq!((preview.side, preview.kind), (Side::Sell, EntryKind::Limit));

    // Both keys is ambiguous, no key is no gesture.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 100.0),
        both,
        false,
    ));
    assert!(paper.cmd_preview.is_none(), "ambiguity shows nothing");
    paper.handle_chart_input(&frame(chart, &scale, 100.0, false, false, false));
    assert!(paper.cmd_preview.is_none(), "no key, no line");

    // The click places exactly what the preview said, wherever in the
    // plot it lands, through the same path as the right-click menu —
    // a label that rides the pointer can never be landed on, so the
    // held modifier is the deliberate act.
    let aim = egui::pos2(120.0, 300.0);
    assert!(
        paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, true)),
        "the click is the gesture's"
    );
    let orders = paper.working_orders();
    assert_eq!(orders.len(), 1, "the click rested the order");
    assert_eq!(orders[0].side, Side::Buy);
    assert_eq!(orders[0].price, Some(Decimal::from(90)));

    // No modifier, no preview, no order: an unmodified click on empty
    // canvas belongs to the chart.
    assert!(
        !paper.handle_chart_input(&frame(chart, &scale, 100.0, true, true, false)),
        "a bare click is nobody's order"
    );
    assert_eq!(paper.working_orders().len(), 1, "still just the one");

    // Disabled means invisible.
    paper.set_cmd_trading(CmdTradingSettings {
        enabled: false,
        ..Default::default()
    });
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 100.0),
        shift,
        false,
    ));
    assert!(paper.cmd_preview.is_none(), "the toggle hides the gesture");
}

/// Off-screen render proof (for environments where no window can
/// present): the preview paints a dashed line, a label carrying
/// side+kind+qty, and the gutter chip with the snapped price.
#[test]
fn the_cmd_preview_paints_line_label_and_price_chip() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 300.0),
        shift,
        false,
    ));
    assert!(paper.cmd_preview.is_some(), "the held key builds a preview");

    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let paint = PaintCtx {
            painter: &painter,
            chart_rect: chart,
            tag_right: chart.right(),
            axis_x: chart.right(),
            scale: &scale,
            reserved_chip_y: None,
            pointer: Some(egui::pos2(400.0, 300.0)),
        };
        paper.draw_cmd_preview(&paint);
    });
    let shapes = format!("{:?}", output.shapes);
    assert!(shapes.contains("BUY"), "the label names the side: {shapes}");
    assert!(
        shapes.contains("90"),
        "the gutter chip carries the snapped price"
    );
    let segments = shapes.matches("LineSegment").count();
    assert!(
        segments >= 8,
        "a dashed line paints as many short segments, got {segments}"
    );
}

/// The aim label rides the pointer instead of parking at the right
/// edge — the whole point of the change — while the dashed line still
/// reaches the axis, so label and price chip stay one statement.
#[test]
fn the_cmd_label_follows_the_pointer_and_the_line_reaches_the_axis() {
    let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    let mut previous: Option<f32> = None;
    for x in [200.0_f32, 400.0, 600.0] {
        let (start, end, label) = cmd_preview_layout(band, band.right(), egui::pos2(x, 250.0));
        assert_eq!(end.x, 800.0, "the line always reaches the axis");
        assert_eq!(start.x, x, "the line starts under the cursor");
        assert_eq!(
            label.right(),
            x - CMD_LABEL_CURSOR_GAP_PX,
            "the label rides a fixed gap off the pointer"
        );
        assert_eq!(label.width(), CMD_LABEL_WIDTH_PX);
        assert_eq!(label.center().y, 250.0);
        assert!(
            !label.contains(egui::pos2(x, 250.0)),
            "never under the cursor it belongs to"
        );
        if let Some(previous) = previous {
            assert!(label.left() > previous, "moving right moves the label");
        }
        previous = Some(label.left());
    }
}

/// The tape lane is not a wall. Its divider ends the *band* — where a
/// press can still land — and the label stops there with it, but the
/// line carries on to the axis, because a gap across the widest lane on
/// the chart is exactly where a trader loses the order.
#[test]
fn the_aim_line_crosses_the_live_lane_to_the_axis() {
    // A chart 1000 wide whose live tape lane opens at 700: the band the
    // aim lays out against stops at the divider, the gutter does not.
    let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(700.0, 400.0));
    let axis_x = 1000.0;
    let (start, end, label) = cmd_preview_layout(band, axis_x, egui::pos2(400.0, 250.0));
    assert_eq!(
        end.x, axis_x,
        "the line spans the lane instead of stopping at its divider"
    );
    assert!(
        end.x > band.right(),
        "and it is the lane it crosses, not the plot it started in"
    );
    assert_eq!(start.x, 400.0, "it still starts under the cursor");
    assert!(
        label.right() <= band.right(),
        "the label stays inside the band a press can reach: {label:?}"
    );
}

/// A pane with no live lane hands the same x twice, and the line must
/// not double back on itself.
#[test]
fn the_aim_line_ends_at_the_axis_with_no_lane_open() {
    let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
    let (_, end, _) = cmd_preview_layout(band, 800.0, egui::pos2(400.0, 250.0));
    assert_eq!(end.x, 800.0, "band right and axis coincide");
    // A gutter reported left of the plot (a pane mid-resize) must never
    // shorten the line to a stub pointing the wrong way.
    let (start, end, _) = cmd_preview_layout(band, 10.0, egui::pos2(400.0, 250.0));
    assert!(end.x >= start.x, "never a line running backwards");
}

/// The two edges: near the left one the label flips to the pointer's
/// right rather than leaving the band, and near the right one the line
/// starts further left so there is still a line to read.
#[test]
fn the_cmd_layout_clamps_at_both_edges_of_the_band() {
    let band = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));

    let pointer = egui::pos2(20.0, 250.0);
    let (_, _, label) = cmd_preview_layout(band, band.right(), pointer);
    assert!(label.left() >= band.left(), "never off the left edge");
    assert_eq!(
        label.left(),
        pointer.x + CMD_LABEL_CURSOR_GAP_PX,
        "no room on the left, so it flips right"
    );
    assert!(!label.contains(pointer), "still clear of the cursor");

    let pointer = egui::pos2(780.0, 250.0);
    let (start, end, label) = cmd_preview_layout(band, band.right(), pointer);
    assert!(label.right() <= band.right(), "never off the right edge");
    assert_eq!(
        end.x - start.x,
        CMD_LINE_MIN_PX,
        "close to the axis the line starts further left"
    );
    assert!(!label.contains(pointer), "still clear of the cursor");

    // A band narrower than the label plus its gap cannot hold both; it
    // parks at the left edge rather than running off-plot to the left.
    let sliver = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 400.0));
    let (start, _, label) = cmd_preview_layout(sliver, sliver.right(), egui::pos2(50.0, 250.0));
    assert_eq!(label.left(), sliver.left(), "a sliver parks at its edge");
    assert_eq!(start.x, sliver.left(), "and the line spans what there is");
}

/// Paint and press read one geometry: the label the layout hands the
/// painter is the label the pointer that produced it was measured
/// against (the overlay-controls rule), and the preview carries that
/// exact pointer rather than re-deriving it.
#[test]
fn the_cmd_preview_carries_the_pointer_the_paint_lays_out_from() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(180.0, 300.0);
    paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, false));
    let preview = paper.cmd_preview.expect("the held key builds a preview");
    assert_eq!(preview.pointer, aim, "the aim is the pointer, whole");
    let (_, _, label) = cmd_preview_layout(chart, chart.right(), preview.pointer);
    assert_eq!(
        label.right(),
        aim.x - CMD_LABEL_CURSOR_GAP_PX,
        "the paint lays out from that same pointer"
    );
}

/// An annotation under the pointer keeps its pixel: no aim paints and
/// no click places there, so Shift+drag on a channel corner still
/// levels it. One gate governs paint, cursor and press together — the
/// label can never promise an order the press will not make.
#[test]
fn the_aim_yields_the_pixel_to_a_drawing_already_under_it() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let aim = egui::pos2(400.0, 300.0);

    let over_drawing = ChartInput {
        chart,
        scale: Some(&scale),
        pointer: Some(aim),
        primary_pressed: true,
        primary_down: true,
        primary_released: false,
        modifiers: shift,
        canvas_claimed: true,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    };
    assert!(
        !paper.handle_chart_input(&over_drawing),
        "the press belongs to the drawing"
    );
    assert!(paper.cmd_preview.is_none(), "and nothing aims over it");
    assert!(paper.working_orders().is_empty(), "so nothing was placed");
    assert_eq!(
        paper.hover_cursor(aim, chart, &scale),
        None,
        "no hand promising a click that will not happen"
    );

    // The very same pixel, one step off the line: the aim is back.
    assert!(paper.handle_chart_input(&cmd_frame(chart, &scale, aim, shift, true)));
    assert_eq!(paper.working_orders().len(), 1, "clear canvas, order rests");
}

/// A pointer from another pane's band paints nothing here: the label
/// rides an x, so laying it out against a band that does not hold that
/// x would put a click target off the end of the plot.
#[test]
fn the_aim_paints_only_in_the_band_it_was_aimed_in() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(700.0, 300.0),
        shift,
        false,
    ));
    assert!(paper.cmd_preview.is_some(), "aimed on this band");

    // The same simulator drawn against a narrower band — the other
    // pane of a split, whose right edge stops short of that x.
    let other = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(400.0, 400.0));
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        paper.draw_cmd_preview(&PaintCtx {
            painter: &painter,
            chart_rect: other,
            tag_right: other.right(),
            axis_x: other.right(),
            scale: &scale,
            reserved_chip_y: None,
            pointer: Some(egui::pos2(700.0, 300.0)),
        });
    });
    let shapes = format!("{:?}", output.shapes);
    assert!(
        !shapes.contains("BUY"),
        "a foreign pointer paints no label: {shapes}"
    );
}

/// The capture hook: a side, and optionally where along the band to
/// park the hand the run does not have.
#[test]
fn the_cmd_preview_hook_parses_a_side_and_an_optional_x() {
    assert_eq!(
        CmdPreviewForce::parse("buy"),
        Some(CmdPreviewForce {
            side: Side::Buy,
            x_fraction: None
        })
    );
    assert_eq!(
        CmdPreviewForce::parse("SELL@0.15"),
        Some(CmdPreviewForce {
            side: Side::Sell,
            x_fraction: Some(0.15)
        })
    );
    assert_eq!(
        CmdPreviewForce::parse("buy@9"),
        Some(CmdPreviewForce {
            side: Side::Buy,
            x_fraction: Some(1.0)
        }),
        "out of range clamps into the band"
    );
    for bad in [
        "buy@left", "buy@nan", "buy@NaN", "buy@inf", "buy@", "buy@0,15",
    ] {
        assert_eq!(
            CmdPreviewForce::parse(bad),
            Some(CmdPreviewForce {
                side: Side::Buy,
                x_fraction: None
            }),
            "a bad fraction still paints, mid-band: {bad}"
        );
    }
    assert_eq!(CmdPreviewForce::parse("hold"), None);
}

/// The parked x is what a capture run states, so it wins over a real
/// pointer that in such a run is nobody's aim — and without it the
/// hook keeps its old mid-band park.
#[test]
fn the_forced_preview_aims_where_the_hook_says() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.cmd_preview_force = Some(CmdPreviewForce {
        side: Side::Sell,
        x_fraction: Some(0.25),
    });
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(700.0, 100.0),
        egui::Modifiers::default(),
        false,
    ));
    let preview = paper.cmd_preview.expect("the hook forces a preview");
    assert_eq!(preview.side, Side::Sell);
    assert_eq!(preview.pointer.x, 200.0, "a quarter into an 800px band");
    assert_eq!(preview.pointer.y, 100.0, "the real hand still sets price");

    paper.cmd_preview_force = Some(CmdPreviewForce {
        side: Side::Sell,
        x_fraction: None,
    });
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(700.0, 100.0),
        egui::Modifiers::default(),
        false,
    ));
    assert_eq!(
        paper.cmd_preview.expect("still forced").pointer.x,
        700.0,
        "with no stated x the real pointer is left alone"
    );
}

/// Paint one frame of the paper layer and return its shape dump — the
/// off-screen render proof the tag tests read.
fn layer_shapes(
    paper: &PaperTrading,
    chart: egui::Rect,
    scale: &PriceScale,
    pointer: Option<egui::Pos2>,
) -> String {
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        paper.draw_layer(
            &painter,
            chart,
            chart.right(),
            chart.right(),
            scale,
            None,
            pointer,
        );
    });
    format!("{:?}", output.shapes)
}

/// A resting order rests as a pill and opens under the pointer: the
/// full tag used to sit over the candles at the live price all
/// session. The pill still names side, kind and size — an order line
/// is accent-coloured whatever its side, so dropping the word would
/// leave the chart unable to say what waits there. Off-screen render
/// proof.
#[test]
fn a_resting_order_tag_is_a_pill_until_the_pointer_reaches_it() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // A buy limit at 90 — y 300 on this scale.
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id.0;

    // The frame the pointer arrives in is the frame that decides; the
    // paint reads that decision rather than re-asking its own pointer.
    let frame_at = |paper: &mut PaperTrading, y: f32| {
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            egui::pos2(400.0, y),
            egui::Modifiers::default(),
            false,
        ));
        layer_shapes(paper, chart, &scale, Some(egui::pos2(400.0, y)))
    };

    let resting = frame_at(&mut paper, 60.0);
    assert!(
        resting.contains("BUY LMT 1"),
        "the pill still names side, kind and size: {resting}"
    );
    assert!(
        !resting.contains(&format!("#{id}")),
        "the id waits until you mean to act on it: {resting}"
    );
    assert!(
        !resting.contains("@ 90"),
        "the price is the gutter chip's job: {resting}"
    );
    assert!(!resting.contains('×'), "and no ✕ over the candles");

    let opened = frame_at(&mut paper, 300.0);
    assert!(
        opened.contains(&format!("#{id} BUY LMT 1 @ 90")),
        "reaching for it states the order whole: {opened}"
    );
    assert!(opened.contains('×'), "…and offers the cancel: {opened}");
}

/// The ✕ and its press are one thing, checked the only way that means
/// anything: sweep a pointer down the ✕ column, and for every stop ask
/// **the painter** whether a ✕ came out and **`control_at`** whether a
/// cancel is offered. Both sides run off one `handle_chart_input`, so
/// this fails the moment they are handed different pointers, different
/// rects, or different `dragged` terms again.
#[test]
fn a_tag_offers_its_cancel_exactly_while_it_paints_one() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // Two orders: one near the top edge, where `clamp_tag_center`
    // pushes the tag off its own line and the two rows part company,
    // and one mid-plot where they coincide. Above the mark a buy rests
    // as a stop, below it as a limit.
    assert!(paper.place_resting(Side::Buy, EntryKind::Stop, 119.5));
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let x = chart.right() - TAG_GAP_PX - TAG_BUTTON_PX / 2.0;
    let mut painted_anywhere = false;
    for step in -4_i16..=84 {
        let pointer = egui::pos2(x, f32::from(step) * 5.0);
        paper.handle_chart_input(&cmd_frame(
            chart,
            &scale,
            pointer,
            egui::Modifiers::default(),
            false,
        ));
        let painted = layer_shapes(&paper, chart, &scale, Some(pointer)).contains('×');
        let pressable = matches!(
            paper.control_at(pointer, chart, &scale),
            Some(PaperControl::CancelOrder(_))
        );
        assert_eq!(painted, pressable, "at y {}", pointer.y);
        painted_anywhere |= painted;
    }
    assert!(painted_anywhere, "the sweep crossed a ✕ at all");
}

/// The press side is fed a pointer the paint side never sees — the
/// pane nulls `hover_pos` over its own chrome while `latest_pos`
/// survives — so the ✕'s offer must come from the frame's *input*, not
/// from whatever pointer each side happens to hold.
#[test]
fn a_cancel_offered_this_frame_survives_a_paint_with_no_pointer() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let on_row = egui::pos2(chart.right() - TAG_GAP_PX - TAG_BUTTON_PX / 2.0, 300.0);
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        on_row,
        egui::Modifiers::default(),
        false,
    ));
    assert!(
        layer_shapes(&paper, chart, &scale, None).contains('×'),
        "the paint follows the frame's decision, not its own pointer"
    );
    assert!(matches!(
        paper.control_at(on_row, chart, &scale),
        Some(PaperControl::CancelOrder(_))
    ));

    // Pointer off the row: neither side offers anything.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(on_row.x, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    assert!(!layer_shapes(&paper, chart, &scale, Some(on_row)).contains('×'));
    assert!(paper.control_at(on_row, chart, &scale).is_none());
}

/// Switched off, the layer is unpainted — so it is also untouchable:
/// the aim's target is the whole plot, and an invisible plot-sized
/// order button is the worst kind of hidden control.
#[test]
fn a_hidden_layer_paints_nothing_and_takes_no_press() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let hidden = ChartInput {
        chart,
        scale: Some(&scale),
        pointer: Some(egui::pos2(400.0, 200.0)),
        primary_pressed: true,
        primary_down: true,
        primary_released: false,
        modifiers: shift,
        canvas_claimed: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: false,
    };
    assert!(
        !paper.handle_chart_input(&hidden),
        "the press is the chart's"
    );
    assert!(paper.cmd_preview.is_none(), "and nothing is aimed");
    assert_eq!(
        paper.working_orders().len(),
        1,
        "no order rested through a hidden layer"
    );
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 300.0), chart, &scale),
        None,
        "and no cursor promises an invisible control"
    );
}

/// The aim stands down wherever something concrete already holds the
/// pixel — this module's own lines and ✕s included. Otherwise holding
/// the modifier while reaching for a stop rests a new order on top of
/// it, and the hand cursor promises exactly that.
#[test]
fn the_aim_stands_down_over_paper_lines_controls_and_an_armed_ticket() {
    let shift = egui::Modifiers {
        shift: true,
        ..Default::default()
    };
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.stop_offset_text = "10".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);

    // y 300 is the stop at 90 — a line a press would grab.
    let on_stop = egui::pos2(400.0, 300.0);
    paper.handle_chart_input(&cmd_frame(chart, &scale, on_stop, shift, false));
    assert!(paper.cmd_preview.is_none(), "the stop line keeps its pixel");
    assert_eq!(
        paper.hover_cursor(on_stop, chart, &scale),
        Some(egui::CursorIcon::ResizeVertical),
        "and the cursor still says so"
    );
    assert!(
        paper.handle_chart_input(&cmd_frame(chart, &scale, on_stop, shift, true)),
        "the press is paper's"
    );
    assert_eq!(
        paper.drag,
        PaperDrag::Leg {
            owner: BracketTarget::Position,
            leg: Leg::StopLoss,
        },
        "it grabbed the stop instead of resting an order"
    );
    assert!(paper.working_orders().is_empty());
    paper.cancel_interaction();

    // An armed placement is an intent already stated.
    paper.account.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 100.0),
        shift,
        false,
    ));
    assert!(
        paper.cmd_preview.is_none(),
        "the armed ticket keeps the click"
    );
    assert!(paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 320.0),
        shift,
        true
    )));
    assert!(
        paper.account.armed.is_none(),
        "the armed placement fired and disarmed"
    );
    let orders = paper.working_orders();
    assert_eq!(orders.len(), 1);
    assert_eq!(
        orders[0].kind,
        EntryKind::Limit,
        "the kind the ticket armed"
    );
}

/// A forced aim is a capture fixture: it paints so a screenshot has
/// something to show, and it never places — a run with nobody at the
/// keyboard is holding no modifier, and its stray clicks must not
/// write orders into a journal.
#[test]
fn a_forced_aim_paints_but_never_places() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    paper.cmd_preview_force = Some(CmdPreviewForce {
        side: Side::Buy,
        x_fraction: Some(0.5),
    });
    let aim = egui::pos2(400.0, 300.0);
    assert!(!paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        aim,
        egui::Modifiers::default(),
        true
    )));
    assert!(paper.cmd_preview.is_some(), "it still paints");
    assert!(paper.working_orders().is_empty(), "and never places");
    assert_eq!(
        paper.hover_cursor(aim, chart, &scale),
        None,
        "no hand promising a click that does nothing"
    );
}

/// A dragged order keeps every field: a trader repricing one is
/// reading the number they are moving, and the pointer is on the line
/// they grabbed, not on the tag.
#[test]
fn a_dragged_order_keeps_its_full_statement() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id;
    paper.drag = PaperDrag::Order(id);
    paper.drag_price = Some(88.0);
    // The frame decides; the paint reads. A button-free frame leaves
    // the drag exactly where it was.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    let shapes = layer_shapes(&paper, chart, &scale, None);
    assert!(
        shapes.contains(&format!("#{} BUY LMT 1 @ 88", id.0)),
        "the drag reads the price it is moving: {shapes}"
    );
    assert!(
        !shapes.contains('×'),
        "a moving order offers no cancel, as before: {shapes}"
    );
}

/// The capture hook rests a rung around the mark on the first print:
/// one order each side, so whichever way the tape moves a tag is still
/// on screen for the shutter, and it happens at once rather than 220
/// prints in.
#[test]
fn the_orders_hook_rests_a_rung_either_side_of_the_mark() {
    let mut paper = PaperTrading::new();
    paper.orders_demo = Some(2);
    paper.on_trade(&print(1, 100_000));
    assert!(paper.orders_demo.is_none(), "placed once, never again");
    let prices: Vec<_> = paper
        .working_orders()
        .iter()
        .map(|order| (order.side, order.price.expect("a limit has a price")))
        .collect();
    assert_eq!(
        prices,
        vec![
            (Side::Buy, Decimal::from(99_940)),
            (Side::Sell, Decimal::from(100_060)),
            (Side::Buy, Decimal::from(99_880)),
            (Side::Sell, Decimal::from(100_120)),
        ],
        "two rungs, each side, stepping out from the mark"
    );
    paper.on_trade(&print(2, 100_000));
    assert_eq!(paper.working_orders().len(), 4, "a second print adds none");
}

/// The bracket hook dresses those same rungs, so a working order's two
/// dashed legs are photographable without a hand to drag them into
/// being. Each leg lands on the correct side of the *order's own*
/// price, which is what the venue validates against — a hook that
/// placed one the venue refuses would photograph an empty chart.
#[test]
fn the_bracket_hook_dresses_every_rung_it_rests() {
    let mut paper = PaperTrading::new();
    paper.orders_demo = Some(1);
    paper.order_bracket_demo = true;
    paper.on_trade(&print(1, 100_000));

    let orders = paper.working_orders();
    assert_eq!(orders.len(), 2, "one rung, both sides");
    for order in orders {
        let price = order.price.expect("a limit has a price");
        let stop = order.bracket.stop_loss().expect("a stop rides along");
        let target = order.bracket.take_profit().expect("and a target");
        match order.side {
            Side::Buy => {
                assert!(stop < price, "a long's stop sits below its entry");
                assert!(target > price, "and its target above");
            }
            Side::Sell => {
                assert!(stop > price, "a short's stop sits above its entry");
                assert!(target < price, "and its target below");
            }
        }
    }
}

/// A coarsely quoted mark rounds 6 bp to nothing: both legs would
/// price *at* the mark, the simulator would refuse every one of them,
/// and the run would photograph an empty chart with nothing to explain
/// it. The step floors at one unit of the instrument's own precision.
#[test]
fn the_orders_hook_still_rests_on_an_integer_quoted_mark() {
    let mut paper = PaperTrading::new();
    paper.orders_demo = Some(2);
    // 620 * 0.0006 = 0.372, which rounds to zero at scale 0.
    paper.on_trade(&print(1, 620));
    let prices: Vec<_> = paper
        .working_orders()
        .iter()
        .map(|order| (order.side, order.price.expect("a limit has a price")))
        .collect();
    assert_eq!(
        prices,
        vec![
            (Side::Buy, Decimal::from(619)),
            (Side::Sell, Decimal::from(621)),
            (Side::Buy, Decimal::from(618)),
            (Side::Sell, Decimal::from(622)),
        ],
        "one tick per rung, and the rungs stay apart"
    );
    assert!(paper.orders_demo.is_none(), "orders rested, hook disarmed");
}

/// Nothing rested means the hook stays armed: disarming before the
/// simulator has accepted anything is exactly how a silent empty
/// capture happens.
#[test]
fn the_orders_hook_stays_armed_until_something_rests() {
    let mut paper = PaperTrading::new();
    paper.orders_demo = Some(1);
    // No mark yet: nothing to place around, nothing consumed.
    paper.rest_capture_orders();
    assert_eq!(paper.orders_demo, Some(1), "no mark, still armed");
    paper.on_trade(&print(1, 100_000));
    assert_eq!(paper.working_orders().len(), 2);
    assert!(paper.orders_demo.is_none());
}

/// The capture hook opens every tag with nobody at the mouse — the
/// pill's open form is otherwise unreachable from a scripted run.
#[test]
fn the_order_hover_hook_opens_the_tag_with_no_pointer() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id.0;
    assert!(
        !layer_shapes(&paper, chart, &scale, None).contains(&format!("#{id}")),
        "no hand, no open tag"
    );
    paper.order_hover_force = true;
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    assert!(
        layer_shapes(&paper, chart, &scale, None).contains(&format!("#{id} BUY LMT 1 @ 90")),
        "the hook supplies the hand"
    );
}

/// One hover, two surfaces: the dock row already lifted the chart
/// line, and now it opens the tag too.
#[test]
fn hovering_the_dock_row_opens_the_chart_tag() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let id = paper.working_orders()[0].id;
    paper.hovered_order = Some(id);
    // The dock draws before the canvas, so the frame that carries the
    // row's hover is the frame the chart reads.
    paper.handle_chart_input(&cmd_frame(
        chart,
        &scale,
        egui::pos2(400.0, 60.0),
        egui::Modifiers::default(),
        false,
    ));
    assert!(
        layer_shapes(&paper, chart, &scale, None).contains(&format!("#{} BUY LMT 1 @ 90", id.0)),
        "the row's hover reaches the chart"
    );
}

/// A band too short to hold a tag is reachable — `split_panes` carves
/// the indicator strips out of the plot with no floor of its own — and
/// `f32::clamp` panics rather than saturating once its bounds cross.
/// Every tag, every ✕ hit-test and the aim's own layout run through
/// this, so the panic would take a live session down.
#[test]
fn a_band_too_short_for_a_tag_centres_it_instead_of_panicking() {
    // Shorter than a tag, and flat: the two cases that cross the bounds.
    assert_eq!(clamp_tag_center(5.0, 0.0, 10.0), 5.0);
    assert_eq!(clamp_tag_center(99.0, 40.0, 40.0), 40.0);
    assert_eq!(clamp_tag_center(-99.0, 0.0, TAG_HEIGHT_PX), 10.0);
    // And with room, it still clamps exactly as before.
    assert_eq!(clamp_tag_center(0.0, 0.0, 400.0), 10.0);
    assert_eq!(clamp_tag_center(400.0, 0.0, 400.0), 390.0);
    assert_eq!(clamp_tag_center(200.0, 0.0, 400.0), 200.0);

    // The paint and the press both survive it end to end.
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let sliver = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 12.0));
    let scale = PriceScale::from_range(80.0, 120.0, 0.0, 12.0);
    assert!(paper.place_resting(Side::Buy, EntryKind::Limit, 90.0));
    let pointer = egui::pos2(400.0, 6.0);
    paper.handle_chart_input(&cmd_frame(
        sliver,
        &scale,
        pointer,
        egui::Modifiers::default(),
        false,
    ));
    let _ = layer_shapes(&paper, sliver, &scale, Some(pointer));
    let _ = paper.control_at(pointer, sliver, &scale);
    let _ = cmd_preview_layout(sliver, sliver.right(), pointer);
}

#[test]
fn cmd_modifier_tokens_round_trip_and_state_defaults_fill_gaps() {
    for modifier in CmdModifier::ALL {
        assert_eq!(CmdModifier::parse(modifier.as_str()), Some(modifier));
    }
    assert_eq!(CmdModifier::parse("hyper"), None);
    let state = crate::paper_state::PaperState {
        cmd_trading_enabled: Some(false),
        cmd_buy_modifier: Some("alt".to_owned()),
        cmd_sell_modifier: Some("hyper".to_owned()),
        ..Default::default()
    };
    let settings = CmdTradingSettings::from_state(&state);
    assert!(!settings.enabled);
    assert_eq!(settings.buy, CmdModifier::Alt);
    assert_eq!(
        settings.sell,
        CmdModifier::Ctrl,
        "an unknown token falls back to the default"
    );
}

#[test]
fn dragging_the_stop_loss_reprices_it_on_release() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.stop_offset_text = "10".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    assert_eq!(
        paper.account.venue.position().expect("long").stop_loss,
        Some(Decimal::from(90)),
    );
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // The stop at 90 sits at y = 300; grab it, pull to 95 (y = 250), drop.
    assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, true, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 250.0, false, false, true)));
    assert_eq!(
        paper
            .account
            .venue
            .position()
            .expect("still long")
            .stop_loss,
        Some(Decimal::from(95)),
        "the drop resubmitted the bracket at the dragged price"
    );
}

/// The TradingView gesture: pull away from the entry line and the
/// missing bracket leg is born on release — the profit side makes a
/// take profit, the losing side a stop.
#[test]
fn dragging_from_the_entry_line_creates_the_missing_leg() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // Grab the entry at 100 (y = 200), pull up to 105 (y = 150), drop.
    assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, false, true)));
    let position = paper.account.venue.position().expect("still long");
    assert_eq!(
        position.take_profit,
        Some(Decimal::from(105)),
        "above a long is the profit side"
    );
    assert_eq!(
        position.avg_price,
        Decimal::from(100),
        "the entry itself never moves"
    );
    // The same pull downward births the stop.
    assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true)));
    let position = paper.account.venue.position().expect("still long");
    assert_eq!(position.stop_loss, Some(Decimal::from(90)));
    assert_eq!(position.take_profit, Some(Decimal::from(105)), "untouched");
}

#[test]
fn a_fully_bracketed_entry_line_blocks_the_gesture_but_never_moves() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.stop_offset_text = "10".to_owned();
    paper.profit_offset_text = "10".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    assert_eq!(
        paper.hover_cursor(egui::pos2(400.0, 200.0), chart, &scale),
        Some(egui::CursorIcon::NotAllowed),
        "both legs exist, so their own lines are the handles"
    );
    // Grabbing the entry still consumes the gesture (the chart must not
    // pan under it) but repositions nothing.
    assert!(paper.handle_chart_input(&frame(chart, &scale, 200.0, true, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 150.0, false, false, true)));
    let position = paper.account.venue.position().expect("long");
    assert_eq!(position.avg_price, Decimal::from(100));
    assert_eq!(position.stop_loss, Some(Decimal::from(90)), "untouched");
    assert_eq!(position.take_profit, Some(Decimal::from(110)), "untouched");
    // Empty space is not ours: the press falls through to the chart.
    assert!(
        !paper.handle_chart_input(&frame(chart, &scale, 40.0, true, true, false)),
        "a press far from every line belongs to the pan"
    );
}

/// The ✕ on a working order's chart tag: the hit is pure geometry from
/// the live scale — no hover, no prior paint, no cached rect that goes
/// stale while a live chart autoscales — it wins over the armed click
/// (which used to eat it), and it never reads as a drag on the line.
#[test]
fn the_order_tags_close_is_geometric_and_beats_the_armed_click() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    let events = paper.account.dispatch(Command::PlaceLimit {
        side: Side::Buy,
        quantity: Decimal::ONE,
        price: Decimal::from(95),
        bracket: Bracket::none(),
        cancel_at: None,
        flat_only: false,
    });
    paper.account.handle_events(events);
    // The trap that used to swallow every chart click.
    paper.account.armed = Some(ArmedPlacement {
        side: Side::Sell,
        kind: EntryKind::Stop,
    });
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    let close = close_button_rect(
        chart.right(),
        clamp_tag_center(scale.y(95.0), chart.top(), chart.bottom()),
    );
    let press = ChartInput {
        chart,
        scale: Some(&scale),
        pointer: Some(close.center()),
        primary_pressed: true,
        primary_down: true,
        primary_released: false,
        modifiers: egui::Modifiers::default(),
        canvas_claimed: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    };
    assert!(paper.handle_chart_input(&press), "the ✕ owns the press");
    assert!(
        paper.account.venue.working_orders().is_empty(),
        "the order is gone and the armed click placed nothing"
    );
    assert!(
        paper.account.armed.is_some(),
        "the armed placement neither fired nor died"
    );
    assert_eq!(paper.drag, PaperDrag::None, "and nothing started dragging");
}

/// A bracket handle press starts the create-drag — its rect is the
/// hit-test's own geometry beside the entry tag, above the line for
/// the profit side and below it for the losing side.
#[test]
fn a_bracket_handle_press_starts_the_create_drag() {
    let mut paper = PaperTrading::new();
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (chart, scale) = chart_and_scale(80.0, 120.0);
    // A long's SL handle sits below the entry line (above = false).
    let entry_center = clamp_tag_center(scale.y(100.0), chart.top(), chart.bottom());
    let handle = bracket_handle_rect(chart.right(), entry_center, false);
    let press = ChartInput {
        chart,
        scale: Some(&scale),
        pointer: Some(handle.center()),
        primary_pressed: true,
        primary_down: true,
        primary_released: false,
        modifiers: egui::Modifiers::default(),
        canvas_claimed: false,
        scroll_y: 0.0,
        middle_pressed: false,
        layer_visible: true,
    };
    assert!(
        paper.handle_chart_input(&press),
        "the handle owns the press"
    );
    assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, true, false)));
    assert!(paper.handle_chart_input(&frame(chart, &scale, 300.0, false, false, true)));
    assert_eq!(
        paper.account.venue.position().expect("long").stop_loss,
        Some(Decimal::from(90)),
        "the handle drag placed the stop"
    );
}

/// One long, then every label the entry buttons can wear: the button
/// must disclose close-or-reverse, because the quantity deciding it
/// lives in a tab the toolbar never shows.
#[test]
fn entry_labels_disclose_what_the_press_would_do() {
    let mut paper = PaperTrading::new();
    assert_eq!(
        paper.entry_label(Side::Buy),
        "BUY 1",
        "flat is a plain entry"
    );
    paper.qty_text = "x".to_owned();
    assert_eq!(
        paper.entry_label(Side::Sell),
        "SELL",
        "an unparseable quantity promises nothing"
    );
    paper.qty_text = "2".to_owned();
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));

    paper.qty_text = "1".to_owned();
    assert_eq!(paper.entry_label(Side::Buy), "BUY 1 (adds to 3)");
    assert_eq!(paper.entry_label(Side::Sell), "SELL 1 (closes 1 of 2)");
    paper.qty_text = "2".to_owned();
    assert_eq!(paper.entry_label(Side::Sell), "SELL 2 (closes)");
    paper.qty_text = "5".to_owned();
    assert_eq!(
        paper.entry_label(Side::Sell),
        "SELL 5 (reverses to short 3)"
    );
}

/// The status cell answers the reported question — "am I in a trade?" —
/// not just "how many points".
#[test]
fn the_status_cell_distinguishes_open_from_flat() {
    let mut paper = PaperTrading::new();
    assert!(paper.status_cell().is_none(), "untouched owes no line");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let (text, _) = paper.status_cell().expect("a position is state");
    assert_eq!(text, "SIM LONG 1 · 0 pts");
    paper.on_trade(&print(2, 105));
    let (text, sign) = paper.status_cell().expect("still open");
    assert_eq!(text, "SIM LONG 1 · +5 pts");
    assert_eq!(sign, std::cmp::Ordering::Greater);

    paper.close_position();
    paper.on_trade(&print(3, 107));
    let (text, sign) = paper.status_cell().expect("history keeps the cell");
    assert_eq!(text, "SIM +7 pts · flat");
    assert_eq!(sign, std::cmp::Ordering::Greater);
    assert!(paper.close_button_label().is_none(), "flat has no close");
}

#[test]
fn the_close_button_names_the_position_it_exits() {
    let mut paper = PaperTrading::new();
    paper.qty_text = "3".to_owned();
    paper.seed(&print(0, 100));
    paper.market(Side::Sell);
    paper.on_trade(&print(1, 100));
    assert_eq!(paper.close_button_label().as_deref(), Some("Close 3 SHORT"));
    let summary = paper.position_summary().expect("open");
    assert_eq!(summary.side, Side::Sell);
    assert_eq!(summary.quantity, Decimal::from(3));
    assert_eq!(summary.avg_price, Decimal::from(100));
}

/// Reverse flips side and size in one market order, and the form's
/// protective offsets ride along to the new entry.
#[test]
fn reverse_flips_the_position_with_the_forms_bracket() {
    let mut paper = PaperTrading::new();
    paper.qty_text = "2".to_owned();
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    paper.stop_offset_text = "5".to_owned();
    paper.reverse_position();
    paper.on_trade(&print(2, 100));
    let position = paper.account.venue.position().expect("reversed, not flat");
    assert_eq!(position.side, Side::Sell);
    assert_eq!(position.quantity, Decimal::from(2));
    assert_eq!(
        position.stop_loss,
        Some(Decimal::from(105)),
        "the new short is protected by the form's offset"
    );
}

/// The chip dodge: lines keep their price, chips clear the last-price
/// row by the minimum, and the fill-moment tie steps down.
#[test]
fn paper_chips_dodge_the_last_price_chip_never_the_line() {
    // No reservation, or far enough away: the chip stays at its line.
    assert_eq!(dodged_chip_y(100.0, None, 0.0, 400.0), 100.0);
    assert_eq!(dodged_chip_y(100.0, Some(200.0), 0.0, 400.0), 100.0);
    // Inside the band: pushed just clear, towards its own side.
    assert_eq!(
        dodged_chip_y(210.0, Some(200.0), 0.0, 400.0),
        200.0 + CHIP_CLEAR_PX
    );
    assert_eq!(
        dodged_chip_y(190.0, Some(200.0), 0.0, 400.0),
        200.0 - CHIP_CLEAR_PX
    );
    // The fill moment: entry == last price, and the chip steps down.
    assert_eq!(
        dodged_chip_y(200.0, Some(200.0), 0.0, 400.0),
        200.0 + CHIP_CLEAR_PX
    );
    // Never dodged out of the pane.
    assert_eq!(dodged_chip_y(398.0, Some(399.0), 0.0, 400.0), 383.0);
}

#[test]
fn snapping_uses_the_instruments_learned_precision() {
    let mut paper = PaperTrading::new();
    paper.seed(&Trade {
        agg_id: 1,
        timestamp_ms: 1000,
        price: Decimal::new(10325, 2), // 103.25 → two decimal places
        quantity: Decimal::ONE,
        side: Side::Buy,
    });
    assert_eq!(paper.account.snap(101.23456), Decimal::new(10123, 2));
    // A cent-printing instrument keeps cents, whatever round numbers
    // come after: precision the tape has shown does not go away because
    // the next print happened to land on a whole one.
    paper.on_trade(&print(2, 103));
    assert_eq!(paper.account.snap(101.23456), Decimal::new(10123, 2));

    // An instrument that only ever prints whole points snaps to them -
    // its own instance, because a tick belongs to one market.
    let mut whole = PaperTrading::new();
    whole.seed(&print(1, 182_035));
    assert_eq!(whole.account.snap(182_036.7), Decimal::from(182_037));
}

/// The session file [`the_journal_bytes_are_fixed`] must open, named
/// from the first close's own timestamp.
const JOURNAL_GOLDEN_FILE: &str = "19700101-000002.csv";

/// Every byte [`the_journal_bytes_are_fixed`] must write. Recorded from
/// a run against this file *before* the policy half moved out, and not
/// touched since.
///
/// SHA-256 of these bytes:
/// `ab74859479f2f1e471dfb5a1556a15d2891d440c7c119db49c0e2ad64be094d6`.
/// The hash is written down so that the "before" and the "after" of the
/// extraction can be compared by someone who is reading neither this
/// file's history nor the diff — a reviewer, or the trader.
const JOURNAL_GOLDEN: &str = concat!(
    "# quantick-trades 2\n",
    "# symbol=GOLDEN\n",
    "# source=live\n",
    "opened_ms,closed_ms,side,quantity,entry_price,exit_price,pnl_points,",
    "exit_reason,entry_agg_id,exit_agg_id,mae_points,mfe_points\n",
    // A long taken at the market and closed by hand: 100 to 105.
    "1000,2000,long,1,100,105,5,manual,1,2,0,5\n",
    // A short, the same way: 105 down to 103.
    "3000,4000,short,1,105,103,2,manual,3,4,0,2\n",
    // A long stopped out. The entry is 103 and the ticket's stop offset
    // is 2, so the stop sits at 101 and the tape reaches it.
    "5000,6000,long,1,103,101,-2,stop_loss,5,6,2,0\n",
    // A long taken at its target: entry 101, offset 6, filled at 107.
    "7000,8000,long,1,101,107,6,take_profit,7,8,0,6\n",
);

/// One fixed tape, one journal, asserted byte for byte.
///
/// This is the money path's golden. It is written *before* the policy
/// half of this file moves into `paper_account.rs`, and its expected
/// bytes do not change when it does — that is the whole point. An
/// extraction that alters a fill rule, a bracket price, the risk lock's
/// arithmetic, a rounding or the journal's own format fails here rather
/// than in front of the trader, and it fails naming the byte.
///
/// The tape is fixed in every respect the writer reads: prices and
/// quantities are exact decimals, every timestamp is derived from the
/// print's own `agg_id` rather than a clock, and the session file's name
/// comes from the first close's `closed_ms`. So the file name is
/// asserted too — a session that opened a differently named file would
/// still hold the right rows, and the trader would still have lost the
/// trade in a folder nobody reads.
///
/// Four round trips, chosen to cover the four ways a position ends:
/// a long closed by hand, a short closed by hand, a long stopped out,
/// and a long taken at its target. The last two go through the ticket's
/// offset text, so the bracket arithmetic is under the golden and not
/// only the flat manual close.
#[test]
fn the_journal_bytes_are_fixed() {
    // Its own scratch folder, carrying a run token and removed with the
    // value: a reused process id would otherwise hand this run the last
    // one's journal, and the golden would fail on a file it never wrote.
    let dir = crate::scratch::ScratchDir::new("paper-journal-golden");
    let mut paper = PaperTrading::new();
    paper.account.dir = dir.path().to_path_buf();
    paper.set_symbol("GOLDEN");
    paper.seed(&print(0, 100));

    // 1. A long, entered at the market and closed by hand: +5.
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));

    // 2. A short, the same way: 105 down to 103 is +2.
    paper.market(Side::Sell);
    paper.on_trade(&print(3, 105));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(4, 103));

    // 3. A long with protection, stopped out. The offsets are the
    //    ticket's own text, so `ticket_bracket` and the rounding that
    //    follows it are under the golden with everything else.
    paper.stop_offset_text = "2".to_owned();
    paper.profit_offset_text = "6".to_owned();
    paper.market(Side::Buy);
    paper.on_trade(&print(5, 103));
    paper.on_trade(&print(6, 101));

    // 4. A long with the same protection, taken at its target.
    paper.market(Side::Buy);
    paper.on_trade(&print(7, 101));
    paper.on_trade(&print(8, 107));

    let folder = dir.path().join("GOLDEN");
    let mut files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .map(|entry| entry.path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 1, "one session, one file: {files:?}");
    assert_eq!(
        files[0].file_name().and_then(|name| name.to_str()),
        Some(JOURNAL_GOLDEN_FILE),
        "the session file is named from the first close, not from a clock"
    );

    let text = std::fs::read_to_string(&files[0]).expect("readable");
    assert_eq!(
        text, JOURNAL_GOLDEN,
        "the journal's bytes moved; the money path is not what it was"
    );
}

#[test]
fn closed_trades_journal_to_one_session_file_and_reload() {
    let dir = crate::scratch::ScratchDir::new("paper-journal-test");
    let mut paper = PaperTrading::new();
    paper.account.dir = dir.path().to_path_buf();
    paper.set_symbol("TESTUSDT");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));
    // A second round trip appends to the same session file.
    paper.market(Side::Sell);
    paper.on_trade(&print(3, 105));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(4, 103));

    let folder = dir.join("TESTUSDT");
    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .collect();
    assert_eq!(files.len(), 1, "one session, one file");
    let text = std::fs::read_to_string(files[0].path()).expect("readable");
    let parsed = history::parse(&text).expect("valid history");
    assert_eq!(parsed.symbol.as_deref(), Some("TESTUSDT"));
    assert_eq!(parsed.trades.len(), 2);
    assert!(parsed.problems.is_empty());
    assert_eq!(parsed.trades[0].pnl_points, Decimal::from(5));
    assert_eq!(parsed.trades[1].pnl_points, Decimal::from(2));

    let history = load_history(&dir, Some("TESTUSDT"), &[]);
    assert_eq!(history.rows.len(), 2);
    assert_eq!(report_from_history(&history).net_points, Decimal::from(7));
    assert_eq!(history.files, 1);
    assert_eq!(history.unreadable_files, 0);
}

#[test]
fn a_second_session_adds_a_file_and_never_touches_the_first() {
    let dir = crate::scratch::ScratchDir::new("paper-accumulate-test");
    // Session one: a round trip closing at t=2s.
    let mut first = PaperTrading::new();
    first.account.dir = dir.path().to_path_buf();
    first.set_symbol("ACCUM");
    first.seed(&print(0, 100));
    first.market(Side::Buy);
    first.on_trade(&print(1, 100));
    let events = first.account.dispatch(Command::ClosePosition);
    first.account.handle_events(events);
    first.on_trade(&print(2, 103));
    let folder = dir.join("ACCUM");
    let first_file = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .next()
        .expect("one session file")
        .path();
    let first_bytes = std::fs::read(&first_file).expect("readable");

    // Session two: a fresh host — a restart — closing hours later.
    let mut second = PaperTrading::new();
    second.account.dir = dir.path().to_path_buf();
    second.set_symbol("ACCUM");
    second.seed(&print(10_000, 200));
    second.market(Side::Sell);
    second.on_trade(&print(10_001, 200));
    let events = second.account.dispatch(Command::ClosePosition);
    second.account.handle_events(events);
    second.on_trade(&print(10_002, 190));

    let files: Vec<_> = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .collect();
    assert_eq!(files.len(), 2, "each session opens its own file");
    assert_eq!(
        std::fs::read(&first_file).expect("still readable"),
        first_bytes,
        "the earlier session's file is byte-for-byte untouched"
    );
    let history = load_history(&dir, Some("ACCUM"), &[]);
    assert_eq!(history.files, 2);
    assert_eq!(history.rows.len(), 2, "both sessions' trades load");
}

#[test]
fn a_timeline_reset_journals_the_flatten_and_clears_the_form_state() {
    let dir = crate::scratch::ScratchDir::new("paper-reset-test");
    let mut paper = PaperTrading::new();
    paper.account.dir = dir.path().to_path_buf();
    paper.set_symbol("RESETX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    paper.account.armed = Some(ArmedPlacement {
        side: Side::Buy,
        kind: EntryKind::Limit,
    });
    paper.on_timeline_reset();
    assert!(paper.account.venue.position().is_none());
    assert!(
        paper.account.armed.is_none(),
        "an armed click dies with the timeline"
    );
    assert!(
        paper.account.peek_toast().is_some(),
        "the flatten is never silent"
    );
    let history = load_history(&dir, Some("RESETX"), &[]);
    assert_eq!(history.rows.len(), 1);
    assert_eq!(
        report_from_history(&history).trades,
        1,
        "the reset exit is a real, journaled trade"
    );
}

// The stored-pick-vs-configured-base precedence now lives in
// `paper_home::chosen`, tested there beside the documents default.

/// The panel's folder picker retargets everything downstream: the next
/// close opens a new session file under the new home, and the ledger
/// and report re-read from it. Files already written stay put.
#[test]
fn switching_the_trades_dir_retargets_journal_ledger_and_report() {
    let dir_a = crate::scratch::ScratchDir::new("paper-dir-a");
    let dir_b = crate::scratch::ScratchDir::new("paper-dir-b");
    let mut paper = PaperTrading::new();
    paper.account.dir = dir_a.path().to_path_buf();
    paper.set_symbol("SWITCHX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 103));
    assert!(
        paper.account.journal_path.is_some(),
        "the close journaled under A"
    );
    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    assert!(paper.account.report_state().saved_rows_loaded().is_some());

    paper.account.set_trades_dir(dir_b.path().to_path_buf());
    assert_eq!(paper.account.trades_dir(), dir_b.path());
    assert!(
        paper.account.journal_path.is_none(),
        "the next close opens a new session file under B"
    );
    assert!(
        paper.account.report_state().saved_rows_loaded().is_none(),
        "the ledger re-reads from the new home"
    );
    assert!(
        paper.account.peek_toast().is_some(),
        "the switch is never silent"
    );
    assert!(
        dir_a.join("SWITCHX").exists(),
        "files already written stay where they are"
    );
}

/// The ledger's cache reads every saved file except the live session's
/// own (its trades are already in the simulator), and remembers which
/// symbol each row came from.
#[test]
fn the_ledger_cache_excludes_the_live_session_file() {
    let dir = crate::scratch::ScratchDir::new("paper-ledger-test");
    let mut paper = PaperTrading::new();
    paper.account.dir = dir.path().to_path_buf();
    paper.set_symbol("LEDGX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));
    assert!(paper.account.journal_path.is_some(), "the close journaled");

    // An earlier session's file, written by hand beside the live one.
    let trade = ClosedTrade {
        side: Side::Sell,
        quantity: Decimal::ONE,
        entry_price: Decimal::from(200),
        exit_price: Decimal::from(195),
        opened_ms: 10,
        closed_ms: 20,
        pnl_points: Decimal::from(5),
        exit_reason: quantick_sim::ExitReason::Manual,
        entry_agg_id: Some(1),
        exit_agg_id: Some(2),
        mae_points: Some(Decimal::ZERO),
        mfe_points: Some(Decimal::from(5)),
    };
    let mut text = history::write_header("LEDGX", history::SessionSource::Live);
    text.push_str(&history::write_trade(&trade));
    std::fs::write(dir.join("LEDGX").join("20200101-000000.csv"), text)
        .expect("the earlier session file writes");

    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    let cache = paper
        .account
        .report_state()
        .saved_rows_loaded()
        .expect("loaded");
    assert_eq!(
        cache.len(),
        1,
        "the live session's file is excluded, the earlier one loads"
    );
    assert_eq!(cache[0].symbol, "LEDGX");
    assert_eq!(cache[0].trade, trade);
    assert_eq!(cache[0].source, Some(history::SessionSource::Live));
}

#[test]
fn a_replay_rerun_lands_beside_its_first_run_never_inside_it() {
    let dir = crate::scratch::ScratchDir::new("paper-rerun-test");
    // The same recording replayed twice: identical prints, identical
    // venue times, so both sessions derive the same file stamp.
    for _ in 0..2 {
        let mut paper = PaperTrading::new();
        paper.account.dir = dir.path().to_path_buf();
        paper.set_symbol("RERUN");
        paper
            .account_mut()
            .set_session_source(history::SessionSource::Replay);
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(2, 103));
    }

    let folder = dir.join("RERUN");
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .expect("the symbol folder exists")
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .collect();
    names.sort();
    assert_eq!(
        names.len(),
        2,
        "the second run opened its own file instead of appending duplicates"
    );
    assert!(names[1].contains(".rerun-1."), "{names:?}");
    let history = load_history(&dir, Some("RERUN"), &[]);
    assert_eq!(history.rows.len(), 2);
    assert!(
        history
            .rows
            .iter()
            .all(|row| row.source == Some(history::SessionSource::Replay)),
        "both files carry the replay source"
    );
}

#[test]
fn the_ledger_never_lists_this_sessions_trades_twice_after_a_retarget() {
    // Hunt-confirmed: close live, flip to replay (what a same-symbol
    // replay open does), reload the ledger — the live session's file
    // must stay excluded, or every trade counts twice in the totals
    // and the export.
    let mut paper = PaperTrading::new();
    paper.set_symbol("DUPX");
    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));
    assert_eq!(paper.account.venue.closed_trades().len(), 1);

    paper
        .account_mut()
        .set_session_source(history::SessionSource::Replay);
    paper.on_timeline_reset();
    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    let cache = paper
        .account
        .report_state()
        .saved_rows_loaded()
        .expect("loaded");
    assert_eq!(
        cache.len(),
        0,
        "the session's own files stay excluded across the retarget"
    );
}

/// A revealed page must survive the ledger's own lazy first load.
/// `QUANTICK_LEDGER_PAGES` sets the page count during construction,
/// long before the Trades tab is first drawn; the load that tab
/// triggers used to reset it, so the hook reached page one and the
/// state it exists to photograph was unreachable.
#[test]
fn a_revealed_page_survives_the_ledgers_lazy_first_load() {
    let dir = crate::scratch::ScratchDir::new("paper-pages-test");
    let mut paper = PaperTrading::new();
    paper.account.dir = dir.path().to_path_buf();
    paper.set_symbol("PAGEX");

    paper.account.report_state_mut().autostart_ledger_pages(3);
    assert_eq!(paper.account.report_state().revealed_pages(), 3);
    // What the first `draw_trades_tab` does before painting a row.
    assert!(paper.account.report_state().saved_rows_loaded().is_none());
    {
        let (state, env) = paper.report_parts();
        state.reload_ledger(&env)
    };
    assert_eq!(
        paper.account.report_state().revealed_pages(),
        3,
        "the lazy load must not retire the hook's page count"
    );
    // And what every tab does on every drain: sync the journal to its
    // symbol. This runs on the frame, so it must not retire the page
    // either — the hook set it before the feed had a symbol at all.
    paper.set_symbol("PAGEY");
    assert_eq!(
        paper.account.report_state().revealed_pages(),
        3,
        "the per-frame symbol sync must not retire the page"
    );

    // A *scope* change is the one thing that does reset it: a deep
    // page cannot survive a list it was never counted against.
    {
        let (state, env) = paper.report_parts();
        state.rescope_ledger(&env)
    };
    assert_eq!(paper.account.report_state().revealed_pages(), 1);
}

#[test]
fn the_report_scopes_by_symbol_folder_on_disk() {
    let dir = crate::scratch::ScratchDir::new("paper-symbols-test");
    for (symbol, id0, price) in [("AAAUSDT", 0, 100), ("BBBUSDT", 100, 200)] {
        let mut paper = PaperTrading::new();
        paper.account.dir = dir.path().to_path_buf();
        paper.set_symbol(symbol);
        paper.seed(&print(id0, price));
        paper.market(Side::Buy);
        paper.on_trade(&print(id0 + 1, price));
        let events = paper.account.dispatch(Command::ClosePosition);
        paper.account.handle_events(events);
        paper.on_trade(&print(id0 + 2, price + 5));
    }

    assert_eq!(
        list_symbol_folders(&dir),
        vec!["AAAUSDT".to_owned(), "BBBUSDT".to_owned()],
        "the combo lists every traded asset"
    );
    let all = load_history(&dir, None, &[]);
    assert_eq!(all.rows.len(), 2, "All symbols reads both journals");
    let symbols: Vec<&str> = all.rows.iter().map(|row| row.symbol.as_str()).collect();
    assert_eq!(symbols, vec!["AAAUSDT", "BBBUSDT"]);
    let one = load_history(&dir, Some("BBBUSDT"), &[]);
    assert_eq!(one.rows.len(), 1, "a symbol scope reads only its folder");
    assert_eq!(one.rows[0].symbol, "BBBUSDT");
}

#[test]
fn a_close_refreshes_an_open_report_by_itself() {
    let utc = TzOffset::new(0);
    let mut paper = PaperTrading::new();
    paper.set_symbol("FRESH");
    {
        let (state, env) = paper.report_parts();
        state.open(&env)
    };
    paper.account.report_state_mut().ensure_report_view(utc);
    assert!(
        paper
            .account
            .report_state()
            .view_rows()
            .expect("view")
            .is_empty(),
        "nothing saved yet"
    );

    paper.seed(&print(0, 100));
    paper.market(Side::Buy);
    paper.on_trade(&print(1, 100));
    let events = paper.account.dispatch(Command::ClosePosition);
    paper.account.handle_events(events);
    paper.on_trade(&print(2, 105));

    paper.account.report_state_mut().ensure_report_view(utc);
    let view = paper.account.report_state().view_rows().expect("refreshed");
    assert_eq!(
        view.len(),
        1,
        "the close re-read the journal without a manual refresh"
    );
}
