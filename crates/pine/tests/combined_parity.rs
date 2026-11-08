//! C90-C93: retained-child acceptance, runnable offline without Node.
#[path = "parity_support/inventory.rs"]
mod inventory;
#[path = "parity_support/native.rs"]
mod native;
mod parity_support;
#[path = "parity_support/retained.rs"]
mod retained;
use parity_support::{bars, golden, root, run, snapshot};

#[test]
fn combined_offline_golden_parity() {
    inventory::assert_inventory();
    retained::negative_golden();
    assert_eq!(retained::scalar_goldens(), 100);
    let (cells, _) = retained::graph_goldens();
    assert_eq!(cells, 136);
    assert_eq!(native::kernel_goldens(), 3072);
    let report = parity_support::oracle_report();
    assert!(report.contains("matching_cells=1920/1920"), "{report}");
    assert!(report.contains("rejected_cells=0/1920"), "{report}");
}

#[test]
fn combined_byte_identical_runs() {
    let first = retained::graph_goldens();
    let second = retained::graph_goldens();
    assert_eq!(first.0, second.0);
    assert_eq!(first.1.as_bytes(), second.1.as_bytes());
    let source = include_str!("fixtures/oracle/macd.pine");
    for tape in parity_support::TAPES {
        let data = bars(tape);
        assert_eq!(
            snapshot(&run(source, &data)).as_bytes(),
            snapshot(&run(source, &data)).as_bytes()
        );
    }
}

#[test]
fn combined_app_script_regression() {
    let mut paths: Vec<_> = std::fs::read_dir(root().join("../../app/scripts"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 9);
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        golden(
            &format!("app-{}.txt", path.file_stem().unwrap().to_str().unwrap()),
            &snapshot(&run(&source, &bars("tick"))),
        );
    }
}

#[test]
fn combined_report_and_dialect_contract() {
    let historical = include_str!("golden/oracle-report.txt");
    assert!(historical.contains("matching_cells=1344/1920"));
    assert!(historical.contains("rejected_cells=576/1920"));
    let proxy: Vec<f64> = include_str!("golden/semantics/conditional-proxy.csv")
        .lines()
        .skip(2)
        .map(|line| line.parse().unwrap())
        .collect();
    assert_eq!(proxy, vec![1.0, 0.0, 4.0, 0.0, 9.0]);
    let mut tape = bars("tick");
    tape.truncate(5);
    for (index, bar) in tape.iter_mut().enumerate() {
        bar.close = (index + 1) as f64;
    }
    let current = run(
        "//@version=5\nindicator(\"eager\")\nplot(bar_index % 2 == 0 ? ta.cum(close) : 0)\n",
        &tape,
    );
    use quantick_indicators::Indicator;
    let actual = current.plots().column(quantick_indicators::PlotId::new(0));
    assert_eq!(actual, &[1.0, 0.0, 6.0, 0.0, 15.0]);
    assert_ne!(actual, proxy);
    let baseline = include_str!("golden/corpus-report.txt");
    for denominator in [
        "mixed_corpus_loads=13/20",
        "positive_load_rate=13/13",
        "negative_rejections=7/7",
        "corpus_oracle_verified_columns=0/21",
    ] {
        assert!(baseline.contains(denominator), "{baseline}");
    }
    let report = parity_support::corpus_report();
    assert!(report.contains("mixed_corpus_loads=13/20"));
    assert!(report.contains("positive_load_rate=13/13"), "{report}");
    assert!(report.contains("negative_rejections=7/7"), "{report}");
    assert!(
        report.contains("corpus_oracle_verified_columns=0/21"),
        "{report}"
    );
    let dialect = include_str!("../../../docs/pine-dialect.md");
    for version in [5, 6] {
        let source = format!("//@version={version}\nindicator(\"calendar\")\nplot(year(0))\n");
        let calendar = run(&source, &tape);
        assert_eq!(
            calendar.plots().column(quantick_indicators::PlotId::new(0)),
            &[1970.0; 5]
        );
    }
    for (source, code) in [
        (
            "//@version=7\nindicator(\"unsupported\")\nplot(close)\n",
            "PINE_VERSION",
        ),
        (
            "//@version=6\nindicator(\"unsupported\")\nplot(timeframe.period)\n",
            "PINE_NO_TIMEFRAME",
        ),
    ] {
        let errors = quantick_pine::compile(source, "unsupported")
            .err()
            .expect("unsupported construct must fail to compile");
        assert!(errors.iter().any(|error| error.code.as_str() == code));
        assert!(dialect.contains(code));
    }
    let language = dialect
        .split("## Language")
        .nth(1)
        .unwrap()
        .split("## Series variables")
        .next()
        .unwrap();
    for version in ["//@version=5", "//@version=6"] {
        assert!(
            language.contains(version),
            "undocumented support for {version}"
        );
    }
    assert!(
        !dialect.contains("PINE_NO_CALENDAR"),
        "retired calendar rejection must not be documented"
    );
    let rejected = dialect
        .split("## Rejected constructs")
        .nth(1)
        .unwrap()
        .split("## Error codes")
        .next()
        .unwrap();
    for supported in [
        "calendar",
        "`year`",
        "`month`",
        "`dayofmonth`",
        "`dayofweek`",
        "`hour`",
        "`minute`",
    ] {
        assert!(
            !rejected.contains(supported),
            "supported {supported} must not be listed as rejected"
        );
    }
    for name in quantick_pine::builtins::Builtin::all_names() {
        assert!(dialect.contains(name), "undocumented {name}");
    }
    assert!(dialect.contains("compile_with_libraries"));
    let parity = include_str!("../../../docs/pine-parity.md");
    assert!(parity.contains("Final retained-child acceptance"));
    for denominator in ["1,920", "13/13", "9/9", "0/9"] {
        assert!(parity.contains(denominator), "missing {denominator}");
    }
}
