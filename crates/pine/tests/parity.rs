//! Fixed, offline oracle and application regression measurements.
mod parity_support;
use parity_support::{bars, golden, root, run, snapshot};
use quantick_indicators::Indicator;

#[test]
fn oracle_baseline_and_determinism() {
    golden("oracle-report.txt", &parity_support::oracle_report());
    let app = root().join("../../app/scripts");
    let mut paths: Vec<_> = std::fs::read_dir(app)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        let data = bars("tick");
        let first = snapshot(&run(&source, &data));
        assert_eq!(first, snapshot(&run(&source, &data)));
        golden(
            &format!("app-{}.txt", path.file_stem().unwrap().to_str().unwrap()),
            &first,
        );
    }
}

#[test]
fn corpus_measurement_baseline() {
    let first = parity_support::corpus_report();
    assert_eq!(first, parity_support::corpus_report());
    golden("corpus-report.txt", &first);
}

#[test]
fn activity_fixture_flow_has_independent_expected_values() {
    for tape in parity_support::TAPES {
        let data = bars(tape);
        let source = "//@version=5\nindicator(\"flow\")\nplot(buy_volume)\nplot(sell_volume)\nplot(delta)\nplot(cvd)\nplot(trade_count)\n";
        let script = run(source, &data);
        let mut cvd = 0.0;
        for (i, bar) in data.iter().enumerate() {
            let buy = (3 + i % 5) as f64;
            let sell = if *tape == "volume" {
                12.0 - buy
            } else {
                (2 + i % 4) as f64
            };
            cvd += buy - sell;
            let count = if *tape == "tick" {
                10.0
            } else {
                (5 + i % 13) as f64
            };
            assert_eq!(
                (bar.buy_volume, bar.sell_volume, bar.trade_count),
                (buy, sell, count)
            );
            for (column, expected) in [buy, sell, buy - sell, cvd, count].iter().enumerate() {
                assert_eq!(
                    script
                        .plots()
                        .column(quantick_indicators::PlotId::new(column))[i],
                    *expected
                );
            }
        }
    }
}
