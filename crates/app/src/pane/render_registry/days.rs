//! Where the display day turns over. Across the candles, a near-invisible
//! full-height rule. On the time strip, a tick at the boundary with the time
//! the old day's last bar closed left of it, and the date the new day opens
//! with its first bar's open time right of it (`18:24 | Tue 29 09:00`), plus
//! the date of the leftmost bar pinned to the strip's left edge. The time
//! labels carry a clock time only, so without it a chart spanning midnight
//! reads as one day, and nothing says when each session ran.
use super::{Contribution, Package};
use crate::{pointer_compass, theme, viewport::Viewport};
use eframe::egui;
use quantick_civil::{CivilDate, TzOffset, weekday_abbr};
/// Space between the tick and the labels either side of it, and between two
/// labels.
const DAY_LABEL_GAP_PX: f32 = 3.0;
/// The rule across the candles: white at an alpha low enough to be found
/// when looked for and never to compete with a candle or the flow, a step
/// quieter than the venue seam's [`theme::SEAM_LINE`].
const DAY_RULE: egui::Color32 = egui::Color32::from_rgba_premultiplied(0x1E, 0x1E, 0x1E, 0x1E);
pub(super) const PACKAGE: Package = Package {
    layers: &[quantick_layers::ChartLayer::DaySeparator],
    contributions: &[Contribution::DaySeparator(day_separator)],
};
/// A slot whose bar opens a new display day, and when the two days ran.
pub(in crate::pane) struct DayTurn {
    pub slot: usize,
    /// The date the new day opens, from `quantick_civil::day_starts`.
    pub date: CivilDate,
    /// The last print of the bar before the turn: when the old day ended.
    pub ended_ms: Option<i64>,
    /// When the first bar of the new day opened.
    pub opened_ms: Option<i64>,
}
pub(in crate::pane) struct DaySeparatorPass<'a> {
    pub painter: &'a egui::Painter,
    /// The candles' extent: the rule spans its height.
    pub history: egui::Rect,
    pub strip: egui::Rect,
    pub total: usize,
    pub candle_width: f32,
    pub viewport: &'a Viewport,
    pub turns: &'a [DayTurn],
    /// The display timezone the times are written in.
    pub tz: TzOffset,
    /// The date of the leftmost visible bar, pinned to the strip's left edge.
    pub first_visible: Option<CivilDate>,
    /// The pointer's chip outranks a label: it is where the trader is looking.
    pub claims: &'a pointer_compass::AxisClaims,
    /// Written here: the spans the labels took, which the time labels avoid.
    pub reserved: Vec<(f32, f32)>,
}
fn day_separator(p: &mut DaySeparatorPass<'_>) {
    let font = egui::FontId::monospace(crate::chart::TIME_LABEL_FONT_PX);
    let (painter, claims, tz) = (p.painter, p.claims, p.tz);
    let (left, right) = (p.history.left(), p.history.right());
    let y = p.strip.center().y;
    let chip_width = painter
        .layout_no_wrap(
            crate::chart::TimeLabelFormat::Full.sample().to_owned(),
            font.clone(),
            theme::TEXT_MUTED,
        )
        .size()
        .x;
    let free = |start: f32, width: f32| {
        !pointer_compass::claimed(
            start + width / 2.0,
            width,
            chip_width,
            claims.iter().copied(),
        )
    };
    let text = |text: String, color| painter.layout_no_wrap(text, font.clone(), color);
    let mut written_right = f32::NEG_INFINITY;
    // Writes a label and returns its right edge.
    let draw = |p: &mut DaySeparatorPass<'_>, start: f32, galley: std::sync::Arc<egui::Galley>| {
        let end = start + galley.size().x;
        painter.galley(
            egui::pos2(start, y - galley.size().y / 2.0),
            galley,
            theme::TEXT_MUTED,
        );
        p.reserved.push((start, end));
        end
    };
    let ticks: Vec<(f32, &DayTurn)> = p
        .turns
        .iter()
        .map(|turn| {
            // The left edge of the first bar of the day: between the two
            // days' bars rather than on either.
            let x = p.viewport.x_center(turn.slot, right, p.total) - p.candle_width / 2.0;
            (x, turn)
        })
        .filter(|&(x, _)| x >= left && x <= right)
        .collect();
    // `limit` is where the next tick stands, or the strip's end: a label is
    // never crossed by the tick after it.
    let limit_before = |index: usize| {
        ticks
            .get(index)
            .map_or(right, |&(x, _)| x - DAY_LABEL_GAP_PX)
    };
    // Pinned first, so a boundary label close to the left edge wins by
    // pushing it out rather than being pushed itself.
    if let Some(date) = p.first_visible {
        let galley = text(
            format!("{} {}", weekday_abbr(date.weekday()), date.short()),
            theme::TEXT_MUTED,
        );
        let (start, width) = (left + DAY_LABEL_GAP_PX, galley.size().x);
        if start + width <= limit_before(0) && free(start, width) {
            written_right = draw(p, start, galley);
        }
    }
    for (index, &(x, turn)) in ticks.iter().enumerate() {
        // Across the candles, under the chip too: it is not on the strip.
        painter.line_segment(
            [
                egui::pos2(x, p.history.top()),
                egui::pos2(x, p.history.bottom()),
            ],
            egui::Stroke::new(1.0_f32, DAY_RULE),
        );
        // Under the pointer's chip the chip wins, tick and labels alike.
        if pointer_compass::claimed(x, 0.0, chip_width, claims.iter().copied()) {
            continue;
        }
        painter.line_segment(
            [
                egui::pos2(x, p.strip.top()),
                egui::pos2(x, p.strip.bottom()),
            ],
            egui::Stroke::new(1.0_f32, theme::TEXT_MUTED),
        );
        // No time label is written across the tick either.
        p.reserved.push((x, x));
        let ended = turn
            .ended_ms
            .map(|ms| text(clock(ms, tz), theme::TEXT_MUTED));
        let dated = text(day_text(turn.date), theme::TEXT_PRIMARY);
        let dated_opened = turn
            .opened_ms
            .map(|ms| painter.layout_job(opened_job(turn.date, ms, tz, &font)));
        let widths = TurnWidths {
            ended: ended.as_ref().map(|galley| galley.size().x),
            dated_opened: dated_opened.as_ref().map(|galley| galley.size().x),
            dated: dated.size().x,
        };
        let floor = (written_right + DAY_LABEL_GAP_PX).max(left);
        let Some(placed) = place_turn(x, widths, floor, limit_before(index + 1), free) else {
            continue;
        };
        if let (Some(at), Some(galley)) = (placed.ended_at, ended) {
            draw(p, at, galley);
        }
        let right_side = match (placed.with_opened, dated_opened) {
            (true, Some(galley)) => galley,
            _ => dated,
        };
        written_right = draw(p, x + DAY_LABEL_GAP_PX, right_side);
    }
}
/// `Tue 29`; the month joins it on the first of the month, where the month
/// is what changed.
fn day_text(date: CivilDate) -> String {
    let (_, _, day) = date.ymd();
    if day == 1 {
        format!("{} {}", weekday_abbr(date.weekday()), date.short())
    } else {
        format!("{} {day}", weekday_abbr(date.weekday()))
    }
}
/// `18:24` in the display timezone.
fn clock(ms: i64, tz: TzOffset) -> String {
    let minute = ms
        .saturating_add(tz.offset_ms())
        .rem_euclid(quantick_engine::time_bucket::DAY_MS)
        / 60_000;
    format!("{:02}:{:02}", minute / 60, minute % 60)
}
/// `Tue 29 09:00`: the day a turn opens and when its first bar opened.
fn opened_text(date: CivilDate, opened_ms: i64, tz: TzOffset) -> String {
    format!("{} {}", day_text(date), clock(opened_ms, tz))
}
/// [`opened_text`] with the date as bright as a date alone and the time as
/// muted as the clock labels beside it.
fn opened_job(
    date: CivilDate,
    opened_ms: i64,
    tz: TzOffset,
    font: &egui::FontId,
) -> egui::text::LayoutJob {
    let text = opened_text(date, opened_ms, tz);
    let split = day_text(date).len();
    let mut job = egui::text::LayoutJob::default();
    for (part, color) in [
        (&text[..split], theme::TEXT_PRIMARY),
        (&text[split..], theme::TEXT_MUTED),
    ] {
        job.append(part, 0.0, egui::TextFormat::simple(font.clone(), color));
    }
    job
}
/// How wide each label of one turn is; a time that is unknown is `None`.
#[derive(Clone, Copy)]
struct TurnWidths {
    /// The old day's end time, left of the tick.
    ended: Option<f32>,
    /// The date with the new day's start time, right of the tick.
    dated_opened: Option<f32>,
    /// The date alone.
    dated: f32,
}
/// Which of a turn's labels are written: the end time starts at `ended_at`,
/// the date always right of the tick, with its start time or without.
#[derive(Debug, PartialEq)]
struct TurnPlacement {
    ended_at: Option<f32>,
    with_opened: bool,
}
/// Where a turn's labels go beside the tick at `x`, inside `floor..limit`
/// and where `free` allows. Short of space the end time is dropped first,
/// then the start time; the date is never dropped while it fits, and when it
/// does not, nothing is written.
fn place_turn(
    x: f32,
    widths: TurnWidths,
    floor: f32,
    limit: f32,
    free: impl Fn(f32, f32) -> bool,
) -> Option<TurnPlacement> {
    let right_at = x + DAY_LABEL_GAP_PX;
    let right_fits = |width: Option<f32>| {
        width.is_some_and(|width| {
            right_at >= floor && right_at + width <= limit && free(right_at, width)
        })
    };
    let ended_at = widths
        .ended
        .map(|width| (x - DAY_LABEL_GAP_PX - width, width))
        .filter(|&(at, width)| at >= floor && free(at, width))
        .map(|(at, _)| at);
    if right_fits(widths.dated_opened) {
        return Some(TurnPlacement {
            ended_at,
            with_opened: true,
        });
    }
    right_fits(Some(widths.dated)).then_some(TurnPlacement {
        ended_at: None,
        with_opened: false,
    })
}
/// Whether a time label spanning `from..to` would touch a date's span.
pub(super) fn reserved_by(from: f32, to: f32, reserved: &[(f32, f32)]) -> bool {
    reserved
        .iter()
        .any(|&(start, end)| from < end + DAY_LABEL_GAP_PX && to > start - DAY_LABEL_GAP_PX)
}
#[cfg(test)]
mod days_tests {
    use super::*;

