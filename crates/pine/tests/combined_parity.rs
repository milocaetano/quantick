//! C90-C93: retained-child acceptance, runnable offline without Node.
mod parity_support;
use parity_support::{bars, golden, root, run, snapshot};

#[test]
fn combined_offline_golden_parity() {
    let report = parity_support::oracle_report();
    assert!(report.contains("matching_cells=1920/1920"), "{report}");
    assert!(report.contains("rejected_cells=0/1920"), "{report}");
}

#[test]
fn combined_byte_identical_runs() {
    let source = include_str!("fixtures/oracle/macd.pine");
    for tape in parity_support::TAPES {
        let data = bars(tape);
        assert_eq!(snapshot(&run(source,&data)).as_bytes(),snapshot(&run(source,&data)).as_bytes());
    }
}

#[test]
fn combined_app_script_regression() {
    let mut paths: Vec<_> = std::fs::read_dir(root().join("../../app/scripts")).unwrap().map(|entry| entry.unwrap().path()).collect();
    paths.sort();
    assert_eq!(paths.len(),9);
    for path in paths {
        let source = std::fs::read_to_string(&path).unwrap();
        golden(&format!("app-{}.txt",path.file_stem().unwrap().to_str().unwrap()),&snapshot(&run(&source,&bars("tick"))));
    }
}

#[test]
fn combined_report_and_dialect_contract() {
    let report = parity_support::corpus_report();
    assert!(report.contains("positive_load_rate=13/13"),"{report}");
    assert!(report.contains("negative_rejections=7/7"),"{report}");
    assert!(report.contains("corpus_oracle_verified_columns=0/21"),"{report}");
    let dialect = include_str!("../../../docs/pine-dialect.md");
    for name in quantick_pine::builtins::Builtin::all_names() { assert!(dialect.contains(name),"undocumented {name}"); }
    assert!(dialect.contains("compile_with_libraries"));
    let parity = include_str!("../../../docs/pine-parity.md");
    assert!(parity.contains("Final retained-child acceptance"));
    for denominator in ["1,920", "13/13", "9/9", "0/9"] { assert!(parity.contains(denominator),"missing {denominator}"); }
}
