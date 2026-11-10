use quantick_appearance::Token;
use quantick_civil::TzOffset;
use quantick_indicators::spot_perp_cvd::{CvdPoint, OppositeSpan, SpotPerpCvd};
use quantick_orderflow::widgets::liquidations::{
    LiquidationBucket, LiquidationTotals, Liquidations,
};
use quantick_orderflow::widgets::open_interest::{FundingState, OpenInterest};
use quantick_orderflow::widgets::ports::{LiquidatedSide, LiquidationPrint, OpenInterestSample};
use rust_decimal::Decimal;

use super::WidgetCell;
use super::big_trades::NotionalUnit;
use super::liquidations::{
    LIQUIDATIONS_NOT_PROVIDED, LiquidationBarView, LiquidationRowView, LiquidationTotalsView,
    LiquidationsLayout, liquidations_view,
};
use super::open_interest::{
    FUNDING_STALE, OPEN_INTEREST_NOT_PROVIDED, OpenInterestLayout, READING_NOTE, open_interest_view,
};
use super::spot_perp_cvd::{SPOT_PERP_NOT_PROVIDED, SpotPerpCvdLayout, spot_perp_cvd_view};

fn d(text: &str) -> Decimal {
    text.parse().expect("fixture decimal")
}

fn cell(text: &str, token: Token) -> WidgetCell {
    WidgetCell {
        text: text.to_owned(),
        token,
    }
}

/// 2026-11-10 00:13:52 UTC.
const T0: i64 = 1_794_269_632_000;

fn liquidations_model() -> Liquidations {
    let totals = |window_ms, long: &str, short: &str| LiquidationTotals {
        window_ms,
        long_notional: d(long),
        short_notional: d(short),
    };
    let bucket = |start_ms, long: &str, short: &str| LiquidationBucket {
        start_ms,
        long_notional: d(long),
        short_notional: d(short),
    };
    Liquidations {
        totals: [
            totals(60_000, "420000", "30000"),
            totals(300_000, "2910000", "180000"),
            totals(3_600_000, "11600000", "3400000"),
        ],
        buckets: vec![bucket(0, "100", "0"), bucket(60_000, "50", "200")],
        rows: vec![
            LiquidationPrint {
                timestamp_ms: T0,
                side: LiquidatedSide::Long,
                price: d("75979.8"),
                quantity: d("12.4"),
            },
            LiquidationPrint {
                timestamp_ms: T0 - 5_000,
                side: LiquidatedSide::Short,
                price: d("75992.1"),
                quantity: d("0.48"),
            },
        ],
    }
}

fn liquidations_layout() -> LiquidationsLayout {
    LiquidationsLayout {
        tick: d("0.1"),
        tz: TzOffset::new(0),
        unit: NotionalUnit::USD,
        histogram_width_px: 100.0,
        histogram_height_px: 40.0,
    }
}

fn oi_model() -> OpenInterest {
    let sample = |timestamp_ms, oi: &str, price: &str| OpenInterestSample {
        timestamp_ms,
        open_interest: d(oi),
        price: d(price),
    };
    OpenInterest {
        latest: Some(sample(T0, "92418", "75984.32")),
        notional: Some(d("6980000000")),
        oi_change_1h: Some(d("-0.0184")),
        price_change_1h: Some(d("-0.003")),
        funding: Some(FundingState {
            rate: d("0.0001"),
            predicted_rate: Some(d("0.000071")),
            annualised_rate: Some(d("0.1095")),
            countdown_ms: 11_564_000,
            stale: false,
        }),
        provenance: "OI polled every 5 s from venue REST".to_owned(),
        series: vec![
            sample(0, "100", "10"),
            sample(50, "200", "20"),
            sample(100, "150", "30"),
        ],
    }
}

fn oi_layout() -> OpenInterestLayout<'static> {
    OpenInterestLayout {
        tick: d("0.01"),
        base_unit: "BTC",
        unit: NotionalUnit::USD,
        plot_width_px: 100.0,
        plot_height_px: 50.0,
    }
}

