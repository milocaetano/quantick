//! Price framing and the separate visible-bar reference for gutter inversion.
use crate::geometry::{PriceScale, price_window, tape_price_window};
use quantick_engine::Bar;
use rust_decimal::prelude::ToPrimitive as _;

/// Inputs shared by automatic framing and the gutter's flip threshold.
pub struct PriceAxisFit {
    pub native_tape: bool,
    pub tape_range: Option<(f64, f64)>,
    pub previous_auto: Option<(f64, f64)>,
    pub bounds: (f32, f32),
    pub tape_padding_px: f32,
}

impl PriceAxisFit {
    /// The native Tape frames its prints together with `visible`, the candles
    /// that stand beside it; ordinary charts fit their candles. Tape only
    /// passes the candles the beside view would show, so switching views
    /// never rescales the prices.
    pub fn resolve<'a>(
        &self,
        visible: impl Iterator<Item = &'a Bar> + Clone,
        partial: Option<&'a Bar>,
        newest: Option<&'a Bar>,
    ) -> Option<((f64, f64), f64)> {
        let (top, bottom) = self.bounds;
        let auto = if self.native_tape {
            let tape_range = PriceScale::auto_including(
                visible.clone(),
                partial,
                self.tape_range,
                top,
                bottom,
                0.0,
            )
            .map(|scale| scale.range())
            .or(self.tape_range);
            tape_price_window(
                tape_range,
                newest.and_then(|bar| bar.close.to_f64()),
                self.previous_auto,
                top,
                bottom,
                self.tape_padding_px,
            )
        } else {
            price_window(
                visible.clone(),
                partial,
                None,
                self.previous_auto,
                newest,
                top,
                bottom,
            )
        }?;
        let range = auto.range();
        let span = range.1 - range.0;
        let flip_span = if self.native_tape {
            price_window(visible, partial, Some(range), None, None, top, bottom).map_or(
                span,
                |bars| {
                    let (lo, hi) = bars.range();
                    (hi - lo).max(span)
                },
            )
        } else {
            span
        };
        Some((range, flip_span))
    }
}

/// The candles' right edge beside the tape whose divider stands at `divider`
/// on a chart from `left`: the trail strip's left edge where
/// [`trail_strip`](crate::flow_band::trail_strip) stands a `strip_px` strip,
/// else the divider. The beside layout stops its candles there, and tape only
/// fits the candles that edge would hold ([`tape_only_candles_px`]).
#[must_use]
pub fn beside_candles_right(left: f32, divider: f32, strip_px: f32) -> f32 {
    let beside = Some(quantick_orderflow::LaneView::Beside);
    crate::flow_band::trail_strip(beside, left, Some(divider), strip_px)
        .map_or(divider, |(strip_left, _)| strip_left)
}

/// The candles' width in the beside view tape only returns to, on a chart
/// `chart_width` wide with a `strip_px` live strip: the tape back on its
/// reopening band (`LiveLaneStyle::reopened_beside_px`) and the strip on the
/// trail beside it. Tape only fits its prints with the candles this holds, so
/// both views share one price window.
#[must_use]
pub fn tape_only_candles_px(chart_width: f32, strip_px: f32) -> f32 {
    let band = quantick_orderflow::LiveLaneStyle::reopened_beside_px(chart_width);
    beside_candles_right(0.0, chart_width - band, strip_px).max(0.0)
}

