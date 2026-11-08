//! C61: pure local resolution, user references, methods and rollback.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ScriptIndicator, libraries::compile_with_libraries};

#[test]
fn local_libraries_types_and_methods() {
    let source = include_str!("fixtures/libraries/main.pine");
    let compiled = compile_with_libraries(source, "libraries", |key| {
        (key == "local/Stats/1").then(|| include_str!("fixtures/libraries/stats.pine").to_owned())
    })
    .unwrap_or_else(|errors| panic!("{errors:?}"));
    let mut script = ScriptIndicator::new(compiled, source);
    for (index, line) in include_str!("golden/libraries/main.csv")
        .lines()
        .skip(1)
        .enumerate()
    {
        let row: Vec<f64> = line.split(',').map(|cell| cell.parse().unwrap()).collect();
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: row[0],
            high: row[0],
            low: row[0],
            close: row[0],
            buy_volume: 0.0,
            sell_volume: 0.0,
            trade_count: 0.0,
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[0.0; 3],
        };
        for _ in 0..2 {
            assert_eq!(script.preview(&bar, &mut ctx).unwrap().values, &row[1..]);
        }
        script.on_close(&bar, &mut ctx).unwrap();
        for (column, value) in row[1..].iter().enumerate() {
            assert_eq!(script.plots().column(PlotId::new(column))[index], *value);
        }
    }
    for (source, expected) in [
        (
            "import local/Missing/1 as missing\n",
            "missing local library",
        ),
        ("import remote/Stats/1 as stats\n", "remote or invalid"),
        ("import local/Cycle/1 as cycle\n", "cyclic local library"),
    ] {
        let run = || {
            compile_with_libraries(source, "reject", |key| {
                (key == "local/Cycle/1").then(|| source.to_owned())
            })
            .err()
            .unwrap()
        };
        let first = run();
        assert_eq!(format!("{first:?}"), format!("{:?}", run()));
        assert!(first[0].message.contains(expected), "{first:?}");
    }
}

#[test]
fn object_alias_copy_and_version_history() {
    let body = "type Point\n    float value = 0.0\npoint = Point.new(close)\nalias = point\nalias.value := close + 1\ncopy = point.copy()\ncopy.value := close + 5\nprevious = point[1]\ntemporary = point.value\nplot(point.value)\nplot(copy.value)\nplot(point == alias ? 1 : 0)\nplot(point == copy ? 1 : 0)\nplot(bar_index > 0 ? previous.value : 0)\nplot(nz(temporary[1]))\n";
    for version in [5, 6] {
        let source = format!("//@version={version}\nindicator(\"objects\")\n{body}");
        let compiled = quantick_pine::compile(&source, "objects")
            .unwrap_or_else(|errors| panic!("{errors:?}"));
        let mut script = ScriptIndicator::new(compiled, &source);
        for (index, close) in [2.0, 3.0, 5.0].into_iter().enumerate() {
            let bar = IndicatorBar {
                open_time: 0,
                close_time: 1,
                open: close,
                high: close,
                low: close,
                close,
                buy_volume: 0.0,
                sell_volume: 0.0,
                trade_count: 0.0,
            };
            let mut ctx = Ctx {
                bar_index: index,
                cvd: &[0.0; 3],
            };
            let previous = [0.0, 3.0, 4.0][index];
            for _ in 0..2 {
                assert_eq!(
                    script.preview(&bar, &mut ctx).unwrap().values,
                    vec![close + 1.0, close + 5.0, 1.0, 0.0, previous, previous]
                );
            }
            script.on_close(&bar, &mut ctx).unwrap();
        }
        let source = format!(
            "//@version={version}\nindicator(\"field history\")\ntype Point\n    float value = 0.0\np = Point.new(close)\nplot(p.value[1])\n"
        );
        assert_eq!(
            quantick_pine::compile(&source, "history").is_ok(),
            version == 5
        );
    }
    for body in [
        "type Point\n    float x\n    int x\np = Point.new()",
        "type Point\n    unknown x",
        "type Point\n    float x = close + 1",
        "type Point\n    float x\np = Point.new(1,2)",
        "type Point\n    float x\np = Point.new(\"bad\")",
        "type Point\n    float x\np = Point.new()\np.x := \"bad\"",
        "type Point\n    float x\np = Point.new()\nplot(p.missing)",
    ] {
        assert!(
            quantick_pine::compile(
                &format!("//@version=6\nindicator(\"reject\")\n{body}\n"),
                "reject"
            )
            .is_err(),
            "{body}"
        );
    }
}

