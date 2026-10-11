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
fn a_turns_widths_bound_its_labels_as_the_window_lays_them_out() {
    let turn = DayTurn {
        slot: 4,
        date: CivilDate::from_ymd(2026, 9, 29),
        ended_ms: Some(0),
        opened_ms: Some(0),
    };
    // A 1.5 character is rounded up to 2 per character, plus 1 for the
    // galley's edge: never narrower than the label once drawn.
    assert_eq!(
        TurnWidths::of(&turn, 1.5),
        TurnWidths {
            ended: Some(11.0),
            // `Tue 29 09:00`
            dated_opened: Some(25.0),
            dated: 13.0,
        }
    );
    // The pinned date on the same rule: `Mon 28 Sep`.
    assert_eq!(label_width("Mon 28 Sep", 1.5), 21.0);
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

#[test]
fn an_end_time_never_crosses_the_tick_before_it() {
    // The first turn's date does not fit before the second tick, so nothing
    // is written right of it; the second turn's end time would still reach
    // back across the first tick.
    let ticks = [(100.0, WIDTHS), (120.0, WIDTHS)];
    let plan = plan_strip(0.0, 600.0, None, &ticks, ANYWHERE, |_| true);
    assert_eq!(plan.ticks[0].labels, None);
    assert_eq!(
        plan.ticks[1].labels,
        Some(TurnPlacement {
            ended_at: None,
            with_opened: true,
        })
    );
}

#[test]
fn a_bar_spanning_midnight_writes_no_end_time() {
    let date = CivilDate::from_ymd(2026, 9, 29);
    // 23:50 and 23:58 on the 28th in São Paulo: the old day's last close.
    let open = 1_790_650_200_000;
    let close = 1_790_650_680_000;
    let opened = Some(1_790_683_200_000);
    let turn = DayTurn::new(7, date, Some((open, close)), opened, BRT);
    assert_eq!(turn.ended_ms, Some(close));
    // Closed at 00:05 on the 29th: that is the new day, not when the old one
    // ended, so no end time is written rather than a misleading one.
    let past_midnight = 1_790_651_100_000;
    let turn = DayTurn::new(7, date, Some((open, past_midnight)), opened, BRT);
    assert_eq!(turn.ended_ms, None);
    // The new day's first prints are in that bar too, so the next bar's open
    // is not when the day opened: the date is written alone.
    assert_eq!(turn.opened_ms, None);
}

#[test]
fn a_tick_bar_straddling_the_session_break_writes_the_date_alone() {
    // WIN tick:50, bar 49530: opens Tue 06 18:31:13 and closes Wed 07
    // 09:00:59 in São Paulo. The turn read `09:00 | Wed 7 09:00`; the end
    // is the new day's time, so it is dropped, and the day's first trades
    // (09:00:00-09:00:59) are in that bar, so the turn bar's 09:01 is not
    // when the day opened either.
    let at = |date: CivilDate, seconds: i64| date.start_ms(BRT) + seconds * 1000;
    let (tue, wed) = (
        CivilDate::from_ymd(2026, 10, 6),
        CivilDate::from_ymd(2026, 10, 7),
    );
    let open = at(tue, 18 * 3600 + 31 * 60 + 13);
    let close = at(wed, 9 * 3600 + 59);
    let opened = at(wed, 9 * 3600 + 60);
    let turn = DayTurn::new(49_531, wed, Some((open, close)), Some(opened), BRT);
    assert_eq!(turn.ended_ms, None);
    assert_eq!(turn.opened_ms, None);
}

#[test]
fn a_time_bar_crossing_local_midnight_writes_the_date_alone() {
    // A 2h bucket from 23:00 Tue to 01:00 Wed in São Paulo, last print at
    // 00:59: neither 00:59 nor the next bucket's 01:00 is when a day turned.
    let at = |date: CivilDate, seconds: i64| date.start_ms(BRT) + seconds * 1000;
    let (tue, wed) = (
        CivilDate::from_ymd(2026, 10, 6),
        CivilDate::from_ymd(2026, 10, 7),
    );
    let open = at(tue, 23 * 3600);
    let close = at(wed, 59 * 60);
    let opened = at(wed, 3600);
    let turn = DayTurn::new(12, wed, Some((open, close)), Some(opened), BRT);
    assert_eq!(turn.ended_ms, None);
    assert_eq!(turn.opened_ms, None);
    // The same bucket closing before midnight keeps both times.
    let before_midnight = at(tue, 23 * 3600 + 59 * 60);
    let turn = DayTurn::new(12, wed, Some((open, before_midnight)), Some(opened), BRT);
    assert_eq!(turn.ended_ms, Some(before_midnight));
    assert_eq!(turn.opened_ms, Some(opened));
}

/// 2026-10-09 13:57:03 UTC: 10:57:03 in UTC-3.
const OCT_9_1357_UTC: i64 = 1_791_554_223_000;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

/// Below a day the pointer's tag names the day as well as the second, in
/// the display zone: near midnight UTC that is the zone's own date.
#[test]
fn the_pointer_tag_names_the_day_in_the_display_zone() {
    assert_eq!(
        pointer_time_label(OCT_9_1357_UTC, BRT),
        "Fri 09 Oct '26 10:57:03"
    );
    // 01:30 UTC on the 10th is still 22:30 on the 9th in UTC-3.
    let late = OCT_9_1357_UTC + 11 * HOUR_MS + 33 * 60_000 - 3_000;
    assert_eq!(pointer_time_label(late, BRT), "Fri 09 Oct '26 22:30:00");
    let utc = TzOffset::new(0);
    assert_eq!(pointer_time_label(late, utc), "Sat 10 Oct '26 01:30:00");
    // One width on every day: the chip is measured from its own text.
    let wide = pointer_time_label(OCT_9_1357_UTC + 60 * DAY_MS, BRT);
    assert_eq!(wide.len(), pointer_time_label(late, BRT).len());
}

/// The first label painted in a new local day writes the day, in a new
/// month the month, in a new year the year; the rest write the clock.
#[test]
fn a_tick_label_names_the_day_month_or_year_it_turns_into() {
    let full = TimeLabelFormat::Full;
    let at = OCT_9_1357_UTC;
    assert_eq!(
        TickLabels::new(BRT, full, None, &[]).label_at(at),
        "10:57:03"
    );
    let short = TimeLabelFormat::Short;
    assert_eq!(
        TickLabels::new(BRT, short, Some(at - HOUR_MS), &[]).label_at(at),
        "10:57"
    );
    // 02:00 UTC on the 10th is 23:00 on the 9th in UTC-3: no day turn yet
    // there, though UTC has crossed midnight.
    let late = at + 12 * HOUR_MS + 2 * 60_000 + 57_000;
    let mut brt = TickLabels::new(BRT, full, Some(at), &[]);
    assert_eq!(brt.label_at(late), "23:00:00");
    assert_eq!(brt.label_at(late + HOUR_MS), "10");
    assert_eq!(brt.label_at(late + 2 * HOUR_MS), "01:00:00");
    let utc = TzOffset::new(0);
    assert_eq!(
        TickLabels::new(utc, full, Some(at), &[]).label_at(late),
        "10"
    );
    // Into November, and into 2027.
    let nov_1 = late + HOUR_MS + 22 * DAY_MS;
    let mut month = TickLabels::new(BRT, full, Some(nov_1 - HOUR_MS), &[]);
    assert_eq!(month.label_at(nov_1), "Nov");
    let jan_1 = nov_1 + 61 * DAY_MS;
    assert_eq!(month.label_at(jan_1), "2027");
    for text in ["10", "Nov", "2027"] {
        assert!(text.len() <= TimeLabelFormat::Short.sample().len());
    }
}

/// The comparison is with the last label *painted*: labels skipped between
/// two painted ones never consume the turn, so the first label painted in
/// the new day still names it, however many were skipped before it.
#[test]
fn a_turn_is_named_on_the_first_label_painted_in_it() {
    let full = TimeLabelFormat::Full;
    let mut labels = TickLabels::new(BRT, full, None, &[]);
    let at = OCT_9_1357_UTC;
    assert_eq!(labels.label_at(at), "10:57:03");
    // The strip skipped every slot of the evening and the first two of the
    // 10th (no room, the chip, no open time): none was fed in. The next
    // label painted is the first one painted on the 10th.
    let next_day = at + DAY_MS;
    assert_eq!(labels.label_at(next_day), "10");
    assert_eq!(labels.label_at(next_day + HOUR_MS), "11:57:03");
}

/// A date the day separator already wrote on the strip is not written a
/// second time by the label after its tick: that label stays a clock, and
/// the next turn the separator did not write is named again.
#[test]
fn a_date_the_separator_wrote_keeps_its_labels_a_clock() {
    let full = TimeLabelFormat::Full;
    let at = OCT_9_1357_UTC;
    let named = [CivilDate::from_ms(at + DAY_MS, BRT)];
    let mut labels = TickLabels::new(BRT, full, Some(at), &named);
    assert_eq!(labels.label_at(at + DAY_MS), "10:57:03");
    assert_eq!(labels.label_at(at + 2 * DAY_MS), "11");
}
