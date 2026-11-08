//! C31: scalar identity, qualifier propagation and overload failures at load.
use quantick_indicators::{Ctx, Indicator, IndicatorBar, PlotId};
use quantick_pine::{ErrorCode, ScriptIndicator, compile};

fn execute(body: &str) -> Vec<f64> {
    let source = format!("//@version=5\nindicator(\"types\")\n{body}\n");
    let compiled = compile(&source, "types").unwrap_or_else(|errors| panic!("{body}\n{errors:?}"));
    let mut script = ScriptIndicator::new(compiled, &source);
    let bar = IndicatorBar {
        open_time: 0,
        close_time: 1,
        open: 10.0,
        high: 12.0,
        low: 9.0,
        close: 10.0,
        buy_volume: 1.0,
        sell_volume: 0.0,
        trade_count: 1.0,
    };
    let mut ctx = Ctx {
        bar_index: 0,
        cvd: &[1.0],
    };
    let preview = script.preview(&bar, &mut ctx).unwrap();
    let second = script.preview(&bar, &mut ctx).unwrap();
    assert_eq!(format!("{preview:?}"), format!("{second:?}"));
    script.on_close(&bar, &mut ctx).unwrap();
    for (i, value) in preview.values.iter().enumerate() {
        let actual = script.plots().column(PlotId::new(i))[0];
        assert!(actual == *value || actual.is_nan() && value.is_nan());
    }
    preview.values
}
fn rejects(body: &str) {
    let source = format!("//@version=5\nindicator(\"types\")\n{body}\n");
    let errors = match compile(&source, "types") {
        Ok(_) => panic!("accepted {body}"),
        Err(errors) => errors,
    };
    assert!(
        errors.iter().any(|e| e.code == ErrorCode::PineType),
        "{body}: {errors:?}"
    );
}
#[test]
fn types_qualifiers_and_overloads() {
    let source = include_str!("fixtures/types/scalars.pine");
    let golden = include_str!("golden/types/scalars.csv");
    let mut script = ScriptIndicator::new(
        compile(source, "types").unwrap_or_else(|e| panic!("{e:?}")),
        source,
    );
    for (index, line) in golden.lines().skip(1).enumerate() {
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
            cvd: &[0.0, 0.0],
        };
        let preview = script.preview(&bar, &mut ctx).unwrap();
        assert_eq!(preview.values, &row[1..]);
        script.on_close(&bar, &mut ctx).unwrap();
        for (i, value) in row[1..].iter().enumerate() {
            assert_eq!(script.plots().column(PlotId::new(i))[index], *value);
        }
    }
    for line in include_str!("fixtures/types/invalid.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        rejects(&line.replace("\\n", "\n"));
    }
}
#[test]
fn types_arithmetic_and_conversion_results() {
    assert_eq!(
        execute(
            "int a = 5 / 2\nfloat b = 5.0 / 2\ninput int n = input.int(5)\nfloat c = n / 2\nplot(a)\nplot(b)\nplot(c)\nplot(int(-2.9))\nbool flag = 2\nplot(flag ? 1 : 0)"
        ),
        [2.0, 2.5, 2.5, -2.0, 1.0]
    );
    rejects("int x = math.max(1, 2.0)");
    rejects("int x = float(1)");
    rejects("plot(ta.ema(close, 2.0))");
    assert_eq!(
        execute("int x = math.max(1, 2)\nplot(x)\nplot(math.floor(2.9))\nplot(math.round(2.4))"),
        [2.0, 2.0, 2.0]
    );
}
#[test]
fn types_function_branch_loop_and_reassignment_qualifiers() {
    for body in [
        "simple float x = 1\nx := close",
        "const int x = 1\nx := 2",
        "x = 1\nx := x + 1\nsimple int y = x",
        "x = 1.0\nif close > 0\n    x := 2.0\nsimple float y = x",
        "f(x) => x + 1\nsimple float y = f(close)",
        "f(x) => x > 0 ? 1.0 : 2.0\nsimple float y = f(close)",
        "simple int x = for i = 0 to bar_index\n    i",
        "simple int x = while close > 0\n    1",
        "f(x) =>\n    y = x\n    y := \"bad\"\n    y\nplot(f(1))",
        "f(simple int x) => x\nplot(f(bar_index))",
    ] {
        rejects(body);
    }
    assert_eq!(
        execute("f(simple int x) => x + 1\ninput int n = input.int(2)\nplot(f(n))"),
        [3.0]
    );
}
#[test]
fn types_missing_values_and_opaque_handles() {
    assert_eq!(
        execute(
            "float n = na\nint i = int(na)\nbool b = bool(na)\nstring s = string(na)\ncolor c = color(na)\nplot(na(n) and na(i) and na(b) and na(s) and na(c) ? 1 : 0)\nplot(nz(i, 7))\nplot(nz(b, true) ? 1 : 0)"
        ),
        [1.0, 7.0, 1.0]
    );
    execute(
        "line l = line.new(0, 1.0, 1, 2.0)\nl.set_color(color.red)\np = plot(close)\nq = plot(open)\nfill(p, q, color.red)",
    );
    rejects("float x = color(na)");
    rejects("plot(nz(color.red, 1))");
    rejects("plot(\"text\")");
}

