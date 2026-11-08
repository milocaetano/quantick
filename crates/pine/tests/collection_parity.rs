//! C51: hand-calculated commit/preview golden, recorded before IR implementation.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, compile};

#[test]
fn collections_copy_on_write_rollback() {
    let source = include_str!("fixtures/collections/rollback.pine");
    let mut compiled = compile(source, "rollback").unwrap_or_else(|errors| panic!("{errors:?}"));
    // Production execution must use lowered instructions even when the source
    // syntax is absent. A wrapper around the old tree walker fails here.
    compiled.ast = quantick_pine::ast::Ast::new();
    let mut script = ScriptIndicator::new(compiled, source);
    for line in include_str!("golden/collections/rollback.csv")
        .lines()
        .skip(1)
    {
        let cells: Vec<_> = line.split(',').collect();
        let bar_index: usize = cells[0].parse().unwrap();
        let close = (bar_index + 1) as f64 * 10.0;
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: close,
            high: close,
            low: close,
            close,
            buy_volume: 1.0,
            sell_volume: 0.0,
            trade_count: 1.0,
        };
        let mut ctx = Ctx {
            bar_index,
            cvd: &[1.0, 2.0, 3.0],
        };
        let actual = if cells[1] == "preview" {
            let committed_before = format!("{:?}", script.plots());
            let preview = script.preview(&bar, &mut ctx).unwrap();
            assert_eq!(committed_before, format!("{:?}", script.plots()));
            preview.values
        } else {
            script.on_close(&bar, &mut ctx).unwrap();
            (0..5)
                .map(|column| script.plots().column(PlotId::new(column))[bar_index])
                .collect()
        };
        assert_eq!(actual.len(), cells.len() - 2);
        for (value, expected) in actual.iter().zip(&cells[2..]) {
            let expected: f64 = expected.parse().unwrap();
            assert!(
                *value == expected || value.is_nan() && expected.is_nan(),
                "{line}: {actual:?}"
            );
        }
    }
}