fn cvd_model() -> SpotPerpCvd {
    let points = |values: &[(i64, f64)]| {
        values
            .iter()
            .map(|&(time_ms, value)| CvdPoint { time_ms, value })
            .collect()
    };
    SpotPerpCvd {
        perp: points(&[(0, 5.0), (60_000, -3.0), (120_000, -7.0), (180_000, 3.0)]),
        spot: points(&[
            (30_000, 2.0),
            (90_000, 3.0),
            (150_000, 4.0),
            (210_000, -2.0),
        ]),
        opposite_spans: vec![
            OppositeSpan {
                start_ms: 60_000,
                end_ms: 180_000,
                ongoing: false,
            },
            OppositeSpan {
                start_ms: 210_000,
                end_ms: 210_000,
                ongoing: true,
            },
        ],
    }
}

fn cvd_layout() -> SpotPerpCvdLayout<'static> {
    SpotPerpCvdLayout {
        base_unit: "BTC",
        plot_width_px: 210.0,
        plot_height_px: 104.0,
    }
}

#[test]
fn views_match_golden_and_empty_state() {
    // Liquidations.
    let view = liquidations_view(Some(&liquidations_model()), &liquidations_layout());
    assert_eq!(view.columns, ["TIME", "LIQUIDATED", "PRICE", "SIZE", "USD"]);
    let totals = |label: &str, long: &str, short: &str| LiquidationTotalsView {
        label: label.to_owned(),
        long: cell(long, Token::Sell),
        short: cell(short, Token::Buy),
    };
    assert_eq!(
        view.totals,
        vec![
            totals("1m", "L $420.0k", "S $30.0k"),
            totals("5m", "L $2.9M", "S $180.0k"),
            totals("1h", "L $11.6M", "S $3.4M"),
        ]
    );
    // Longs hang below the baseline, shorts stand on it; the largest side
    // of any minute fills half the height.
    assert_eq!(view.baseline_y_px, 20.0);
    assert_eq!(
        view.bars,
        vec![
            LiquidationBarView {
                x_px: 1.0,
                width_px: 48.0,
                long_px: 10.0,
                short_px: 0.0,
            },
            LiquidationBarView {
                x_px: 51.0,
                width_px: 48.0,
                long_px: 5.0,
                short_px: 20.0,
            },
        ]
    );
    assert_eq!(
        view.rows,
        vec![
            LiquidationRowView {
                time: cell("00:13:52", Token::TextFaint),
                side: cell("LONG", Token::Sell),
                price: cell("75979.8", Token::TextPrimary),
                size: cell("12.4", Token::Sell),
                notional: cell("$942.1k", Token::TextPrimary),
                emphasised: true,
            },
            LiquidationRowView {
                time: cell("00:13:47", Token::TextFaint),
                side: cell("SHORT", Token::Buy),
                price: cell("75992.1", Token::TextMuted),
                size: cell("0.48", Token::Buy),
                notional: cell("$36.5k", Token::TextMuted),
                emphasised: false,
            },
        ]
    );
    assert_eq!(view.empty, None);
    let empty = liquidations_view(None, &liquidations_layout());
    assert_eq!(empty.empty.as_deref(), Some(LIQUIDATIONS_NOT_PROVIDED));
    assert!(empty.totals.is_empty() && empty.bars.is_empty() && empty.rows.is_empty());

    // OI + Funding.
    let view = open_interest_view(Some(&oi_model()), &oi_layout());
    assert_eq!(view.open_interest, cell("92418 BTC", Token::TextPrimary));
    assert_eq!(view.oi_change, cell("-1.84% 1h", Token::Sell));
    assert_eq!(view.notional, cell("$6.98B", Token::TextFaint));
    let funding = view.funding.expect("funding card");
    assert_eq!(funding.countdown, cell("3:12:44", Token::TextPrimary));
    assert_eq!(funding.rate, cell("+0.0100%", Token::Buy));
    assert_eq!(funding.predicted, Some(cell("+0.0071%", Token::Buy)));
    assert_eq!(funding.annualised, cell("10.95%/yr", Token::TextFaint));
    assert_eq!(
        view.reading.as_deref(),
        Some("price ↓ · OI ↓ = longs closing")
    );
    assert_eq!(READING_NOTE, "(reading rule, not a signal)");
    assert_eq!(view.price_label, "price 75984.32");
    assert_eq!(view.oi_label, "OI 92418");
    assert_eq!(
        view.provenance,
        cell("OI polled every 5 s from venue REST", Token::DrawViolet)
    );
    // Each line on its own scale, four pixels in from top and bottom.
    assert_eq!(view.oi_line, vec![[0.0, 46.0], [50.0, 4.0], [100.0, 25.0]]);
    assert_eq!(
        view.price_line,
        vec![[0.0, 46.0], [50.0, 25.0], [100.0, 4.0]]
    );
    assert_eq!(view.empty, None);
    let empty = open_interest_view(None, &oi_layout());
    assert_eq!(empty.empty.as_deref(), Some(OPEN_INTEREST_NOT_PROVIDED));
    assert!(empty.funding.is_none() && empty.oi_line.is_empty());

    // Spot vs Perp CVD.
    let view = spot_perp_cvd_view(Some(&cvd_model()), &cvd_layout());
    // One scale for both lines: 5 at the top pad, -7 at the bottom one.
    assert_eq!(
        view.perp_line,
        vec![[0.0, 4.0], [60.0, 68.0], [120.0, 100.0], [180.0, 20.0]]
    );
    assert_eq!(
        view.spot_line,
        vec![[30.0, 28.0], [90.0, 20.0], [150.0, 12.0], [210.0, 60.0]]
    );
    assert_eq!(view.perp_token, Token::DrawCyan);
    assert_eq!(view.spot_token, Token::DrawViolet);
    assert_eq!(view.zero_y_px, Some(44.0));
    // The ongoing span opened on the last point: still drawn, at the edge.
    assert_eq!(view.shaded, vec![[60.0, 180.0], [207.0, 210.0]]);
    assert_eq!(view.perp_legend, cell("— perp +3 BTC", Token::DrawCyan));
    assert_eq!(view.spot_legend, cell("— spot -2 BTC", Token::DrawViolet));
    assert_eq!(view.note, "shaded = opposite signs");
    assert_eq!(view.empty, None);
    let empty = spot_perp_cvd_view(None, &cvd_layout());
    assert_eq!(empty.empty.as_deref(), Some(SPOT_PERP_NOT_PROVIDED));
    assert!(empty.perp_line.is_empty() && empty.shaded.is_empty());

    // Each "not provided" text says the feed lacks it, never that it is
    // still loading.
    for text in [
        LIQUIDATIONS_NOT_PROVIDED,
        OPEN_INTEREST_NOT_PROVIDED,
        SPOT_PERP_NOT_PROVIDED,
    ] {
        assert!(text.starts_with("feed does not provide"), "{text}");
    }
}

