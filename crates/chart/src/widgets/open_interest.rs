//! The OI + Funding pane's view-model: the open interest and funding cards,
//! the price and OI lines each on its own scale, the reading rule their
//! hour of change falls under, and where the readings came from.

use quantick_appearance::Token;
use quantick_engine::display::round_for_display;
use quantick_orderflow::widgets::open_interest::{FundingState, OpenInterest};
use quantick_orderflow::widgets::ports::OpenInterestSample;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::big_trades::NotionalUnit;
use super::{WidgetCell, line_px, price_text, value_range};
use crate::geometry::compact_value;

/// What the pane says on a feed with no open interest or funding.
pub const OPEN_INTEREST_NOT_PROVIDED: &str = "feed does not provide open interest or funding";

/// What the pane says on a feed that provides it before the first reading.
pub const OPEN_INTEREST_WAITING: &str = "waiting for the first open interest reading";

/// Written after the reading rule, so nobody takes it for a signal.
pub const READING_NOTE: &str = "(reading rule, not a signal)";

/// The funding card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingCardView {
    /// `03:12:44` until the next settlement.
    pub countdown: WidgetCell,
    /// `+0.0100%`, buy colour when longs pay, sell colour when shorts do.
    pub rate: WidgetCell,
    /// The venue's estimate, when it publishes one.
    pub predicted: Option<WidgetCell>,
    /// `10.95%/yr`.
    pub annualised: WidgetCell,
}

/// The whole pane body.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenInterestView {
    /// `92418 BTC`.
    pub open_interest: WidgetCell,
    /// `-1.84% 1h`; `— 1h` with under an hour of readings.
    pub oi_change: WidgetCell,
    /// `$6.98B`.
    pub notional: WidgetCell,
    pub funding: Option<FundingCardView>,
    /// `price ↓ · OI ↓ = longs closing`, followed by [`READING_NOTE`];
    /// nothing when either moved by nothing or there is no hour yet.
    pub reading: Option<String>,
    /// `price 75984.32`, over the plot's left corner.
    pub price_label: String,
    /// `OI 92418`, over the plot's right corner.
    pub oi_label: String,
    /// Pixels in the plot, oldest first, [`Token::TextMuted`].
    pub price_line: Vec<[f32; 2]>,
    /// Pixels in the plot, oldest first, [`Token::Accent`].
    pub oi_line: Vec<[f32; 2]>,
    /// `OI polled every 5 s from venue REST`.
    pub provenance: WidgetCell,
    /// Set, with nothing else, when there is nothing to show.
    pub empty: Option<String>,
}

impl OpenInterestView {
    /// The pane saying `text` and nothing else.
    #[must_use]
    pub fn empty_state(text: &str) -> Self {
        let blank = WidgetCell::new("", Token::TextFaint);
        Self {
            open_interest: blank.clone(),
            oi_change: blank.clone(),
            notional: blank.clone(),
            funding: None,
            reading: None,
            price_label: String::new(),
            oi_label: String::new(),
            price_line: Vec::new(),
            oi_line: Vec::new(),
            provenance: blank,
            empty: Some(text.to_owned()),
        }
    }
}

/// How the model is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenInterestLayout<'a> {
    /// Prices are written at this tick's precision.
    pub tick: Decimal,
    /// What the open interest is counted in: `BTC`.
    pub base_unit: &'a str,
    pub unit: NotionalUnit,
    pub plot_width_px: f32,
    pub plot_height_px: f32,
}