fn bar() -> IndicatorBar {
    IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 10.0,
        high: 10.0,
        low: 10.0,
        close: 10.0,
        buy_volume: 1.0,
        sell_volume: 0.0,
        trade_count: 1.0,
    }
}
fn run(source: &str) -> Vec<f64> {
    let compiled = compile(source, "collections").unwrap_or_else(|e| panic!("{e:?}"));
    let count = compiled.plots.len();
    let mut script = ScriptIndicator::new(compiled, source);
    script
        .on_close(
            &bar(),
            &mut Ctx {
                bar_index: 0,
                cvd: &[],
            },
        )
        .unwrap();
    (0..count)
        .map(|i| script.plots().column(PlotId::new(i))[0])
        .collect()
}
#[test]
fn collections_versioned_negative_bounds() {
    let v5 = include_str!("fixtures/collections/negative_v5.pine");
    let mut script = ScriptIndicator::new(compile(v5, "v5").unwrap(), v5);
    assert!(
        script
            .on_close(
                &bar(),
                &mut Ctx {
                    bar_index: 0,
                    cvd: &[]
                }
            )
            .is_err()
    );
    assert_eq!(
        run(include_str!("fixtures/collections/negative_v6.pine")),
        vec![40.0, 30.0]
    );
    for (version, index) in [(5, "-1"), (5, "2"), (6, "-3"), (6, "2")] {
        for op in ["get", "set", "remove"] {
            let tail = if op == "set" { ", 9" } else { "" };
            let source = format!(
                "//@version={version}\nindicator(\"bounds\")\na = array.from(1, 2)\narray.{op}(a, {index}{tail})\nplot(1)\n"
            );
            let mut script = ScriptIndicator::new(compile(&source, "bounds").unwrap(), &source);
            assert!(
                script
                    .on_close(
                        &bar(),
                        &mut Ctx {
                            bar_index: 0,
                            cvd: &[]
                        }
                    )
                    .is_err(),
                "{source}"
            );
        }
    }
}
#[test]
fn collections_order_remove_reinsert_and_extraction() {
    assert_eq!(
        run(r#"//@version=6
indicator("order")
var map<string, int> m = map.new<string, int>()
map.put(m, "z", 1)
map.put(m, "a", 2)
map.put(m, "z", 3)
map.remove(m, "z")
map.put(m, "z", 4)
k = map.keys(m)
v = map.values(m)
array.set(k, 0, "x")
array.set(v, 0, 99)
copy = map.copy(m)
map.clear(copy)
plot(array.get(map.keys(m), 0) == "a" ? 1 : 0)
plot(array.get(map.keys(m), 1) == "z" ? 1 : 0)
plot(map.get(m, "a"))
plot(map.get(m, "z"))
plot(map.size(copy))
plot(map.contains(m, "x") ? 1 : 0)
"#),
        vec![1.0, 1.0, 2.0, 4.0, 0.0, 0.0]
    );
}
#[test]
fn collections_typed_elements_and_mutators() {
    assert_eq!(
        run(r#"//@version=6
indicator("typed")
array<int> a = array.new<int>()
array.push(a, 2)
array.unshift(a, 1)
array.insert(a, 2, 3)
array.reverse(a)
plot(array.shift(a))
plot(array.pop(a))
plot(array.first(a))
b = array.from(4, 5)
array.concat(a, b)
array.fill(a, 7, 1, 3)
plot(array.sum(a))
plot(array.indexof(a, 7))
plot(array.lastindexof(a, 7))
plot(array.includes(a, 2) ? 1 : 0)
plot(array.join(a, ":") == "2:7:7" ? 1 : 0)
"#),
        vec![3.0, 1.0, 2.0, 16.0, 1.0, 2.0, 1.0, 1.0]
    );
    for body in [
        "a = array.new_int()\narray.push(a, 1.5)",
        "a = array.new_bool()\narray.push(a, 1)",
        "a = array.new_string()\narray.push(a, 1)",
        "a = map.new<string, int>()\nmap.put(a, 1, 2)",
        "a = map.new<int, bool>()\nmap.put(a, 1, 2)",
    ] {
        let source = format!("//@version=6\nindicator(\"types\")\n{body}\nplot(1)");
        assert!(compile(&source, "types").is_err(), "{source}");
    }
}
#[test]
fn collections_empty_errors_and_varip() {
    for operation in [
        "get(a, 0)",
        "set(a, 0, 1)",
        "remove(a, 0)",
        "pop(a)",
        "shift(a)",
        "first(a)",
        "last(a)",
    ] {
        let source = format!(
            "//@version=6\nindicator(\"empty\")\na = array.new_int()\narray.{operation}\nplot(1)"
        );
        let mut script = ScriptIndicator::new(compile(&source, "empty").unwrap(), &source);
        assert!(
            script
                .on_close(
                    &bar(),
                    &mut Ctx {
                        bar_index: 0,
                        cvd: &[]
                    }
                )
                .is_err()
        );
    }
    let source = "//@version=6\nindicator(\"varip\")\nvarip a = array.new_int()\narray.push(a, 1)\nplot(array.size(a))";
    let mut script = ScriptIndicator::new(compile(source, "varip").unwrap(), source);
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[],
    };
    for expected in [1.0, 2.0, 3.0] {
        assert_eq!(
            script.preview(&bar(), &mut ctx).unwrap().values,
            vec![expected]
        );
    }
    script.on_close(&bar(), &mut ctx).unwrap();
    assert_eq!(script.plots().column(PlotId::new(0)), &[4.0]);
}
#[test]
fn collections_history_keeps_prior_instances_and_methods() {
    let source = "//@version=6\nindicator(\"history\")\na = array.from(bar_index)\na.push(7)\nb = a[1]\nplot(bar_index > 0 ? array.get(b, 0) : -1)\nplot(a.size())";
    let mut script = ScriptIndicator::new(compile(source, "history").unwrap(), source);
    for i in 0..30 {
        script
            .on_close(
                &bar(),
                &mut Ctx {
                    bar_index: i,
                    cvd: &[],
                },
            )
            .unwrap();
    }
    assert_eq!(script.plots().column(PlotId::new(0))[29], 28.0);
    assert_eq!(script.plots().column(PlotId::new(1))[29], 2.0);
}

#[test]
fn collections_varip_keeps_shared_var_identity_after_growth() {
    let source = r#"//@version=6
indicator("aliases")
varip a = array.new_int()
var b = a
for i = 0 to 30
    array.push(a, bar_index)
plot(array.size(a))
plot(array.size(b))
"#;
    let mut script = ScriptIndicator::new(compile(source, "aliases").unwrap(), source);
    script
        .on_close(
            &bar(),
            &mut Ctx {
                bar_index: 0,
                cvd: &[],
            },
        )
        .unwrap();
    let mut ctx = Ctx {
        bar_index: 1,
        cvd: &[],
    };
    for expected in [62.0, 93.0, 124.0] {
        assert_eq!(
            script.preview(&bar(), &mut ctx).unwrap().values,
            vec![expected, expected]
        );
    }
    script.on_close(&bar(), &mut ctx).unwrap();
    for plot in 0..2 {
        assert_eq!(script.plots().column(PlotId::new(plot))[1], 155.0);
    }
}
#[test]
fn collections_named_methods_and_missing_map_values() {
    let values = run(r#"//@version=6
indicator("named")
a = array.from(1, 2)
a.set(value = 9, index = 0)
m = map.new<string, bool>()
m.put(value = true, key = "a")
plot(a.get(index = 0))
plot(m.get("a") ? 1 : 0)
plot(m.get("missing") ? 1 : 0)
"#);
    assert_eq!(values, vec![9.0, 1.0, 0.0]);
}

#[test]
fn collections_sorting_golden() {
    let expected: Vec<f64> = include_str!("golden/collections/sorting.csv")
        .trim()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(
        run(include_str!("fixtures/collections/sorting.pine")),
        expected
    );
}
#[test]
fn collections_reject_invalid_scalar_methods() {
    for body in [
        "a = array.new_bool()\narray.join(a)",
        "a = array.new_color()\narray.join(a)",
        "a = array.new_bool(1, na)",
        "a = array.new_bool()\na.push(na)",
        "a = array.new_int()\na.new_int()",
        "a = array.new_bool()\na.sort()",
    ] {
        let source = format!("//@version=6\nindicator(\"reject\")\n{body}\nplot(1)");
        assert!(compile(&source, "reject").is_err(), "{source}");
    }
}

#[test]
fn collections_shared_slice_golden() {
    let expected: Vec<f64> = include_str!("golden/collections/slices.csv")
        .trim()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(
        run(include_str!("fixtures/collections/slices.pine")),
        expected
    );
}
#[test]
fn collections_slice_parent_lifetime_and_preview() {
    let source = "//@version=6\nindicator(\"slice lifetime\")\nvar a = array.from(1, 2, 3)\nvar s = a.slice(1, 3)\na := array.new_int()\ns.set(0, s.get(0) + 1)\nplot(s.get(0))";
    let mut script = ScriptIndicator::new(compile(source, "slice").unwrap(), source);
    for index in 0..20 {
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        for _ in 0..3 {
            assert_eq!(
                script.preview(&bar(), &mut ctx).unwrap().values,
                vec![index as f64 + 3.0]
            );
        }
        script.on_close(&bar(), &mut ctx).unwrap();
        assert_eq!(
            script.plots().column(PlotId::new(0))[index],
            index as f64 + 3.0
        );
    }
}
#[test]
fn collections_varip_slice_transplants_parent_aliases() {
    let source = "//@version=6\nindicator(\"slice varip\")\nvar a = array.from(1, 2, 3)\nvarip s = a.slice(1, 3)\ns.push(4)\nplot(a.size())\nplot(s.size())";
    let mut script = ScriptIndicator::new(compile(source, "slice").unwrap(), source);
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[],
    };
    script.on_close(&bar(), &mut ctx).unwrap();
    ctx.bar_index = 1;
    for size in 5..8 {
        assert_eq!(
            script.preview(&bar(), &mut ctx).unwrap().values,
            vec![size as f64, size as f64 - 1.0]
        );
    }
}

#[test]
fn collections_parameter_and_legacy_annotations() {
    let expected: Vec<f64> = include_str!("golden/collections/annotations.csv")
        .trim()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(
        run(include_str!("fixtures/collections/annotations.pine")),
        expected
    );
    let source = "//@version=6\nindicator(\"types\")\nf(array<int> a) => a.first()\nplot(f(array.from(1.5)))";
    assert!(compile(source, "types").is_err());
}

#[test]
fn collections_statistics_golden() {
    let expected: Vec<f64> = include_str!("golden/collections/statistics.csv")
        .trim()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    let actual = run(include_str!("fixtures/collections/statistics.pine"));
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() <= 1e-8 + 1e-8 * expected.abs(),
            "{actual} != {expected}"
        );
    }
}

