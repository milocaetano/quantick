//! `barcolor` semantics: which bar gets painted, which colour, and what the
//! absence of a call means.
//!
//! The builtin used to be accepted and inert. Now it writes the bar paint
//! channel, so what needs proving is not "it compiles" but "the colour lands
//! on the bar that asked for it, and nowhere else".

use quantick_indicators::{Ctx, EvalError, Indicator, IndicatorBar, PreviewFrame, Rgba8};
use quantick_pine::{ScriptIndicator, compile};

const RED: Rgba8 = Rgba8::opaque(255, 0, 0);
const GREEN: Rgba8 = Rgba8::opaque(0, 255, 0);

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

/// Drives a script the way the host does: cvd accumulated, one commit per
/// closed bar, previews staged and popped.
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

    fn close_all(&mut self, closes: &[f64]) -> Result<(), EvalError> {
        for (i, &c) in closes.iter().enumerate() {
            let b = bar(i, c);
            self.sum += b.delta();
            self.cvd.push(self.sum);
            let mut ctx = Ctx {
                bar_index: self.cvd.len() - 1,
                cvd: &self.cvd,
            };
            self.indicator.on_close(&b, &mut ctx)?;
        }
        Ok(())
    }

    fn preview(&mut self, close: f64) -> Result<PreviewFrame, EvalError> {
        let b = bar(self.cvd.len(), close);
        self.cvd.push(self.sum + b.delta());
        let mut ctx = Ctx {
            bar_index: self.cvd.len() - 1,
            cvd: &self.cvd,
        };
        let frame = self.indicator.preview(&b, &mut ctx);
        self.cvd.pop();
        frame
    }

    fn paint(&self, row: usize) -> Option<Rgba8> {
        self.indicator.plots().bar_paint(row)
    }
}

#[test]
fn a_colour_paints_its_bar_and_na_leaves_the_bar_alone() {
    let mut h = Harness::load(
        "//@version=5\n\
         indicator(\"paint\")\n\
         barcolor(close > 100 ? color.rgb(255, 0, 0) : na)\n\
         plot(close)\n",
    );
    h.close_all(&[99.0, 101.0, 98.0, 102.0]).expect("evaluates");

    assert_eq!(h.paint(0), None, "99 is not above 100");
    assert_eq!(h.paint(1), Some(RED));
    assert_eq!(h.paint(2), None, "na un-paints; it does not inherit bar 1");
    assert_eq!(h.paint(3), Some(RED));
}

#[test]
fn a_barcolor_the_if_never_reached_leaves_the_bar_unpainted() {
    // The idiomatic form, and the one that would be wrong if a skipped call
    // left the previous bar's colour standing.
    let mut h = Harness::load(
        "//@version=5\n\
         indicator(\"paint\")\n\
         if close > 100\n\
         \x20   barcolor(color.rgb(0, 255, 0))\n\
         plot(close)\n",
    );
    h.close_all(&[101.0, 99.0]).expect("evaluates");

    assert_eq!(h.paint(0), Some(GREEN));
    assert_eq!(h.paint(1), None, "the if did not fire, so nothing painted");
}

#[test]
fn the_last_call_of_the_bar_wins() {
    let mut h = Harness::load(
        "//@version=5\n\
         indicator(\"paint\")\n\
         barcolor(color.rgb(255, 0, 0))\n\
         barcolor(color.rgb(0, 255, 0))\n\
         plot(close)\n",
    );
    h.close_all(&[100.0]).expect("evaluates");
    assert_eq!(h.paint(0), Some(GREEN));
}

#[test]
fn transparency_survives_into_the_channel() {
    // color.new(c, 60) keeps 40% alpha; a channel that dropped alpha would
    // paint an opaque candle over the heatmap instead of a tinted one.
    let mut h = Harness::load(
        "//@version=5\n\
         indicator(\"paint\")\n\
         barcolor(color.new(color.rgb(255, 0, 0), 60))\n\
         plot(close)\n",
    );
    h.close_all(&[100.0]).expect("evaluates");
    let paint = h.paint(0).expect("painted");
    assert_eq!((paint.r, paint.g, paint.b), (255, 0, 0));
    assert_eq!(paint.a, 102, "255 * (1 - 0.60) rounded");
}

#[test]
fn the_forming_bar_is_painted_by_its_preview_and_commits_nothing() {
    let mut h = Harness::load(
        "//@version=5\n\
         indicator(\"paint\")\n\
         barcolor(close > 100 ? color.rgb(255, 0, 0) : na)\n\
         plot(close)\n",
    );
    h.close_all(&[99.0]).expect("evaluates");

    let cold = h.preview(99.5).expect("preview evaluates");
    assert_eq!(cold.paint, None, "the forming bar has not earned it yet");

    let hot = h.preview(101.0).expect("preview evaluates");
    assert_eq!(hot.paint, Some(RED), "and now it has");

    assert_eq!(
        h.indicator.plots().len(),
        1,
        "no preview ever committed a row"
    );
    assert_eq!(h.paint(1), None, "nor a colour");
}

#[test]
fn a_non_colour_argument_is_a_type_error() {
    let mut h = Harness::load(
        "//@version=5\n\
         indicator(\"paint\")\n\
         barcolor(close)\n\
         plot(close)\n",
    );
    let error = h.close_all(&[100.0]).expect_err("a number is not a colour");
    assert!(error.message.contains("PINE_TYPE"), "{error}");
    assert!(error.message.contains("a color"), "{error}");
}

#[test]
fn barcolor_no_longer_warns_but_bgcolor_still_does() {
    let compiled = compile(
        "//@version=5\n\
         indicator(\"paint\")\n\
         barcolor(color.rgb(255, 0, 0))\n\
         bgcolor(color.rgb(0, 0, 255))\n\
         plot(close)\n",
        "test.pine",
    )
    .expect("compiles");

    let warnings: Vec<&str> = compiled
        .warnings
        .iter()
        .map(|w| w.message.as_str())
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "exactly one inert builtin is left in this script: {warnings:?}"
    );
    assert!(
        warnings[0].starts_with("bgcolor"),
        "bgcolor is still inert and must keep saying so, and barcolor must \
         no longer complain at all: {warnings:?}"
    );
}

#[test]
fn a_script_without_barcolor_carries_no_paint_channel() {
    let mut h = Harness::load(
        "//@version=5\n\
         indicator(\"plain\")\n\
         plot(close)\n",
    );
    h.close_all(&[100.0, 101.0]).expect("evaluates");
    assert!(
        !h.indicator.plots().paints_any(),
        "an unpainted script pays nothing for the channel"
    );
}
