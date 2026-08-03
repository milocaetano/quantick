//! Compile-pass behavior, tested through the public `compile()` entry:
//! declaration collection, slot resolution, call-site identity, const-folded
//! kernel lengths, max_bars_back inference and the §3.1 reject list.

use quantick_indicators::{InputSpec, PlotStyle, SourceId};
use quantick_pine::compile::{MAX_BARS_BACK_CAP, Resolution};
use quantick_pine::{ErrorCode, PineError, compile};

fn compile_ok(source: &str) -> quantick_pine::CompiledScript {
    match compile(source, "test.pine") {
        Ok(script) => script,
        Err(errors) => panic!(
            "fixture must compile:\n{}",
            errors
                .iter()
                .map(|e| e.render("test.pine", source))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    }
}

fn compile_err(source: &str) -> Vec<PineError> {
    compile(source, "test.pine")
        .err()
        .expect("fixture must fail")
}

#[test]
fn header_plots_and_inputs_are_collected() {
    let script = compile_ok(
        "//@version=5\n\
         indicator(\"My EMA\", shorttitle=\"ME\", overlay=true)\n\
         len = input.int(9, \"Length\", minval=1)\n\
         src = input.source(close, \"Source\")\n\
         plot(ta.ema(src, len), title=\"ema\", color=color.orange, linewidth=2)\n\
         plot(delta, style=plot.style_histogram)\n",
    );
    assert_eq!(script.title, "My EMA");
    assert_eq!(script.short_title.as_deref(), Some("ME"));
    assert!(script.overlay);

    assert_eq!(script.inputs.len(), 2);
    assert!(matches!(
        &script.inputs[0],
        InputSpec::Int {
            default: 9,
            min: Some(1),
            ..
        }
    ));
    assert!(matches!(
        &script.inputs[1],
        InputSpec::Source {
            default: SourceId::Close,
            ..
        }
    ));

    assert_eq!(script.plots.len(), 2);
    assert_eq!(script.plots[0].title, "ema");
    assert_eq!(script.plots[0].width, 2.0);
    assert_eq!(script.plots[1].style, PlotStyle::Histogram);
    assert_eq!(script.call_site_count, 1, "one stateful ta.ema call");
}

#[test]
fn kernel_length_folds_through_an_input() {
    // len = input.int(9); doubled arithmetic still folds.
    compile_ok(
        "//@version=5\n\
         indicator(\"t\")\n\
         len = input.int(9, \"Length\")\n\
         plot(ta.sma(close, len * 2))\n",
    );
}

#[test]
fn a_dynamic_kernel_length_is_refused() {
    let errors = compile_err(
        "//@version=5\n\
         indicator(\"t\")\n\
         plot(ta.sma(close, bar_index))\n",
    );
    assert!(
        errors.iter().any(|e| e.code == ErrorCode::PineSeriesLength),
        "{errors:?}"
    );
}

#[test]
fn a_reassigned_length_no_longer_folds() {
    let errors = compile_err(
        "//@version=5\n\
         indicator(\"t\")\n\
         len = 9\n\
         len := 10\n\
         plot(ta.sma(close, len))\n",
    );
    assert!(
        errors.iter().any(|e| e.code == ErrorCode::PineSeriesLength),
        "{errors:?}"
    );
}

#[test]
fn stateful_kernels_inside_loops_are_refused() {
    // Inline \n + real spaces: `\`-continued literals strip leading
    // whitespace, which would erase the loop body's indentation.
    let errors = compile_err(
        "//@version=5\nindicator(\"t\")\ns = 0.0\nfor i = 0 to 4\n    s := s + ta.ema(close, 9)\nplot(s)\n",
    );
    assert!(
        errors
            .iter()
            .any(|e| e.code == ErrorCode::PineStatefulInLoop),
        "{errors:?}"
    );
}

#[test]
fn recursion_is_refused() {
    let errors = compile_err(
        "//@version=5\n\
         indicator(\"t\")\n\
         f(x) => f(x - 1)\n\
         plot(f(3))\n",
    );
    assert!(
        errors.iter().any(|e| e.code == ErrorCode::PineRecursion),
        "{errors:?}"
    );
}

#[test]
fn the_reject_list_names_its_reasons() {
    let source = "//@version=5\n\
         indicator(\"t\")\n\
         s = request.security(close)\n\
         plot(s)\n";
    let errors = compile_err(source);
    let security = errors
        .iter()
        .find(|e| e.code == ErrorCode::PineNoSecurity)
        .expect("request.* refused");
    let rendered = security.render("file.pine", source);
    assert!(rendered.starts_with("file.pine:3:"), "{rendered}");
    assert!(rendered.contains("activity-sampled"), "{rendered}");

    let errors =
        compile_err("//@version=5\nindicator(\"t\")\nstrategy.entry(\"x\")\nplot(close)\n");
    assert!(errors.iter().any(|e| e.code == ErrorCode::PineNoStrategy));

    let errors =
        compile_err("//@version=5\nindicator(\"t\")\na = array.new_float(3)\nplot(close)\n");
    assert!(
        errors
            .iter()
            .any(|e| e.code == ErrorCode::PineNoCollections)
    );
}

#[test]
fn unknown_names_get_did_you_mean() {
    let errors = compile_err(
        "//@version=5\n\
         indicator(\"t\")\n\
         plot(ta.emma(colse, 9))\n",
    );
    let unknown: Vec<_> = errors
        .iter()
        .filter(|e| e.code == ErrorCode::PineUnknownName)
        .collect();
    assert_eq!(unknown.len(), 2, "{errors:?}");
    assert!(
        unknown
            .iter()
            .any(|e| e.notes.iter().any(|n| n.contains("ta.ema")))
    );
    assert!(
        unknown
            .iter()
            .any(|e| e.notes.iter().any(|n| n.contains("close")))
    );
}

#[test]
fn multiple_problems_all_report() {
    let errors = compile_err(
        "//@version=5\n\
         indicator(\"t\")\n\
         a = request.security(close)\n\
         b = ta.sma(close, bar_index)\n\
         c = qwidgets\n\
         plot(a + b + c)\n",
    );
    assert!(errors.len() >= 3, "{errors:?}");
}

#[test]
fn history_offsets_size_max_bars_back() {
    let script = compile_ok(
        "//@version=5\n\
         indicator(\"t\")\n\
         plot(close[3] - close[12])\n",
    );
    assert_eq!(script.max_bars_back, 13, "deepest constant offset + 1");
    assert_eq!(script.history_series_count, 2);

    let dynamic = compile_ok(
        "//@version=5\n\
         indicator(\"t\")\n\
         plot(close[bar_index % 7])\n",
    );
    assert_eq!(dynamic.max_bars_back, MAX_BARS_BACK_CAP);
}

#[test]
fn version_other_than_5_is_refused_and_missing_is_a_warning() {
    let errors = compile_err("//@version=4\nindicator(\"t\")\nplot(close)\n");
    assert!(errors.iter().any(|e| e.code == ErrorCode::PineVersion));

    let script = compile_ok("indicator(\"t\")\nplot(close)\n");
    assert!(
        script
            .warnings
            .iter()
            .any(|w| w.code == ErrorCode::PineVersion),
        "missing directive is a warning, not an error"
    );
}

#[test]
fn names_resolve_to_slots_and_functions() {
    let script = compile_ok(
        "//@version=5\n\
         indicator(\"t\")\n\
         double(x) => x * 2\n\
         base = close\n\
         plot(double(base))\n",
    );
    assert_eq!(script.global_slots, 1, "one global: base");
    assert_eq!(script.functions.len(), 1);
    assert_eq!(script.functions[0].param_count, 1);
    assert_eq!(script.functions[0].local_slots, 1);
    // Somewhere in the tables: a Global(0) reference and a Function(0) call.
    assert!(script.resolutions.contains(&Resolution::Global(0)));
    assert!(script.resolutions.contains(&Resolution::Function(0)));
}

#[test]
fn ignored_header_args_and_alertcondition_warn() {
    let script = compile_ok(
        "//@version=5\n\
         indicator(\"t\", format=format.price, precision=2)\n\
         alertcondition(close > open, \"up\")\n\
         plot(close)\n",
    );
    assert!(
        script.warnings.len() >= 3,
        "format=, precision= and alertcondition each warn: {:?}",
        script.warnings
    );
}

#[test]
fn shape_plots_register_markers_and_fills_resolve() {
    use quantick_indicators::{MarkerLocation, MarkerShape};
    let script = compile_ok(
        "//@version=5\nindicator(\"t\")\na = plot(close, title=\"a\")\nb = plot(open, title=\"b\")\nfill(a, b, color=color.new(color.aqua, 80))\nplotshape(close > open, style=shape.triangledown, location=location.abovebar, text=\"S\")\n",
    );
    assert_eq!(script.plots.len(), 3);
    let marker = script.plots[2].marker.as_ref().expect("marker plot");
    assert_eq!(marker.shape, MarkerShape::TriangleDown);
    assert_eq!(marker.location, MarkerLocation::AboveBar);
    assert_eq!(marker.text.as_deref(), Some("S"));
    assert_eq!(script.fills.len(), 1);
    assert_eq!(script.fills[0].a.index(), 0);
    assert_eq!(script.fills[0].b.index(), 1);
}

#[test]
fn a_fill_between_non_plots_is_refused_honestly() {
    let errors = compile_err(
        "//@version=5\nindicator(\"t\")\nx = close\ny = open\nfill(x, y)\nplot(close)\n",
    );
    assert!(
        errors
            .iter()
            .any(|e| e.code == ErrorCode::PineUnsupported && e.message.contains("plot() results")),
        "{errors:?}"
    );
}
#[test]
fn a_plot_outside_the_top_level_is_refused() {
    // `plot()` inside an `if` had neither a frame nor a loop, so the
    // top-level rule missed it — and pass 1 only scans top-level statements,
    // so the plot was never registered either. The script drew nothing and
    // said nothing.
    for source in [
        "//@version=5\nindicator(\"t\")\nif close > open\n    plot(close)\n",
        "//@version=5\nindicator(\"t\")\nfor i = 0 to 2\n    plot(close)\n",
        "//@version=5\nindicator(\"t\")\nf() => plot(close)\nplot(close)\n",
    ] {
        let errors = compile_err(source);
        assert!(
            errors
                .iter()
                .any(|e| e.code == ErrorCode::PineSyntax && e.message.contains("top level")),
            "{source:?} -> {errors:?}"
        );
    }

    // The same call at the top level is of course fine.
    let script = compile_ok("//@version=5\nindicator(\"t\")\nplot(close)\n");
    assert_eq!(script.plots.len(), 1);
}

#[test]
fn plot_arguments_fold_through_names() {
    // Plots are registered after resolution, so a colour or width held in a
    // variable reaches the spec instead of being silently replaced by the
    // default.
    let script = compile_ok(
        "//@version=5\n\
         indicator(\"t\")\n\
         c = color.red\n\
         w = 3\n\
         name = \"my plot\"\n\
         plot(close, title=name, color=c, linewidth=w)\n",
    );
    let spec = &script.plots[0];
    assert_eq!(spec.title, "my plot");
    assert_eq!(spec.width, 3.0);
    assert_eq!(
        (spec.base_color.r, spec.base_color.g, spec.base_color.b),
        (242, 54, 69),
        "color.red, not the amber default (255, 179, 0)"
    );
}

#[test]
fn a_plot_argument_that_cannot_fold_says_so() {
    // One constant look per plot: a per-bar colour cannot be honoured, and
    // substituting the default in silence is what the warning prevents.
    let script = compile_ok(
        "//@version=5\n\
         indicator(\"t\")\n\
         up = close > open\n\
         plot(close, color = up ? color.green : color.red)\n",
    );
    assert!(
        script
            .warnings
            .iter()
            .any(|w| w.code == ErrorCode::PineUnsupported && w.message.contains("`color`")),
        "{:?}",
        script.warnings
    );
}

#[test]
fn a_later_reassignment_still_blocks_folding() {
    // The `:=` sits *after* the call, which the walk could not see when it
    // filled `reassigned` as it went: the length folded anyway and the kernel
    // pinned bar zero's window forever.
    let errors = compile_err(
        "//@version=5\n\
         indicator(\"t\")\n\
         var len = 9\n\
         plot(ta.sma(close, len))\n\
         len := len + 1\n",
    );
    assert!(
        errors.iter().any(|e| e.code == ErrorCode::PineSeriesLength),
        "{errors:?}"
    );
}

#[test]
fn a_dynamic_offset_raises_the_floor_instead_of_replacing_it() {
    // Order must not decide the answer: a 700-bar constant read needs 701
    // rows whether it is written before or after the dynamic one.
    for source in [
        "//@version=5\nindicator(\"t\")\ni = 3\nplot(close[700] + close[i])\n",
        "//@version=5\nindicator(\"t\")\ni = 3\nplot(close[i] + close[700])\n",
    ] {
        let script = compile_ok(source);
        assert_eq!(
            script.max_bars_back,
            701.max(MAX_BARS_BACK_CAP),
            "{source:?}"
        );
    }
}

#[test]
fn a_refused_family_keeps_its_reason_as_a_bare_name() {
    // `dayofweek` is a variable in Pine, so it reaches the name arm rather
    // than the call or member arms; "unknown name" would be the wrong reason
    // for a name the dialect knows and refuses.
    let errors = compile_err("//@version=5\nindicator(\"t\")\nplot(dayofweek)\n");
    assert!(
        errors.iter().any(|e| e.code == ErrorCode::PineNoCalendar),
        "{errors:?}"
    );
}

#[test]
fn a_folded_division_by_zero_is_na_like_the_interpreter() {
    // The folder and the evaluator must agree; IEEE would say `inf` here and
    // that value would reach an input default or a plot width.
    let script = compile_ok(
        "//@version=5\n\
         indicator(\"t\")\n\
         w = 1.0 / 0\n\
         plot(close, linewidth=w)\n",
    );
    assert_eq!(
        script.plots[0].width, 1.5,
        "an `na` width falls back to the default, never to inf"
    );
}
