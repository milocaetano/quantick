//! The measurement readout the Measure family shares.
//!
//! Ruler, price range and date range ask the same question with different
//! axes suppressed, so "how do we phrase a move" is written once here
//! (`docs/ux/drawing-tools-2026-08.md` §D5).
//!
//! What the readout deliberately does **not** show is ticks. A tick count
//! needs the instrument's tick size, which several feeds never declare; a
//! tick figure derived from a guessed step would be an invented number on a
//! panel whose whole job is to be exact. Points and percent are computable
//! from the anchors alone, and they are what is shown.

use eframe::egui;
use egui_phosphor::regular as icons;

use super::{ChartPoint, DrawingStyle, ToolFamily, ValueUnit, drawing_fill, drawing_stroke};
use crate::theme;

/// The one rail family every measurement tool declares.
pub(super) const MEASURE_FAMILY: ToolFamily = ToolFamily {
    id: "measure",
    title: "Measure",
    icon: icons::RULER,
};

/// Which axes a measurement reports. Price range suppresses time, date range
/// suppresses price; the ruler reports both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Axes {
    pub price: bool,
    pub time: bool,
}

pub(super) const BOTH_AXES: Axes = Axes {
    price: true,
    time: true,
};
pub(super) const PRICE_ONLY: Axes = Axes {
    price: true,
    time: false,
};
pub(super) const TIME_ONLY: Axes = Axes {
    price: false,
    time: true,
};

const READOUT_TEXT_PX: f32 = 10.0;
const READOUT_LINE_GAP_PX: f32 = 2.0;
const READOUT_PAD_X_PX: f32 = 5.0;
const READOUT_PAD_Y_PX: f32 = 3.0;
const READOUT_RADIUS_PX: f32 = 3.0;
/// The plate under the readout. Opaque enough to stay legible over candles,
/// dark enough not to become a second chart object.
const READOUT_PLATE: egui::Color32 = egui::Color32::from_rgba_premultiplied(14, 18, 26, 216);

/// What the measured leg says, already worded. Empty lines never happen: an
/// axis that is suppressed contributes no line at all.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Readout {
    pub lines: Vec<String>,
    /// `None` when the move is flat or the price axis is suppressed — the
    /// readout is then painted in the object's own colour rather than
    /// claiming a direction it does not have.
    pub rising: Option<bool>,
}

/// Percent of the *starting* price, which is the convention every platform
/// uses and the only one that makes a round trip read as zero. A start price
/// at or below zero has no meaningful percentage, and none is shown.
fn percent_move(from: f64, to: f64) -> Option<f64> {
    (from > 0.0).then(|| (to - from) / from * 100.0)
}

fn format_points(delta: f64) -> String {
    let magnitude = delta.abs();
    if magnitude != 0.0 && magnitude < 1.0 {
        format!("{delta:+.6}")
    } else {
        format!("{delta:+.2}")
    }
}

/// Elapsed market time, coarsened to the largest unit that still carries
/// information — a trader reads "4m 21s", never "261000 ms".
fn format_elapsed(ms: i64) -> String {
    let total = ms.unsigned_abs();
    let seconds = total / 1_000;
    let (days, hours, minutes, secs) = (
        seconds / 86_400,
        (seconds % 86_400) / 3_600,
        (seconds % 3_600) / 60,
        seconds % 60,
    );
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else if seconds > 0 {
        format!("{seconds}s")
    } else {
        format!("{total}ms")
    }
}