#[test]
fn types_function_overloads_named_arguments_and_aliases() {
    assert_eq!(
        execute(
            "f(int x) => x + 1\nf(float x) => x + 0.5\nplot(f(2))\nplot(f(2.0))\ng(int x, int y) => x - y\nplot(g(y=2, x=7))"
        ),
        [3.0, 2.5, 5.0]
    );
    rejects("f(int x) => x\nplot(f(true))");
    rejects("f(int x) => x\nplot(f(y=1))");
    rejects("f(int x, int y) => x\nplot(f(x=1, x=2))");
    rejects("f(x) =>\n    simple float y = 1\n    y := x\n    y\nplot(f(close))");
    rejects("a = close\nf(x) => x\nsimple float b = f(a)");
    rejects("x = na\nx := 1\nx := color.red");
    rejects("f() => 1 + true\nplot(close)");
    rejects("x = math.abs\nplot(x)");
    assert_eq!(
        execute("x = na\nif close > 0\n    x := close\nplot(x)"),
        [10.0]
    );
}

#[test]
fn types_missing_defaults_and_builtin_named_arguments() {
    assert_eq!(
        execute(
            "bool b = na\ncolor c = na\nplot(nz(b) == false ? 1 : 0)\nplot(nz(c) == #00000000 ? 1 : 0)\nplot(math.pow(exponent=3, base=2))\nplot(nz(replacement=9, source=float(na)))\nplot(nz(true[1]) == false ? 1 : 0)"
        ),
        [1.0, 1.0, 8.0, 9.0, 1.0]
    );
    rejects("plot(math.pow(exponent=3, base=2, base=4))");
    execute("color c = na\nstring s = na\nlabel.new(0, close, s, textcolor=c)\nbarcolor(c)");
}

#[test]
fn types_constant_folding_honors_casts_and_annotations() {
    assert_eq!(
        execute(
            "float x = 5\nfloat n = input.float(x / 2)\nplot(n)\nint len = input.int(int(2.9))\nplot(len)\ninput int a = input.int(5)\nhline(a / 2)"
        ),
        [2.5, 2.0, 2.5]
    );
    let source = "//@version=5\nindicator(\"t\")\nplot(ta.sma(close, int(2.9)))\n";
    assert!(compile(source, "types").is_ok());
}

