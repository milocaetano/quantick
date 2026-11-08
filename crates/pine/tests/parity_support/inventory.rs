//! Closed inventory: newly committed goldens must acquire an execution/evidence path.
use std::{collections::BTreeSet, path::Path};

pub fn assert_inventory() {
    let expected: BTreeSet<_> = INVENTORY.iter().map(|s| s.to_string()).collect();
    let mut actual = BTreeSet::new();
    collect(
        &super::parity_support::root().join("golden"),
        "",
        &mut actual,
    );
    assert_eq!(
        actual, expected,
        "assign new goldens an execution or historical/provenance assertion path"
    );
    for (path, evidence) in [
        ("calendar/README.md", "independently calculated"),
        ("candle_plots/README.md", "independently authored"),
        ("collections/README.md", "Hand-calculated"),
        ("libraries/README.md", "not\nPineTS output"),
        ("version/provenance.md", "manually calculated"),
    ] {
        let text = std::fs::read_to_string(super::parity_support::root().join("golden").join(path))
            .unwrap();
        assert!(text.contains(evidence), "{path}: provenance changed");
    }
}
fn collect(directory: &Path, prefix: &str, paths: &mut BTreeSet<String>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let name = format!("{prefix}{}", entry.file_name().to_str().unwrap());
        if entry.path().is_dir() {
            collect(&entry.path(), &format!("{name}/"), paths);
        } else {
            paths.insert(name);
        }
    }
}

// C90: oracle/*, ir/native-kernels, retained scalars and graph numeric rows.
// C92: app-*; C93: historical oracle/corpus reports and conditional proxy.
// Child tests execute calendar/fields, candle_plots/plots, ir/rollback,
// collections/rollback. C90 below executes collections/negative.
// Provenance documents are asserted above, not counted as numeric oracle cells.
const INVENTORY: &[&str] = &[
    "app-copilot.txt",
    "app-cvd.txt",
    "app-delta_histogram.txt",
    "app-ema.txt",
    "app-exhaustion_reversal.txt",
    "app-force_bar.txt",
    "app-range_box.txt",
    "app-vwap_cumulative.txt",
    "app-zigzag.txt",
    "calendar/README.md",
    "calendar/fields.csv",
    "candle_plots/README.md",
    "candle_plots/plots.csv",
    "collections/README.md",
    "collections/annotations.csv",
    "collections/boundaries.csv",
    "collections/defaults.csv",
    "collections/eager.csv",
    "collections/negative.csv",
    "collections/rollback.csv",
    "collections/search.csv",
    "collections/slices.csv",
    "collections/sorting.csv",
    "collections/statistics.csv",
    "corpus-report.txt",
    "ir/native-kernels.csv",
    "ir/rollback.csv",
    "libraries/README.md",
    "libraries/defaults.csv",
    "libraries/eager.csv",
    "libraries/history_graph.csv",
    "libraries/main.csv",
    "libraries/mixed.csv",
    "libraries/varip_graph.csv",
    "oracle-report.txt",
    "oracle/tick-atr.csv",
    "oracle/tick-crossover.csv",
    "oracle/tick-ema.csv",
    "oracle/tick-macd.csv",
    "oracle/tick-pivothigh.csv",
    "oracle/tick-rsi.csv",
    "oracle/tick-sma.csv",
    "oracle/tick-stdev.csv",
    "oracle/time-atr.csv",
    "oracle/time-crossover.csv",
    "oracle/time-ema.csv",
    "oracle/time-macd.csv",
    "oracle/time-pivothigh.csv",
    "oracle/time-rsi.csv",
    "oracle/time-sma.csv",
    "oracle/time-stdev.csv",
    "oracle/volume-atr.csv",
    "oracle/volume-crossover.csv",
    "oracle/volume-ema.csv",
    "oracle/volume-macd.csv",
    "oracle/volume-pivothigh.csv",
    "oracle/volume-rsi.csv",
    "oracle/volume-sma.csv",
    "oracle/volume-stdev.csv",
    "semantics/conditional-proxy.csv",
    "semantics/conditional.csv",
    "semantics/loop.csv",
    "types/scalars.csv",
    "version/provenance.md",
    "version/v5.csv",
    "version/v6.csv",
];
