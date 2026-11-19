//! The pane series' changes — a venue prefix, older history, the indicator
//! rows drifting — and the view effects each hands the window, in order.

use quantick_chart::state::ChartState;
use quantick_chart_interaction::pane_history::{HistoryState, PrefixInstallEffect};
use quantick_engine::time_bucket::DAY_MS;
use quantick_engine::{Bar, BarSpec, Side, Trade};
use rust_decimal::Decimal;

use crate::series::{IndicatorRows, PaneSeries, PrefixInstall, SeriesEffect, seam_lead_action};

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
            effects: [
                ShiftViewport(3),
                ShiftDrawings(3),
                ShiftIndicatorRows(3),
                RebuildIndicators,
            ]
            .map(SeriesEffect::Prefix)
            .to_vec(),
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
            effects: vec![SeriesEffect::Prefix(RebuildIndicators)],
        }
    );
    assert_eq!(series.state.venue_lead(), None);

    // A shorter prefix moves the view back; indicator rows wait for the rebuild.
    let install = series.install_venue_prefix(&mut history, 1, prefix(1), None, false, None);
    assert_eq!(
        install.effects,
        [ShiftViewport(-2), ShiftDrawings(-2), RebuildIndicators].map(SeriesEffect::Prefix)
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

/// `prices` as consecutive prints a second apart, the first numbered `first`.
fn prints(first: u64, prices: impl IntoIterator<Item = i64>) -> Vec<Trade> {
    prices
        .into_iter()
        .zip(first..)
        .map(|(price, id)| {
            print(
                1_700_000_000_000 + i64::try_from(id).unwrap() * 1_000,
                price,
            )
        })
        .collect()
}

/// Older history re-cuts a Renko series' first bricks, so the view goes back
/// to the market time under its right edge — the brick that now holds it, at
/// the same offset — and the marks are re-hung by time, never shifted by
/// the count. A tick series' bars only shift by the bars the trades added.
#[test]
fn history_prepend_returns_its_reanchor() {
    use PrefixInstallEffect::{
        RebuildIndicators, ShiftDrawings, ShiftIndicatorRows, ShiftViewport,
    };
    use quantick_chart::state::SpecSelector;
    use quantick_chart::viewport::Viewport;
    use quantick_engine::bar_registry::BUILTIN_BARS;

    // Older: seventy prints at 120/121 read the step, then a fall to 99 cuts
    // eleven bricks down to 100. Held: seventy prints at 100/101 read the
    // step, then a climb cuts 100->102 at 103 and 102->104 at 105.
    let older = prints(0, (0..70).map(|i| 120 + i % 2).chain((99..=119).rev()));
    let held = prints(
        older.len() as u64,
        (0..70).map(|i| 100 + i % 2).chain(102..=105),
    );
    let spec = SpecSelector::new(BUILTIN_BARS.parse("renko:3").unwrap());
    let mut series = PaneSeries::new(ChartState::new(spec.spec()));
    let mut history = HistoryState::default();
    let _ = series.ingest_backfill(&mut history, &held);
    let mark_time = series.state.bars()[0].open_time;
    // The view parked on the first brick, off the live edge.
    let mut viewport = Viewport::default();
    let old_slots = series.read(&spec).slots();
    viewport.pan_pixels(1.0e6, old_slots);
    assert_eq!(
        series.read(&spec).right_edge_time(&viewport),
        Some(mark_time)
    );
    let (_, offset) = HistoryState::edge_anchor(&viewport, old_slots);

    let (added, effects) = series.prepend_history(&mut history, &viewport, &spec, &older);
    // The climb now reverses only at 105: 102->104 opens on the fall's last
    // print, eleven bricks in, and holds the mark's instant.
    assert_eq!((series.state.bars().len(), added), (12, 10));
    assert_eq!(series.read(&spec).slot_at_time(mark_time), Some(11));
    assert_eq!(
        effects,
        [
            SeriesEffect::ReanchorViewport {
                slot: Some(11),
                offset,
            },
            SeriesEffect::ReanchorDrawings(old_slots),
            SeriesEffect::Prefix(ShiftIndicatorRows(10)),
            SeriesEffect::PublishTapePriceStep,
            SeriesEffect::Prefix(RebuildIndicators),
        ]
    );
    assert_eq!(history.revision(), 2, "the backfill and the prepend");

    // A tick series: forty prints cut twenty bars, twenty older ones ten.
    let spec = SpecSelector::new(BarSpec::Tick(2));
    let mut series = PaneSeries::new(ChartState::new(spec.spec()));
    let _ = series.ingest_backfill(&mut history, &prints(20, (20..60).map(|i| 100 + i % 3)));
    let (added, effects) = series.prepend_history(
        &mut history,
        &viewport,
        &spec,
        &prints(0, (0..20).map(|i| 100 + i % 3)),
    );
    assert_eq!(added, 10);
    assert_eq!(
        effects,
        [ShiftViewport(10), ShiftDrawings(10), ShiftIndicatorRows(10),]
            .map(SeriesEffect::Prefix)
            .into_iter()
            .chain([
                SeriesEffect::PublishTapePriceStep,
                SeriesEffect::Prefix(RebuildIndicators),
            ])
            .collect::<Vec<_>>()
    );
}

/// Indicator rows standing in for the window's views: `rows` committed, a
/// disagreement answered once, and forgotten when the rows catch up.
struct Rows {
    rows: usize,
    answered: bool,
}

impl IndicatorRows for Rows {
    fn row_drift(&mut self, bars: usize) -> Option<usize> {
        self.answered &= self.rows != bars;
        (self.rows != bars && !self.answered).then_some(self.rows)
    }
    fn answer_row_drift(&mut self, bars: usize) -> Option<usize> {
        let rows = self.row_drift(bars)?;
        self.answered = true;
        Some(rows)
    }
}

/// Indicator rows behind the series' closed bars are normal while the
/// worker has work; settled and still behind, a rebuild is owed — once.
#[test]
fn indicator_row_drift_returns_its_answer() {
    // Four closed tick bars.
    let mut series = PaneSeries::new(ChartState::new(BarSpec::Tick(2)));
    let _ = series.ingest_backfill(
        &mut HistoryState::default(),
        &prints(0, (0..9).map(|i| 100 + i % 3)),
    );
    assert_eq!(series.state.bars().len(), 4);
    let asked = std::cell::Cell::new(0);
    let settle = |idle: bool| {
        let asked = &asked;
        move |_: &mut Rows| {
            asked.set(asked.get() + 1);
            idle
        }
    };
    let mut views = Rows {
        rows: 4,
        answered: false,
    };
    // In step: the worker is never asked.
    assert_eq!(series.row_drift_effect(1, &mut views, settle(true)), None);
    assert_eq!(asked.get(), 0);

    views.rows = 3;
    // Behind while the worker has work.
    assert_eq!(series.row_drift_effect(1, &mut views, settle(false)), None);
    // Settled and still behind: one rebuild.
    let rebuild = Some(SeriesEffect::Prefix(PrefixInstallEffect::RebuildIndicators));
    assert_eq!(
        series.row_drift_effect(1, &mut views, settle(true)),
        rebuild
    );
    // Said once per disagreement.
    assert_eq!(series.row_drift_effect(1, &mut views, settle(true)), None);
    assert_eq!(asked.get(), 2);

    // The settle takes in what the worker had published: caught up, no rebuild.
    let mut views = Rows {
        rows: 2,
        answered: false,
    };
    let caught_up = series.row_drift_effect(1, &mut views, |views| {
        views.rows = 4;
        true
    });
    assert_eq!((caught_up, views.answered), (None, false));
}

/// A ladder refold or drop asks for the indicators' replay — their `poc`
/// came from the ladders — and a switch that changes nothing does not.
#[test]
fn a_footprint_refold_replays_the_indicators() {
    let mut series = series();
    let replay = Some(SeriesEffect::ReplayIndicators);
    assert_eq!(series.set_footprint_enabled(true), replay, "switched on");
    assert_eq!(series.set_footprint_enabled(true), None, "already on");
    assert_eq!(
        series.set_footprint_group(Decimal::from(2)),
        replay,
        "new row width"
    );
    assert_eq!(
        series.set_footprint_group(Decimal::from(2)),
        None,
        "same width"
    );
    assert_eq!(
        series.set_footprint_enabled(false),
        replay,
        "ladders dropped"
    );
}
