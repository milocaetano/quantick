//! The Spot vs Perp CVD pane's view-model: both CVD lines on one scale, the
//! zero line, the opposite-sign spans to shade and the legend.

use quantick_appearance::Token;
use quantick_indicators::spot_perp_cvd::{CvdPoint, SpotPerpCvd};
use rust_decimal::Decimal;

use super::{WidgetCell, line_px, value_range};
use crate::footprint_projection::fmt_qty;

/// What the pane says on a feed with no spot and perp pair.
pub const SPOT_PERP_NOT_PROVIDED: &str = "feed does not provide a spot and perp pair";

/// What the pane says on a pair before either leg trades.
pub const SPOT_PERP_WAITING: &str = "waiting for trades on both legs";

/// The legend's key to the shading.
pub const SPOT_PERP_NOTE: &str = "shaded = opposite signs";

/// The whole pane body.
#[derive(Debug, Clone, PartialEq)]
pub struct SpotPerpCvdView {
    /// Pixels in the plot, oldest first, inked [`Self::perp_token`].
    pub perp_line: Vec<[f32; 2]>,
    /// Pixels in the plot, oldest first, inked [`Self::spot_token`].
    pub spot_line: Vec<[f32; 2]>,
    pub perp_token: Token,
    pub spot_token: Token,
    /// The zero line, when zero is inside the scale.
    pub zero_y_px: Option<f32>,
    /// `[left, right]` of each opposite-sign span, translucent
    /// [`Self::spot_token`].
    pub shaded: Vec<[f32; 2]>,
    /// `— perp -2341 BTC`.
    pub perp_legend: WidgetCell,
    /// `— spot +412 BTC`.
    pub spot_legend: WidgetCell,
    pub note: &'static str,
    /// Set, with nothing else, when there is nothing to draw.
    pub empty: Option<String>,
}

impl SpotPerpCvdView {
    /// The pane saying `text` and nothing else.
    #[must_use]
    pub fn empty_state(text: &str) -> Self {
        Self {
            perp_line: Vec::new(),
            spot_line: Vec::new(),
            perp_token: Token::DrawCyan,
            spot_token: Token::DrawViolet,
            zero_y_px: None,
            shaded: Vec::new(),
            perp_legend: WidgetCell::new("", Token::DrawCyan),
            spot_legend: WidgetCell::new("", Token::DrawViolet),
            note: SPOT_PERP_NOTE,
            empty: Some(text.to_owned()),
        }
    }
}

/// How the model is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpotPerpCvdLayout<'a> {
    /// What the volume is counted in: `BTC`.
    pub base_unit: &'a str,
    pub plot_width_px: f32,
    pub plot_height_px: f32,
}

/// Lay `model` out; the empty state when the feed provides no pair.
#[must_use]
pub fn spot_perp_cvd_view(
    model: Option<&SpotPerpCvd>,
    layout: &SpotPerpCvdLayout<'_>,
) -> SpotPerpCvdView {
    let Some(model) = model else {
        return SpotPerpCvdView::empty_state(SPOT_PERP_NOT_PROVIDED);
    };
    let all = || model.perp.iter().chain(&model.spot);
    let (Some(t0), Some(t1)) = (
        all().map(|point| point.time_ms).min(),
        all().map(|point| point.time_ms).max(),
    ) else {
        return SpotPerpCvdView::empty_state(SPOT_PERP_WAITING);
    };
    let (lo, hi) = value_range(all().map(|point| point.value)).unwrap_or((0.0, 0.0));
    let size = (layout.plot_width_px, layout.plot_height_px);
    let line = |points: &[CvdPoint]| {
        line_px(
            points.iter().map(|point| (point.time_ms, point.value)),
            (t0, t1),
            (lo, hi),
            size,
        )
    };
    let x_of = |time_ms| line_px([(time_ms, 0.0)], (t0, t1), (lo, hi), size)[0][0];
    let zero_y_px = (lo <= 0.0 && 0.0 <= hi && hi > lo)
        .then(|| line_px([(t0, 0.0)], (t0, t1), (lo, hi), size)[0][1]);
    SpotPerpCvdView {
        perp_line: line(&model.perp),
        spot_line: line(&model.spot),
        perp_token: Token::DrawCyan,
        spot_token: Token::DrawViolet,
        zero_y_px,
        shaded: model
            .opposite_spans
            .iter()
            .map(|span| [x_of(span.start_ms), x_of(span.end_ms)])
            .collect(),
        perp_legend: legend("perp", &model.perp, layout.base_unit, Token::DrawCyan),
        spot_legend: legend("spot", &model.spot, layout.base_unit, Token::DrawViolet),
        note: SPOT_PERP_NOTE,
        empty: None,
    }
}

/// `— perp -2341 BTC`: the leg's latest CVD, signed.
fn legend(leg: &str, points: &[CvdPoint], base_unit: &str, token: Token) -> WidgetCell {
    let value = match points.last() {
        Some(point) => {
            let text = fmt_qty(Decimal::from_f64_retain(point.value).unwrap_or(Decimal::ZERO));
            if point.value > 0.0 {
                format!("+{text}")
            } else {
                text
            }
        }
        None => "—".to_owned(),
    };
    WidgetCell::new(
        format!("— {leg} {value} {base_unit}").trim_end().to_owned(),
        token,
    )
}