/// Word the move between two anchors.
#[must_use]
pub(super) fn readout(anchors: &[ChartPoint], axes: Axes, unit: ValueUnit<'_>) -> Option<Readout> {
    let [from, to, ..] = anchors else {
        return None;
    };
    let mut lines = Vec::with_capacity(2);
    let mut rising = None;

    if axes.price {
        let delta = to.price - from.price;
        rising = (delta != 0.0).then_some(delta > 0.0);
        let mut price_line = format_points(delta);
        match unit {
            ValueUnit::Price => {
                price_line.push_str(" pts");
                if let Some(percent) = percent_move(from.price, to.price) {
                    price_line.push_str(&format!("   {percent:+.2}%"));
                }
            }
            // On an indicator's own axis both price words are wrong. `pts`
            // names a price unit, and a percent of a signed cumulative series
            // is a false number rather than a coarse one — a move from -100
            // to +100 is not "-200%". The delta and the series it was
            // measured on are the whole honest answer.
            ValueUnit::Indicator(label) => {
                price_line.push_str(&format!("  ·  {label}"));
            }
        }
        lines.push(price_line);
    }

    if axes.time {
        let bars = (to.bar - from.bar).abs().round() as i64;
        let mut time_line = format!("{bars} bar{}", if bars == 1 { "" } else { "s" });
        // Elapsed time needs both anchors to know when they were: an anchor
        // dropped past the newest bar has no instant behind it, and the
        // readout says nothing rather than inventing one.
        if let (Some(start), Some(end)) = (from.time_ms, to.time_ms) {
            time_line.push_str(&format!("   {}", format_elapsed(end - start)));
        }
        lines.push(time_line);
    }

    (!lines.is_empty()).then_some(Readout { lines, rising })
}

/// The colour a readout is spoken in: the direction of the move, so the sign
/// is readable without reading the sign.
#[must_use]
pub(super) fn readout_color(readout: &Readout, style: DrawingStyle) -> egui::Color32 {
    match readout.rising {
        Some(true) => theme::BUY,
        Some(false) => theme::SELL,
        None => style.color,
    }
}

/// What is being measured, as opposed to where it is painted: the anchors,
/// which axes take part, what the value axis means, and whether this is the
/// halo pass. One bundle because they always travel together — a tool that
/// knew its axes but not its unit could word a CVD move in points.
#[derive(Clone, Copy)]
pub(super) struct Measured<'a> {
    pub anchors: &'a [ChartPoint],
    pub axes: Axes,
    pub unit: ValueUnit<'a>,
    pub halo: bool,
    /// Whether this is the band that prints the readout — see
    /// [`crate::drawings::DrawContext::primary_band`].
    pub primary_band: bool,
}

/// Paint the measured area, its leg and the readout plate.
///
/// `points` are the two screen anchors. The plate is centred on the leg and
/// nudged so it stays inside the chart — a readout half off-screen is worse
/// than no readout.
pub(super) fn paint_measure(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    style: DrawingStyle,
    points: &[egui::Pos2],
    measured: Measured<'_>,
) {
    let Measured {
        anchors,
        axes,
        unit,
        halo,
        primary_band,
    } = measured;
    let [from, to, ..] = points else {
        return;
    };
    let area = span_rect(chart_rect, *from, *to, axes);
    let stroke = drawing_stroke(style);

    // The halo pass is stroke-only, like every other tool: no fill, no plate.
    if !halo && style.fill_alpha > 0 {
        painter.rect_filled(area, egui::Rounding::ZERO, drawing_fill(style));
    }
    painter.rect_stroke(area, egui::Rounding::ZERO, stroke);
    painter.line_segment([*from, *to], stroke);
    // The stroke crosses every band a time-only measurement passes through;
    // the plate is stamped once. Three copies of "17 bars 4m 21s" down the
    // screen are not three facts.
    if halo || !primary_band {
        return;
    }

    let Some(readout) = readout(anchors, axes, unit) else {
        return;
    };
    paint_plate(
        painter,
        chart_rect,
        area.center(),
        &readout,
        readout_color(&readout, style),
    );
}

/// The rectangle a measurement covers, with a suppressed axis spanning the
/// whole chart instead of the anchors.
fn span_rect(chart_rect: egui::Rect, from: egui::Pos2, to: egui::Pos2, axes: Axes) -> egui::Rect {
    let x = if axes.time {
        (from.x.min(to.x), from.x.max(to.x))
    } else {
        (chart_rect.left(), chart_rect.right())
    };
    let y = if axes.price {
        (from.y.min(to.y), from.y.max(to.y))
    } else {
        (chart_rect.top(), chart_rect.bottom())
    };
    egui::Rect::from_min_max(egui::pos2(x.0, y.0), egui::pos2(x.1, y.1))
}