#[test]
fn collections_binary_search_golden() {
    let expected: Vec<f64> = include_str!("golden/collections/search.csv")
        .trim()
        .split(',')
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(
        run(include_str!("fixtures/collections/search.pine")),
        expected
    );
}
#[test]
fn collections_statistic_integer_overloads() {
    assert_eq!(
        run(
            "//@version=6\nindicator(\"int stats\")\na = array.from(1, 2)\nint mean = a.avg()\nint median = a.median()\nint deviation = a.stdev()\nint[] z = a.standardize()\nplot(mean)\nplot(median)\nplot(deviation)\nplot(z.get(0))"
        ),
        vec![1.0, 1.0, 0.0, -1.0]
    );
}

#[test]
fn collections_documented_boundary_policy() {
    assert_eq!(
        run(include_str!("fixtures/collections/boundaries.pine")),
        include_str!("golden/collections/boundaries.csv")
            .trim()
            .split(',')
            .map(|s| s.parse::<f64>().unwrap())
            .collect::<Vec<_>>()
    );
    for body in [
        "a = array.from(1, 2)\ns = a.slice(1, 2)\na.clear()\ns.push(3)",
        "a = array.new_int(100000)\ns = a.slice(0, 1)\ns.push(3)",
        "a = array.from(1, 2)\na.slice(1, 1)",
        "a = array.from(1.0)\na.percentile_linear_interpolation(101)",
        "a = array.from(1.0)\na.percentrank(1)",
        "a = array.from(1.0)\na.covariance(array.from(1.0, 2.0))",
    ] {
        let source = format!("//@version=6\nindicator(\"boundary\")\n{body}\nplot(1)");
        let mut script = ScriptIndicator::new(compile(&source, "boundary").unwrap(), &source);
        assert!(
            script
                .on_close(
                    &bar(),
                    &mut Ctx {
                        bar_index: 0,
                        cvd: &[]
                    }
                )
                .is_err(),
            "{source}"
        );
    }
    let source = "//@version=6\nindicator(\"bool na\")\na = array.from(true, na)\nplot(a.size())";
    assert!(compile(source, "bool na").is_err());
}