#[test]
fn oi_view_labels_what_it_does_not_know() {
    // Funding before the first OI reading still shows its card.
    let funding_only = OpenInterest {
        latest: None,
        notional: None,
        oi_change_1h: None,
        price_change_1h: None,
        series: Vec::new(),
        ..oi_model()
    };
    let view = open_interest_view(Some(&funding_only), &oi_layout());
    assert_eq!(view.empty, None);
    assert!(view.funding.is_some());
    assert!(view.oi_line.is_empty());

    // A settled snapshot no newer one replaced says so, in faint ink.
    let mut stale = oi_model();
    if let Some(funding) = stale.funding.as_mut() {
        funding.stale = true;
        funding.countdown_ms = 0;
        funding.annualised_rate = None;
    }
    let card = open_interest_view(Some(&stale), &oi_layout())
        .funding
        .expect("funding card");
    assert_eq!(card.countdown, cell(FUNDING_STALE, Token::TextFaint));
    assert_eq!(card.rate, cell("+0.0100%", Token::TextFaint));
    assert_eq!(card.annualised, cell("—/yr", Token::TextFaint));

    // A move that prints as zero is neither coloured nor read.
    let flat = OpenInterest {
        oi_change_1h: Some(d("-0.00001")),
        ..oi_model()
    };
    let view = open_interest_view(Some(&flat), &oi_layout());
    assert_eq!(view.oi_change, cell("0.00% 1h", Token::TextMuted));
    assert_eq!(view.reading, None);
}