#[test]
fn nested_shallow_copy_and_polymorphic_fields() {
    let source = "//@version=6\nindicator(\"nested\")\ntype Inner\n    float value = 2\ntype Outer\n    Inner child\n    float value = 4\nread(object) => object.value\ninner = Inner.new()\nouter = Outer.new(inner)\ncopy = outer.copy()\ncopy.child.value := 7\nplot(outer.child.value)\nplot(read(inner))\nplot(read(outer))\n";
    let compiled =
        quantick_pine::compile(source, "nested").unwrap_or_else(|errors| panic!("{errors:?}"));
    let mut script = ScriptIndicator::new(compiled, source);
    let bar = IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 2.0,
        high: 2.0,
        low: 2.0,
        close: 2.0,
        buy_volume: 0.0,
        sell_volume: 0.0,
        trade_count: 0.0,
    };
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[0.0],
    };
    for _ in 0..2 {
        assert_eq!(
            script.preview(&bar, &mut ctx).unwrap().values,
            vec![7.0, 7.0, 4.0]
        );
    }
    script.on_close(&bar, &mut ctx).unwrap();
}

#[test]
fn object_declarations_validate_unused_defaults_and_receivers() {
    for body in [
        "type Point\n    float x = \"bad\"",
        "type Point\n    bool x = na",
        "method read(object) => object",
        "method read() => 1",
    ] {
        assert!(
            quantick_pine::compile(
                &format!("//@version=6\nindicator(\"reject\")\n{body}\n"),
                "reject"
            )
            .is_err(),
            "{body}"
        );
    }
    assert!(quantick_pine::compile("//@version=6\nindicator(\"bool\")\ntype Flag\n    bool value\nf = Flag.new()\nplot(f.value ? 1 : 0)\n", "bool").is_ok());
}

#[test]
fn library_private_names_and_field_labels_do_not_escape() {
    let library = "//@version=6\nlibrary(\"Scope\")\nvalue = 7.0\nexport type Point\n    float value = 2.0\nmethod secret(Point self) => self.value\nexport read(Point point) => point.secret() + value\nexport identity(float value) => value\nexport relay() => identity(value = 4)\n";
    let source = "//@version=6\nindicator(\"scope\")\nimport local/Scope/1 as scope\nvalue = 20.0\np = scope.Point.new()\nplot(scope.read(p))\nplot(scope.identity(value = 3))\nplot(scope.relay())\nplot(value)\n";
    let compiled = compile_with_libraries(source, "scope", |_| Some(library.into())).unwrap();
    let mut script = ScriptIndicator::new(compiled, source);
    let bar = IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 2.0,
        high: 2.0,
        low: 2.0,
        close: 2.0,
        buy_volume: 0.0,
        sell_volume: 0.0,
        trade_count: 0.0,
    };
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[0.0],
    };
    assert_eq!(
        script.preview(&bar, &mut ctx).unwrap().values,
        vec![9.0, 3.0, 4.0, 20.0]
    );
    for call in [
        "p.secret()",
        "scope.secret(p)",
        "secret(p)",
        "qlib0_secret(p)",
    ] {
        assert!(
            compile_with_libraries(&format!("{source}plot({call})\n"), "scope", |_| Some(
                library.into()
            ))
            .is_err(),
            "{call}"
        );
    }
    assert!(
        compile_with_libraries(source, "scope", |_| Some(
            "export read(float x) => x".into()
        ))
        .is_err()
    );
}

#[test]
fn mixed_object_collection_golden() {
    let source = include_str!("fixtures/libraries/mixed.pine");
    let compiled = quantick_pine::compile(source, "mixed").unwrap_or_else(|e| panic!("{e:?}"));
    let mut script = ScriptIndicator::new(compiled, source);
    for (index, line) in include_str!("golden/libraries/mixed.csv")
        .lines()
        .skip(1)
        .enumerate()
    {
        let row: Vec<f64> = line.split(',').map(|s| s.parse().unwrap()).collect();
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: row[0],
            high: row[0],
            low: row[0],
            close: row[0],
            buy_volume: 0.0,
            sell_volume: 0.0,
            trade_count: 0.0,
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[0.0; 3],
        };
        for _ in 0..2 {
            assert_eq!(script.preview(&bar, &mut ctx).unwrap().values, &row[1..]);
        }
        script.on_close(&bar, &mut ctx).unwrap();
        for (column, value) in row[1..].iter().enumerate() {
            assert_eq!(script.plots().column(PlotId::new(column))[index], *value);
        }
    }
}

#[test]
fn object_eager_reads_preserve_conditional_mutations() {
    let source = include_str!("fixtures/libraries/eager.pine");
    let compiled = quantick_pine::compile(source, "eager").unwrap_or_else(|e| panic!("{e:?}"));
    let mut script = ScriptIndicator::new(compiled, source);
    let bar = IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        buy_volume: 0.0,
        sell_volume: 0.0,
        trade_count: 0.0,
    };
    for (index, line) in include_str!("golden/libraries/eager.csv")
        .lines()
        .enumerate()
    {
        let expected: Vec<f64> = line.split(',').map(|s| s.parse().unwrap()).collect();
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        for _ in 0..2 {
            assert_eq!(script.preview(&bar, &mut ctx).unwrap().values, expected);
        }
        script.on_close(&bar, &mut ctx).unwrap();
        for (column, value) in expected.iter().enumerate() {
            assert_eq!(script.plots().column(PlotId::new(column))[index], *value);
        }
    }
}