    #[test]
    fn a_day_reads_weekday_and_date_and_names_the_month_when_it_turns() {
        assert_eq!(day_text(CivilDate::from_ymd(2026, 9, 29)), "Tue 29");
        assert_eq!(day_text(CivilDate::from_ymd(2026, 10, 1)), "Thu 01 Oct");
    }

    #[test]
    fn a_turn_reads_when_the_old_day_ended_and_the_new_one_opened() {
        let brt = quantick_civil::TzOffset::new(-180);
        // 2026-09-28 21:24 UTC is 18:24 in São Paulo; 2026-09-29 12:00 UTC is 09:00.
        let ended = 1_790_630_640_000;
        let opened = 1_790_683_200_000;
        assert_eq!(clock(ended, brt), "18:24");
        assert_eq!(
            opened_text(CivilDate::from_ymd(2026, 9, 29), opened, brt),
            "Tue 29 09:00"
        );
        assert_eq!(
            opened_text(CivilDate::from_ymd(2026, 10, 1), opened, brt),
            "Thu 01 Oct 09:00",
            "the month still joins the date on the first"
        );
        assert_eq!(clock(-60_000, quantick_civil::TzOffset::new(0)), "23:59");
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
    fn a_time_label_stands_aside_only_where_a_date_is_written() {
        let reserved = [(100.0, 140.0)];
        assert!(reserved_by(120.0, 150.0, &reserved), "overlapping");
        assert!(reserved_by(141.0, 170.0, &reserved), "inside the gap");
        assert!(!reserved_by(150.0, 190.0, &reserved), "clear of it");
        assert!(!reserved_by(40.0, 90.0, &reserved), "clear before it");
        assert!(!reserved_by(120.0, 150.0, &[]), "no dates, no gaps");
    }
}
