//! The OI + Funding pane's model: the latest open interest and its notional,
//! the change over the last hour of readings, the funding rate with its
//! annualised figure and the countdown to settlement on a clock the caller
//! passes in, how the readings reached us, and the OI and price series.
//!
//! The hour is measured between readings, not from the clock: the latest
//! reading against the newest one at least an hour older, and at most
//! [`OI_CHANGE_MAX_SPAN_MS`] older. With less history than that, or a gap in
//! it that would stretch the figure past that, there is no change to show,
//! and the model says none.
//!
//! A funding snapshot whose settlement time has passed is stale: it still
//! names the settled period's rate. The model says so instead of showing it
//! as the next one.

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use super::ports::{FundingSnapshot, OpenInterestOrigin, OpenInterestSample};

/// The span the change figures cover.
pub const OI_CHANGE_SPAN_MS: i64 = 3_600_000;

/// The oldest a reference reading may be and still stand for an hour ago.
pub const OI_CHANGE_MAX_SPAN_MS: i64 = 5_400_000;

/// The year an annualised funding rate is scaled to.
pub const FUNDING_YEAR_MS: i64 = 365 * 86_400_000;

/// How the series is sampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenInterestParams {
    /// One series point per step: the last reading inside it.
    pub step_ms: i64,
    /// Most series points, the newest kept.
    pub max_points: usize,
}

/// The funding card's figures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingState {
    /// As a fraction per settlement.
    pub rate: Decimal,
    /// The venue's estimate of the settlement after the next, if published.
    pub predicted_rate: Option<Decimal>,
    /// `rate` scaled to [`FUNDING_YEAR_MS`] of settlements; nothing when
    /// the venue gave no usable interval.
    pub annualised_rate: Option<Decimal>,
    /// Until the next settlement; zero once its time has passed.
    pub countdown_ms: i64,
    /// The settlement this snapshot announced has passed and no newer
    /// snapshot arrived: `rate` is the settled period's, not the next one's.
    pub stale: bool,
}

/// The pane's model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenInterest {
    /// The newest reading.
    pub latest: Option<OpenInterestSample>,
    /// Latest open interest times its price, in quote units.
    pub notional: Option<Decimal>,
    /// Open interest change over [`OI_CHANGE_SPAN_MS`], as a fraction.
    pub oi_change_1h: Option<Decimal>,
    /// Price change over the same readings, as a fraction.
    pub price_change_1h: Option<Decimal>,
    pub funding: Option<FundingState>,
    /// Where the readings came from, said plainly.
    pub provenance: String,
    /// Oldest first, one reading per [`OpenInterestParams::step_ms`].
    pub series: Vec<OpenInterestSample>,
}

/// Build the model from `samples` (any order), the latest `funding`
/// snapshot and the clock `now_ms`.
#[must_use]
pub fn open_interest(
    samples: &[OpenInterestSample],
    funding: Option<&FundingSnapshot>,
    origin: OpenInterestOrigin,
    params: &OpenInterestParams,
    now_ms: i64,
) -> OpenInterest {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| {
        (a.timestamp_ms, a.open_interest, a.price).cmp(&(b.timestamp_ms, b.open_interest, b.price))
    });
    let latest = sorted.last().cloned();
    let reference = latest.as_ref().and_then(|latest| {
        let cutoff = latest.timestamp_ms.saturating_sub(OI_CHANGE_SPAN_MS);
        sorted
            .iter()
            .rev()
            .find(|sample| sample.timestamp_ms <= cutoff)
            .filter(|sample| latest.timestamp_ms - sample.timestamp_ms <= OI_CHANGE_MAX_SPAN_MS)
    });
    let (oi_change_1h, price_change_1h) = match (&latest, reference) {
        (Some(latest), Some(reference)) => (
            change(reference.open_interest, latest.open_interest),
            change(reference.price, latest.price),
        ),
        _ => (None, None),
    };
    OpenInterest {
        notional: latest.as_ref().map(|s| s.open_interest * s.price),
        latest,
        oi_change_1h,
        price_change_1h,
        funding: funding.map(|snapshot| funding_state(snapshot, now_ms)),
        provenance: provenance(origin),
        series: series(&sorted, params),
    }
}

/// `(to - from) / from`; nothing from zero.
fn change(from: Decimal, to: Decimal) -> Option<Decimal> {
    (!from.is_zero()).then(|| (to - from) / from)
}

fn funding_state(snapshot: &FundingSnapshot, now_ms: i64) -> FundingState {
    let annualised_rate = (snapshot.interval_ms > 0).then(|| {
        snapshot.rate * Decimal::from(FUNDING_YEAR_MS) / Decimal::from(snapshot.interval_ms)
    });
    FundingState {
        rate: snapshot.rate,
        predicted_rate: snapshot.predicted_rate,
        annualised_rate,
        countdown_ms: snapshot.next_funding_ms.saturating_sub(now_ms).max(0),
        stale: now_ms >= snapshot.next_funding_ms,
    }
}

fn provenance(origin: OpenInterestOrigin) -> String {
    match origin {
        OpenInterestOrigin::VenueRest { poll_interval_ms } if poll_interval_ms % 1_000 == 0 => {
            format!(
                "OI polled every {} s from venue REST",
                poll_interval_ms / 1_000
            )
        }
        OpenInterestOrigin::VenueRest { poll_interval_ms } => {
            format!("OI polled every {poll_interval_ms} ms from venue REST")
        }
        OpenInterestOrigin::VenueStream => "OI streamed by the venue".to_owned(),
    }
}

/// The last reading of each step, the newest `max_points`, oldest first.
fn series(sorted: &[OpenInterestSample], params: &OpenInterestParams) -> Vec<OpenInterestSample> {
    let step = params.step_ms.max(1);
    let mut by_step: BTreeMap<i64, &OpenInterestSample> = BTreeMap::new();
    for sample in sorted {
        by_step.insert(sample.timestamp_ms.div_euclid(step), sample);
    }
    let skip = by_step.len().saturating_sub(params.max_points);
    by_step.into_values().skip(skip).cloned().collect()
}
