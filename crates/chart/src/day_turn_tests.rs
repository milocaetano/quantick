use super::*;

const BRT: TzOffset = TzOffset::new(-180);

#[test]
fn a_day_reads_weekday_and_date_and_names_the_month_when_it_turns() {
    assert_eq!(day_text(CivilDate::from_ymd(2026, 9, 29)), "Tue 29");
    assert_eq!(day_text(CivilDate::from_ymd(2026, 10, 1)), "Thu 01 Oct");
    assert_eq!(pinned_text(CivilDate::from_ymd(2026, 9, 28)), "Mon 28 Sep");
}

#[test]
fn a_turn_reads_when_the_old_day_ended_and_the_new_one_opened() {
    // 2026-09-28 21:24 UTC is 18:24 in São Paulo; 2026-09-29 12:00 UTC is 09:00.
    let ended = 1_790_630_640_000;
    let opened = 1_790_683_200_000;
    assert_eq!(ended_text(ended, BRT), "18:24");
    assert_eq!(opened_suffix(opened, BRT), " 09:00");
    assert_eq!(ended_text(-60_000, TzOffset::new(0)), "23:59");
}

#[test]
fn a_turns_widths_are_its_labels_in_monospace() {
    let turn = DayTurn {
        slot: 4,
        date: CivilDate::from_ymd(2026, 9, 29),
        ended_ms: Some(0),
        opened_ms: Some(0),
    };
    assert_eq!(
        TurnWidths::of(&turn, 2.0),
        TurnWidths {
            ended: Some(10.0),
            // `Tue 29 09:00`
            dated_opened: Some(24.0),
            dated: 12.0,
        }
    );
    let unknown = DayTurn {
        ended_ms: None,
        opened_ms: None,
        ..turn
    };
    let widths = TurnWidths::of(&unknown, 2.0);
    assert_eq!((widths.ended, widths.dated_opened), (None, None));
}

const WIDTHS: TurnWidths = TurnWidths {
    ended: Some(30.0),
    dated_opened: Some(70.0),
    dated: 40.0,
};
const ANYWHERE: fn(f32, f32) -> bool = |_, _| true;

#[test]
fn with_room_the_end_time_sits_left_of_the_tick_and_the_rest_right() {
    let placed = place_turn(200.0, WIDTHS, 0.0, 400.0, ANYWHERE).expect("room for all");
    assert_eq!(
        placed,
        TurnPlacement {
            ended_at: Some(200.0 - DAY_LABEL_GAP_PX - 30.0),
            with_opened: true,
        }
    );
}

#[test]
fn short_of_space_the_end_time_goes_first_then_the_start_time_never_the_date() {
    // The label before reaches close to the tick: no room on the left.
    let placed = place_turn(200.0, WIDTHS, 180.0, 400.0, ANYWHERE).expect("date fits");
    assert_eq!(
        placed,
        TurnPlacement {
            ended_at: None,
            with_opened: true,
        }
    );
    // The next tick is close too: only the date is left.
    let limit = 200.0 + DAY_LABEL_GAP_PX + 50.0;
    let placed = place_turn(200.0, WIDTHS, 180.0, limit, ANYWHERE).expect("date fits");
    assert_eq!(
        placed,
        TurnPlacement {
            ended_at: None,
            with_opened: false,
        }
    );
    // Not even the date: nothing is written, the tick stands alone.
    let limit = 200.0 + DAY_LABEL_GAP_PX + 20.0;
    assert_eq!(place_turn(200.0, WIDTHS, 0.0, limit, ANYWHERE), None);
}

#[test]
fn the_end_time_is_dropped_before_the_start_time_even_when_both_would_fit_alone() {
    // The right side holds only the date; the left has room for the end
    // time, but the end time goes first, so it is not written alone.
    let limit = 200.0 + DAY_LABEL_GAP_PX + 50.0;
    let placed = place_turn(200.0, WIDTHS, 0.0, limit, ANYWHERE).expect("date fits");
    assert_eq!(
        placed,
        TurnPlacement {
            ended_at: None,
            with_opened: false,
        }
    );
}

#[test]
fn the_pointer_chip_claims_a_time_like_a_date() {
    // The chip sits over the end time's span only.
    let free = |start: f32, _width: f32| start > 190.0;
    let placed = place_turn(200.0, WIDTHS, 0.0, 400.0, free).expect("right side is free");
    assert_eq!(
        placed,
        TurnPlacement {
            ended_at: None,
            with_opened: true,
        }
    );
}

#[test]
fn a_turn_without_a_known_end_writes_the_date_and_start() {
    let widths = TurnWidths {
        ended: None,
        ..WIDTHS
    };
    let placed = place_turn(200.0, widths, 0.0, 400.0, ANYWHERE).expect("room");
    assert_eq!(
        placed,
        TurnPlacement {
            ended_at: None,
            with_opened: true,
        }
    );
}

#[test]
fn the_strip_pins_the_first_date_and_skips_a_tick_under_the_chip() {
    let ticks = [(200.0, WIDTHS), (400.0, WIDTHS)];
    let plan = plan_strip(0.0, 600.0, Some(50.0), &ticks, ANYWHERE, |x| x != 400.0);
    assert_eq!(plan.pinned_at, Some(DAY_LABEL_GAP_PX));
    assert!(plan.ticks[0].drawn && plan.ticks[0].labels.is_some());
    assert_eq!(
        plan.ticks[1],
        TickPlan {
            drawn: false,
            labels: None,
        }
    );
}

#[test]
fn a_time_label_stands_aside_only_where_a_date_is_written() {
    let reserved = [(100.0, 140.0)];
    assert!(reserved_by(120.0, 150.0, &reserved), "overlapping");
    assert!(reserved_by(141.0, 170.0, &reserved), "inside the gap");
    assert!(!reserved_by(150.0, 190.0, &reserved), "clear of it");
    assert!(!reserved_by(40.0, 90.0, &reserved), "clear before it");
    assert!(!reserved_by(120.0, 150.0, &[]), "no dates, no gaps");
}
