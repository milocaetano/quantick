use rust_decimal::Decimal;

use super::open_interest::{FundingState, OpenInterestParams, open_interest};
use super::ports::{FundingSnapshot, OpenInterestOrigin, OpenInterestSample};

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn sample(timestamp_ms: i64, open_interest: &str, price: &str) -> OpenInterestSample {
    OpenInterestSample {
        timestamp_ms,
        open_interest: d(open_interest),
        price: d(price),
    }
}

fn samples() -> Vec<OpenInterestSample> {
    // Out of order on purpose: the model sorts.
    vec![
        sample(3_900_000, "1020", "98"),
        sample(0, "1000", "100"),
        sample(4_200_000, "980", "97"),
        sample(1_800_000, "1100", "101"),
        sample(3_600_000, "1050", "99"),
    ]
}

/// Eight-hourly funding, the next settlement 03:12:44 after the clock.
const NOW_MS: i64 = 4_230_000;
const NEXT_FUNDING_MS: i64 = NOW_MS + 11_564_000;

fn funding() -> FundingSnapshot {
    FundingSnapshot {
        timestamp_ms: 4_200_000,
        rate: d("0.0001"),
        predicted_rate: Some(d("0.000071")),
        next_funding_ms: NEXT_FUNDING_MS,
        interval_ms: 28_800_000,
    }
}

const PARAMS: OpenInterestParams = OpenInterestParams {
    step_ms: 1_800_000,
    max_points: 2,
};

const POLLED: OpenInterestOrigin = OpenInterestOrigin::VenueRest {
    poll_interval_ms: 5_000,
};

#[test]
fn oi_change_and_funding_countdown() {
    let model = open_interest(&samples(), Some(&funding()), POLLED, &PARAMS, NOW_MS);

    assert_eq!(model.latest, Some(sample(4_200_000, "980", "97")));
    assert_eq!(model.notional, Some(d("95060")));
    // Against the newest reading at least an hour older than the latest.
    assert_eq!(model.oi_change_1h, Some(d("-0.02")));
    assert_eq!(model.price_change_1h, Some(d("-0.03")));
    assert_eq!(
        model.funding,
        Some(FundingState {
            rate: d("0.0001"),
            predicted_rate: Some(d("0.000071")),
            annualised_rate: d("0.1095"),
            countdown_ms: 11_564_000,
        })
    );
    assert_eq!(model.provenance, "OI polled every 5 s from venue REST");
    // The last reading of each half hour, the newest two, oldest first.
    assert_eq!(
        model.series,
        vec![
            sample(1_800_000, "1100", "101"),
            sample(4_200_000, "980", "97")
        ]
    );
}

#[test]
fn a_settlement_already_past_counts_down_to_zero() {
    let late = NEXT_FUNDING_MS + 1_000;
    let model = open_interest(&samples(), Some(&funding()), POLLED, &PARAMS, late);
    assert_eq!(model.funding.map(|f| f.countdown_ms), Some(0));
}

#[test]
fn less_than_an_hour_of_history_has_no_change() {
    let short = [sample(0, "1000", "100"), sample(3_000_000, "990", "99")];
    let model = open_interest(
        &short,
        None,
        OpenInterestOrigin::VenueStream,
        &PARAMS,
        NOW_MS,
    );
    assert_eq!(model.oi_change_1h, None);
    assert_eq!(model.price_change_1h, None);
    assert_eq!(model.funding, None);
    assert_eq!(model.provenance, "OI streamed by the venue");
}

#[test]
fn no_samples_is_an_empty_model() {
    let model = open_interest(&[], None, POLLED, &PARAMS, NOW_MS);
    assert_eq!(model.latest, None);
    assert_eq!(model.notional, None);
    assert!(model.series.is_empty());
}
