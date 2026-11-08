//! Rust-only measurement helpers. Golden files contain data, never oracle code.
use quantick_indicators::{Ctx, Indicator, IndicatorBar};
use quantick_pine::{ScriptIndicator, compile};
use std::{collections::BTreeMap, fmt::Write, fs, path::PathBuf};

pub const CASES: &[&str] = &[
    "sma",
    "ema",
    "rsi",
    "atr",
    "macd",
    "stdev",
    "crossover",
    "pivothigh",
];
pub const TAPES: &[&str] = &["time", "tick", "volume"];
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests")
}
pub fn bars(tape: &str) -> Vec<IndicatorBar> {
    fs::read_to_string(root().join(format!("fixtures/oracle/{tape}.csv")))
        .unwrap()
        .lines()
        .skip(1)
        .map(|line| {
            let v: Vec<&str> = line.split(',').collect();
            let n = |i: usize| v[i].parse::<f64>().unwrap();
            IndicatorBar {
                open_time: v[0].parse().unwrap(),
                close_time: v[1].parse().unwrap(),
                open: n(2),
                high: n(3),
                low: n(4),
                close: n(5),
                buy_volume: n(6),
                sell_volume: n(7),
                trade_count: n(8),
            }
        })
        .collect()
}
pub fn run(source: &str, tape: &[IndicatorBar]) -> ScriptIndicator {
    let mut script = ScriptIndicator::new(
        compile(source, "fixture").unwrap_or_else(|errors| panic!("{errors:?}")),
        source,
    );
    let mut cvd = Vec::new();
    let mut sum = 0.0;
    for (bar_index, bar) in tape.iter().enumerate() {
        sum += bar.delta();
        cvd.push(sum);
        let mut ctx = Ctx {
            bar_index,
            cvd: &cvd,
        };
        // Preview the same bar twice; commit must agree and rollback must be stable.
        let before = snapshot(&script);
        let first = script.preview(bar, &mut ctx).unwrap();
        let second = script.preview(bar, &mut ctx).unwrap();
        assert_eq!(format!("{first:?}"), format!("{second:?}"));
        assert_eq!(before, snapshot(&script));
        script.on_close(bar, &mut ctx).unwrap();
    }
    script
}
pub fn snapshot(script: &ScriptIndicator) -> String {
    let mut out = String::new();
    for spec in &script.descriptor().plots {
        writeln!(out, "plot {} {:?}", spec.id.index(), spec).unwrap();
        for value in script.plots().column(spec.id) {
            if value.is_nan() {
                out.push_str("na\n");
            } else {
                writeln!(out, "{:016x}", value.to_bits()).unwrap();
            }
        }
    }
    for row in 0..script.plots().len() {
        writeln!(out, "paint {row} {:?}", script.plots().bar_paint(row)).unwrap();
    }
    if let Some(objects) = script.objects() {
        writeln!(out, "objects {:?}", objects.snapshot()).unwrap();
    }
    out
}
pub fn golden(name: &str, actual: &str) {
    let path = root().join("golden").join(name);
    if std::env::var_os("QUANTICK_UPDATE_PARITY").is_some() {
        fs::write(&path, actual).unwrap();
    }
    assert_eq!(fs::read_to_string(path).unwrap(), actual, "snapshot {name}");
}
pub fn oracle_report() -> String {
    let mut report = String::from(
        "proxy=PineTS-0.13.1; not verified TradingView equivalence\ntolerance=1e-8 absolute + 1e-8 relative; na matches only na\n",
    );
    let mut total = 0;
    let mut matched = 0;
    let mut rejected_cells = 0;
    for tape in TAPES {
        let data = bars(tape);
        for name in CASES {
            let source =
                fs::read_to_string(root().join(format!("fixtures/oracle/{name}.pine"))).unwrap();
            if let Err(errors) = compile(&source, "fixture") {
                let oracle =
                    fs::read_to_string(root().join(format!("golden/oracle/{tape}-{name}.csv")))
                        .unwrap();
                let columns = oracle.lines().nth(1).unwrap().split(',').count();
                let count = columns * data.len();
                total += count;
                rejected_cells += count;
                writeln!(
                    report,
                    "{tape}/{name}: rejected cells={count} codes={}",
                    errors
                        .iter()
                        .map(|e| e.code.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
                continue;
            }
            let script = run(&source, &data);
            let rerun = run(&source, &data);
            assert_eq!(snapshot(&script), snapshot(&rerun), "{tape}/{name}");
            let oracle =
                fs::read_to_string(root().join(format!("golden/oracle/{tape}-{name}.csv")))
                    .unwrap();
            let lines: Vec<_> = oracle.lines().skip(1).collect();
            assert_eq!(lines.len(), data.len() + 1);
            let titles: Vec<_> = lines[0].split(',').collect();
            assert_eq!(titles.len(), script.descriptor().plots.len());
            let mut differences = Vec::new();
            for (row, line) in lines[1..].iter().enumerate() {
                let cells: Vec<_> = line.split(',').collect();
                assert_eq!(cells.len(), titles.len());
                for (column, cell) in cells.iter().enumerate() {
                    let spec = &script.descriptor().plots[column];
                    assert_eq!(spec.title, titles[column]);
                    let actual = script.plots().column(spec.id)[row];
                    let expected = if *cell == "na" {
                        f64::NAN
                    } else {
                        cell.parse::<f64>().unwrap()
                    };
                    total += 1;
                    let agrees = if expected.is_nan() {
                        actual.is_nan()
                    } else {
                        actual.is_finite()
                            && (actual - expected).abs() <= 1e-8 + 1e-8 * expected.abs()
                    };
                    if agrees {
                        matched += 1;
                    } else {
                        differences.push(format!("{row}:{column}"));
                    }
                }
            }
            writeln!(
                report,
                "{tape}/{name}: cells={} divergent={} locations={}",
                data.len() * titles.len(),
                differences.len(),
                differences.join(",")
            )
            .unwrap();
        }
    }
    writeln!(
        report,
        "matching_cells={matched}/{total}\nrejected_cells={rejected_cells}/{total}"
    )
    .unwrap();
    report
}
pub fn corpus_report() -> String {
    let mut report = String::new();
    let mut histogram = BTreeMap::<String, usize>::new();
    let mut loaded = 0;
    let mut total = 0;
    let mut plots = 0;
    let mut positive = 0;
    let mut positive_loaded = 0;
    let mut negative_rejected = 0;
    for kind in ["ok", "err"] {
        let mut paths: Vec<_> = fs::read_dir(root().join("corpus").join(kind))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        paths.sort();
        for path in paths {
            let source = fs::read_to_string(&path).unwrap();
            total += 1;
            if kind == "ok" {
                positive += 1;
            }
            match compile(&source, "corpus") {
                Ok(script) => {
                    loaded += 1;
                    if kind == "ok" {
                        positive_loaded += 1;
                    }
                    plots += script.plots.len();
                    writeln!(
                        report,
                        "{kind}/{}: loaded plots={}",
                        path.file_name().unwrap().to_str().unwrap(),
                        script.plots.len()
                    )
                    .unwrap();
                }
                Err(errors) => {
                    if kind == "err" {
                        negative_rejected += 1;
                    }
                    let mut codes: Vec<_> = errors.iter().map(|e| e.code.as_str()).collect();
                    codes.sort();
                    for code in &codes {
                        *histogram.entry((*code).into()).or_default() += 1;
                    }
                    writeln!(
                        report,
                        "{kind}/{}: {}",
                        path.file_name().unwrap().to_str().unwrap(),
                        codes.join(",")
                    )
                    .unwrap();
                }
            }
        }
    }
    writeln!(report, "mixed_corpus_loads={loaded}/{total}\npositive_load_rate={positive_loaded}/{positive}\nnegative_rejections={negative_rejected}/{}\nloaded_plot_columns={plots}\ncorpus_oracle_verified_columns=0/{plots}", total - positive).unwrap();
    for (code, count) in histogram {
        writeln!(report, "error {code}={count}").unwrap();
    }
    report
}
