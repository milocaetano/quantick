//! Interpreter semantics, tested through the public `Indicator` contract:
//! history vs warmup, `var` vs plain vs `varip`, tuples, block expressions,
//! call-site identity, the loop budget, division by zero, and the rollback
//! property — the §6 list, written as the spec the interpreter must meet.

use quantick_indicators::{Ctx, EvalError, Indicator, IndicatorBar, PlotId, PreviewFrame, ta};
use quantick_pine::{ScriptIndicator, compile};

fn bar(index: usize, close: f64) -> IndicatorBar {
    let t = 1_700_000_000_000 + index as i64 * 1_000;
    IndicatorBar {
        open_time: t,
        close_time: t + 999,
        open: close - 0.5,
        high: close + 1.0,
        low: close - 1.0,
        close,
        buy_volume: 2.0,
        sell_volume: 1.0,
        trade_count: 3.0,
    }
}

/// Drives a ScriptIndicator exactly like the host: cvd accumulated, commit
/// per closed bar, previews with the staged cvd.
struct Harness {
    indicator: ScriptIndicator,
    cvd: Vec<f64>,
    sum: f64,
}

impl Harness {
    fn load(source: &str) -> Self {
        let compiled = match compile(source, "test.pine") {
            Ok(c) => c,
            Err(errors) => panic!(
                "script must compile:\n{}",
                errors
                    .iter()
                    .map(|e| e.render("test.pine", source))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        };
        Self {
            indicator: ScriptIndicator::new(compiled, source),
            cvd: Vec::new(),
            sum: 0.0,
        }
    }

    fn close(&mut self, b: &IndicatorBar) -> Result<(), EvalError> {
        self.sum += b.delta();
        self.cvd.push(self.sum);
        let mut ctx = Ctx {
            bar_index: self.cvd.len() - 1,
            cvd: &self.cvd,
        };
        self.indicator.on_close(b, &mut ctx)
    }

    fn close_all(&mut self, closes: &[f64]) {
        for (i, &c) in closes.iter().enumerate() {
            self.close(&bar(i, c)).expect("commit run succeeds");
        }
    }

    fn preview(&mut self, b: &IndicatorBar) -> Result<PreviewFrame, EvalError> {
        self.cvd.push(self.sum + b.delta());
        let mut ctx = Ctx {
            bar_index: self.cvd.len() - 1,
            cvd: &self.cvd,
        };
        let frame = self.indicator.preview(b, &mut ctx);
        self.cvd.pop();
        frame
    }

    fn column(&self, plot: usize) -> Vec<f64> {
        self.indicator.plots().column(PlotId::new(plot)).to_vec()
    }
}

fn fmt(values: &[f64]) -> String {
    format!("{values:?}")
}

#[test]
fn plot_close_is_the_identity_script() {
    let mut h = Harness::load("//@version=5\nindicator(\"t\")\nplot(close)\n");
    h.close_all(&[3.0, 6.0, 9.0]);
    assert_eq!(h.column(0), vec![3.0, 6.0, 9.0]);
}

#[test]
fn scripted_ema_is_bit_identical_to_the_native_kernel() {
    let closes = [3.0, 6.0, 9.0, 12.0, 15.0, 18.0, 15.0, 12.0];
    let mut h = Harness::load("//@version=5\nindicator(\"t\")\nplot(ta.ema(close, 3))\n");
    h.close_all(&closes);

    let mut kernel = ta::Ema::new(3);
    let expected: Vec<f64> = closes.iter().map(|&c| kernel.push(c)).collect();
    assert_eq!(fmt(&h.column(0)), fmt(&expected));
}

#[test]
fn var_initialises_once_and_plain_reinitialises() {
    let mut counted = Harness::load(
        "//@version=5\nindicator(\"t\")\nvar count = 0.0\ncount := count + 1\nplot(count)\n",
    );
    counted.close_all(&[1.0, 1.0, 1.0]);
    assert_eq!(counted.column(0), vec![1.0, 2.0, 3.0], "var persists");

    let mut plain = Harness::load(
        "//@version=5\nindicator(\"t\")\ncount = 0.0\ncount := count + 1\nplot(count)\n",
    );
    plain.close_all(&[1.0, 1.0, 1.0]);
    assert_eq!(
        plain.column(0),
        vec![1.0, 1.0, 1.0],
        "plain re-initialises every bar"
    );
}

#[test]
fn varip_survives_previews_var_rolls_back() {
    let source_varip =
        "//@version=5\nindicator(\"t\")\nvarip ticks = 0.0\nticks := ticks + 1\nplot(ticks)\n";
    let mut h = Harness::load(source_varip);
    h.close(&bar(0, 1.0)).unwrap();
    // Two previews mutate varip and the mutation is kept (§2.3).
    h.preview(&bar(1, 1.0)).unwrap();
    h.preview(&bar(1, 1.0)).unwrap();
    h.close(&bar(1, 1.0)).unwrap();
    assert_eq!(h.column(0), vec![1.0, 4.0], "varip kept both preview bumps");

    let source_var =
        "//@version=5\nindicator(\"t\")\nvar ticks = 0.0\nticks := ticks + 1\nplot(ticks)\n";
    let mut h = Harness::load(source_var);
    h.close(&bar(0, 1.0)).unwrap();
    h.preview(&bar(1, 1.0)).unwrap();
    h.preview(&bar(1, 1.0)).unwrap();
    h.close(&bar(1, 1.0)).unwrap();
    assert_eq!(h.column(0), vec![1.0, 2.0], "var rolled the previews back");
}

#[test]
fn history_reads_committed_bars_and_warms_up_with_na() {
    let mut h = Harness::load("//@version=5\nindicator(\"t\")\nplot(close[2])\n");
    h.close_all(&[10.0, 20.0, 30.0, 40.0]);
    let col = h.column(0);
    assert!(col[0].is_nan() && col[1].is_nan(), "{col:?}");
    assert_eq!(&col[2..], &[10.0, 20.0], "{col:?}");
}

#[test]
fn previews_do_not_commit_history() {
    let mut h = Harness::load("//@version=5\nindicator(\"t\")\nplot(close[1])\n");
    h.close(&bar(0, 10.0)).unwrap();
    // A wild forming bar must not become anyone's close[1].
    h.preview(&bar(1, 999.0)).unwrap();
    h.close(&bar(1, 20.0)).unwrap();
    h.close(&bar(2, 30.0)).unwrap();
    let col = h.column(0);
    assert!(col[0].is_nan());
    assert_eq!(&col[1..], &[10.0, 20.0], "{col:?}");
}

#[test]
fn functions_tuples_and_blocks_compose() {
    let source = "//@version=5\nindicator(\"t\")\nbounds(src, len) =>\n    lo = ta.lowest(src, len)\n    hi = ta.highest(src, len)\n    [lo, hi]\n[l, h] = bounds(close, 2)\nplot((l + h) / 2)\n";
    let mut harness = Harness::load(source);
    harness.close_all(&[10.0, 20.0, 30.0]);
    let col = harness.column(0);
    assert!(col[0].is_nan(), "lowest/highest warm up");
    assert_eq!(&col[1..], &[15.0, 25.0], "{col:?}");
}

#[test]
fn each_textual_call_site_owns_its_state_through_call_paths() {
    // f() advances a cum kernel; called from two sites, each owns one.
    let source = "//@version=5\nindicator(\"t\")\nf() => ta.cum(1.0)\nplot(f() + f())\n";
    let mut h = Harness::load(source);
    h.close_all(&[1.0, 1.0, 1.0]);
    assert_eq!(
        h.column(0),
        vec![2.0, 4.0, 6.0],
        "two independent cum kernels, one per call path"
    );
}

#[test]
fn the_loop_budget_errors_instead_of_hanging() {
    let source = "//@version=5\nindicator(\"t\")\nx = 0.0\nwhile x >= 0\n    x := x + 1\nplot(x)\n";
    let mut h = Harness::load(source);
    let error = h.close(&bar(0, 1.0)).expect_err("must hit the budget");
    assert_eq!(error.bar_index, 0);
    assert!(
        error.message.contains("PINE_LOOP_BUDGET"),
        "{}",
        error.message
    );
    assert_eq!(h.indicator.plots().len(), 0, "no half-appended row");
}

#[test]
fn division_by_zero_is_na_not_infinity() {
    let mut h = Harness::load("//@version=5\nindicator(\"t\")\nplot(close / 0)\n");
    h.close_all(&[5.0]);
    assert!(h.column(0)[0].is_nan());
}

#[test]
fn if_and_for_are_expressions() {
    let source = "//@version=5\nindicator(\"t\")\ntrend = if close > 10\n    1\nelse\n    -1\ns = 0.0\nfor i = 1 to 3\n    s := s + i\nplot(trend * s)\n";
    let mut h = Harness::load(source);
    h.close_all(&[5.0, 15.0]);
    assert_eq!(h.column(0), vec![-6.0, 6.0]);
}

#[test]
fn nz_replaces_na_and_flow_series_flow() {
    let mut h = Harness::load(
        "//@version=5\nindicator(\"t\")\nplot(nz(close[1], 42.0))\nplot(cvd)\nplot(delta)\n",
    );
    h.close_all(&[10.0, 20.0]);
    assert_eq!(h.column(0), vec![42.0, 10.0]);
    // Each bar's delta is +1 (buy 2, sell 1); cvd accumulates.
    assert_eq!(h.column(1), vec![1.0, 2.0]);
    assert_eq!(h.column(2), vec![1.0, 1.0]);
}

#[test]
fn kernel_previews_roll_back_like_natives() {
    let closes = [3.0, 6.0, 9.0, 12.0, 15.0];
    let source = "//@version=5\nindicator(\"t\")\nplot(ta.ema(close, 3))\n";

    let mut plain = Harness::load(source);
    plain.close_all(&closes);

    let mut previewed = Harness::load(source);
    for (i, &c) in closes.iter().enumerate() {
        // Three previews of a wiggling partial before every close.
        for wiggle in [0.5, -0.25, 1.0] {
            previewed.preview(&bar(i, c + wiggle)).unwrap();
        }
        previewed.close(&bar(i, c)).unwrap();
    }
    assert_eq!(
        fmt(&plain.column(0)),
        fmt(&previewed.column(0)),
        "an EMA over close,preview*3,close must equal the EMA over close,close"
    );
}

#[test]
fn runtime_errors_render_with_line_and_column() {
    // Calling a number: a type error only the runtime can see.
    let source = "//@version=5\nindicator(\"t\")\nx = 1\ny = x + true\nplot(y)\n";
    let mut h = Harness::load(source);
    let error = h.close(&bar(0, 1.0)).expect_err("type error");
    assert!(error.message.contains("4:"), "{}", error.message);
    assert!(error.message.contains("PINE_TYPE"), "{}", error.message);
}

#[test]
fn a_kernel_length_that_changes_between_bars_is_refused() {
    // The kernel is built once and keeps its window forever, so a length
    // that moves used to be silently pinned to bar zero's value while the
    // script read as if it changed.
    let mut h = Harness::load(
        "//@version=5
indicator(\"t\")
f(n) => ta.sma(close, n)
plot(f(bar_index + 1))
",
    );
    h.close(&bar(0, 3.0))
        .expect("the first bar builds the kernel");
    let err = h
        .close(&bar(1, 6.0))
        .expect_err("a changed length must be refused, not silently ignored");
    assert!(err.message.contains("cannot change"), "{err}");
}

#[test]
fn an_absurd_kernel_length_errors_instead_of_aborting() {
    // `vec![0.0; 2e9]` is 16 GB, and a failed allocation aborts the process
    // rather than unwinding into this indicator's error state.
    let source = "//@version=5
indicator(\"t\")
plot(ta.sma(close, 2000000000))
";
    let Err(errors) = compile(source, "test.pine") else {
        panic!("an absurd length must be refused at load time");
    };
    assert!(
        errors
            .iter()
            .any(|e| e.code == quantick_pine::ErrorCode::PineSeriesLength),
        "{errors:?}"
    );
}

#[test]
fn math_max_uses_infinite_identities() {
    // `f64::MIN`/`MAX` are the extreme *finite* values, so the fold answered
    // -1.797e308 where -inf is the true maximum of the arguments.
    let mut h = Harness::load(
        "//@version=5
indicator(\"t\")
plot(math.max(math.log(0), math.log(0)))
",
    );
    h.close_all(&[3.0]);
    assert_eq!(h.column(0), vec![f64::NEG_INFINITY]);
}
