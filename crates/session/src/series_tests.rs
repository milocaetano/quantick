//! The venue-prefix install: what it does to a pane's series and the view
//! effects it hands the window, in order.

use quantick_chart::state::ChartState;
use quantick_chart_interaction::pane_history::{HistoryState, PrefixInstallEffect};
use quantick_engine::time_bucket::DAY_MS;
use quantick_engine::{Bar, BarSpec, Side, Trade};
use rust_decimal::Decimal;

use crate::series::{PaneSeries, PrefixInstall, seam_lead_action};

const HOUR: i64 = 3_600_000;
/// 06:14:03 on day 9 after the epoch.
const FIRST_PRINT: i64 = 9 * DAY_MS + 6 * HOUR + 14 * 60_000 + 3_000;

fn print(at: i64, price: i64) -> Trade {
    Trade {
        agg_id: u64::try_from(at).unwrap(),
        timestamp_ms: at,
        price: Decimal::from(price),
        quantity: Decimal::ONE,
        side: Side::Buy,
    }
}

fn bar(open_time: i64, close_time: i64) -> Bar {
    Bar {
        open_time,
        close_time,
        open: Decimal::from(90),
        high: Decimal::from(130),
        low: Decimal::from(80),
        close: Decimal::from(99),
        buy_volume: Decimal::from(40),
        sell_volume: Decimal::from(60),
        trade_count: 500,
    }
}

/// The venue's 00:00 to 06:13 of day 9, before the first print.
fn lead() -> Bar {
    bar(9 * DAY_MS, FIRST_PRINT - 4_000)
}

/// Three venue days before the seam day.
fn prefix(days: i64) -> Vec<Bar> {
    (9 - days..9)
        .map(|day| bar(day * DAY_MS, (day + 1) * DAY_MS - 1))
        .collect()
}

/// A daily pane whose only bar is the seam day, still forming.
fn series() -> PaneSeries {
    let mut state = ChartState::new(BarSpec::Time(DAY_MS));
    state.ingest_backfill(&[print(FIRST_PRINT, 100), print(FIRST_PRINT + HOUR, 101)]);
    assert!(state.partial().is_some(), "the seam bar is forming");
    PaneSeries::new(state)
}

#[test]
fn prefix_install_returns_its_view_effects() {
    use PrefixInstallEffect::{
        RebuildIndicators, ShiftDrawings, ShiftIndicatorRows, ShiftViewport,
    };
    let mut series = series();
    let mut history = HistoryState::default();

    let install = series.install_venue_prefix(&mut history, 1, prefix(3), Some(lead()), true, None);
    assert_eq!(
        install,
        PrefixInstall {
            changed: true,
            effects: vec![
                ShiftViewport(3),
                ShiftDrawings(3),
                ShiftIndicatorRows(3),
                RebuildIndicators,
            ],
        }
    );
    assert_eq!(series.history_prefix.len(), 3);
    assert_eq!(series.state.venue_lead(), Some(&lead()));
    assert_eq!(series.state.partial().unwrap().open_time, 9 * DAY_MS);
    assert_eq!(history.revision(), 1);

    // The same prefix and lead again: nothing to apply.
    let install = series.install_venue_prefix(&mut history, 1, prefix(3), Some(lead()), true, None);
    assert_eq!(
        install,
        PrefixInstall {
            changed: false,
            effects: Vec::new(),
        }
    );

    // Only the lead taken away: the bars stay put, the indicators rebuild.
    let install = series.install_venue_prefix(&mut history, 1, prefix(3), None, false, None);
    assert_eq!(
        install,
        PrefixInstall {
            changed: true,
            effects: vec![RebuildIndicators],
        }
    );
    assert_eq!(series.state.venue_lead(), None);

    // A shorter prefix moves the view back; indicator rows wait for the rebuild.
    let install = series.install_venue_prefix(&mut history, 1, prefix(1), None, false, None);
    assert_eq!(
        install.effects,
        [ShiftViewport(-2), ShiftDrawings(-2), RebuildIndicators]
    );
    assert_eq!(series.history_prefix.len(), 1);
    assert_eq!(history.revision(), 3);
}

#[test]
fn bar_config_spells_the_rule_the_engine_cuts_by() {
    assert_eq!(
        series().bar_config(),
        BarSpec::Time(DAY_MS).to_config_string()
    );
}

/// The seam line speaks when the seated lead changed, or when there is none
/// for a reason other than an empty bucket; an unchanged seat stays quiet.
#[test]
fn the_seam_line_speaks_only_for_a_change_or_a_missing_lead() {
    let (old, new) = (lead(), bar(9 * DAY_MS, FIRST_PRINT - 8_000));
    let cases = [
        ("merged", None, Some(&old), Some("merged_into_seam_bar")),
        (
            "merged",
            Some(&old),
            Some(&new),
            Some("merged_into_seam_bar"),
        ),
        ("merged", Some(&old), Some(&old), None),
        (
            "lead_after_first_trade",
            Some(&old),
            None,
            Some("lead_after_first_trade"),
        ),
        (
            "lead_after_first_trade",
            None,
            None,
            Some("lead_after_first_trade"),
        ),
        ("nothing_before_first_trade", None, None, None),
        (
            "nothing_before_first_trade",
            Some(&old),
            None,
            Some("nothing_before_first_trade"),
        ),
    ];
    for (token, had, seated, action) in cases {
        assert_eq!(
            seam_lead_action(token, had, seated),
            action,
            "{token} {had:?} {seated:?}"
        );
    }
}