fn paint_plate(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    centre: egui::Pos2,
    readout: &Readout,
    color: egui::Color32,
) {
    let font = egui::FontId::monospace(READOUT_TEXT_PX);
    let galleys: Vec<_> = readout
        .lines
        .iter()
        .map(|line| painter.layout_no_wrap(line.clone(), font.clone(), color))
        .collect();
    let width = galleys
        .iter()
        .fold(0.0_f32, |widest, galley| widest.max(galley.size().x));
    let height: f32 = galleys.iter().map(|galley| galley.size().y).sum::<f32>()
        + READOUT_LINE_GAP_PX * (galleys.len().saturating_sub(1)) as f32;
    let size = egui::vec2(
        width + 2.0 * READOUT_PAD_X_PX,
        height + 2.0 * READOUT_PAD_Y_PX,
    );

    let mut plate = egui::Rect::from_center_size(centre, size);
    // Nudge, never clamp to a corner: the readout must stay attached to the
    // leg it describes.
    plate = plate.translate(egui::vec2(
        (chart_rect.left() - plate.left()).max(0.0) + (chart_rect.right() - plate.right()).min(0.0),
        (chart_rect.top() - plate.top()).max(0.0) + (chart_rect.bottom() - plate.bottom()).min(0.0),
    ));

    painter.rect_filled(
        plate,
        egui::Rounding::same(READOUT_RADIUS_PX),
        READOUT_PLATE,
    );
    let mut cursor = plate.min + egui::vec2(READOUT_PAD_X_PX, READOUT_PAD_Y_PX);
    for galley in galleys {
        let advance = galley.size().y + READOUT_LINE_GAP_PX;
        painter.galley(cursor, galley, color);
        cursor.y += advance;
    }
}

