use super::*;

const BRT: TzOffset = TzOffset::new(-180);

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
    assert_eq!(TickLabels::new(BRT, full, None).label_at(at), "10:57:03");
    let short = TimeLabelFormat::Short;
    assert_eq!(
        TickLabels::new(BRT, short, Some(at - HOUR_MS)).label_at(at),
        "10:57"
    );
    // 02:00 UTC on the 10th is 23:00 on the 9th in UTC-3: no day turn yet
    // there, though UTC has crossed midnight.
    let late = at + 12 * HOUR_MS + 2 * 60_000 + 57_000;
    let mut brt = TickLabels::new(BRT, full, Some(at));
    assert_eq!(brt.label_at(late), "23:00:00");
    assert_eq!(brt.label_at(late + HOUR_MS), "10");
    assert_eq!(brt.label_at(late + 2 * HOUR_MS), "01:00:00");
    let utc = TzOffset::new(0);
    assert_eq!(TickLabels::new(utc, full, Some(at)).label_at(late), "10");
    // Into November, and into 2027.
    let nov_1 = late + HOUR_MS + 22 * DAY_MS;
    let mut month = TickLabels::new(BRT, full, Some(nov_1 - HOUR_MS));
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
    let mut labels = TickLabels::new(BRT, full, None);
    let at = OCT_9_1357_UTC;
    assert_eq!(labels.label_at(at), "10:57:03");
    // The strip skipped every slot of the evening and the first two of the
    // 10th (no room, the chip, no open time): none was fed in. The next
    // label painted is the first one painted on the 10th.
    let next_day = at + DAY_MS;
    assert_eq!(labels.label_at(next_day), "10");
    assert_eq!(labels.label_at(next_day + HOUR_MS), "11:57:03");
}

/// On a strip too narrow for the full tag, the widest shorter form that is
/// still true; the short clock when nothing else fits.
#[test]
fn the_pointer_tag_falls_back_to_the_widest_form_that_fits() {
    let fits = |room: usize| move |text: &str| text.len() <= room;
    let at = OCT_9_1357_UTC;
    assert_eq!(
        pointer_time_fitting(at, BRT, fits(40)),
        "Fri 09 Oct '26 10:57:03"
    );
    assert_eq!(pointer_time_fitting(at, BRT, fits(22)), "Fri 09 10:57:03");
    assert_eq!(pointer_time_fitting(at, BRT, fits(14)), "10:57:03");
    assert_eq!(pointer_time_fitting(at, BRT, fits(7)), "10:57");
    assert_eq!(pointer_time_fitting(at, BRT, fits(2)), "10:57");
}