/// Lay `model` out; the empty state when the feed provides none.
#[must_use]
pub fn open_interest_view(
    model: Option<&OpenInterest>,
    layout: &OpenInterestLayout<'_>,
) -> OpenInterestView {
    let Some(model) = model else {
        return OpenInterestView::empty_state(OPEN_INTEREST_NOT_PROVIDED);
    };
    let Some(latest) = &model.latest else {
        return OpenInterestView::empty_state(OPEN_INTEREST_WAITING);
    };
    let oi_text = round_for_display(latest.open_interest).normalize();
    let oi_change = match model.oi_change_1h {
        Some(change) => WidgetCell::new(format!("{} 1h", percent(change, 2)), signed_token(change)),
        None => WidgetCell::new("— 1h", Token::TextFaint),
    };
    let notional = model.notional.map_or_else(
        || "—".to_owned(),
        |value| {
            format!(
                "{}{}",
                layout.unit.prefix,
                compact_value(value.to_f64().unwrap_or(f64::NAN))
            )
        },
    );
    let (t0, t1) = match (model.series.first(), model.series.last()) {
        (Some(first), Some(last)) => (first.timestamp_ms, last.timestamp_ms),
        _ => (0, 0),
    };
    let size = (layout.plot_width_px, layout.plot_height_px);
    let line = |value: fn(&OpenInterestSample) -> Decimal| {
        let points: Vec<(i64, f64)> = model
            .series
            .iter()
            .map(|sample| {
                (
                    sample.timestamp_ms,
                    value(sample).to_f64().unwrap_or(f64::NAN),
                )
            })
            .collect();
        let range = value_range(points.iter().map(|(_, v)| *v)).unwrap_or((0.0, 0.0));
        line_px(points, (t0, t1), range, size)
    };
    OpenInterestView {
        open_interest: WidgetCell::new(
            format!("{oi_text} {}", layout.base_unit)
                .trim_end()
                .to_owned(),
            Token::TextPrimary,
        ),
        oi_change,
        notional: WidgetCell::new(notional, Token::TextFaint),
        funding: model.funding.as_ref().map(funding_card),
        reading: reading(model.price_change_1h, model.oi_change_1h)
            .map(|(price, oi, says)| format!("price {price} · OI {oi} = {says}")),
        price_label: format!("price {}", price_text(latest.price, layout.tick)),
        oi_label: format!("OI {oi_text}"),
        price_line: line(|sample| sample.price),
        oi_line: line(|sample| sample.open_interest),
        provenance: WidgetCell::new(model.provenance.clone(), Token::DrawViolet),
        empty: None,
    }
}

fn funding_card(funding: &FundingState) -> FundingCardView {
    FundingCardView {
        countdown: WidgetCell::new(countdown_text(funding.countdown_ms), Token::TextPrimary),
        rate: WidgetCell::new(percent(funding.rate, 4), signed_token(funding.rate)),
        predicted: funding
            .predicted_rate
            .map(|rate| WidgetCell::new(percent(rate, 4), signed_token(rate))),
        annualised: WidgetCell::new(
            format!(
                "{:.2}%/yr",
                (funding.annualised_rate * Decimal::ONE_HUNDRED).round_dp(2)
            ),
            Token::TextFaint,
        ),
    }
}

/// The classic reading of price and open interest moving together, as
/// arrows and what it is usually read as.
fn reading(
    price: Option<Decimal>,
    oi: Option<Decimal>,
) -> Option<(&'static str, &'static str, &'static str)> {
    let (price, oi) = (price?, oi?);
    if price.is_zero() || oi.is_zero() {
        return None;
    }
    Some(match (price > Decimal::ZERO, oi > Decimal::ZERO) {
        (true, true) => ("↑", "↑", "new longs"),
        (true, false) => ("↑", "↓", "shorts closing"),
        (false, true) => ("↓", "↑", "new shorts"),
        (false, false) => ("↓", "↓", "longs closing"),
    })
}

/// A fraction as a signed percentage: `+0.0100%`, `-1.84%`.
fn percent(fraction: Decimal, places: u32) -> String {
    let value = (fraction * Decimal::ONE_HUNDRED).round_dp(places);
    let sign = if value > Decimal::ZERO { "+" } else { "" };
    format!("{sign}{value:.prec$}%", prec = places as usize)
}

fn signed_token(value: Decimal) -> Token {
    if value > Decimal::ZERO {
        Token::Buy
    } else if value < Decimal::ZERO {
        Token::Sell
    } else {
        Token::TextMuted
    }
}

/// `03:12:44`; hours run past 24 rather than wrap.
fn countdown_text(countdown_ms: i64) -> String {
    let seconds = countdown_ms.max(0) / 1_000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds % 3_600 / 60,
        seconds % 60
    )
}