/// Hit-test a measurement: its border and its leg, plus the interior while
/// the fill is visible — the same rule the shapes follow.
pub(super) fn hit_measure(
    chart_rect: egui::Rect,
    style: DrawingStyle,
    points: &[egui::Pos2],
    position: egui::Pos2,
    radius_px: f32,
    axes: Axes,
) -> bool {
    let [from, to, ..] = points else {
        return false;
    };
    let area = span_rect(chart_rect, *from, *to, axes);
    if super::distance_to_segment(position, *from, *to) <= radius_px {
        return true;
    }
    if !area.expand(radius_px).contains(position) {
        return false;
    }
    style.fill_alpha > 0 || !area.shrink(radius_px).contains(position)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchors(from: (f32, f64, i64), to: (f32, f64, i64)) -> Vec<ChartPoint> {
        vec![
            ChartPoint::at_time(from.0, from.1, Some(from.2)),
            ChartPoint::at_time(to.0, to.1, Some(to.2)),
        ]
    }

    /// The whole point of the session's request: points *and* percent, on one
    /// line, from one gesture.
    #[test]
    fn the_ruler_reads_points_and_percent() {
        let readout = readout(
            &anchors((0.0, 100.0, 0), (17.0, 101.83, 261_000)),
            BOTH_AXES,
            ValueUnit::Price,
        )
        .expect("two anchors");
        assert_eq!(readout.lines[0], "+1.83 pts   +1.83%");
        assert_eq!(readout.lines[1], "17 bars   4m 21s");
        assert_eq!(readout.rising, Some(true));
    }

    /// Measured on an indicator's own axis, the same gesture must not speak
    /// price. `pts` is a price unit and a percent of a signed cumulative
    /// series is false, not coarse — a move from -100 to +100 is not "-200%".
    #[test]
    fn a_measurement_on_a_band_names_the_series_instead_of_points_and_percent() {
        let readout = readout(
            &anchors((0.0, -100.0, 0), (17.0, 312.0, 261_000)),
            BOTH_AXES,
            ValueUnit::Indicator("CVD"),
        )
        .expect("two anchors");
        assert_eq!(readout.lines[0], "+412.00  ·  CVD");
        assert!(!readout.lines[0].contains("pts"));
        assert!(!readout.lines[0].contains('%'));
        // The time line is the same sentence on every band.
        assert_eq!(readout.lines[1], "17 bars   4m 21s");
        assert_eq!(readout.rising, Some(true));
    }

    #[test]
    fn a_fall_is_signed_and_reads_as_a_fall() {
        let readout = readout(
            &anchors((0.0, 200.0, 0), (4.0, 180.0, 60_000)),
            BOTH_AXES,
            ValueUnit::Price,
        )
        .expect("two anchors");
        assert_eq!(readout.lines[0], "-20.00 pts   -10.00%");
        assert_eq!(readout.rising, Some(false));
    }

    /// Percent is of the *start*, so a round trip nets to zero rather than to
    /// the asymmetric figure a percent-of-end would produce.
    #[test]
    fn percent_is_measured_against_the_starting_price() {
        assert_eq!(percent_move(50.0, 75.0), Some(50.0));
        let back = percent_move(75.0, 50.0).expect("a positive start price");
        assert!(
            (back + 100.0 / 3.0).abs() < 1e-9,
            "down from 75 to 50 is -33.33%, not the -50% a percent-of-end would give: {back}"
        );
    }

    /// A non-positive start price has no meaningful percentage; the readout
    /// drops the figure instead of printing an infinity or a nonsense number.
    #[test]
    fn a_non_positive_start_price_reports_no_percent() {
        assert_eq!(percent_move(0.0, 10.0), None);
        assert_eq!(percent_move(-5.0, 10.0), None);
        let readout = readout(
            &anchors((0.0, 0.0, 0), (2.0, 10.0, 1_000)),
            BOTH_AXES,
            ValueUnit::Price,
        )
        .expect("two anchors");
        assert!(!readout.lines[0].contains('%'));
        assert!(readout.lines[0].contains("pts"));
    }

    /// An anchor past the newest bar has no time behind it (`ChartPoint`), so
    /// the readout reports bars and stays quiet about the clock.
    #[test]
    fn an_untimed_anchor_reports_bars_without_inventing_a_duration() {
        let anchors = vec![
            ChartPoint::at_time(0.0, 100.0, Some(0)),
            ChartPoint::at(9.0, 110.0),
        ];
        let readout = readout(&anchors, BOTH_AXES, ValueUnit::Price).expect("two anchors");
        assert_eq!(readout.lines[1], "9 bars");
    }

    #[test]
    fn a_flat_move_claims_no_direction() {
        let readout = readout(
            &anchors((0.0, 100.0, 0), (3.0, 100.0, 5_000)),
            BOTH_AXES,
            ValueUnit::Price,
        )
        .expect("anchors");
        assert_eq!(readout.rising, None);
    }

    #[test]
    fn price_range_and_date_range_suppress_the_other_axis() {
        let anchors = anchors((0.0, 100.0, 0), (17.0, 110.0, 261_000));
        let price = readout(&anchors, PRICE_ONLY, ValueUnit::Price).expect("anchors");
        assert_eq!(price.lines.len(), 1);
        assert!(price.lines[0].contains('%'));
        let time = readout(&anchors, TIME_ONLY, ValueUnit::Price).expect("anchors");
        assert_eq!(time.lines.len(), 1);
        assert!(time.lines[0].contains("bars"));
    }

    #[test]
    fn elapsed_time_coarsens_to_the_unit_that_carries_information() {
        assert_eq!(format_elapsed(450), "450ms");
        assert_eq!(format_elapsed(9_000), "9s");
        assert_eq!(format_elapsed(261_000), "4m 21s");
        assert_eq!(format_elapsed(3_900_000), "1h 5m");
        assert_eq!(format_elapsed(180_000_000), "2d 2h");
        assert_eq!(format_elapsed(-261_000), "4m 21s", "duration is unsigned");
    }

    #[test]
    fn one_bar_is_not_pluralised() {
        let readout = readout(
            &anchors((0.0, 100.0, 0), (1.0, 100.5, 1_000)),
            TIME_ONLY,
            ValueUnit::Price,
        )
        .expect("anchors");
        assert!(readout.lines[0].starts_with("1 bar "));
    }

    #[test]
    fn a_sub_unit_move_keeps_the_digits_that_matter() {
        let readout = readout(
            &anchors((0.0, 0.5, 0), (2.0, 0.500_12, 1_000)),
            PRICE_ONLY,
            ValueUnit::Price,
        )
        .expect("anchors");
        assert!(
            readout.lines[0].starts_with("+0.000120"),
            "a crypto-sized move must not round to +0.00: {}",
            readout.lines[0]
        );
    }
}