#[test]
fn mixed_graph_history_and_first_preview_varip() {
    let source = include_str!("fixtures/libraries/varip_graph.pine");
    let compiled = quantick_pine::compile(source, "graph").unwrap_or_else(|e| panic!("{e:?}"));
    let mut script = ScriptIndicator::new(compiled, source);
    let bar = IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        buy_volume: 0.0,
        sell_volume: 0.0,
        trade_count: 0.0,
    };
    for (step, line) in include_str!("golden/libraries/varip_graph.csv")
        .lines()
        .enumerate()
    {
        let expected: Vec<f64> = line.split(',').map(|s| s.parse().unwrap()).collect();
        let mut ctx = Ctx {
            bar_index: step / 3,
            cvd: &[],
        };
        if step % 3 < 2 {
            assert_eq!(script.preview(&bar, &mut ctx).unwrap().values, expected);
        } else {
            script.on_close(&bar, &mut ctx).unwrap();
            for (column, value) in expected.iter().enumerate() {
                assert_eq!(script.plots().column(PlotId::new(column))[step / 3], *value);
            }
        }
    }
    let source = include_str!("fixtures/libraries/history_graph.pine");
    let compiled = quantick_pine::compile(source, "history").unwrap_or_else(|e| panic!("{e:?}"));
    let mut script = ScriptIndicator::new(compiled, source);
    for (index, line) in include_str!("golden/libraries/history_graph.csv")
        .lines()
        .enumerate()
    {
        let expected: Vec<f64> = line.split(',').map(|s| s.parse().unwrap()).collect();
        let bar = IndicatorBar {
            close: expected[0],
            ..bar
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        for _ in 0..2 {
            assert_eq!(
                script.preview(&bar, &mut ctx).unwrap().values,
                &expected[1..]
            );
        }
        script.on_close(&bar, &mut ctx).unwrap();
        for (column, value) in expected[1..].iter().enumerate() {
            assert_eq!(script.plots().column(PlotId::new(column))[index], *value);
        }
    }
}

#[test]
fn object_varip_fields_are_independent_of_reference_mode() {
    for mode in ["var", "varip"] {
        let source = format!(
            "//@version=6\nindicator(\"fields\")\ntype Counter\n    int bars = 0\n    varip int ticks = 0\n{mode} counter = Counter.new()\nalias = counter\ncounter.bars := counter.bars + 1\ncounter.ticks := counter.ticks + 1\nplot(alias.bars)\nplot(alias.ticks)\n"
        );
        let compiled =
            quantick_pine::compile(&source, "fields").unwrap_or_else(|e| panic!("{e:?}"));
        let mut script = ScriptIndicator::new(compiled, &source);
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: 1.0,
            high: 1.0,
            low: 1.0,
            close: 1.0,
            buy_volume: 0.0,
            sell_volume: 0.0,
            trade_count: 0.0,
        };
        let mut ctx = Ctx {
            bar_index: 0,
            cvd: &[],
        };
        script.on_close(&bar, &mut ctx).unwrap();
        ctx.bar_index = 1;
        assert_eq!(
            script.preview(&bar, &mut ctx).unwrap().values,
            vec![2.0, 2.0]
        );
        assert_eq!(
            script.preview(&bar, &mut ctx).unwrap().values,
            vec![2.0, 3.0]
        );
        script.on_close(&bar, &mut ctx).unwrap();
        assert_eq!(script.plots().column(PlotId::new(0))[1], 2.0);
        assert_eq!(script.plots().column(PlotId::new(1))[1], 4.0);
    }
}

#[test]
fn builtin_field_defaults_evaluate_at_construction() {
    let source = include_str!("fixtures/libraries/defaults.pine");
    let compiled = quantick_pine::compile(source, "defaults").unwrap_or_else(|e| panic!("{e:?}"));
    let mut script = ScriptIndicator::new(compiled, source);
    for (index, line) in include_str!("golden/libraries/defaults.csv")
        .lines()
        .enumerate()
    {
        let expected: Vec<f64> = line.split(',').map(|s| s.parse().unwrap()).collect();
        let bar = IndicatorBar {
            open_time: 0,
            close_time: 1,
            open: expected[0],
            high: expected[0],
            low: expected[0],
            close: expected[0],
            buy_volume: 3.0,
            sell_volume: 4.0,
            trade_count: 0.0,
        };
        let mut ctx = Ctx {
            bar_index: index,
            cvd: &[],
        };
        for _ in 0..2 {
            assert_eq!(
                script.preview(&bar, &mut ctx).unwrap().values,
                &expected[1..]
            );
        }
        script.on_close(&bar, &mut ctx).unwrap();
    }
    for field in [
        "int value = close",
        "bool value = volume",
        "float value = math.abs(close)",
        "float value = unknown",
    ] {
        let source = format!(
            "//@version=6\nindicator(\"invalid defaults\")\ntype Unused\n    {field}\nplot(close)\n"
        );
        assert!(
            quantick_pine::compile(&source, "invalid").is_err(),
            "{field}"
        );
    }
}