/// The bars in slots `start..end` of the venue `prefix` followed by `closed`,
/// plus the forming `partial` if its slot falls in it. The window can straddle
/// both series, so it is two slices, chained where they are read rather than
/// copied into one: copying was 24-48 KB every frame for the life of the pane.
#[must_use]
pub fn window_slices<'a>(
    series: (&'a [Bar], &'a [Bar], Option<&'a Bar>),
    (start, end): (usize, usize),
) -> (&'a [Bar], &'a [Bar], Option<&'a Bar>) {
    let (prefix, closed, partial) = series;
    let closed_total = prefix.len() + closed.len();
    let (from, to) = (
        start.min(closed_total),
        end.min(closed_total).max(start.min(closed_total)),
    );
    let shift = |slot: usize| slot.saturating_sub(prefix.len());
    (
        &prefix[from.min(prefix.len())..to.min(prefix.len())],
        &closed[shift(from)..shift(to)],
        partial.filter(|_| closed_total >= start && closed_total < end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    fn bar(low: i64, high: i64, close: i64) -> Bar {
        Bar {
            open_time: 0,
            close_time: 0,
            open: Decimal::from(close),
            high: Decimal::from(high),
            low: Decimal::from(low),
            close: Decimal::from(close),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 1,
        }
    }

    fn split_fit() -> PriceAxisFit {
        PriceAxisFit {
            native_tape: true,
            tape_range: Some((90.0, 130.0)),
            previous_auto: None,
            bounds: (20.0, 220.0),
            tape_padding_px: 30.0,
        }
    }

    #[test]
    fn split_fit_pads_the_union_once_and_holds_across_frames() {
        let bars = [bar(70, 100, 95)];
        let mut fit = split_fit();
        let (range, _) = fit.resolve(bars.iter(), None, bars.last()).unwrap();
        let scale = PriceScale::from_range(range.0, range.1, 20.0, 220.0);
        assert!((scale.y(70.0) - 190.0).abs() < 1e-4);
        assert!((scale.y(130.0) - 50.0).abs() < 1e-4);
        for _ in 0..20 {
            fit.previous_auto = Some(range);
            assert_eq!(
                fit.resolve(bars.iter(), None, bars.last()).unwrap().0,
                range
            );
        }
    }

    #[test]
    fn empty_split_falls_back_to_tape_and_partial_only_split_fits_its_candle() {
        let mut fit = split_fit();
        let empty = fit.resolve(std::iter::empty(), None, None).unwrap().0;
        let expected = tape_price_window(fit.tape_range, None, None, 20.0, 220.0, 30.0)
            .unwrap()
            .range();
        assert_eq!(empty, expected);
        fit.tape_range = None;
        fit.previous_auto = Some(empty);
        assert_eq!(
            fit.resolve(std::iter::empty(), None, None).unwrap().0,
            empty
        );
        let partial = bar(40, 160, 100);
        let partial_fit = fit
            .resolve(std::iter::empty(), Some(&partial), Some(&partial))
            .unwrap()
            .0;
        assert!(partial_fit.0 < 40.0 && partial_fit.1 > 160.0);
    }

    #[test]
    fn window_slices_straddle_the_seam_and_take_the_partial_in_its_slot() {
        let (prefix, closed, partial) =
            (vec![bar(1, 2, 1); 3], vec![bar(3, 4, 3); 2], bar(5, 6, 5));
        let series = (&prefix[..], &closed[..], Some(&partial));
        let (p, c, f) = window_slices(series, (2, 6));
        assert_eq!((p.len(), c.len(), f.is_some()), (1, 2, true));
        let (p, c, f) = window_slices(series, (4, 5));
        assert_eq!((p.len(), c.len(), f.is_some()), (0, 1, false));
        let (p, c, f) = window_slices(series, (9, 12));
        assert_eq!((p.len(), c.len(), f.is_some()), (0, 0, false));
    }

    /// The beside view tape only returns to: the tape back on its default
    /// band, the strip on the trail left of it, and the candles' floor kept.
    #[test]
    fn tape_only_measures_the_candles_of_the_beside_view_it_returns_to() {
        assert_eq!(tape_only_candles_px(1_000.0, 0.0), 650.0);
        assert_eq!(tape_only_candles_px(1_000.0, 84.0), 566.0);
        let floor = crate::flow_band::TRAIL_CANDLES_MIN_PX;
        assert_eq!(tape_only_candles_px(100.0, 84.0), floor);
        assert_eq!(beside_candles_right(10.0, 660.0, 84.0), 576.0);
        assert_eq!(beside_candles_right(10.0, 660.0, 0.0), 660.0);
    }

    /// Trader 2026-11-03: a few cents of prints filled the whole tape-only
    /// pane, and a short squeeze of the gutter turned it upside down. Tape
    /// only fits a narrow tape with the candles the beside view holds, never
    /// the spike just left of them, and the flip counts in that fit.
    #[test]
    fn tape_only_fits_the_beside_candles_and_flips_against_them() {
        let view = crate::viewport::Viewport::new();
        let (total, chart_width, strip) = (400, 1_000.0, 84.0);
        let (start, end) = view.visible_range(tape_only_candles_px(chart_width, strip), total);
        assert!(start > 0 && end == total, "{start}..{end}");
        let mut closed = vec![bar(95, 105, 100); total];
        closed[start - 1] = bar(95, 500, 100);
        let mut fit = split_fit();
        fit.tape_range = Some((100.0, 100.2));
        let none: &[Bar] = &[];
        let fitted = |slots| {
            let (prefix, closed, partial) = window_slices((none, &closed, None), slots);
            fit.resolve(prefix.iter().chain(closed), partial, closed.last())
                .unwrap()
        };
        let (range, flip_span) = fitted((start, end));
        assert!(range.0 < 95.0 && range.1 > 105.0, "the candles: {range:?}");
        assert!(range.1 < 500.0, "never the spike left of them: {range:?}");
        let wider = view.visible_range(tape_only_candles_px(chart_width, 0.0), total);
        assert!(
            fitted(wider).0.1 > 500.0,
            "the strip's width holds the spike"
        );
        let mut price = crate::price_view::PriceView::new();
        price.drag_zoom_against(1e6, range, flip_span);
        assert!(!price.is_inverted(), "the first pull only flattens");
        let (lo, hi) = price.resolve(range);
        let threshold = flip_span * crate::price_view::FLIP_SPAN_FACTOR;
        assert!(
            (hi - lo - threshold).abs() < 1e-6,
            "{} vs {threshold}",
            hi - lo
        );
        price.drag_zoom_against(1.01, range, flip_span);
        assert!(price.is_inverted(), "and the next pull flips it");
    }
}
