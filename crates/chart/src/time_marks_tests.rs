use super::*;

const UTC: TzOffset = TzOffset::new(0);
/// 2026-11-06 00:00 UTC.
const NOV_6: i64 = 1_793_923_200_000;

/// Bars every `interval_ms` from `start_ms`, `px` apart, slots from zero.
fn bars(start_ms: i64, interval_ms: i64, count: usize, px: f32) -> Vec<BarAt> {
    (0..count)
        .map(|slot| BarAt {
            slot,
            x: slot as f32 * px,
            open_ms: start_ms + slot as i64 * interval_ms,
        })
        .collect()
}

/// Five pixels a character, as a monospace axis font roughly measures.
fn width(text: &str) -> f32 {
    text.chars().count() as f32 * 5.0
}

fn texts(marks: &[TimeMark]) -> Vec<&str> {
    marks.iter().map(|mark| mark.text.as_str()).collect()
}

#[test]
fn the_fixture_starts_on_the_sixth_of_november() {
    assert_eq!(CivilDate::from_ms(NOV_6, UTC).ymd(), (2026, 11, 6));
    assert_eq!(NOV_6.rem_euclid(DAY_MS), 0);
}

/// The reference: 15-minute bars about 9.75 px apart step every three hours,
/// and the day change writes the day number alone.
#[test]
fn fifteen_minute_bars_step_three_hours_and_name_the_new_day() {
    let start = NOV_6 + 18 * HOUR_MS;
    let view = bars(start, 15 * MINUTE_MS, 64, 9.75);
    let marks = time_marks(&view, Some(start - 15 * MINUTE_MS), UTC, 12.0, width);
    assert_eq!(
        texts(&marks),
        ["18:00", "21:00", "7", "03:00", "06:00", "09:00"]
    );
    let spacing = marks[1].x - marks[0].x;
    assert!((100.0..=130.0).contains(&spacing), "{spacing}");
    assert_eq!(marks[2].weight, MarkWeight::Day);
}

#[test]
fn minute_bars_write_hours_and_minutes_never_seconds() {
    let start = NOV_6 + 17 * HOUR_MS + 41 * MINUTE_MS;
    let view = bars(start, MINUTE_MS, 180, 8.0);
    let marks = time_marks(&view, Some(start - MINUTE_MS), UTC, 12.0, width);
    assert_eq!(texts(&marks)[..3], ["17:45", "18:00", "18:15"]);
    assert!(marks.iter().all(|mark| mark.text.len() == 5));
    assert!(
        marks
            .windows(2)
            .all(|pair| pair[1].x - pair[0].x >= TIME_MARK_SPACING_PX)
    );
}

#[test]
fn seconds_appear_only_when_the_step_is_under_a_minute() {
    let start = NOV_6 + 9 * HOUR_MS;
    let view = bars(start, SECOND_MS, 120, 8.0);
    let marks = time_marks(&view, Some(start - SECOND_MS), UTC, 12.0, width);
    assert_eq!(texts(&marks)[..3], ["09:00:00", "09:00:15", "09:00:30"]);
}

#[test]
fn coarser_steps_name_months_and_years() {
    // Hourly bars squeezed to 0.5 px: a week per label, months and a year named.
    let start = CivilDate::from_ymd(2026, 11, 20).start_ms(UTC);
    let view = bars(start, HOUR_MS, 24 * 70, 0.5);
    let marks = time_marks(&view, Some(start - HOUR_MS), UTC, 12.0, width);
    let texts = texts(&marks);
    assert!(texts.contains(&"Dec"), "{texts:?}");
    assert!(texts.contains(&"2027"), "{texts:?}");
    assert!(!texts.iter().any(|text| text.contains(':')), "{texts:?}");
}

#[test]
fn a_crowded_day_change_wins_over_the_clock_beside_it() {
    // A gap in the tape puts 22:00 and the new day ten pixels apart: the
    // clock label gives way to the day number.
    let marks = time_marks(
        &[
            BarAt {
                slot: 0,
                x: 0.0,
                open_ms: NOV_6 - 2 * HOUR_MS,
            },
            BarAt {
                slot: 1,
                x: 10.0,
                open_ms: NOV_6,
            },
            BarAt {
                slot: 2,
                x: 300.0,
                open_ms: NOV_6 + 3 * HOUR_MS,
            },
        ],
        Some(NOV_6 - 3 * HOUR_MS),
        UTC,
        12.0,
        width,
    );
    assert_eq!(texts(&marks), ["6", "03:00"]);
}

#[test]
fn the_display_zone_decides_where_a_day_turns() {
    let brasilia = TzOffset::new(-180);
    let start = NOV_6 + HOUR_MS;
    let view = bars(start, 15 * MINUTE_MS, 40, 9.75);
    let marks = time_marks(&view, Some(start - 15 * MINUTE_MS), brasilia, 12.0, width);
    // 03:00 UTC is local midnight: the day number lands there.
    let day = marks
        .iter()
        .find(|mark| mark.weight == MarkWeight::Day)
        .unwrap();
    assert_eq!(day.text, "6");
    assert_eq!(view[day.slot].open_ms, NOV_6 + 3 * HOUR_MS);
}

#[test]
fn irregular_bars_and_degenerate_views_are_deterministic() {
    // Tick bars: uneven instants, the same answer every time.
    let view: Vec<BarAt> = (0..200)
        .map(|slot| BarAt {
            slot,
            x: slot as f32 * 6.0,
            open_ms: NOV_6 + (slot as i64 * slot as i64) * 900,
        })
        .collect();
    let first = time_marks(&view, None, UTC, 12.0, width);
    assert_eq!(first, time_marks(&view, None, UTC, 12.0, width));
    assert!(!first.is_empty());
    for pair in first.windows(2) {
        assert!(
            pair[1].x - pair[0].x >= (width(&pair[0].text) + width(&pair[1].text)) / 2.0 + 12.0
        );
    }
    assert!(time_marks(&[], None, UTC, 12.0, width).is_empty());
    assert!(time_marks(&view[..1], None, UTC, 12.0, width).is_empty());
    let frozen = [view[0], BarAt { x: 50.0, ..view[0] }];
    assert!(time_marks(&frozen, None, UTC, 12.0, width).is_empty());
}
