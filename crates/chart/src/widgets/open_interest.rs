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
use super::scene::{Align, INSET_PX, ROW_PX, TextSize, WidgetScene, row_y};
use super::{WidgetCell, amount, line_px, price_text, value_range};
use crate::bar_progress::countdown_text;

/// What the pane says on a feed with no open interest or funding.
pub const OPEN_INTEREST_NOT_PROVIDED: &str = "feed does not provide open interest or funding";

/// What the pane says on a feed that provides it before the first reading.
pub const OPEN_INTEREST_WAITING: &str = "waiting for the first open interest reading";

/// The countdown's place once the announced settlement has passed and no
/// newer funding arrived: the rate shown is the settled period's.
pub const FUNDING_STALE: &str = "settled, awaiting the next rate";

/// Places a change or annualised rate is printed at, in percent.
const CHANGE_PLACES: u32 = 2;

/// Places a funding rate is printed at, in percent.
const RATE_PLACES: u32 = 4;

/// Written after the reading rule, so nobody takes it for a signal.
pub const READING_NOTE: &str = "(reading rule, not a signal)";

/// The funding card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingCardView {
    /// `3:12:44` until the next settlement, or [`FUNDING_STALE`].
    pub countdown: WidgetCell,
    /// `+0.0100%`, buy colour when longs pay, sell colour when shorts do,
    /// faint when stale.
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

/// Lay `model` out; the empty state when the feed provides none. Funding
/// without a first OI reading still shows its card.
#[must_use]
pub fn open_interest_view(
    model: Option<&OpenInterest>,
    layout: &OpenInterestLayout<'_>,
) -> OpenInterestView {
    let Some(model) = model else {
        return OpenInterestView::empty_state(OPEN_INTEREST_NOT_PROVIDED);
    };
    let funding = model.funding.as_ref().map(funding_card);
    let Some(latest) = &model.latest else {
        if funding.is_none() {
            return OpenInterestView::empty_state(OPEN_INTEREST_WAITING);
        }
        return OpenInterestView {
            funding,
            provenance: WidgetCell::new(model.provenance.clone(), Token::DrawViolet),
            empty: None,
            ..OpenInterestView::empty_state("")
        };
    };
    let oi_text = round_for_display(latest.open_interest).normalize();
    let oi_change = match model.oi_change_1h {
        Some(change) => {
            let (text, token) = percent(change, CHANGE_PLACES);
            WidgetCell::new(format!("{text} 1h"), token)
        }
        None => WidgetCell::new("— 1h", Token::TextFaint),
    };
    let notional = model
        .notional
        .map_or_else(|| "—".to_owned(), |value| amount(value, layout.unit));
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
        funding,
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
    let (rate, rate_token) = percent(funding.rate, RATE_PLACES);
    let annualised = match funding.annualised_rate {
        Some(rate) => format!(
            "{}/yr",
            percent(rate, CHANGE_PLACES).0.trim_start_matches('+')
        ),
        None => "—/yr".to_owned(),
    };
    FundingCardView {
        countdown: if funding.stale {
            WidgetCell::new(FUNDING_STALE, Token::TextFaint)
        } else {
            WidgetCell::new(countdown_text(funding.countdown_ms), Token::TextPrimary)
        },
        rate: WidgetCell::new(
            rate,
            if funding.stale {
                Token::TextFaint
            } else {
                rate_token
            },
        ),
        predicted: funding.predicted_rate.map(|rate| {
            let (text, token) = percent(rate, RATE_PLACES);
            WidgetCell::new(text, token)
        }),
        annualised: WidgetCell::new(annualised, Token::TextFaint),
    }
}