#[test]
fn types_nested_polymorphic_calls_keep_separate_lowering() {
    assert_eq!(
        execute(
            "g(int x) => x + 1\ng(float x) => x + 0.5\nf(x) => g(x)\nplot(f(2))\nplot(f(2.0))\nh(x) =>\n    y = x\n    y := x\n    y\nplot(h(3))\nplot(h(true) ? 1 : 0)\nk(bool x) => x\nplot(k(1) == true ? 1 : 0)"
        ),
        [3.0, 2.5, 3.0, 1.0, 1.0]
    );
}

#[test]
fn types_skipped_nested_overload_advances_only_selected_kernel() {
    let source = "//@version=5\nindicator(\"t\")\ng(int x) => ta.cum(x)\ng(float x) => x\nf(x) => g(x)\nplot(bar_index > 0 ? f(2) : 0)\nplot(f(2.0))\n";
    let mut script = ScriptIndicator::new(
        compile(source, "types").unwrap_or_else(|e| panic!("{e:?}")),
        source,
    );
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
    for index in 0..2 {
        script
            .on_close(
                &bar,
                &mut Ctx {
                    bar_index: index,
                    cvd: &[0.0, 0.0],
                },
            )
            .unwrap();
    }
    assert_eq!(script.plots().column(PlotId::new(0)), &[0.0, 4.0]);
    assert_eq!(script.plots().column(PlotId::new(1)), &[2.0, 2.0]);
}

#[test]
fn types_local_constants_float_loops_and_tuple_qualifiers() {
    assert_eq!(
        execute(
            "x = if close > 0\n    const int k = 7\n    k\nelse\n    0\nplot(x)\ny = for i = 0.5 to 1.5 by 0.5\n    i\nplot(y)"
        ),
        [7.0, 1.5]
    );
    rejects("f() => [close, 1]\n[a,b] = f()\nsimple int n = b");
    rejects("const int n = for i = 0 to 2\n    i");
}

#[test]
fn types_candle_plot_registration_accepts_scalars_and_rejects_bad_ohlc() {
    execute(
        "color c = na\nplotcandle(open, high, low, close, color=c)\nplotbar(open, high, low, close, color=c)",
    );
    rejects(r#"plotcandle("bad", high, low, close)"#);
    rejects("plotbar(open, high, low, close, color=1)");
}

#[test]
fn types_missing_branch_history_and_string_results_keep_identity() {
    assert_eq!(
        execute(
            "b = if close < 0\n    true\nc = if close < 0\n    color.red\nstring s = na\nplot(nz(b) == false ? 1 : 0)\nplot(nz(c) == #00000000 ? 1 : 0)\nplot(na(s + \"x\") ? 1 : 0)"
        ),
        [1.0, 1.0, 1.0]
    );
}

#[test]
fn types_reordered_input_defaults_bind_by_parameter_name() {
    assert_eq!(
        execute(
            r#"i = input.int(title="I", defval=5)
b = input.bool(title="B", defval=true)
f = input.float(title="F", defval=2.5)
s = input.string(title="S", defval="x")
c = input.color(title="C", defval=color.red)
src = input.source(title="Source", defval=close)
plot(i)
plot(b ? 1 : 0)
plot(f)
plot(s == "x" ? 1 : 0)
plot(c == color.red ? 1 : 0)
plot(src)"#
        ),
        [5.0, 1.0, 2.5, 1.0, 1.0, 10.0]
    );
    rejects(r#"i = input.int(title="I", defval="bad")"#);
}
#[test]
fn types_cast_division_folding_matches_runtime_and_aliases() {
    assert_eq!(
        execute(
            r#"plot(int(5) / 2)
n = input.float(int(5) / 2)
plot(n)
int m = input.int(int(5) / 2)
plot(m)
const float value = 5.9
const int alias = int(value)
plot(alias / 2)
p = input.float(alias / 2)
plot(p)
input int live = input.int(5)
hline(int(live) / 2)
plot(int(live) / 2)"#
        ),
        [2.0, 2.0, 2.0, 2.0, 2.0, 2.5, 2.5]
    );
}