/// The classic reading of price and open interest moving together, as
/// arrows and what it is usually read as. A move that prints as zero is
/// no move.
fn reading(
    price: Option<Decimal>,
    oi: Option<Decimal>,
) -> Option<(&'static str, &'static str, &'static str)> {
    let price = shown_percent(price?, CHANGE_PLACES);
    let oi = shown_percent(oi?, CHANGE_PLACES);
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

/// A fraction as the percentage figure printed at `places`; a figure that
/// rounds to zero is plain zero, never `-0`.
fn shown_percent(fraction: Decimal, places: u32) -> Decimal {
    let figure = (fraction * Decimal::ONE_HUNDRED).round_dp(places);
    if figure.is_zero() {
        Decimal::ZERO
    } else {
        figure
    }
}

/// A fraction as a signed percentage, `+0.0100%` or `-1.84%`, and the
/// colour of the printed figure's sign.
fn percent(fraction: Decimal, places: u32) -> (String, Token) {
    let figure = shown_percent(fraction, places);
    let (sign, token) = if figure > Decimal::ZERO {
        ("+", Token::Buy)
    } else if figure < Decimal::ZERO {
        ("", Token::Sell)
    } else {
        ("", Token::TextMuted)
    };
    (
        format!("{sign}{figure:.prec$}%", prec = places as usize),
        token,
    )
}

/// Rows above the plot (the two cards) and below it (the reading and the
/// provenance).
const OI_PLOT_ROWS_ABOVE: f32 = 3.5;
const OI_PLOT_ROWS_BELOW: f32 = 2.0;

/// The plot's size in a `width` by `height` body.
#[must_use]
pub fn open_interest_plot_px(width: f32, height: f32) -> (f32, f32) {
    let rows = OI_PLOT_ROWS_ABOVE + OI_PLOT_ROWS_BELOW;
    (width, (height - rows * ROW_PX).max(0.0))
}

impl OpenInterestView {
    /// Lay the pane out in a `width` by `height` body: the open interest
    /// and funding cards, the two lines on their own scales, the reading
    /// rule with [`READING_NOTE`] after it, and where the readings came
    /// from.
    #[must_use]
    pub fn paint(&self, width: f32, height: f32) -> WidgetScene {
        if let Some(empty) = self.empty.as_ref().filter(|text| !text.is_empty()) {
            return WidgetScene::empty(empty, Vec::new(), width, height);
        }
        let mut scene = WidgetScene::default();
        let (small, large) = (TextSize::Small, TextSize::Large);
        let faint = Token::TextFaint;
        let row = |index| row_y(0.0, index);
        if !self.open_interest.text.is_empty() {
            scene.sized(
                [INSET_PX, row(0)],
                "OPEN INTEREST",
                faint,
                Align::Left,
                small,
            );
            let oi = &self.open_interest;
            scene.sized([INSET_PX, row(1)], &oi.text, oi.token, Align::Left, large);
            scene.cell([INSET_PX, row(2)], &self.oi_change, Align::Left);
            scene.cell([width * 0.25, row(2)], &self.notional, Align::Left);
        }
        if let Some(funding) = &self.funding {
            let left = width / 2.0;
            scene.sized(
                [left, row(0)],
                "FUNDING · NEXT IN",
                faint,
                Align::Left,
                small,
            );
            scene.cell(
                [left + width * 0.22, row(0)],
                &funding.countdown,
                Align::Left,
            );
            let rate = &funding.rate;
            scene.sized([left, row(1)], &rate.text, rate.token, Align::Left, large);
            let mut x = left;
            for cell in funding.predicted.iter().chain([&funding.annualised]) {
                scene.cell([x, row(2)], cell, Align::Left);
                x += width * 0.16;
            }
        }
        let top = OI_PLOT_ROWS_ABOVE * ROW_PX;
        scene.sized(
            [INSET_PX, top],
            &self.price_label,
            faint,
            Align::Left,
            small,
        );
        let oi_at = [width - INSET_PX, top];
        scene.sized(oi_at, &self.oi_label, Token::Accent, Align::Right, small);
        scene.line(&self.price_line, top, Token::TextMuted);
        scene.line(&self.oi_line, top, Token::Accent);
        scene.rows = self.oi_line.len();
        if let Some(reading) = &self.reading {
            let at = [INSET_PX, height - 1.5 * ROW_PX];
            let text = format!("{reading} {READING_NOTE}");
            scene.sized(at, &text, Token::TextMuted, Align::Left, small);
        }
        let provenance = &self.provenance;
        let at = [INSET_PX, height - ROW_PX / 2.0];
        scene.sized(at, &provenance.text, provenance.token, Align::Left, small);
        scene
    }
}
